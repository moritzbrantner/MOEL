use moel::{Value, parse, to_string};
use pretty_assertions::assert_eq;

fn table(value: &Value) -> &std::collections::BTreeMap<String, Value> {
    let Value::Table(table) = value else {
        panic!("expected document table")
    };
    table
}

#[test]
fn valid_toml_remains_toml_compatible() {
    let document = r#"
title = "example"
count = 42
enabled = true
ratio = 1.5
looks_like_uuid = "550e8400-e29b-41d4-a716-446655440000"
created = 1979-05-27T07:32:00-08:00
tags = ["one", "two"]
"#;

    let parsed = parse(document).expect("valid TOML must parse as MOEL");
    let root = table(&parsed);

    assert_eq!(root["title"], Value::String("example".into()));
    assert_eq!(root["count"], Value::Integer(42));
    assert_eq!(root["enabled"], Value::Boolean(true));
    assert_eq!(root["ratio"], Value::Float(1.5));
    assert_eq!(
        root["looks_like_uuid"],
        Value::String("550e8400-e29b-41d4-a716-446655440000".into())
    );
    assert!(matches!(root["created"], Value::TomlDatetime(_)));
}

#[test]
fn explicit_uuid_literals_are_typed() {
    let parsed = parse(
        r#"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
ids = [uuid"6ba7b810-9dad-11d1-80b4-00c04fd430c8"]
"#,
    )
    .expect("MOEL UUID literals must parse");

    let root = table(&parsed);
    assert!(matches!(root["id"], Value::Uuid(_)));
    let Value::Array(ids) = &root["ids"] else {
        panic!("expected UUID array")
    };
    assert!(matches!(ids[0], Value::Uuid(_)));
}

#[test]
fn extension_looking_text_in_strings_and_comments_is_not_rewritten() {
    let parsed = parse(
        r#"
text = 'uuid"550e8400-e29b-41d4-a716-446655440000"'
# id = uuid"550e8400-e29b-41d4-a716-446655440000"
multiline = """
uuid"550e8400-e29b-41d4-a716-446655440000"
"""
"#,
    )
    .expect("extension-looking TOML text must remain ordinary text");

    let root = table(&parsed);
    assert!(matches!(root["text"], Value::String(_)));
    assert!(matches!(root["multiline"], Value::String(_)));
}

#[test]
fn multiline_string_closing_quote_runs_do_not_hide_following_extensions() {
    let parsed = parse(
        r#"
basic = """ends with a quote""""
literal = '''ends with a quote''''
id = uuid"550e8400-e29b-41d4-a716-446655440000"
"#,
    )
    .expect("valid TOML quote-run endings must preserve scanner state");

    let root = table(&parsed);
    assert_eq!(root["basic"], Value::String("ends with a quote\"".into()));
    assert_eq!(root["literal"], Value::String("ends with a quote'".into()));
    assert!(matches!(root["id"], Value::Uuid(_)));
}

#[test]
fn internal_marker_collision_does_not_change_user_strings() {
    let parsed = parse(
        r#"
existing = "__MOEL_UUID_0_0__"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
"#,
    )
    .expect("marker collision must be avoided");

    let root = table(&parsed);
    assert_eq!(root["existing"], Value::String("__MOEL_UUID_0_0__".into()));
    assert!(matches!(root["id"], Value::Uuid(_)));
}

#[test]
fn serialization_marker_collision_with_table_key_is_avoided() {
    let source = r#"
"__MOEL_SERIALIZED_UUID_0__" = "keep this key"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
"#;

    let first = parse(source).expect("source must parse");
    let serialized = to_string(&first).expect("document must serialize");
    let second = parse(&serialized).expect("serialized MOEL must parse");

    assert_eq!(second, first);
}

#[test]
fn utc_timestamp_semantics_are_explicit() {
    let parsed = parse(
        r#"
zulu = 2026-09-12T19:11:00Z
zero_offset = 2026-09-12T19:11:00+00:00
non_zero_offset = 2026-09-12T21:11:00+02:00
local_date = 2026-09-12
"#,
    )
    .expect("TOML date and time forms must parse");

    let root = table(&parsed);
    assert!(matches!(root["zulu"], Value::UtcTimestamp(_)));
    assert!(matches!(root["zero_offset"], Value::UtcTimestamp(_)));
    assert!(matches!(root["non_zero_offset"], Value::TomlDatetime(_)));
    assert!(matches!(root["local_date"], Value::TomlDatetime(_)));
}

#[test]
fn canonical_serialization_round_trips_semantics() {
    let source = r#"
name = "MOEL"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T19:11:00Z
values = [1, 2, 3]

[feature]
enabled = true
"#;

    let first = parse(source).expect("source must parse");
    let serialized = to_string(&first).expect("document must serialize");
    let second = parse(&serialized).expect("serialized MOEL must parse");

    assert_eq!(second, first);
    assert!(serialized.contains("uuid\"550e8400-e29b-41d4-a716-446655440000\""));
}
