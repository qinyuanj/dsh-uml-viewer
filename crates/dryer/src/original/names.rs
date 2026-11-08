//! `dryer/tests/test_names.py` at pinned b3f72ad, ported case by case.

use super::{scratch, write_source};
use crate::names::owner;
use crate::report;
use crate::scan::{Candidate, Span, duplicates as find_duplicates, scan_files};
use quality_core::functions::Function;
use quality_core::language::Language;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

/// `(file, namespace, name)` of one candidate side.
type Side = (String, Option<String>, Option<String>);

fn sides(root: &Path, threshold: f64, min_lines: usize) -> Vec<Vec<Side>> {
    let files = super::all_files(root);
    let (entries, warnings) = scan_files(&files, root, min_lines, 1).unwrap();
    assert_eq!(Vec::<String>::new(), warnings);
    find_duplicates(entries, threshold)
        .iter()
        .map(|item| {
            vec![
                (
                    item.left.file.clone(),
                    item.left.namespace.clone(),
                    item.left.name.clone(),
                ),
                (
                    item.right.file.clone(),
                    item.right.namespace.clone(),
                    item.right.name.clone(),
                ),
            ]
        })
        .collect()
}

fn function(name: &str, start: usize, end: usize) -> Function {
    Function::new(
        name.to_string(),
        "ns".to_string(),
        1,
        start,
        end,
        "a.py".to_string(),
        Language::Python,
    )
}
fn owner_name(functions: &[Function], start: usize, end: usize) -> Option<&str> {
    owner(functions, start, end).map(|function| function.name.as_str())
}
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn python_sides_carry_crappers_namespace_and_name() {
    let root = scratch("names-python");
    write_source(
        &root,
        "src/shop/cart.py",
        "class Cart:\n    def total(self, xs):\n        ys = filter(xs, odd)\n        return map(ys, inc)\n",
    );
    write_source(
        &root,
        "src/shop/order.py",
        "def amount(items):\n    kept = filter(items, even)\n    return map(kept, dec)\n",
    );
    assert_eq!(
        sides(&root, 0.5, 1),
        vec![vec![
            (
                "src/shop/cart.py".to_string(),
                Some("shop.cart.Cart".to_string()),
                Some("total".to_string())
            ),
            (
                "src/shop/order.py".to_string(),
                Some("shop.order".to_string()),
                Some("amount".to_string())
            ),
        ]]
    );
}

#[test]
fn lua_sides_carry_the_require_path() {
    let root = scratch("names-lua");
    let project = root.join("lua_project");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dry/lua_project"),
        &project,
    );
    let files = super::all_files(&project.join("src"));
    let (entries, _warnings) = scan_files(&files, &project, 4, 20).unwrap();
    let found = find_duplicates(entries, 0.82);
    assert_eq!(
        found
            .iter()
            .map(|item| (
                item.left.namespace.clone(),
                item.left.name.clone(),
                item.right.namespace.clone(),
                item.right.name.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![(
            Some("calc".to_string()),
            Some("M.total".to_string()),
            Some("calc.stats".to_string()),
            Some("M.sum_clamped".to_string()),
        )]
    );
}

#[test]
fn clojure_defns_are_named_and_other_forms_are_not() {
    let root = scratch("names-clojure");
    let body = "  (let [ys (filter odd? xs)]\n    (map inc ys)))\n";
    write_source(
        &root,
        "src/app/left.clj",
        &format!("(ns app.left)\n\n(defn alpha [xs]\n{body}"),
    );
    write_source(
        &root,
        "src/app/right.cljd",
        &format!(
            "(ns app.right)\n\n(def beta (fn [xs]\n{})\n",
            &body[..body.len() - 1]
        ),
    );
    assert_eq!(
        sides(&root, 0.5, 1),
        vec![vec![
            (
                "src/app/left.clj".to_string(),
                Some("app.left".to_string()),
                Some("alpha".to_string())
            ),
            ("src/app/right.cljd".to_string(), None, None),
        ]]
    );
}

#[test]
fn the_function_starting_on_the_form_line_wins() {
    let outer = function("outer", 1, 20);
    let inner = function("inner", 5, 9);
    let other = function("other", 5, 30);
    assert_eq!(
        owner_name(&[outer.clone(), other.clone(), inner.clone()], 5, 9),
        Some("inner")
    );
    assert_eq!(
        owner_name(&[outer.clone(), other.clone()], 5, 9),
        Some("other")
    );
    assert_eq!(owner_name(&[inner, outer], 5, 9), Some("inner"));
}

#[test]
fn otherwise_the_smallest_function_around_the_form_wins() {
    let outer = function("outer", 1, 20);
    let middle = function("middle", 3, 12);
    assert_eq!(
        owner_name(&[outer.clone(), middle.clone()], 4, 12),
        Some("middle")
    );
    assert_eq!(
        owner_name(&[middle.clone(), outer.clone()], 4, 12),
        Some("middle")
    );
    assert_eq!(
        owner_name(&[outer.clone(), middle.clone()], 2, 13),
        Some("outer")
    );
    assert_eq!(owner_name(&[outer.clone(), middle.clone()], 4, 21), None);
    assert_eq!(owner_name(&[outer, middle], 21, 22), None);
    assert_eq!(owner_name(&[], 1, 1), None);
}

#[test]
fn size_is_the_number_of_lines_spanned() {
    let early = function("early", 1, 12);
    let late = function("late", 6, 15);
    assert_eq!(owner_name(&[early, late], 7, 11), Some("late"));
}

#[test]
fn equal_sizes_prefer_the_earlier_function() {
    let first = function("first", 2, 6);
    let second = function("second", 3, 7);
    assert_eq!(owner_name(&[second, first], 4, 6), Some("first"));
}

#[test]
fn edn_sides_end_with_namespace_and_name() {
    let candidate = Candidate {
        score: 0.875,
        language: Language::Python,
        left: Span {
            file: "a.py".to_string(),
            start: 10,
            end: 14,
            namespace: Some("pkg.a".to_string()),
            name: Some("say \"hi\"".to_string()),
        },
        right: Span {
            file: "b.py".to_string(),
            start: 20,
            end: 24,
            namespace: None,
            name: None,
        },
        left_nodes: 30,
        right_nodes: 31,
    };
    assert_eq!(
        report::edn(&[candidate]),
        "{:candidates [\n {:score 0.875\n  :language \"python\"\n  :left {:file \"a.py\", :start-line 10, :end-line 14, :namespace \"pkg.a\", :name \"say \\\"hi\\\"\"}\n  :right {:file \"b.py\", :start-line 20, :end-line 24, :namespace nil, :name nil}\n  :left-nodes 30\n  :right-nodes 31}\n]}\n"
    );
}

/// `test_the_snapshot_is_replaced_in_one_step`: the observable half — the
/// published path, its bytes, no staged leftovers and the file mode. Python's
/// `os.replace` spy has no equivalent here; the replacement itself is
/// `quality_core::atomic::write_bytes`.
#[test]
fn the_snapshot_is_replaced_in_one_step() {
    let root = scratch("names-publish");
    let path = report::write_metrics(&[], &root).unwrap();
    assert_eq!(path, root.join(".metrics").join("dry.edn"));
    assert_eq!(report::write_metrics(&[], &root).unwrap(), path);
    assert_eq!(fs::read(&path).unwrap(), b"{:candidates []}\n");
    let entries: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["dry.edn".to_string()]);
    assert!(!fs::metadata(&path).unwrap().permissions().readonly());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}

/// `test_a_failed_write_keeps_the_previous_snapshot`. A read-only directory
/// refuses the staged write on POSIX; Windows does not honour that bit, so
/// the refusal is exercised on POSIX only (loudly, not silently).
#[cfg(unix)]
#[test]
fn a_failed_write_keeps_the_previous_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("names-failed-write");
    let path = root.join(".metrics").join("dry.edn");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "previous\n").unwrap();
    let directory = path.parent().unwrap();
    let original = fs::metadata(directory).unwrap().permissions();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o555)).unwrap();
    let result = report::write_metrics(&[], &root);
    fs::set_permissions(directory, original).unwrap();
    assert!(result.is_err(), "a read-only metrics directory must fail");
    assert_eq!(fs::read(&path).unwrap(), b"previous\n");
    let entries: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries, vec![path]);
}
