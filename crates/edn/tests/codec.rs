use edn::{Value, encode, parse};

fn kw(name: &str) -> Value {
    Value::keyword(name)
}

fn round(text: &str) -> String {
    encode(&parse(text).unwrap()).unwrap()
}

#[test]
fn unicode_escapes_decode_including_surrogate_pairs() {
    assert_eq!(
        Value::String("é中😀 A".into()),
        parse(r#""\u00e9\u4E2d\ud83d\ude00 \u0041""#).unwrap()
    );
}

#[test]
fn invalid_unicode_escapes_are_rejected_not_dropped() {
    for text in [
        r#""\u12""#,
        r#""\uZZZZ""#,
        r#""\ud83d""#,
        r#""\ud83dx""#,
        r#""\ude00""#,
    ] {
        assert!(parse(text).is_err(), "{text} must fail");
    }
}

#[test]
fn control_characters_encode_as_lowercase_unicode_escapes() {
    let value = Value::String("a\u{1}\n\t\"\\\u{1f}中".into());
    assert_eq!(r#""a\u0001\n\t\"\\\u001f中""#, encode(&value).unwrap());
}

#[test]
fn map_keys_encode_in_dictionary_order_whatever_the_input_order() {
    assert_eq!(
        "{:a 1 :b {:x nil :y true} :c [2 3]}",
        round("{:c [2 3] :b {:y true :x nil} :a 1}")
    );
}

#[test]
fn equal_maps_in_any_order_encode_to_identical_bytes() {
    assert_eq!(round("{:b 1 :a 2}"), round("{:a 2, :b 1}"));
    assert_eq!(parse("{:b 1 :a 2}").unwrap(), parse("{:a 2 :b 1}").unwrap());
}

#[test]
fn sets_round_trip_sorted_and_reject_duplicates() {
    assert_eq!("#{\"c\" :a :b}", round("#{\"c\" :b :a}"));
    assert_eq!(parse("#{1 2}").unwrap(), parse("#{2 1}").unwrap());
    assert!(parse("#{:a :a}").is_err());
}

#[test]
fn tagged_values_keep_tag_and_value() {
    let value = parse(r#"#inst "2026-10-22T00:00:00Z""#).unwrap();
    assert_eq!(
        Value::Tagged(
            "inst".into(),
            Box::new(Value::String("2026-10-22T00:00:00Z".into()))
        ),
        value
    );
    assert_eq!(r#"#my.ns/tag {:a 1}"#, round("#my.ns/tag {:a 1}"));
}

#[test]
fn comments_and_discarded_forms_are_skipped() {
    let text =
        ";; header\n{:a 1 ; trailing\n #_ :gone #_ {:nested [1]} :b [1 #_ 2 3] #_ :end}\n; tail";
    assert_eq!("{:a 1 :b [1 3]}", round(text));
    assert_eq!("[]", round("[#_ 1]"));
    assert_eq!("nil", round("#_ 1 nil"));
}

#[test]
fn scalars_keywords_and_symbols_follow_the_lua_codec() {
    assert_eq!(
        "[nil true false :ns/key uml.domain -> + -1 0 42]",
        round("[nil true false :ns/key uml.domain -> + -1 +0 42]")
    );
}

#[test]
fn integers_and_floats_stay_distinct_but_compare_numerically() {
    assert_eq!("[68.0 100 1e-6 0.5 1.0]", round("[68.0 100 1.0E-6 0.5 1.]"));
    assert_eq!(parse("68").unwrap(), parse("68.0").unwrap());
}

#[test]
fn unsupported_numbers_are_rejected() {
    for text in [
        "1/2",
        "1N",
        "1.5M",
        "0x10",
        "1e",
        "1.2.3",
        "1e400",
        "9007199254740992",
        "1e16",
    ] {
        assert!(parse(text).is_err(), "{text} must fail");
    }
    assert_eq!(
        Value::Integer(-9_007_199_254_740_991),
        parse("-9007199254740991").unwrap()
    );
}

#[test]
fn structural_errors_are_reported_with_a_byte_position() {
    for (text, message) in [
        ("{:a 1 :a 2}", "duplicate map key"),
        ("{:a}", "map requires key/value pairs"),
        ("[1 2", "unterminated collection"),
        ("\"abc", "unterminated string"),
        ("1 2", "trailing input"),
        ("(1 2)", "unsupported token"),
    ] {
        let error = parse(text).unwrap_err();
        assert!(error.message.starts_with(message), "{text}: {error}");
        assert!(error.offset >= 1, "{text}: {error}");
    }
}

#[test]
fn non_finite_floats_are_refused_by_the_encoder() {
    assert!(encode(&Value::Float(f64::NAN)).is_err());
    assert!(encode(&Value::Float(f64::INFINITY)).is_err());
}

#[test]
fn map_access_compares_keys_structurally() {
    let mut value = parse("{[:a 1] :vector-key :x 1}").unwrap();
    assert_eq!(
        Some(&kw("vector-key")),
        value.get(&parse("[:a 1]").unwrap())
    );
    value.assoc(kw("x"), Value::Integer(2));
    value.assoc(kw("y"), Value::Nil);
    assert_eq!("{:x 2 :y nil [:a 1] :vector-key}", encode(&value).unwrap());
}
