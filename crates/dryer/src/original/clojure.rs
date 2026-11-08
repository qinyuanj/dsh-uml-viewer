//! `dryer/tests/test_clojure.py` at pinned b3f72ad, ported case by case.

use super::{duplicates, scratch, write_source};
use crate::clojure::{Coll, Form, is_candidate_form, max_line, normalize, read_source};
use crate::normalize::Normalized;
use crate::report;
use crate::scan::{Candidate, Span, duplicates as find_duplicates, scan_files};
use quality_core::language::Language;
use std::collections::HashSet;

fn pr(form: &Normalized) -> String {
    form.printed()
}
fn coll(form: &Form) -> &Coll {
    match form {
        Form::Coll(coll) => coll,
        other => panic!("not a collection: {other:?}"),
    }
}
fn name(form: &Form) -> &str {
    match form {
        Form::Sym(name, _) | Form::Kw(name, _) => name,
        other => panic!("not a symbol or keyword: {other:?}"),
    }
}
/// `type(item).__name__` in the original assertions.
fn tag(form: &Form) -> &'static str {
    match form {
        Form::Sym(..) => "Sym",
        Form::Kw(..) => "Kw",
        Form::Lit(..) => "Lit",
        Form::Coll(..) => "Coll",
        Form::Splice(..) => "Splice",
    }
}
fn read(source: &str) -> Vec<Form> {
    let (forms, warning) = read_source(source);
    assert_eq!(None, warning, "{source:?}");
    forms
}
fn warning_of(source: &str) -> Option<String> {
    read_source(source).1
}
fn sym(name: &str) -> Form {
    Form::Sym(name.to_string(), 1)
}
fn collection(kind: &'static str, items: Vec<Form>) -> Form {
    Form::Coll(Coll {
        kind,
        line: 1,
        items,
        pairs: Vec::new(),
    })
}
fn candidate() -> Candidate {
    Candidate {
        score: 0.875,
        language: Language::Clojure,
        left: Span {
            file: "a.clj".to_string(),
            start: 10,
            end: 14,
            namespace: None,
            name: None,
        },
        right: Span {
            file: "b.clj".to_string(),
            start: 20,
            end: 24,
            namespace: None,
            name: None,
        },
        left_nodes: 30,
        right_nodes: 31,
    }
}

#[test]
fn edn_report_matches_the_text_fields() {
    assert_eq!(
        report::edn(&[candidate()]),
        "{:candidates [\n {:score 0.875\n  :language \"clojure\"\n  :left {:file \"a.clj\", :start-line 10, :end-line 14, :namespace nil, :name nil}\n  :right {:file \"b.clj\", :start-line 20, :end-line 24, :namespace nil, :name nil}\n  :left-nodes 30\n  :right-nodes 31}\n]}\n"
    );
}

#[test]
fn text_report_matches_dry4clj() {
    assert_eq!(
        report::text(&[candidate()]),
        "DUPLICATE score=0.88\n  a.clj:10-14\n  b.clj:20-24\n"
    );
    assert_eq!(report::text(&[]), "No duplicate candidates found.\n");
}

#[test]
fn reports_structural_duplicates_with_line_ranges() {
    let root = scratch("clj-ranges");
    write_source(
        root.as_path(),
        "left.cljc",
        "(ns sample.left)\n\n(defn alpha [xs]\n  (let [ys (filter odd? xs)]\n    (map inc ys)))\n",
    );
    write_source(
        root.as_path(),
        "right.cljc",
        "(ns sample.right)\n\n(defn beta [items]\n  (let [kept (filter even? items)]\n    (map dec kept)))\n",
    );
    let found = duplicates(&root, 0.80, 3, 8);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
    assert_eq!(found[0].language, Language::Clojure);
    assert_eq!(found[0].left.file, "left.cljc");
    assert_eq!((found[0].left.start, found[0].left.end), (3, 5));
    assert_eq!(found[0].right.file, "right.cljc");
    assert_eq!((found[0].right.start, found[0].right.end), (3, 5));
}

#[test]
fn matches_maps_sets_and_keyword_calls() {
    let root = scratch("clj-maps");
    write_source(
        root.as_path(),
        "left.cljc",
        "(ns sample.left)\n\n(defn gamma [m]\n  (when (#{:a :b} (:kind m))\n    {:left (:a m) :right (:b m)}))\n",
    );
    write_source(
        root.as_path(),
        "right.cljc",
        "(ns sample.right)\n\n(defn delta [row]\n  (when (#{:c :d} (:kind row))\n    {:left (:c row) :right (:d row)}))\n",
    );
    let found = duplicates(&root, 0.80, 3, 8);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
}

#[test]
fn reads_cljc_reader_conditionals() {
    let root = scratch("clj-conditionals");
    write_source(
        root.as_path(),
        "left.cljc",
        "(ns sample.left)\n\n(defn alpha [x]\n  #?(:clj (when (pos? x)\n            (inc x))))\n",
    );
    write_source(
        root.as_path(),
        "right.cljc",
        "(ns sample.right)\n\n(defn beta [y]\n  #?(:clj (when (pos? y)\n            (inc y))))\n",
    );
    let found = duplicates(&root, 0.50, 1, 1);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
}

#[test]
fn filters_forms_shorter_than_the_minimum_line_count() {
    let root = scratch("clj-min-lines");
    write_source(
        root.as_path(),
        "one.clj",
        "(ns one)\n(defn a [x] (+ x 1))\n",
    );
    write_source(
        root.as_path(),
        "two.clj",
        "(ns two)\n(defn b [y] (+ y 2))\n",
    );
    assert!(duplicates(&root, 0.80, 3, 1).is_empty());
}

#[test]
fn metadata_comments_and_discards_are_not_structure() {
    let root = scratch("clj-metadata");
    write_source(
        root.as_path(),
        "left.clj",
        "(ns left)\n\n(defn alpha [xs] ; keep\n  #_(println xs)\n  (let [ys (filter odd? xs)]\n    (map inc ys)))\n",
    );
    write_source(
        root.as_path(),
        "right.clj",
        "(ns right)\n\n(defn ^String beta [items]\n  (let [kept (filter even? items)]\n    (map dec kept)))\n",
    );
    let found = duplicates(&root, 0.80, 3, 8);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].score, 1.0);
}

#[test]
fn extra_binding_scores_below_one() {
    let root = scratch("clj-bindings");
    write_source(
        &root,
        "invoice.clj",
        "(ns billing.invoice)\n\n(defn invoice-summary [orders]\n  (let [paid (filter paid? orders), domestic (filter domestic? paid)\n        sorted (sort-by :date domestic), amounts (map :amount sorted)\n        taxes (map tax amounts), ids (map :id sorted)\n        customers (map :customer sorted), regions (group-by :region sorted)\n        flagged (filter flagged? sorted)]\n    {:count (count sorted)\n     :first-id (first ids), :last-id (last ids)\n     :customers (set customers), :regions (keys regions)\n     :flagged (count flagged), :total (reduce + 0 amounts)\n     :tax (reduce + 0 taxes)}))\n",
    );
    write_source(
        &root,
        "receipt.clj",
        "(ns billing.receipt)\n\n(defn receipt-summary [rows]\n  (let [closed (filter closed? rows), local (filter local? closed)\n        ordered (sort-by :date local), amounts (map :amount ordered)\n        taxable (filter taxable? ordered), taxes (map tax amounts)\n        ids (map :id ordered), customers (map :customer ordered)\n        regions (group-by :region ordered)\n        flagged (filter flagged? ordered)]\n    {:count (count ordered)\n     :first-id (first ids), :last-id (last ids)\n     :customers (set customers), :regions (keys regions)\n     :flagged (count flagged), :total (reduce + 0 amounts)\n     :tax (reduce + 0 taxes)}))\n",
    );
    let found = duplicates(&root, 0.82, 4, 20);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].score >= 0.85 && found[0].score < 1.0,
        "{}",
        found[0].score
    );
}

#[test]
fn three_copies_make_three_pairs() {
    let root = scratch("clj-three");
    let body = "(let [ys (filter odd? xs)]\n    (map inc ys))";
    write_source(
        &root,
        "same.clj",
        &format!(
            "(ns same)\n\n(defn a [xs]\n  {body})\n\n(defn b [xs]\n  {body})\n\n(defn c [xs]\n  {body})\n"
        ),
    );
    let found = duplicates(&root, 0.80, 3, 8);
    assert_eq!(found.len(), 3);
    assert!(found.iter().all(|item| item.score == 1.0));
}

#[test]
fn unreadable_tail_does_not_drop_an_earlier_form() {
    let root = scratch("clj-broken");
    write_source(
        &root,
        "left.clj",
        "(ns left)\n\n(defn alpha [xs]\n  (let [ys (filter odd? xs)]\n    (map inc ys)))\n(defn broken [x]\n",
    );
    write_source(
        &root,
        "right.clj",
        "(ns right)\n\n(defn beta [items]\n  (let [kept (filter even? items)]\n    (map dec kept)))\n",
    );
    let files = vec![root.join("left.clj"), root.join("right.clj")];
    let (entries, warnings) = scan_files(&files, &root, 3, 8).unwrap();
    assert_eq!(warnings, vec!["left.clj:6: unterminated list".to_string()]);
    let found = find_duplicates(entries, 0.80);
    assert_eq!(found.len(), 1);
}

#[test]
fn reads_escaped_strings_and_characters() {
    let forms = read("(defn alpha [x]\n  (str \"a\\nb\" \\newline \\a))\n");
    assert_eq!(name(&coll(&forms[0]).items[0]), "defn");
}

#[test]
fn reports_a_broken_string_or_character() {
    assert_eq!(
        warning_of("(defn alpha [x]\n  \"abc)\n"),
        Some("2: unterminated string".to_string())
    );
    assert_eq!(
        warning_of("(defn alpha [x] \"abc\\"),
        Some("1: unterminated string".to_string())
    );
    assert_eq!(
        warning_of("(defn alpha [x] \\"),
        Some("1: incomplete character".to_string())
    );
}

#[test]
fn bom_and_shebang_are_not_forms() {
    let forms = read("\u{feff}#!/usr/bin/env bb\n(defn alpha [x]\n  (inc x))\n");
    assert_eq!(name(&coll(&forms[0]).items[0]), "defn");
}

#[test]
fn reader_conditional_keeps_clj_and_splices_a_collection() {
    let forms = read("#?(:cljs (inc x) :default (dec x))\n");
    assert_eq!(name(&coll(&forms[0]).items[0]), "dec");

    assert_eq!(read("#?((inc x) (dec x))\n"), Vec::<Form>::new());

    assert_eq!(
        warning_of("#?[:clj (inc x)]\n"),
        Some("1: reader conditional body must be a list".to_string())
    );

    let forms = read("(defn alpha []\n  [#?@(:clj [:a :b])])\n");
    let items = &coll(&coll(&forms[0]).items[3]).items;
    assert_eq!(items.iter().map(name).collect::<Vec<_>>(), vec!["a", "b"]);

    let forms = read_source("(defn alpha [x]\n  (inc x))\n#?@(:clj {:a 1})\n");
    assert_eq!(
        forms.1,
        Some("3: splicing reader conditional needs a collection".to_string())
    );
    assert_eq!(name(&coll(&forms.0[0]).items[0]), "defn");
}

#[test]
fn empty_list_normalizes_to_a_literal_list() {
    let forms = read("(defn alpha [x]\n  ())\n");
    assert!(pr(&normalize(&forms[0], false)).contains("[:list :literal]"));
}

#[test]
fn reader_macros_wrap_the_following_form() {
    let forms = read("'x\n@x\n`x\n~x\n~@x\n");
    assert_eq!(
        forms
            .iter()
            .map(|form| name(&coll(form).items[0]))
            .collect::<Vec<_>>(),
        vec![
            "quote",
            "deref",
            "syntax-quote",
            "unquote",
            "unquote-splicing"
        ]
    );
    assert_eq!(name(&coll(&forms[0]).items[1]), "x");
    assert_eq!(name(&coll(&forms[4]).items[1]), "x");
    let forms = read("'#_x\n");
    assert_eq!(
        coll(&forms[0]).items.iter().map(name).collect::<Vec<_>>(),
        vec!["quote"]
    );
}

#[test]
fn a_character_name_is_consumed() {
    let forms = read("(defn alpha []\n  (str \\newline))\n");
    assert_eq!(
        coll(&coll(&forms[0]).items[3])
            .items
            .iter()
            .map(tag)
            .collect::<Vec<_>>(),
        vec!["Sym", "Lit"]
    );
}

#[test]
fn a_bare_colon_is_an_empty_keyword() {
    let forms = read(":\n");
    assert_eq!(name(&forms[0]), "");
    assert_eq!(read("#!only"), Vec::<Form>::new());
}

#[test]
fn namespaced_maps_keep_the_map() {
    for source in ["#:foo{:a 1}\n", "#::{:a 1}\n", "#:{:a 1}\n"] {
        let forms = read(source);
        let form = coll(&forms[0]);
        assert_eq!(form.kind, "map", "{source:?}");
        assert_eq!(name(&form.pairs[0].0), "a");
    }
}

#[test]
fn map_pairs_keep_source_order() {
    let forms = read("{:a 1 :b 2}\n");
    assert_eq!(
        coll(&forms[0])
            .pairs
            .iter()
            .map(|(key, _)| name(key))
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let left = read("{foo 1}\n");
    let right = read("{bar 2}\n");
    assert!(!pr(&normalize(&left[0], false)).contains("foo"));
    assert_eq!(
        pr(&normalize(&left[0], false)),
        pr(&normalize(&right[0], false))
    );
    let paired = read("{:a 1\n :b 2}\n");
    assert_eq!(
        coll(&paired[0])
            .pairs
            .iter()
            .map(|(_, value)| value.line())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let symbols = read("{foo bar}\n");
    let printed = pr(&normalize(&symbols[0], false));
    assert!(!printed.contains("foo"));
    assert!(!printed.contains("bar"));
}

#[test]
fn a_trailing_feature_without_an_expression_is_ignored() {
    let forms = read("#?(:clj (inc x) :extra)\n");
    assert_eq!(name(&coll(&forms[0]).items[0]), "inc");
    assert_eq!(read("#?(:cljs (dec x) :extra)\n"), Vec::<Form>::new());
}

#[test]
fn names_inside_a_set_are_erased() {
    let forms = read("#{foo bar}\n");
    let printed = pr(&normalize(&forms[0], false));
    assert!(printed.starts_with("[:set "));
    assert!(!printed.contains("foo"));
}

#[test]
fn only_a_non_empty_list_that_is_not_ns_is_a_candidate() {
    assert!(!is_candidate_form(&collection("vector", vec![sym("a")])));
    assert!(!is_candidate_form(&collection("list", vec![])));
    assert!(!is_candidate_form(&sym("a")));
    assert!(!is_candidate_form(&collection(
        "list",
        vec![sym("ns"), sym("demo")]
    )));
    assert!(is_candidate_form(&collection("list", vec![sym("defn")])));
    assert_eq!(
        pr(&normalize(&collection("bag", vec![]), false)),
        "[:literal]"
    );
}

#[test]
fn node_count_and_keyword_printing() {
    assert_eq!(
        Normalized::Keyword("symbol".to_string()).printed(),
        ":symbol"
    );
    let mut found = HashSet::new();
    assert_eq!(
        Normalized::Keyword("symbol".to_string()).collect(&mut found),
        1
    );
    let mut found = HashSet::new();
    assert_eq!(
        Normalized::Vector(vec![
            Normalized::Keyword("list".to_string()),
            Normalized::Keyword("symbol".to_string()),
            Normalized::Keyword("literal".to_string()),
        ])
        .collect(&mut found),
        4
    );
}

#[test]
fn function_literal_reads_as_an_fn_list() {
    let forms = read("(map #(inc %)\n  xs)\n");
    let literal = &coll(&forms[0]).items[1];
    assert_eq!(coll(literal).kind, "list");
    assert_eq!(literal.line(), 1);
    assert_eq!(
        coll(literal).items.iter().map(name).collect::<Vec<_>>(),
        vec!["fn*", "inc", "%"]
    );
    assert_eq!(
        pr(&normalize(literal, false)),
        "[:list [:symbol \"fn*\"] :symbol :symbol]"
    );
}

#[test]
fn regex_and_tagged_literals_are_literals() {
    let forms = read("(re-find #\"a\\+\" s)\n(f #inst \"2020\" #js {:a 1} y)\n");
    assert_eq!(
        coll(&forms[0]).items.iter().map(tag).collect::<Vec<_>>(),
        vec!["Sym", "Lit", "Sym"]
    );
    assert_eq!(
        coll(&forms[1]).items.iter().map(tag).collect::<Vec<_>>(),
        vec!["Sym", "Lit", "Lit", "Sym"]
    );
    assert_eq!(coll(&forms[1]).items[1].line(), 2);
    assert_eq!(
        warning_of("(re-find #\"a+ s)\n"),
        Some("1: unterminated string".to_string())
    );
}

#[test]
fn reader_errors_name_the_line() {
    for (tail, expected) in [
        ("#", "2: incomplete dispatch"),
        ("'", "2: unexpected end of file"),
        (")", "2: expected a symbol"),
        ("{:a}", "2: map literal has an odd number of forms"),
    ] {
        let source = format!("(inc x)\n{tail}");
        let (forms, warning) = read_source(&source);
        assert_eq!(warning, Some(expected.to_string()), "{tail}");
        assert_eq!(name(&coll(&forms[0]).items[0]), "inc");
    }
}

#[test]
fn an_auto_resolved_keyword_keeps_its_name() {
    let forms = read("::alpha\n");
    assert_eq!(name(&forms[0]), "alpha");
}

#[test]
fn max_line_is_the_last_line_of_any_child() {
    let forms = read("(defn alpha [x]\n  {:a\n   (inc x)})\n");
    assert_eq!(max_line(&forms[0]), 3);
    assert_eq!(max_line(&coll(&forms[0]).items[1]), 1);
}
