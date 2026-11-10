//! Scene frame goldens (#16): `uml-viewer scene --frame` on the shop
//! fixture, byte for byte. The goldens under `fixtures/frames/` are the
//! frame contract the viewer panel consumes (`docs/scene-frame.md`); a
//! deliberate change regenerates them with `UPDATE_FRAME_GOLDENS=1` and the
//! diff is reviewed like the schema change it is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Run the `scene` role in process and return the frame it wrote.
fn frame(args: &[&str]) -> String {
    let shop = fixtures().join("shop");
    let out = std::env::temp_dir().join(format!(
        "uml-frame-{}-{}.json",
        std::process::id(),
        args.join("_")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
    ));
    let mut argv: Vec<OsString> = vec![
        "--frame".into(),
        "--ir".into(),
        shop.join("shop.ir.edn").into(),
        "--metrics-root".into(),
        shop.clone().into(),
        "--output".into(),
        out.clone().into(),
    ];
    argv.extend(args.iter().map(OsString::from));
    assert_eq!(ExitCode::SUCCESS, frame::run_scene(&argv), "{args:?}");
    let text = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    text
}

/// The golden of one view in each language the resident lays text out in
/// (#44): Chinese is `{name}.frame.json`, English `{name}.en.frame.json`.
fn golden(name: &str, args: &[&str]) {
    for (lang, file) in [
        ("zh", format!("{name}.frame.json")),
        ("en", format!("{name}.en.frame.json")),
    ] {
        let mut argv = vec!["--lang", lang];
        argv.extend_from_slice(args);
        compare(&file, &frame(&argv));
    }
}

fn compare(file: &str, actual: &str) {
    let path = fixtures().join("frames").join(file);
    if std::env::var_os("UPDATE_FRAME_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "{file}: frame differs from {}; rerun with UPDATE_FRAME_GOLDENS=1 and review the diff",
        path.display()
    );
}

fn golden_text(file: &str) -> String {
    std::fs::read_to_string(fixtures().join("frames").join(file))
        .unwrap()
        .replace("\r\n", "\n")
}

/// An unmeasured class box says 无数据 in Chinese and "No data" in English.
#[test]
fn the_no_data_marker_follows_the_language() {
    let marker = |file: &str| {
        let frame: serde_json::Value = serde_json::from_str(&golden_text(file)).unwrap();
        let texts: Vec<String> = frame["world"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "no-data")
            .map(|item| item["text"].as_str().unwrap().to_owned())
            .collect();
        assert!(!texts.is_empty(), "{file} has no no-data marker");
        texts
    };
    assert!(marker("root.frame.json").iter().all(|t| t == "无数据"));
    assert!(marker("root.en.frame.json").iter().all(|t| t == "No data"));
}

/// Only Chinese and English are shipped: any other language, and none, is English.
#[test]
fn an_unsupported_or_missing_language_falls_back_to_english() {
    let english = golden_text("root.en.frame.json");
    assert_eq!(english, frame(&["--lang", "fr"]));
    assert_eq!(english, frame(&["--lang", "ja-JP"]));
    assert_eq!(english, frame(&[]));
    assert_eq!(golden_text("root.frame.json"), frame(&["--lang", "zh-CN"]));
}

#[test]
fn root_view_fitted_to_the_default_canvas() {
    golden("root", &[]);
}

#[test]
fn a_selected_class_lights_its_dependency_chain_and_dims_the_rest() {
    golden(
        "core-selected",
        &[
            "--focus",
            "[:core]",
            "--select",
            "{:kind :class :id :core.order}",
        ],
    );
}

#[test]
fn a_hovered_class_warms_its_edges_and_names_itself_at_overview_zoom() {
    golden(
        "root-hover-frames",
        &[
            "--zoom",
            "0.3",
            "--camera",
            "-200,-100",
            "--hover",
            "{:kind :class :id :core}",
            "--pointer",
            "300,240",
        ],
    );
}

#[test]
fn decluttered_triangles_at_block_zoom() {
    golden(
        "core-triangles-blocks",
        &[
            "--focus",
            "[:core]",
            "--extra",
            "{:declutter :triangles}",
            "--zoom",
            "0.12",
            "--camera",
            "0,0",
        ],
    );
}

/// 移除箭头 at reading zoom (dsh-uml-viewer#25): compact dependency
/// indicators instead of edges, violating ones in 违规关系色; selecting
/// `:api` dims the unrelated violating and plain indicators alike.
#[test]
fn removed_arrows_show_violating_and_dimmed_dependency_indicators() {
    golden(
        "root-triangles-selected",
        &[
            "--extra",
            "{:declutter :triangles}",
            "--select",
            "{:kind :class :id :api}",
        ],
    );
}

/// The panel primitives at a reading zoom under 1 (dsh-uml-viewer#24): a
/// selected (marked) dependency port, its sibling ports and arrows, α/I,
/// level numbers and stereotypes, all scaled and rounded like the window.
#[test]
fn a_selected_port_marks_itself_at_reading_zoom() {
    golden(
        "core-port-selected",
        &[
            "--focus",
            "[:core]",
            "--select",
            "{:kind :port :id :api :parent :core.order :dir :in}",
            "--zoom",
            "0.6",
            "--camera",
            "0,40",
        ],
    );
}

#[test]
fn a_proposal_view_draws_the_same_way_without_a_banner() {
    golden(
        "proposal",
        &["--extra", "{:proposal true :proposal-id :p-shop}"],
    );
}

/// The legend is typeset in Rust (#50, ADR 0015): the rows the panel draws are
/// in the frame, in the language the resident was told, so each language has
/// its own set. The goldens are the fixture for both.
#[test]
fn the_legend_rows_are_typeset_in_the_residents_language() {
    let legend = |file: &str| -> serde_json::Value {
        let frame: serde_json::Value = serde_json::from_str(&golden_text(file)).unwrap();
        frame["legend"].clone()
    };
    let rows = |legend: &serde_json::Value| -> Vec<(String, String, String)> {
        legend["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|section| {
                section["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| {
                        (
                            row["id"].as_str().unwrap().to_owned(),
                            row["label"].as_str().unwrap().to_owned(),
                            section["id"].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let zh = legend("root.frame.json");
    let en = legend("root.en.frame.json");
    let (zh_rows, en_rows) = (rows(&zh), rows(&en));
    // The same legend, the same row identities, word for word in each language.
    assert_eq!(
        zh_rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
        en_rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
        "both languages carry the same rows"
    );
    assert_eq!(
        zh_rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>(),
        en_rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>(),
        "both languages group the rows the same way"
    );
    let cjk = |text: &str| text.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c));
    assert!(
        zh_rows.iter().all(|r| cjk(&r.1)),
        "every Chinese row is Chinese: {zh_rows:?}"
    );
    assert!(
        en_rows.iter().all(|r| !cjk(&r.1)),
        "every English row is English: {en_rows:?}"
    );
    // The semantic vocabulary of the legend: boxes, relations, the quality
    // ramp, the chain highlight and no-data.
    let ids: Vec<&str> = zh_rows.iter().map(|r| r.0.as_str()).collect();
    for expected in [
        "class-box",
        "component-box",
        "foreign-package",
        "quality-ramp",
        "no-data",
        "selected",
        "relation",
        "violation",
        "relation-hover",
        "dependency-chain",
        "dimmed",
    ] {
        assert!(
            ids.contains(&expected),
            "the legend has {expected}: {ids:?}"
        );
    }
}

/// The legend's swatches name the roles the diagram itself is drawn with, so
/// the panel resolves both through one theme and the colors cannot drift
/// (#50). A role the legend names but no item uses would be its own color.
#[test]
fn the_legend_samples_use_the_roles_the_diagram_draws() {
    fn roles(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(role) = map.get("role").and_then(|r| r.as_str()) {
                    out.push(role.to_owned());
                }
                for (key, item) in map {
                    if key != "role" {
                        roles(item, out);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    roles(item, out);
                }
            }
            _ => {}
        }
    }
    let mut legend_roles = Vec::new();
    let mut drawn_roles = Vec::new();
    for file in [
        "root.frame.json",
        "root.en.frame.json",
        "root-triangles-selected.frame.json",
        "root-hover-frames.frame.json",
        "core-selected.frame.json",
    ] {
        let frame: serde_json::Value = serde_json::from_str(&golden_text(file)).unwrap();
        roles(&frame["legend"], &mut legend_roles);
        roles(&frame["world"], &mut drawn_roles);
        roles(&frame["edges"], &mut drawn_roles);
    }
    legend_roles.sort();
    legend_roles.dedup();
    assert!(!legend_roles.is_empty(), "the legend has samples");
    for role in &legend_roles {
        assert!(
            drawn_roles.contains(role),
            "the legend names {role}, which the diagram also draws: {drawn_roles:?}"
        );
    }
    // The quality ramp and the chain highlight are the roles the diagram uses
    // for its quality colors and its dependency chain.
    for expected in [
        "quality-fill",
        "quality-stroke",
        "no-data",
        "relation",
        "dependency-chain",
    ] {
        assert!(
            legend_roles.iter().any(|role| role == expected),
            "the legend explains {expected}: {legend_roles:?}"
        );
    }
}

/// Chrome left the frame (#46, ADR 0015): the title, the proposal banner, the
/// return line, the status line and the centred hint are the panel's DOM now
/// (breadcrumbs, status bar). A view has no chrome at all, and the pointer's
/// hover popup is the only chrome kind left.
#[test]
fn the_frame_carries_no_canvas_chrome() {
    let chrome = |text: &str| -> Vec<serde_json::Value> {
        let frame: serde_json::Value = serde_json::from_str(text).unwrap();
        frame["chrome"].as_array().unwrap().clone()
    };
    for text in [
        frame(&["--lang", "zh"]),
        frame(&["--focus", "[:core]", "--lang", "zh"]),
        frame(&[
            "--extra",
            "{:proposal true :proposal-id :p-shop}",
            "--lang",
            "zh",
        ]),
    ] {
        assert_eq!(
            Vec::<serde_json::Value>::new(),
            chrome(&text),
            "a view has no canvas chrome"
        );
    }
    let hovered = frame(&[
        "--zoom",
        "0.3",
        "--camera",
        "-200,-100",
        "--hover",
        "{:kind :class :id :core}",
        "--pointer",
        "300,240",
        "--lang",
        "zh",
    ]);
    let kinds: Vec<String> = chrome(&hovered)
        .iter()
        .map(|item| item["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        vec!["popup".to_owned()],
        kinds,
        "only the hover popup is chrome"
    );
}
