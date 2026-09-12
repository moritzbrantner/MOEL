use std::path::PathBuf;

use moel::parse;
use moel::schema::{
    DiagnosticKind, Schema, SchemaError, ValidatedDocumentError, parse_schema, parse_validated,
    schema_path_for, validate,
};

const SCHEMA: &str = r#"
name = "string"
id = "uuid"
created = "utc"
score = "number"
tags = ["string"]

[profile]
age = "integer"
active = "boolean"
"#;

#[test]
fn schema_reuses_moel_shape() {
    let schema = parse_schema(SCHEMA).expect("schema must parse");
    let Schema::Table(root) = schema else {
        panic!("schema root must be a table")
    };

    assert_eq!(root["name"], Schema::String);
    assert_eq!(root["id"], Schema::Uuid);
    assert_eq!(root["created"], Schema::Utc);
    assert_eq!(root["score"], Schema::Number);
    assert_eq!(root["tags"], Schema::Array(Box::new(Schema::String)));

    let Schema::Table(profile) = &root["profile"] else {
        panic!("profile must be a table schema")
    };
    assert_eq!(profile["age"], Schema::Integer);
    assert_eq!(profile["active"], Schema::Boolean);
}

#[test]
fn valid_document_passes_exact_schema() {
    let document = parse(
        r#"
name = "MOEL"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T20:20:00Z
score = 4.5
tags = ["config", "typed"]

[profile]
age = 30
active = true
"#,
    )
    .expect("document must parse");
    let schema = parse_schema(SCHEMA).expect("schema must parse");

    assert!(validate(&document, &schema).is_empty());
}

#[test]
fn validation_reports_missing_unexpected_and_type_mismatch_paths() {
    let document = parse(
        r#"
name = 42
id = "550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T22:20:00+02:00
score = 4
extra = true
tags = ["ok", 9]

[profile]
active = "yes"
"#,
    )
    .expect("document must parse");
    let schema = parse_schema(SCHEMA).expect("schema must parse");
    let diagnostics = validate(&document, &schema);

    assert_eq!(diagnostics.len(), 7);
    assert_eq!(diagnostics[0].path, "$[\"created\"]");
    assert!(matches!(
        diagnostics[0].kind,
        DiagnosticKind::TypeMismatch {
            expected: "utc",
            actual: "datetime"
        }
    ));
    assert_eq!(diagnostics[1].path, "$[\"id\"]");
    assert!(matches!(
        diagnostics[1].kind,
        DiagnosticKind::TypeMismatch {
            expected: "uuid",
            actual: "string"
        }
    ));
    assert_eq!(diagnostics[2].path, "$[\"name\"]");
    assert!(matches!(
        diagnostics[2].kind,
        DiagnosticKind::TypeMismatch {
            expected: "string",
            actual: "integer"
        }
    ));
    assert_eq!(diagnostics[3].path, "$[\"profile\"][\"active\"]");
    assert!(matches!(
        diagnostics[3].kind,
        DiagnosticKind::TypeMismatch {
            expected: "boolean",
            actual: "string"
        }
    ));
    assert_eq!(diagnostics[4].path, "$[\"profile\"][\"age\"]");
    assert!(matches!(diagnostics[4].kind, DiagnosticKind::MissingField));
    assert_eq!(diagnostics[5].path, "$[\"tags\"][1]");
    assert!(matches!(
        diagnostics[5].kind,
        DiagnosticKind::TypeMismatch {
            expected: "string",
            actual: "integer"
        }
    ));
    assert_eq!(diagnostics[6].path, "$[\"extra\"]");
    assert!(matches!(
        diagnostics[6].kind,
        DiagnosticKind::UnexpectedField
    ));
}

#[test]
fn datetime_accepts_utc_and_non_utc_but_utc_remains_strict() {
    let schema = parse_schema(
        r#"
when = "datetime"
strict = "utc"
"#,
    )
    .expect("schema must parse");

    let document = parse(
        r#"
when = 2026-09-12T22:20:00+02:00
strict = 2026-09-12T20:20:00Z
"#,
    )
    .expect("document must parse");

    assert!(validate(&document, &schema).is_empty());
}

#[test]
fn malformed_schema_shapes_fail_closed() {
    let unknown = parse_schema("value = \"mystery\"").expect_err("unknown type must fail");
    assert!(matches!(unknown, SchemaError::UnknownType { .. }));

    let empty = parse_schema("values = []").expect_err("empty array schema must fail");
    assert!(matches!(empty, SchemaError::EmptyArray { .. }));

    let many = parse_schema("values = [\"string\", \"integer\"]")
        .expect_err("heterogeneous schema declaration must fail");
    assert!(matches!(
        many,
        SchemaError::ArrayMustHaveSingleElement { actual: 2, .. }
    ));

    let literal = parse_schema("value = 42").expect_err("data literal is not a schema type");
    assert!(matches!(literal, SchemaError::InvalidSchemaValue { .. }));
}

#[test]
fn parse_validated_keeps_parse_schema_and_validation_failures_distinct() {
    let valid = parse_validated("name = \"MOEL\"", "name = \"string\"")
        .expect("valid document must be returned");
    assert!(matches!(valid, moel::Value::Table(_)));

    let error = parse_validated("name = 1", "name = \"string\"")
        .expect_err("type mismatch must reject document");
    let ValidatedDocumentError::Validation(diagnostics) = error else {
        panic!("expected validation diagnostics")
    };
    assert_eq!(diagnostics.len(), 1);
}

#[test]
fn schema_discovery_is_same_directory_only_and_never_recursive() {
    assert_eq!(
        schema_path_for("config/app.moel"),
        Some(PathBuf::from("config/schema.moel"))
    );
    assert_eq!(
        schema_path_for("app.moel"),
        Some(PathBuf::from("schema.moel"))
    );
    assert_eq!(schema_path_for("config/schema.moel"), None);
}

#[test]
fn ordinary_parse_does_not_implicitly_require_a_schema() {
    let document = parse("arbitrary = true").expect("schema-less MOEL remains valid");
    assert!(matches!(document, moel::Value::Table(_)));
}
