use moel::{MoelUuid, TomlDatetime, UtcTimestamp, from_str, from_value, parse};
use pretty_assertions::assert_eq;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Config {
    name: String,
    id: MoelUuid,
    created: UtcTimestamp,
    local_date: TomlDatetime,
    tags: Vec<String>,
    mode: Mode,
    feature: Feature,
    retries: Option<u32>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Fast,
    Safe,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Feature {
    enabled: bool,
    limit: u32,
}

#[test]
fn typed_deserialization_preserves_semantic_types() {
    let source = r#"
name = "MOEL"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T19:11:00Z
local_date = 2026-09-12
tags = ["config", "typed"]
mode = "fast"

[feature]
enabled = true
limit = 12
"#;

    let config: Config = from_str(source).expect("typed MOEL must deserialize");

    assert_eq!(config.name, "MOEL");
    assert_eq!(
        config.id.to_string(),
        "550e8400-e29b-41d4-a716-446655440000"
    );
    assert_eq!(config.created.as_str(), "2026-09-12T19:11:00Z");
    assert_eq!(config.local_date.as_str(), "2026-09-12");
    assert_eq!(config.tags, ["config", "typed"]);
    assert_eq!(config.mode, Mode::Fast);
    assert_eq!(
        config.feature,
        Feature {
            enabled: true,
            limit: 12
        }
    );
    assert_eq!(config.retries, None);
}

#[test]
fn parsed_values_can_deserialize_without_reparsing() {
    let source = r#"
name = "MOEL"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T19:11:00+00:00
local_date = 2026-09-12
tags = []
mode = "safe"

[feature]
enabled = false
limit = 0
"#;

    let value = parse(source).expect("source must parse");
    let config: Config = from_value(value).expect("parsed value must deserialize");

    assert_eq!(config.mode, Mode::Safe);
    assert_eq!(config.created.as_str(), "2026-09-12T19:11:00+00:00");
}

#[derive(Debug, Deserialize)]
struct Identity {
    id: MoelUuid,
}

#[test]
fn uuid_looking_string_is_not_promoted_to_typed_uuid() {
    let source = "id = \"550e8400-e29b-41d4-a716-446655440000\"\n";
    let error = from_str::<Identity>(source).expect_err("ordinary string must stay a string");

    assert_eq!(error.path(), Some("$[\"id\"]"));
    assert!(
        error
            .to_string()
            .contains("expected explicit MOEL UUID, found string")
    );
    let span = error.span().expect("typed error must retain source span");
    assert_eq!(&source[span.start..span.end], "\"550e8400-e29b-41d4-a716-446655440000\"");
}

#[derive(Debug, Deserialize)]
struct StringIdentity {
    id: String,
}

#[test]
fn explicit_uuid_is_not_silently_erased_into_string() {
    let source = "id = uuid\"550e8400-e29b-41d4-a716-446655440000\"\n";
    let error =
        from_str::<StringIdentity>(source).expect_err("explicit UUID must remain semantically typed");

    assert_eq!(error.path(), Some("$[\"id\"]"));
    assert!(error.to_string().contains("expected string, found UUID"));
}

#[derive(Debug, Deserialize)]
struct TimestampConfig {
    created: UtcTimestamp,
}

#[test]
fn non_utc_datetime_does_not_deserialize_as_utc() {
    let source = "created = 2026-09-12T21:11:00+02:00\n";
    let error =
        from_str::<TimestampConfig>(source).expect_err("non-zero offset must not become UTC");

    assert_eq!(error.path(), Some("$[\"created\"]"));
    assert!(
        error
            .to_string()
            .contains("expected UTC timestamp, found TOML date/time")
    );
}

#[derive(Debug, Deserialize)]
struct Ports {
    ports: Vec<u16>,
}

#[test]
fn nested_type_errors_report_deterministic_path_and_source_span() {
    let source = "ports = [80, -1, 443]\n";
    let error = from_str::<Ports>(source).expect_err("negative port must fail");

    assert_eq!(error.path(), Some("$[\"ports\"][1]"));
    let span = error.span().expect("nested typed error must retain source span");
    assert_eq!(&source[span.start..span.end], "-1");
}

#[derive(Debug, Deserialize, PartialEq)]
struct StrategyConfig {
    strategy: Strategy,
}

#[derive(Debug, Deserialize, PartialEq)]
enum Strategy {
    Fixed(u32),
    Window { size: u32 },
}

#[test]
fn externally_tagged_enum_payloads_use_single_entry_tables() {
    let fixed: StrategyConfig =
        from_str("strategy = { Fixed = 3 }\n").expect("newtype enum must deserialize");
    assert_eq!(
        fixed,
        StrategyConfig {
            strategy: Strategy::Fixed(3)
        }
    );

    let window: StrategyConfig = from_str("strategy = { Window = { size = 8 } }\n")
        .expect("struct enum must deserialize");
    assert_eq!(
        window,
        StrategyConfig {
            strategy: Strategy::Window { size: 8 }
        }
    );
}

#[test]
fn parse_failures_remain_parse_failures_with_source_spans() {
    let source = "id = uuid\"not-a-uuid\"\n";
    let error = from_str::<Identity>(source).expect_err("invalid UUID syntax must fail while parsing");

    assert_eq!(error.path(), None);
    let span = error.span().expect("parse failure must retain source span");
    assert_eq!(&source[span.start..span.end], "uuid\"not-a-uuid\"");
}
