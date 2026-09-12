use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Error as ParseError, Value, parse};

pub const SCHEMA_FILE_NAME: &str = "schema.moel";

/// A semantic MOEL schema.
///
/// Schemas deliberately reuse MOEL values instead of introducing a second
/// parser or schema language. Tables describe table shapes, one-element arrays
/// describe homogeneous arrays, strings name scalar types, and enum declarations
/// constrain strings to an explicit set of values.
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
    Table(BTreeMap<String, Schema>),
}

#[derive(Debug)]
pub enum SchemaError {
    Parse(ParseError),
    RootMustBeTable,
    UnknownType { path: String, name: String },
    EmptyArray { path: String },
    ArrayMustHaveSingleElement { path: String, actual: usize },
    EmptyEnum { path: String },
    EnumMustBeArray { path: String, actual: &'static str },
    EnumValueMustBeString {
        path: String,
        index: usize,
        actual: &'static str,
    },
    DuplicateEnumValue { path: String, value: String },
    InvalidSchemaValue { path: String, actual: &'static str },
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
            Self::EmptyEnum { path } => {
                write!(f, "enum declaration at {path} must contain at least one value")
            }
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
            Self::DuplicateEnumValue { path, value } => {
                write!(f, "enum declaration at {path} contains duplicate value {value:?}")
            }
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
/// string enum.
pub fn parse_schema(source: &str) -> Result<Schema, SchemaError> {
    let value = parse(source)?;
    let Value::Table(values) = value else {
        return Err(SchemaError::RootMustBeTable);
    };

    schema_from_table(values, "$".to_owned())
}

/// Validate a parsed MOEL value against a schema.
///
/// Version 1 schemas are intentionally exact: every declared table field is
/// required and undeclared fields are rejected.
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

fn schema_from_table(values: BTreeMap<String, Value>, path: String) -> Result<Schema, SchemaError> {
    let fields = values
        .into_iter()
        .map(|(key, value)| {
            let child_path = field_path(&path, &key);
            Ok((key, schema_from_value(value, child_path)?))
        })
        .collect::<Result<BTreeMap<_, _>, SchemaError>>()?;

    Ok(Schema::Table(fields))
}

fn schema_from_value(value: Value, path: String) -> Result<Schema, SchemaError> {
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
            )?)))
        }
        Value::Table(values) if is_enum_declaration(&values) => enum_schema(values, path),
        Value::Table(values) => schema_from_table(values, path),
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
        let Value::String(value) = value else {
            return Err(SchemaError::EnumValueMustBeString {
                path,
                index,
                actual: value_kind(&value),
            });
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
        Schema::Table(fields) => {
            let Value::Table(values) = value else {
                type_mismatch(value, "table", path, diagnostics);
                return;
            };

            for (key, field_schema) in fields {
                let child_path = field_path(path, key);
                if let Some(field_value) = values.get(key) {
                    validate_at(field_value, field_schema, &child_path, diagnostics);
                } else {
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

fn validate_enum(
    value: &Value,
    allowed: &[String],
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
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
