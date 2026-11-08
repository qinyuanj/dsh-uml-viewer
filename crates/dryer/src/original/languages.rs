//! `dryer/tests/test_languages.py` at pinned b3f72ad, ported case by case.

use super::{forms, pairs, printed_fingerprints, scratch, spans, write_source};
use crate::extract::Entry;
use crate::normalize::normalized;
use crate::scan::{duplicates as find_duplicates, keep};
use quality_core::grammar::{Grammar, children, end_line, preorder};
use quality_core::language::Language;
use std::collections::HashSet;

const JAVA_LEFT: &str = "class Left {\n  int alpha(int[] xs) {\n    int[] ys = filter(xs, odd);\n    return map(ys, inc);\n  }\n}\n";
const JAVA_RIGHT: &str = "class Right {\n  int beta(int[] items) {\n    int[] kept = filter(items, even);\n    return map(kept, dec);\n  }\n}\n";
const GO_LEFT: &str = "package demo\n\nfunc (b *Board) alpha(xs []int) []int {\n\tys := filter(xs, odd)\n\treturn mapFn(ys, inc)\n}\n";
const GO_RIGHT: &str = "package demo\n\nfunc (p *Panel) beta(items []int) []int {\n\tkept := filter(items, even)\n\treturn mapFn(kept, dec)\n}\n";
const PY_LEFT: &str = "class Board:\n    def alpha(self, xs):\n        ys = filter(xs, odd)\n        return map(ys, inc)\n";
const PY_RIGHT: &str = "class Panel:\n    def beta(self, items):\n        kept = filter(items, even)\n        return map(kept, dec)\n";
const TS_LEFT: &str = "export function alpha(xs: number[]): number[] {\n  const ys = filter(xs, odd);\n  return map(ys, inc);\n}\n";
const TS_RIGHT: &str = "export function beta(items: number[]): number[] {\n  const kept = filter(items, even);\n  return map(kept, dec);\n}\n";
const TSX_LEFT: &str = "export const alpha = (xs: number[]): number[] => {\n  const ys = filter(xs, odd);\n  return map(ys, inc);\n};\n";
const TSX_RIGHT: &str = "export const beta = (items: number[]): number[] => {\n  const kept = filter(items, even);\n  return map(kept, dec);\n};\n";
const RUST_LEFT: &str = "pub fn alpha(xs: Vec<i32>) -> Vec<i32> {\n    let ys = filter(xs, odd);\n    map(ys, inc)\n}\n";
const RUST_RIGHT: &str = "pub fn beta(items: Vec<i32>) -> Vec<i32> {\n    let kept = filter(items, even);\n    map(kept, dec)\n}\n";
const LUA_LEFT: &str =
    "local function alpha(xs)\n  local ys = filter(xs, odd)\n  return map(ys, inc)\nend\n";
const LUA_RIGHT: &str = "local function beta(items)\n  local kept = filter(items, even)\n  return map(kept, dec)\nend\n";

fn suffix(language: Language) -> &'static str {
    match language {
        Language::Python => "py",
        Language::Go => "go",
        Language::Rust => "rs",
        Language::Java => "java",
        Language::TypeScript => "ts",
        Language::Lua => "lua",
        Language::Clojure => "clj",
    }
}

#[test]
fn each_language_matches_renamed_locals() {
    let root = scratch("languages-renamed");
    for (name, source) in [
        ("left.java", JAVA_LEFT),
        ("right.java", JAVA_RIGHT),
        ("left.go", GO_LEFT),
        ("right.go", GO_RIGHT),
        ("left.py", PY_LEFT),
        ("right.py", PY_RIGHT),
        ("left.ts", TS_LEFT),
        ("right.ts", TS_RIGHT),
        ("left.tsx", TSX_LEFT),
        ("right.tsx", TSX_RIGHT),
        ("left.rs", RUST_LEFT),
        ("right.rs", RUST_RIGHT),
        ("left.lua", LUA_LEFT),
        ("right.lua", LUA_RIGHT),
    ] {
        write_source(&root, name, source);
    }
    let found = pairs(&root);
    let languages: Vec<Language> = found.iter().map(|item| item.language).collect();
    let mut unique = languages.clone();
    unique.sort_by_key(|language| language.name());
    unique.dedup();
    assert_eq!(unique.len(), 6);
    let typescript: Vec<(&str, &str)> = found
        .iter()
        .filter(|item| item.language == Language::TypeScript)
        .map(|item| (item.left.file.as_str(), item.right.file.as_str()))
        .collect();
    assert_eq!(
        typescript,
        vec![("left.ts", "right.ts"), ("left.tsx", "right.tsx")]
    );
    for language in unique {
        assert!(
            found
                .iter()
                .filter(|item| item.language == language)
                .all(|item| item.score == 1.0),
            "{}",
            language.name()
        );
    }
}

#[test]
fn different_callee_is_not_the_same_structure() {
    let root = scratch("languages-callee");
    write_source(
        &root,
        "a.py",
        "def alpha(xs):\n    return filter(xs, odd)\n",
    );
    write_source(&root, "b.py", "def beta(xs):\n    return select(xs, odd)\n");
    let found = find_duplicates(super::scan(&root, 1, 1), 0.0);
    assert_eq!(found.len(), 1);
    assert!(found[0].score < 1.0);
}

#[test]
fn nested_functions_stay_inside_the_enclosing_function() {
    let root = scratch("languages-nested");
    write_source(
        &root,
        "a.py",
        "def alpha(xs):\n    def inner(ys):\n        return filter(ys, odd)\n    return inner(xs)\n",
    );
    write_source(
        &root,
        "b.py",
        "def beta(items):\n    def inner(kept):\n        return filter(kept, even)\n    return inner(items)\n",
    );
    let found = pairs(&root);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
    assert_eq!(found[0].left.start, 1);
}

#[test]
fn rust_tests_module_is_not_production_code() {
    let root = scratch("languages-rust-tests");
    write_source(
        &root,
        "a.rs",
        "pub fn alpha(xs: Vec<i32>) -> Vec<i32> {\n    let ys = filter(xs, odd);\n    map(ys, inc)\n}\n\nmod tests {\n    pub fn helper(xs: Vec<i32>) -> Vec<i32> {\n        let ys = filter(xs, odd);\n        map(ys, inc)\n    }\n}\n",
    );
    write_source(&root, "b.rs", RUST_RIGHT);
    let found = pairs(&root);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].left.file, "a.rs");
    assert_eq!(found[0].left.start, 1);
}

#[test]
fn an_extra_statement_lowers_the_score() {
    let root = scratch("languages-extra");
    write_source(&root, "a.java", JAVA_LEFT);
    write_source(
        &root,
        "b.java",
        "class Right {\n  int beta(int[] items) {\n    int[] kept = filter(items, even);\n    int[] extra = filter(kept, even);\n    return map(extra, dec);\n  }\n}\n",
    );
    let found = find_duplicates(super::scan(&root, 1, 1), 0.5);
    assert_eq!(found.len(), 1);
    assert!(found[0].score < 1.0);
}

#[test]
fn callee_names_stay_and_member_names_do_not() {
    let root = scratch("languages-callees");
    for (name, source) in [
        (
            "left.py",
            "def alpha(xs):\n    ys = xs.filter(odd)\n    return (len)(xs.total)\n",
        ),
        (
            "right.py",
            "def beta(items):\n    kept = items.filter(even)\n    return (len)(items.count)\n",
        ),
        (
            "left.go",
            "package demo\n\nfunc alpha(xs []int) {\n\tfmt.Println(xs)\n}\n",
        ),
        (
            "right.go",
            "package demo\n\nfunc beta(items []int) {\n\tlog.Println(items)\n}\n",
        ),
        (
            "left.rs",
            "pub fn alpha(xs: Vec<i32>) -> Vec<i32> {\n    let ys = Vec::new();\n    let zs = HashMap::<String, i32>::new();\n    xs.iter()\n}\n",
        ),
        (
            "right.rs",
            "pub fn beta(items: Vec<i32>) -> Vec<i32> {\n    let kept = Vec::new();\n    let rows = HashMap::<Integer, i32>::new();\n    items.iter()\n}\n",
        ),
        (
            "left.java",
            "class Left {\n  int alpha(List xs) {\n    List ys = new ArrayList<String>();\n    ys.add(xs);\n    return xs.total;\n  }\n}\n",
        ),
        (
            "right.java",
            "class Right {\n  int beta(List items) {\n    List kept = new ArrayList<Integer>();\n    kept.add(items);\n    return items.count;\n  }\n}\n",
        ),
        (
            "left.ts",
            "export function alpha(xs: number[]): number {\n  return foo<string>(xs.bar);\n}\n",
        ),
        (
            "right.ts",
            "export function beta(items: number[]): number {\n  return foo<number>(items.baz);\n}\n",
        ),
        (
            "left.lua",
            "function M.alpha(xs)\n  xs:push(xs.total)\n  return string.upper(xs.total)\nend\n",
        ),
        (
            "right.lua",
            "function M.beta(items)\n  items:push(items.count)\n  return string.upper(items.count)\nend\n",
        ),
    ] {
        write_source(&root, name, source);
    }
    let found = pairs(&root);
    let mut languages: Vec<Language> = found.iter().map(|item| item.language).collect();
    languages.sort_by_key(|language| language.name());
    languages.dedup();
    assert_eq!(languages.len(), 6);
    for language in languages {
        let pairs: Vec<(String, String, f64)> = found
            .iter()
            .filter(|item| item.language == language)
            .map(|item| (item.left.file.clone(), item.right.file.clone(), item.score))
            .collect();
        assert_eq!(
            pairs,
            vec![(
                format!("left.{}", suffix(language)),
                format!("right.{}", suffix(language)),
                1.0
            )],
            "{}",
            language.name()
        );
    }
}

#[test]
fn a_different_callee_name_lowers_the_score() {
    let root = scratch("languages-different-callee");
    write_source(&root, "a.py", "def alpha(xs):\n    return xs.filter(odd)\n");
    write_source(
        &root,
        "b.py",
        "def beta(items):\n    return items.select(even)\n",
    );
    write_source(&root, "a.rs", "fn alpha() {\n    Vec::new()\n}\n");
    write_source(&root, "b.rs", "fn beta() {\n    Vec::with_capacity(1)\n}\n");
    let found = find_duplicates(super::scan(&root, 1, 1), 0.0);
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|item| item.score < 1.0));
}

#[test]
fn a_parenthesized_callee_matches_the_bare_call() {
    let wrapped = forms(
        Language::Python,
        "def alpha(xs):\n    return (len)(xs)\n",
        "a.py",
    );
    let bare = forms(
        Language::Python,
        "def beta(items):\n    return len(items)\n",
        "b.py",
    );
    assert!(
        wrapped[0]
            .fingerprints
            .iter()
            .any(|item| item.contains("len"))
    );
    let found = find_duplicates([wrapped, bare].concat(), 0.99);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
}

#[test]
fn only_parentheses_are_unwrapped() {
    let tree = Grammar::Python.parse("def alpha(xs):\n    return (len)(xs)\n");
    let source = "def alpha(xs):\n    return (len)(xs)\n";
    let printed = normalized(
        children(tree.root_node())[0],
        source,
        Some(Language::Python),
    )
    .unwrap()
    .printed();
    assert_eq!(
        printed,
        "[:function_definition :symbol [:parameters :symbol] [:block [:return_statement [:call [:symbol \"len\"] [:argument_list :symbol]]]]]"
    );
}

#[test]
fn a_string_and_a_number_normalize_to_the_same_literal() {
    let string = forms(
        Language::Python,
        "def alpha(xs):\n    return \"xs\"\n",
        "a.py",
    );
    let number = forms(Language::Python, "def beta(items):\n    return 1\n", "b.py");
    assert!(number[0].fingerprints.iter().any(|item| item == ":literal"));
    let found = find_duplicates([string, number].concat(), 0.99);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
}

#[test]
fn an_interpolated_string_is_not_a_plain_literal() {
    let plain = forms(
        Language::Python,
        "def alpha(xs):\n    return \"xs\"\n",
        "a.py",
    );
    let interpolated = forms(
        Language::Python,
        "def beta(items):\n    return f\"{items}\"\n",
        "b.py",
    );
    let found = find_duplicates([plain, interpolated].concat(), 0.0);
    assert_eq!(found.len(), 1);
    assert!(found[0].score < 1.0);
}

#[test]
fn constructed_names_stay_in_the_fingerprint() {
    let rust = forms(
        Language::Rust,
        "pub fn alpha() {\n    let ys = Vec::new();\n    let zs = HashMap::<String, i32>::new();\n}\n",
        "a.rs",
    );
    let java = forms(
        Language::Java,
        "class Left {\n  int alpha() {\n    return new ArrayList<String>();\n  }\n}\n",
        "a.java",
    );
    let rust_names = printed_fingerprints(&rust);
    let java_names = printed_fingerprints(&java);
    assert!(rust_names.contains("Vec"));
    assert!(rust_names.contains("new"));
    assert!(rust_names.contains("HashMap"));
    assert!(java_names.contains("ArrayList"));
}

#[test]
fn a_class_contributes_its_method_only() {
    assert_eq!(
        spans(
            Language::Python,
            "class Board:\n    def alpha(self, xs):\n        return xs\n",
            "a.py"
        ),
        vec![(2, 3)]
    );
}

#[test]
fn java_keeps_the_method_that_has_its_own_body() {
    assert_eq!(
        spans(
            Language::Java,
            "abstract class Left {\n  abstract int declared(int[] xs);\n  int alpha(int[] xs) {\n    class Inner {\n      int run(int[] ys) {\n        return ys.length;\n      }\n    }\n    return xs.length;\n  }\n}\n",
            "a.java"
        ),
        vec![(3, 10)]
    );
}

#[test]
fn a_go_function_needs_a_body() {
    assert_eq!(
        spans(
            Language::Go,
            "package demo\n\nfunc declared(xs []int)\n\nfunc alpha(xs []int) int {\n\treturn len(xs)\n}\n",
            "a.go"
        ),
        vec![(5, 7)]
    );
}

#[test]
fn a_closure_in_a_let_stays_inside_the_function() {
    assert_eq!(
        spans(
            Language::Rust,
            "fn alpha() {\n    let f = || { let x = 1; };\n}\n",
            "a.rs"
        ),
        vec![(1, 3)]
    );
}

#[test]
fn a_method_nested_in_a_function_is_not_its_own_candidate() {
    assert_eq!(
        spans(
            Language::TypeScript,
            "export function outer(xs: number[]): number[] {\n  class Hidden {\n    run(ys: number[]): number[] {\n      return ys;\n    }\n  }\n  return xs;\n}\n",
            "a.ts"
        ),
        vec![(1, 8)]
    );
}

#[test]
fn rust_keeps_helpers_and_skips_tests_and_signatures() {
    assert_eq!(
        spans(
            Language::Rust,
            "fn declared();\n\nfn alpha(xs: Vec<i32>) -> usize {\n    xs.len()\n}\n\nmod helpers {\n    fn beta(xs: Vec<i32>) -> usize {\n        xs.len()\n    }\n}\n\nmod tests {\n    fn gamma(xs: Vec<i32>) -> usize {\n        xs.len()\n    }\n}\n",
            "a.rs"
        ),
        vec![(3, 5), (8, 10)]
    );
}

#[test]
fn typescript_keeps_methods_and_skips_nested_functions() {
    assert_eq!(
        spans(
            Language::TypeScript,
            "export class Box {\n  run(xs: number[]): number[] {\n    return xs;\n  }\n}\nexport function outer(xs: number[]): number[] {\n  function inner(ys: number[]): number[] {\n    return ys;\n  }\n  const cb = (zs: number[]) => zs;\n  return inner(xs);\n}\nxs.map((item: number) => item + 1);\n",
            "a.ts"
        ),
        vec![(2, 4), (6, 12)]
    );
}

#[test]
fn a_private_field_name_is_not_part_of_the_structure() {
    let entries = forms(
        Language::TypeScript,
        "export class Box {\n  value(): number {\n    return this.#total;\n  }\n  run(xs: number[]): number[] {\n    return this.#hidden(xs);\n  }\n}\n",
        "a.ts",
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.start_line, entry.end_line))
            .collect::<Vec<_>>(),
        vec![(2, 4), (5, 7)]
    );
    let printed = printed_fingerprints(&entries);
    assert!(!printed.contains("#total"));
    assert!(!printed.contains("#hidden"));
}

#[test]
fn end_line_uses_the_last_occupied_row() {
    let source = "def alpha(xs):\n    return xs\n";
    let tree = Grammar::Python.parse(source);
    let function = preorder(tree.root_node())
        .into_iter()
        .find(|node| node.kind() == "function_definition")
        .unwrap();
    assert_eq!(end_line(tree.root_node()), 2);
    assert_eq!(end_line(function), 2);
    assert_eq!(spans(Language::Python, source, "a.py"), vec![(1, 2)]);
}

fn entry(
    file: &str,
    start: usize,
    end: usize,
    names: &[&str],
    language: Language,
    nodes: usize,
) -> Entry {
    Entry {
        language,
        file: file.to_string(),
        start_line: start,
        end_line: end,
        nodes,
        fingerprints: names
            .iter()
            .map(|name| name.to_string())
            .collect::<HashSet<_>>(),
        namespace: None,
        name: None,
    }
}

#[test]
fn the_same_span_is_not_a_pair() {
    assert!(
        find_duplicates(
            vec![
                entry("a.py", 1, 4, &["q"], Language::Python, 20),
                entry("a.py", 1, 4, &["q"], Language::Python, 20)
            ],
            0.0
        )
        .is_empty()
    );
    assert_eq!(
        find_duplicates(
            vec![
                entry("a.py", 1, 4, &["q"], Language::Python, 20),
                entry("b.py", 1, 4, &["q"], Language::Python, 20)
            ],
            0.5
        )
        .len(),
        1
    );
    assert_eq!(
        find_duplicates(
            vec![
                entry("a.py", 1, 4, &["q"], Language::Python, 20),
                entry("a.py", 3, 5, &["q"], Language::Python, 20)
            ],
            0.5
        )
        .len(),
        1
    );
    assert_eq!(
        find_duplicates(
            vec![
                entry("a.py", 1, 4, &["q"], Language::Python, 20),
                entry("a.py", 1, 8, &["q"], Language::Python, 20)
            ],
            0.5
        )
        .len(),
        1
    );
}

#[test]
fn a_score_equal_to_the_threshold_is_kept() {
    let left = entry("a.rs", 1, 4, &["x", "y"], Language::Rust, 20);
    let right = entry("b.rs", 1, 4, &["x", "z"], Language::Rust, 20);
    assert_eq!(
        find_duplicates(vec![left.clone(), right.clone()], 1.0 / 3.0).len(),
        1
    );
    assert!(find_duplicates(vec![left, right], 1.0 / 3.0 + 0.01).is_empty());
}

#[test]
fn the_higher_score_is_reported_first() {
    let exact = vec![
        entry("a.py", 1, 4, &["only"], Language::Python, 20),
        entry("b.py", 1, 4, &["only"], Language::Python, 20),
    ];
    let partial = vec![
        entry("a.rs", 1, 4, &["x", "y"], Language::Rust, 20),
        entry("b.rs", 10, 14, &["x", "z"], Language::Rust, 20),
    ];
    let found = find_duplicates([partial, exact].concat(), 0.3);
    assert_eq!(
        found.iter().map(|item| item.score).collect::<Vec<_>>(),
        vec![1.0, 1.0 / 3.0]
    );
}

#[test]
fn keep_uses_the_line_and_node_minimums() {
    assert!(keep(
        &entry("a.py", 1, 4, &["q"], Language::Python, 20),
        4,
        20
    ));
    assert!(!keep(
        &entry("a.py", 1, 3, &["q"], Language::Python, 20),
        4,
        20
    ));
    assert!(!keep(
        &entry("a.py", 1, 4, &["q"], Language::Python, 19),
        4,
        20
    ));
}

fn lua_starts(source: &str) -> Vec<usize> {
    crate::extract::entries_in_source(Language::Lua, source, "a.lua", "a.lua")
        .0
        .iter()
        .map(|entry| entry.start_line)
        .collect()
}

#[test]
fn lua_entries_are_declarations_and_assigned_functions() {
    let source = "local function outer(xs)\n  local function inner(n) return n end\n  table.sort(xs, function(a, b) return a < b end)\n  return inner(1)\nend\nM.bar = function() return 1 end\ntable.sort({}, function(a, b) return a end)\nlocal t = { f = function() end }\n";
    assert_eq!(lua_starts(source), vec![1, 6]);
}

#[test]
fn a_returned_lua_function_is_not_an_entry() {
    let source = "local M = {}\nfor k, v in pairs(M) do end\nreturn function(x) return x end\n";
    assert_eq!(lua_starts(source), Vec::<usize>::new());
}

/// The original compares `repr(normalize(...))`, whose `K` reprs are object
/// addresses; the port compares the printed form (`pr`), the value the
/// fingerprints are actually built from, so the same distinction is asserted
/// without depending on CPython's allocator.
#[test]
fn lua_operators_count_only_in_lua() {
    fn print(language: Option<Language>, source: &str, grammar: Grammar) -> String {
        let tree = grammar.parse(source);
        normalized(tree.root_node(), source, language)
            .unwrap()
            .printed()
    }
    let lua = "return a .. #b ~= c // d\n";
    assert_ne!(
        print(Some(Language::Lua), lua, Grammar::Lua),
        print(None, lua, Grammar::Lua)
    );
    assert!(print(Some(Language::Lua), lua, Grammar::Lua).contains("\"..\""));
    let python = "x = a // b\n";
    assert_eq!(
        print(Some(Language::Python), python, Grammar::Python),
        print(None, python, Grammar::Python)
    );
    let rust = "fn f() { let r = 0..n; }\n";
    assert_eq!(
        print(Some(Language::Rust), rust, Grammar::Rust),
        print(None, rust, Grammar::Rust)
    );
}
