use moel::diagnostics::{DiagnosticSource, parse_error_span, schema_error_span, validation_span};
use moel::parse;
use moel::schema::{parse_schema, validate};

#[test]
fn invalid_uuid_span_points_to_the_explicit_literal() {
    let source = "name = \"ok\"\nid = uuid\"not-a-uuid\"\n";
    let error = parse(source).expect_err("invalid UUID must fail");
    let span = parse_error_span(source, &error).expect("UUID error must have a source span");

    assert_eq!(span.line, 2);
    assert_eq!(span.column, 6);
    assert_eq!(&source[span.start..span.end], "uuid\"not-a-uuid\"");
}

#[test]
fn toml_error_after_uuid_keeps_original_source_coordinates() {
    let source = concat!(
        "id = uuid\"550e8400-e29b-41d4-a716-446655440000\"\n",
        "count = nope\n",
    );
    let error = parse(source).expect_err("invalid TOML value must fail");
    let span = parse_error_span(source, &error).expect("TOML error must have a source span");

    assert_eq!(span.line, 2);
    assert!(span.start >= source.find("count").expect("count line must exist"));
}

#[test]
fn schema_error_span_maps_optional_key_rewrite_back_to_original_source() {
    let source = "name? = \"any\"\n";
    let error = parse_schema(source).expect_err("unknown schema type must fail");
    let span = schema_error_span(source, &error).expect("schema error must have a source span");

    assert_eq!(span.line, 1);
    assert_eq!(&source[span.start..span.end], "\"any\"");
}

#[test]
fn type_mismatch_points_to_the_document_value() {
    let document_source = "count = \"many\"\n";
    let schema_source = "count = \"integer\"\n";
    let document = parse(document_source).expect("document must parse");
    let schema = parse_schema(schema_source).expect("schema must parse");
    let diagnostic = validate(&document, &schema)
        .into_iter()
        .next()
        .expect("validation must fail");

    let located = validation_span(document_source, schema_source, &diagnostic)
        .expect("validation diagnostic must have a source span");

    assert_eq!(located.source, DiagnosticSource::Document);
    assert_eq!(located.span.line, 1);
    assert_eq!(located.span.column, 9);
    assert_eq!(
        &document_source[located.span.start..located.span.end],
        "\"many\""
    );
}

#[test]
fn missing_field_points_to_its_schema_declaration() {
    let document_source = "name = \"MOEL\"\n";
    let schema_source = "name = \"string\"\ncount = \"integer\"\n";
    let document = parse(document_source).expect("document must parse");
    let schema = parse_schema(schema_source).expect("schema must parse");
    let diagnostic = validate(&document, &schema)
        .into_iter()
        .find(|diagnostic| diagnostic.path == "$[\"count\"]")
        .expect("missing field diagnostic must exist");

    let located = validation_span(document_source, schema_source, &diagnostic)
        .expect("missing field must point to schema declaration");

    assert_eq!(located.source, DiagnosticSource::Schema);
    assert_eq!(located.span.line, 2);
    assert_eq!(
        &schema_source[located.span.start..located.span.end],
        "\"integer\""
    );
}

#[test]
fn nested_array_diagnostic_points_to_the_failing_item() {
    let document_source = "values = [1, \"two\"]\n";
    let schema_source = "values = [\"integer\"]\n";
    let document = parse(document_source).expect("document must parse");
    let schema = parse_schema(schema_source).expect("schema must parse");
    let diagnostic = validate(&document, &schema)
        .into_iter()
        .next()
        .expect("array validation must fail");

    assert_eq!(diagnostic.path, "$[\"values\"][1]");
    let located = validation_span(document_source, schema_source, &diagnostic)
        .expect("array item must have a source span");

    assert_eq!(located.source, DiagnosticSource::Document);
    assert_eq!(
        &document_source[located.span.start..located.span.end],
        "\"two\""
    );
}
