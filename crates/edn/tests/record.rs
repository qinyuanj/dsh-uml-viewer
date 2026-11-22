use edn::Value;
use edn::record::{
    flag, get, get_raw, is_true, local_name, num, num_or, put, record, remove, seq, text, truthy,
};

fn read(text: &str) -> Value {
    edn::parse(text).unwrap()
}

#[test]
fn get_reads_a_keyword_entry_and_treats_nil_absent_and_non_maps_as_none() {
    let m = read(r#"{:id :a :gone nil "id" 1}"#);
    assert_eq!(Some(&Value::keyword("a")), get(&m, "id"));
    assert_eq!(None, get(&m, "gone"));
    assert_eq!(None, get(&m, "missing"));
    assert_eq!(None, get(&read("[:id :a]"), "id"));
    assert_eq!(None, get(&Value::Nil, "id"));
}

#[test]
fn get_raw_keeps_an_explicit_nil_apart_from_an_absent_key() {
    let m = read("{:proposal nil :id :a}");
    assert_eq!(Some(&Value::Nil), get_raw(&m, "proposal"));
    assert_eq!(Some(&Value::keyword("a")), get_raw(&m, "id"));
    assert_eq!(None, get_raw(&m, "missing"));
    assert_eq!(None, get_raw(&Value::Integer(1), "id"));
}

#[test]
fn flag_follows_lua_truthiness() {
    let m = read(r#"{:t true :f false :n nil :zero 0 :one 1 :empty "" :v [] :yes :yes}"#);
    for falsy in ["missing", "n", "f"] {
        assert!(!flag(&m, falsy), ":{falsy} reads false");
        assert!(
            !truthy(m.get(&Value::keyword(falsy))),
            ":{falsy} value reads false"
        );
    }
    for truthy_key in ["t", "zero", "empty", "v", "one", "yes"] {
        assert!(flag(&m, truthy_key), ":{truthy_key} reads true");
        assert!(
            truthy(get(&m, truthy_key)),
            ":{truthy_key} value reads true"
        );
    }
    assert!(!flag(&read("[:t true]"), "t"), "non-maps have no flags");
}

#[test]
fn is_true_is_the_lua_strict_comparison_with_true() {
    let m = read(r#"{:t true :f false :n nil :one 1 :yes :yes}"#);
    assert!(is_true(&m, "t"));
    for key in ["f", "n", "missing", "one", "yes"] {
        assert!(!is_true(&m, key), ":{key} is not strictly true");
    }
    assert!(!is_true(&read("[:t true]"), "t"));
}

#[test]
fn num_reads_integers_and_floats_as_the_same_number() {
    let m = read(r#"{:i 68 :f 68.0 :half 0.5 :s "68"}"#);
    assert_eq!(Some(68.0), get(&m, "i").and_then(num));
    assert_eq!(Some(68.0), get(&m, "f").and_then(num));
    assert_eq!(None, get(&m, "s").and_then(num));
    assert_eq!(0.5, num_or(get(&m, "half"), 9.0));
    assert_eq!(9.0, num_or(get(&m, "missing"), 9.0));
    assert_eq!(9.0, num_or(get(&m, "s"), 9.0));
}

#[test]
fn seq_views_an_optional_vector_and_empties_everything_else() {
    let m = read(r#"{:xs [1 2] :m {:a 1} :s "ab" :set #{1}}"#);
    assert_eq!(&[Value::Integer(1), Value::Integer(2)], seq(get(&m, "xs")));
    for other in ["m", "s", "set", "missing"] {
        assert!(seq(get(&m, other)).is_empty(), ":{other} is no sequence");
    }
}

#[test]
fn local_name_takes_the_segment_after_the_last_slash_of_a_keyword_or_symbol() {
    assert_eq!(
        Some("engine.parser"),
        local_name(&Value::keyword("uml/engine.parser"))
    );
    assert_eq!(Some("sym"), local_name(&Value::Symbol("a/sym".into())));
    assert_eq!(Some("c"), local_name(&Value::keyword("a/b/c")));
    assert_eq!(Some("plain"), local_name(&Value::keyword("plain")));
    assert_eq!(Some("x/y"), local_name(&Value::String("x/y".into())));
    assert_eq!(None, local_name(&Value::Integer(1)));
    assert_eq!(None, local_name(&Value::Nil));
}

#[test]
fn text_takes_the_whole_name_of_a_keyword_symbol_or_string() {
    assert_eq!(
        Some("uml/engine.parser"),
        text(&Value::keyword("uml/engine.parser"))
    );
    assert_eq!(Some("a/sym"), text(&Value::Symbol("a/sym".into())));
    assert_eq!(Some("herdr"), text(&Value::keyword("herdr")));
    assert_eq!(
        Some("uml-viewer-A"),
        text(&Value::String("uml-viewer-A".into()))
    );
    assert_eq!(None, text(&Value::Bool(true)));
    assert_eq!(None, text(&read("[:a]")));
}

#[test]
fn record_builds_a_keyword_keyed_map() {
    let m = record(vec![
        ("session", Value::String("a".into())),
        ("pid", Value::Integer(42)),
    ]);
    assert_eq!(read(r#"{:session "a" :pid 42}"#), m);
}

#[test]
fn put_replaces_an_entry_in_place_or_appends_it() {
    let mut m = read(r#"{:session "a"}"#);
    put(&mut m, "session", Value::String("b".into()));
    put(&mut m, "pane", Value::String("w1:p1".into()));
    put(&mut m, "gone", Value::Nil);
    assert_eq!(
        r#"{:gone nil :pane "w1:p1" :session "b"}"#,
        edn::encode(&m).unwrap()
    );
}

#[test]
fn remove_drops_the_keyword_entry() {
    let mut m = read(r#"{:cleanup-phase :a :pid 1 "pid" 2}"#);
    remove(&mut m, "cleanup-phase");
    remove(&mut m, "missing");
    remove(&mut m, "pid");
    assert_eq!(r#"{"pid" 2}"#, edn::encode(&m).unwrap());
}

#[test]
fn put_and_remove_leave_non_maps_alone() {
    let mut v = read("[:pid 1]");
    put(&mut v, "pid", Value::Integer(2));
    remove(&mut v, "pid");
    assert_eq!(read("[:pid 1]"), v);
}
