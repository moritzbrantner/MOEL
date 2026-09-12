use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Error as ParseError, Value, parse};

pub const SCHEMA_FILE_NAME: &str = "schema.moel";

/// A semantic MOEL schema.
///
/// Schemas deliberately stay close to MOEL values instead of introducing a
/// separate schema language. Tables describe table shapes, one-element arrays
/// describe homogeneous arrays, strings name scalar types, enum declarations
/// constrain strings to an explicit set of values, and `Optional` marks fields
/// whose presence is not required.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Schema {
    String,
    Integer,
    Float,
    Number,
    Boolean,
    Uuid,
    Utc,
    Datetime,
    Enum(Vec<String>),
    Array(Box<Schema>),
    Optional(Box<Schema>),
    Table(BTreeMap<String, Schema>),
}

#[derive(Debug)]
pub enum SchemaError {
    Parse(ParseError),
    RootMustBeTable,
    UnknownType {
        path: String,
        name: String,
    },
    EmptyArray {
        path: String,
    },
    ArrayMustHaveSingleElement {
        path: String,
        actual: usize,
    },
    EmptyEnum {
        path: String,
    },
    EnumMustBeArray {
        path: String,
        actual: &'static str,
    },
    EnumValueMustBeString {
        path: String,
        index: usize,
        actual: &'static str,
    },
    DuplicateEnumValue {
        path: String,
        value: String,
    },
    DuplicateFieldDeclaration {
        path: String,
        name: String,
    },
    InvalidSchemaValue {
        path: String,
        actual: &'static str,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "schema parse error: {error}"),
            Self::RootMustBeTable => write!(f, "schema.moel root must be a table"),
            Self::UnknownType { path, name } => {
                write!(f, "unknown schema type {name:?} at {path}")
            }
            Self::EmptyArray { path } => {
                write!(f, "schema array at {path} must contain one item schema")
            }
            Self::ArrayMustHaveSingleElement { path, actual } => write!(
                f,
                "schema array at {path} must contain exactly one item schema, found {actual}"
            ),
            Self::EmptyEnum { path } => write!(
                f,
                "enum declaration at {path} must contain at least one value"
            ),
            Self::EnumMustBeArray { path, actual } => write!(
                f,
                "enum declaration at {path} must use an array of strings, found {actual}"
            ),
            Self::EnumValueMustBeString {
                path,
                index,
                actual,
            } => write!(
                f,
                "enum declaration at {path} has non-string value at index {index}: found {actual}"
            ),
            Self::DuplicateEnumValue { path, value } => write!(
                f,
                "enum declaration at {path} contains duplicate value {value:?}"
            ),
            Self::DuplicateFieldDeclaration { path, name } => write!(
                f,
                "schema declares field {name:?} more than once at {path} after optional-field normalization"
            ),
            Self::InvalidSchemaValue { path, actual } => write!(
                f,
                "invalid schema value at {path}: expected a type name, enum declaration, table, or one-element array, found {actual}"
            ),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for SchemaError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub path: String,
    pub kind: DiagnosticKind,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DiagnosticKind::MissingField => write!(f, "{}: required field is missing", self.path),
            DiagnosticKind::UnexpectedField => {
                write!(f, "{}: field is not declared by schema", self.path)
            }
            DiagnosticKind::TypeMismatch { expected, actual } => {
                write!(f, "{}: expected {expected}, found {actual}", self.path)
            }
            DiagnosticKind::InvalidEnumValue { allowed, actual } => write!(
                f,
                "{}: expected one of {allowed:?}, found {actual:?}",
                self.path
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    MissingField,
    UnexpectedField,
    TypeMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    InvalidEnumValue {
        allowed: Vec<String>,
        actual: String,
    },
}

#[derive(Debug)]
pub enum ValidatedDocumentError {
    Document(ParseError),
    Schema(SchemaError),
    Validation(Vec<Diagnostic>),
}

impl fmt::Display for ValidatedDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => write!(f, "document parse error: {error}"),
            Self::Schema(error) => write!(f, "{error}"),
            Self::Validation(diagnostics) => {
                write!(f, "document failed schema validation")?;
                for diagnostic in diagnostics {
                    write!(f, "\n- {diagnostic}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ValidatedDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document(error) => Some(error),
            Self::Schema(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

/// Parse a `schema.moel` document.
///
/// The schema root is always a table. Leaf strings name types. A one-element
/// array contains the schema for every array item. Nested tables describe nested
/// document tables. A table of the form `{ enum = ["a", "b"] }` declares a
/// string enum. A bare schema field or table key ending in `?` marks that field
/// optional; the `?` is not part of the data-document key.
pub fn parse_schema(source: &str) -> Result<Schema, SchemaError> {
    let rewritten = rewrite_optional_field_keys(source);
    let value = parse(&rewritten.source)?;
    let Value::Table(values) = value else {
        return Err(SchemaError::RootMustBeTable);
    };

    schema_from_table(values, "$".to_owned(), &rewritten.optional_fields)
}

/// Validate a parsed MOEL value against a schema.
///
/// Tables remain closed by default. Required fields must be present; optional
/// fields may be absent. Whenever an optional field is present, its inner schema
/// is validated normally.
pub fn validate(value: &Value, schema: &Schema) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_at(value, schema, "$", &mut diagnostics);
    diagnostics
}

/// Parse a document and schema, then return the document only when validation
/// succeeds.
pub fn parse_validated(
    document_source: &str,
    schema_source: &str,
) -> Result<Value, ValidatedDocumentError> {
    let document = parse(document_source).map_err(ValidatedDocumentError::Document)?;
    let schema = parse_schema(schema_source).map_err(ValidatedDocumentError::Schema)?;
    let diagnostics = validate(&document, &schema);

    if diagnostics.is_empty() {
        Ok(document)
    } else {
        Err(ValidatedDocumentError::Validation(diagnostics))
    }
}

/// Return the conventional sibling schema path for a document.
///
/// This function is pure path resolution: callers decide whether the returned
/// file exists and whether to load it. `schema.moel` itself never resolves to
/// itself, preventing recursive schema discovery.
pub fn schema_path_for(document_path: impl AsRef<Path>) -> Option<PathBuf> {
    let document_path = document_path.as_ref();
    let file_name = document_path.file_name()?;
    if file_name == SCHEMA_FILE_NAME {
        return None;
    }

    Some(
        document_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(SCHEMA_FILE_NAME),
    )
}

#[derive(Debug)]
struct RewrittenSchemaSource {
    source: String,
    optional_fields: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaLexState {
    Normal,
    Comment,
    BasicString,
    LiteralString,
    MultilineBasicString,
    MultilineLiteralString,
}

fn rewrite_optional_field_keys(source: &str) -> RewrittenSchemaSource {
    let mut output = String::with_capacity(source.len());
    let mut optional_fields = BTreeMap::new();
    let mut state = SchemaLexState::Normal;
    let mut i = 0;
    let mut marker_index = 0_u32;

    while i < source.len() {
        match state {
            SchemaLexState::Normal => {
                if source[i..].starts_with('#') {
                    output.push('#');
                    i += 1;
                    state = SchemaLexState::Comment;
                    continue;
                }
                if source[i..].starts_with("\"\"\"") {
                    output.push_str("\"\"\"");
                    i += 3;
                    state = SchemaLexState::MultilineBasicString;
                    continue;
                }
                if source[i..].starts_with("'''") {
                    output.push_str("'''");
                    i += 3;
                    state = SchemaLexState::MultilineLiteralString;
                    continue;
                }
                if source[i..].starts_with('"') {
                    output.push('"');
                    i += 1;
                    state = SchemaLexState::BasicString;
                    continue;
                }
                if source[i..].starts_with('\'') {
                    output.push('\'');
                    i += 1;
                    state = SchemaLexState::LiteralString;
                    continue;
                }

                if source[i..].starts_with('?')
                    && let Some((key_start, key)) = optional_bare_key_before(source, i)
                    && optional_key_delimiter_after(source, i + 1)
                {
                    let key_len = i - key_start;
                    output.truncate(output.len() - key_len);
                    let marker = next_optional_marker(source, &optional_fields, &mut marker_index);
                    output.push('"');
                    output.push_str(&marker);
                    output.push('"');
                    optional_fields.insert(marker, key.to_owned());
                    i += 1;
                    continue;
                }

                push_next_char(source, &mut output, &mut i);
            }
            SchemaLexState::Comment => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\n' {
                    state = SchemaLexState::Normal;
                }
            }
            SchemaLexState::BasicString => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\\' && i < source.len() {
                    push_next_char(source, &mut output, &mut i);
                } else if ch == '"' {
                    state = SchemaLexState::Normal;
                }
            }
            SchemaLexState::LiteralString => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\'' {
                    state = SchemaLexState::Normal;
                }
            }
            SchemaLexState::MultilineBasicString => {
                let quote_run = repeated_ascii_char_len(source, i, '"');
                if quote_run >= 3 {
                    output.push_str(&source[i..i + quote_run]);
                    i += quote_run;
                    state = SchemaLexState::Normal;
                } else {
                    let ch = next_char(source, i);
                    output.push(ch);
                    i += ch.len_utf8();
                    if ch == '\\' && i < source.len() {
                        push_next_char(source, &mut output, &mut i);
                    }
                }
            }
            SchemaLexState::MultilineLiteralString => {
                let quote_run = repeated_ascii_char_len(source, i, '\'');
                if quote_run >= 3 {
                    output.push_str(&source[i..i + quote_run]);
                    i += quote_run;
                    state = SchemaLexState::Normal;
                } else {
                    push_next_char(source, &mut output, &mut i);
                }
            }
        }
    }

    RewrittenSchemaSource {
        source: output,
        optional_fields,
    }
}

fn optional_bare_key_before(source: &str, question_index: usize) -> Option<(usize, &str)> {
    let bytes = source.as_bytes();
    let mut start = question_index;
    while start > 0 && is_bare_key_byte(bytes[start - 1]) {
        start -= 1;
    }
    (start < question_index).then(|| (start, &source[start..question_index]))
}

fn optional_key_delimiter_after(source: &str, mut index: usize) -> bool {
    while index < source.len() && source.as_bytes()[index].is_ascii_whitespace() {
        index += 1;
    }
    source[index..]
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, '=' | ']' | '.'))
}

fn is_bare_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn next_optional_marker(
    source: &str,
    optional_fields: &BTreeMap<String, String>,
    marker_index: &mut u32,
) -> String {
    loop {
        let marker = format!("__MOEL_OPTIONAL_FIELD_{}__", *marker_index);
        *marker_index += 1;
        if !source.contains(&marker) && !optional_fields.contains_key(&marker) {
            return marker;
        }
    }
}

fn repeated_ascii_char_len(source: &str, index: usize, target: char) -> usize {
    source[index..]
        .bytes()
        .take_while(|byte| *byte == target as u8)
        .count()
}

fn next_char(source: &str, index: usize) -> char {
    source[index..]
        .chars()
        .next()
        .expect("index is inside source")
}

fn push_next_char(source: &str, output: &mut String, index: &mut usize) {
    let ch = next_char(source, *index);
    output.push(ch);
    *index += ch.len_utf8();
}

fn schema_from_table(
    values: BTreeMap<String, Value>,
    path: String,
    optional_fields: &BTreeMap<String, String>,
) -> Result<Schema, SchemaError> {
    let mut fields = BTreeMap::new();

    for (raw_key, value) in values {
        let (key, optional) = match optional_fields.get(&raw_key) {
            Some(key) => (key.clone(), true),
            None => (raw_key, false),
        };
        let child_path = field_path(&path, &key);
        let field_schema = schema_from_value(value, child_path, optional_fields)?;
        let field_schema = if optional {
            Schema::Optional(Box::new(field_schema))
        } else {
            field_schema
        };

        if fields.insert(key.clone(), field_schema).is_some() {
            return Err(SchemaError::DuplicateFieldDeclaration {
                path,
                name: key,
            });
        }
    }

    Ok(Schema::Table(fields))
}

fn schema_from_value(
    value: Value,
    path: String,
    optional_fields: &BTreeMap<String, String>,
) -> Result<Schema, SchemaError> {
    match value {
        Value::String(name) => scalar_schema(&name).ok_or(SchemaError::UnknownType { path, name }),
        Value::Array(values) if values.is_empty() => Err(SchemaError::EmptyArray { path }),
        Value::Array(values) if values.len() != 1 => Err(SchemaError::ArrayMustHaveSingleElement {
            path,
            actual: values.len(),
        }),
        Value::Array(mut values) => {
            let item = values.pop().expect("array length checked above");
            Ok(Schema::Array(Box::new(schema_from_value(
                item,
                format!("{path}[0]"),
                optional_fields,
            )?)))
        }
        Value::Table(values) if is_enum_declaration(&values) => enum_schema(values, path),
        Value::Table(values) => schema_from_table(values, path, optional_fields),
        other => Err(SchemaError::InvalidSchemaValue {
            path,
            actual: value_kind(&other),
        }),
    }
}

fn scalar_schema(name: &str) -> Option<Schema> {
    Some(match name {
        "string" => Schema::String,
        "integer" => Schema::Integer,
        "float" => Schema::Float,
        "number" => Schema::Number,
        "boolean" => Schema::Boolean,
        "uuid" => Schema::Uuid,
        "utc" => Schema::Utc,
        "datetime" => Schema::Datetime,
        _ => return None,
    })
}

fn is_enum_declaration(values: &BTreeMap<String, Value>) -> bool {
    values.len() == 1 && values.contains_key("enum")
}

fn enum_schema(mut values: BTreeMap<String, Value>, path: String) -> Result<Schema, SchemaError> {
    let enum_value = values.remove("enum").expect("enum declaration checked");
    let values = match enum_value {
        Value::Array(values) => values,
        other => {
            return Err(SchemaError::EnumMustBeArray {
                path,
                actual: value_kind(&other),
            });
        }
    };

    if values.is_empty() {
        return Err(SchemaError::EmptyEnum { path });
    }

    let mut seen = BTreeSet::new();
    let mut allowed = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let value = match value {
            Value::String(value) => value,
            other => {
                return Err(SchemaError::EnumValueMustBeString {
                    path,
                    index,
                    actual: value_kind(&other),
                });
            }
        };

        if !seen.insert(value.clone()) {
            return Err(SchemaError::DuplicateEnumValue { path, value });
        }
        allowed.push(value);
    }

    Ok(Schema::Enum(allowed))
}

fn validate_at(value: &Value, schema: &Schema, path: &str, diagnostics: &mut Vec<Diagnostic>) {
    match schema {
        Schema::String => require_type(
            value,
            matches!(value, Value::String(_)),
            "string",
            path,
            diagnostics,
        ),
        Schema::Integer => require_type(
            value,
            matches!(value, Value::Integer(_)),
            "integer",
            path,
            diagnostics,
        ),
        Schema::Float => require_type(
            value,
            matches!(value, Value::Float(_)),
            "float",
            path,
            diagnostics,
        ),
        Schema::Number => require_type(
            value,
            matches!(value, Value::Integer(_) | Value::Float(_)),
            "number",
            path,
            diagnostics,
        ),
        Schema::Boolean => require_type(
            value,
            matches!(value, Value::Boolean(_)),
            "boolean",
            path,
            diagnostics,
        ),
        Schema::Uuid => require_type(
            value,
            matches!(value, Value::Uuid(_)),
            "uuid",
            path,
            diagnostics,
        ),
        Schema::Utc => require_type(
            value,
            matches!(value, Value::UtcTimestamp(_)),
            "utc",
            path,
            diagnostics,
        ),
        Schema::Datetime => require_type(
            value,
            matches!(value, Value::UtcTimestamp(_) | Value::TomlDatetime(_)),
            "datetime",
            path,
            diagnostics,
        ),
        Schema::Enum(allowed) => validate_enum(value, allowed, path, diagnostics),
        Schema::Array(item_schema) => {
            let Value::Array(values) = value else {
                type_mismatch(value, "array", path, diagnostics);
                return;
            };
            for (index, item) in values.iter().enumerate() {
                validate_at(item, item_schema, &format!("{path}[{index}]"), diagnostics);
            }
        }
        Schema::Optional(inner) => validate_at(value, inner, path, diagnostics),
        Schema::Table(fields) => {
            let Value::Table(values) = value else {
                type_mismatch(value, "table", path, diagnostics);
                return;
            };

            for (key, field_schema) in fields {
                let child_path = field_path(path, key);
                if let Some(field_value) = values.get(key) {
                    validate_at(field_value, field_schema, &child_path, diagnostics);
                } else if !matches!(field_schema, Schema::Optional(_)) {
                    diagnostics.push(Diagnostic {
                        path: child_path,
                        kind: DiagnosticKind::MissingField,
                    });
                }
            }

            for key in values.keys() {
                if !fields.contains_key(key) {
                    diagnostics.push(Diagnostic {
                        path: field_path(path, key),
                        kind: DiagnosticKind::UnexpectedField,
                    });
                }
            }
        }
    }
}

fn validate_enum(value: &Value, allowed: &[String], path: &str, diagnostics: &mut Vec<Diagnostic>) {
    let Value::String(actual) = value else {
        type_mismatch(value, "enum", path, diagnostics);
        return;
    };

    if !allowed.contains(actual) {
        diagnostics.push(Diagnostic {
            path: path.to_owned(),
            kind: DiagnosticKind::InvalidEnumValue {
                allowed: allowed.to_vec(),
                actual: actual.clone(),
            },
        });
    }
}

fn require_type(
    value: &Value,
    matches: bool,
    expected: &'static str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches {
        type_mismatch(value, expected, path, diagnostics);
    }
}

fn type_mismatch(
    value: &Value,
    expected: &'static str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic {
        path: path.to_owned(),
        kind: DiagnosticKind::TypeMismatch {
            expected,
            actual: value_kind(value),
        },
    });
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::UtcTimestamp(_) => "utc",
        Value::TomlDatetime(_) => "datetime",
        Value::Uuid(_) => "uuid",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
    }
}

fn field_path(parent: &str, key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{parent}[\"{escaped}\"]")
}
