//! Jump-to-source contract and source presentation (#141). Expected values
//! are those of `love/spec/source_spec.lua` and `source_view_spec.lua`; byte
//! positions here are 0-based where the Lua ones were 1-based.

use crate::common;

use common::{composite, contrast};
use viewer::source::{self, Files, Ident, Pos};
use viewer::theme;

fn ident(lang: Option<&str>, file: Option<&str>, name: Option<&str>) -> Ident {
    Ident {
        lang: lang.map(Into::into),
        file: file.map(Into::into),
        ns: None,
        name: name.map(Into::into),
    }
}

#[test]
fn requires_an_explicit_supported_language_without_namespace_inference() {
    let files = Files {
        root: None,
        read: &|_| panic!("unsupported languages must not read files"),
        is_file: &|_| panic!("unsupported languages must not access files"),
    };
    assert!(source::member_source(&ident(None, Some("example.lua"), None), &files).is_none());
    assert!(
        source::member_source(&ident(Some("clojure"), Some("example.clj"), None), &files).is_none()
    );
    let mut by_ns = ident(Some("lua"), None, None);
    by_ns.ns = Some("app.some_module".into());
    assert!(source::member_source(&by_ns, &files).is_none());
}

#[test]
fn rejects_a_missing_file_and_never_reads_it() {
    let files = Files {
        root: None,
        read: &|_| panic!("missing file read"),
        is_file: &|_| false,
    };
    assert!(
        source::member_source(&ident(Some("lua"), Some("missing.lua"), None), &files).is_none()
    );
}

#[test]
fn preserves_literal_names_crlf_trimming_and_regex_word_boundaries() {
    let ts = source::lookup("typescript").unwrap();
    let text = "export function pickMore() {}\r\nexport function pick$() {}\r\nexport function pick() {}  \r\n";
    let pick = ident(None, None, Some("pick"));
    assert_eq!(Some(2), ts.start_line(text, &pick));
    assert_eq!(
        Some("export function pick$() {}".into()),
        ts.extract(text, &pick)
    );
    let dollar = ident(None, None, Some("pick$"));
    assert_eq!(None, ts.extract("function pick$() {}", &dollar));
    assert_eq!(
        Some("function pick$x() {}".into()),
        ts.extract("function pick$x() {}", &dollar)
    );
}

#[test]
fn treats_lua_member_names_literally_and_prefers_declarations() {
    let lua = source::lookup("lua").unwrap();
    let text = "M.pick = function() end\nfunction Mxpick() end\nfunction M.pick() end\n";
    assert_eq!(
        Some(3),
        lua.start_line(text, &ident(None, None, Some("M.pick")))
    );
}

#[test]
fn locates_the_rust_tauri_attribute_rather_than_the_function_line() {
    let rust = source::lookup("rust").unwrap();
    let text = "pub fn pick() {}\n#[tauri::command]\nfn pick() {}\n";
    let pick = ident(None, None, Some("pick"));
    assert_eq!(Some(2), rust.start_line(text, &pick));
    assert_eq!(
        Some("#[tauri::command]\nfn pick() {}".into()),
        rust.extract(text, &pick)
    );
}

#[test]
fn chooses_async_python_definitions_before_earlier_synchronous_ones() {
    let python = source::lookup("python").unwrap();
    assert_eq!(
        Some(2),
        python.start_line(
            "def pick(): pass\nasync def pick(): pass\n",
            &ident(None, None, Some("pick"))
        )
    );
}

#[test]
fn opens_an_exported_typescript_member_with_the_whole_original_file() {
    let body = "export function loadBook(): string {\n  return \"\";\n}\n";
    let files = Files {
        root: None,
        read: &|_| Some(body.to_string()),
        is_file: &|p| p == "f/book.ts",
    };
    let opened = source::member_source(
        &ident(Some("typescript"), Some("f/book.ts"), Some("loadBook")),
        &files,
    )
    .unwrap();
    assert_eq!(opened.file, "f/book.ts");
    assert_eq!(opened.title, "f/book.ts:1");
    assert_eq!(opened.body, body);
    assert_eq!(opened.line, Some(1));
}

#[test]
fn a_relative_file_resolves_under_the_project_root() {
    let files = Files {
        root: Some("/project/".into()),
        read: &|_| Some("local M = {}\nreturn M\n".into()),
        is_file: &|p| p == "/project/src/m.lua",
    };
    let opened =
        source::member_source(&ident(Some("lua"), Some("src/m.lua"), None), &files).unwrap();
    assert_eq!(opened.file, "/project/src/m.lua");
    assert_eq!(
        opened.line, None,
        "a module opens the whole file, no member line"
    );
    assert_eq!(
        opened.title, "src/m.lua",
        "the title names the file as the project does"
    );
}

#[test]
fn colors_keywords_strings_and_comments_for_every_scanner_language() {
    for (lang, text, keyword, string, comment) in [
        (
            "lua",
            "local x = \"value\" -- note",
            "local",
            "\"value\"",
            "-- note",
        ),
        (
            "python",
            "return \"value\" # note",
            "return",
            "\"value\"",
            "# note",
        ),
        (
            "typescript",
            "return \"value\" // note",
            "return",
            "\"value\"",
            "// note",
        ),
        (
            "rust",
            "let x = \"value\"; // note",
            "let",
            "\"value\"",
            "// note",
        ),
    ] {
        let runs = source::tokens(text, lang);
        let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, joined);
        let kind = |t: &str| runs.iter().find(|r| r.text == t).map(|r| r.kind);
        assert_eq!(Some("keyword"), kind(keyword), "{lang}");
        assert_eq!(Some("string"), kind(string), "{lang}");
        assert_eq!(Some("comment"), kind(comment), "{lang}");
    }
}

#[test]
fn keeps_chinese_identifiers_in_whole_utf8_runs() {
    let runs = source::tokens("local 变量 = 1", "lua");
    assert!(runs.iter().any(|r| r.text == "变量"));
}

#[test]
fn maps_tab_expanded_drag_boundaries_back_to_source_bytes() {
    let display = source::display("\tfunction 变量()");
    assert_eq!("  function 变量()", display.text);
    let rows = vec!["\tfunction 变量()".to_string(), "\treturn 变量".to_string()];
    let a = Pos {
        row: 0,
        col: display.to_source[2],
    };
    let b = Pos {
        row: 0,
        col: display.to_source[10],
    };
    assert_eq!("function", source::selected(&rows, Some(b), Some(a)));
    let second = source::display(&rows[1]);
    assert_eq!(
        "function 变量()\n\treturn",
        source::selected(
            &rows,
            Some(Pos {
                row: 1,
                col: second.to_source[8]
            }),
            Some(a)
        )
    );
    assert_eq!(2, display.to_display[1]);
}

#[test]
fn copies_a_backwards_selection_across_rows_without_the_gutter() {
    let rows = vec!["local 中文 = 1".to_string(), "return 中文".to_string()];
    assert_eq!(
        "中文 = 1\nreturn",
        source::selected(
            &rows,
            Some(Pos { row: 1, col: 6 }),
            Some(Pos { row: 0, col: 6 })
        )
    );
    assert_eq!(
        "中文",
        source::selected(
            &rows,
            Some(Pos { row: 0, col: 6 }),
            Some(Pos { row: 0, col: 12 })
        )
    );
    assert_eq!(
        "",
        source::selected(&rows, None, Some(Pos { row: 0, col: 0 }))
    );
}

#[test]
fn splits_a_body_into_numbered_rows_with_the_target_highlighted() {
    let rows = source::rows("a\r\nb\nc\n", Some(2));
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        vec!["a", "b", "c", ""],
        texts,
        "trailing newline keeps an empty last row"
    );
    assert_eq!(
        vec![false, true, false, false],
        rows.iter().map(|r| r.marker.is_some()).collect::<Vec<_>>()
    );
    let marker = rows[1].marker.unwrap();
    assert_eq!(255, theme::rgba(marker.bar).a);
    assert!(
        theme::rgba(marker.wash).a < 128,
        "the row wash stays translucent"
    );
    assert!(contrast(theme::rgb(marker.bar), theme::SOURCE_GUTTER_BG) >= 3.0);
    let background = composite(theme::SOURCE_BG, viewer::color::color(marker.wash));
    for ink in [
        theme::SOURCE_INK,
        theme::SOURCE_KEYWORD,
        theme::SOURCE_STRING,
    ] {
        assert!(contrast(ink, background) >= 4.5, "{ink:?} on target row");
    }
    assert!(contrast(theme::SOURCE_COMMENT, background) >= 3.0);
}

#[test]
fn opening_a_member_of_an_open_file_moves_its_window_to_that_member() {
    let root = std::env::temp_dir().join(format!("uml-source-reuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("a.lua"),
        "function pick() end\n\nfunction drop() end\n",
    )
    .unwrap();
    std::fs::write(root.join("b.lua"), "function other() end\n").unwrap();
    let mut gui = viewer::gui::Gui::default();
    gui.open_source_file("lua", "a.lua", Some("pick"), &root)
        .unwrap();
    gui.open_source_file("lua", "b.lua", Some("other"), &root)
        .unwrap();
    gui.open_source_file("lua", "a.lua", Some("drop"), &root)
        .unwrap();

    let files: Vec<_> = gui.sources.iter().map(|w| w.file.as_str()).collect();
    assert_eq!(2, files.len(), "one window per file: {files:?}");
    let newest = gui.sources.last().unwrap();
    assert!(newest.file.ends_with("a.lua"));
    assert_eq!(Some(3), newest.line);
    assert_eq!("a.lua:3", newest.title);
    let marked: Vec<_> = newest
        .rows
        .iter()
        .filter(|r| r.marker.is_some())
        .map(|r| r.number)
        .collect();
    assert_eq!(vec![3], marked);
}
