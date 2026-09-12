use std::collections::BTreeMap;
use std::fmt;

use uuid::Uuid;

/// Semantic MOEL value tree.
///
/// TOML values remain representable without loss of meaning. MOEL adds explicit
/// UUID values and distinguishes UTC timestamps from other TOML date/time forms.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    UtcTimestamp(String),
    TomlDatetime(String),
    Uuid(Uuid),
    Array(Vec<Value>),
    Table(BTreeMap<String, Value>),
}

#[derive(Debug)]
pub enum Error {
    Toml(toml::de::Error),
    Serialize(toml::ser::Error),
    InvalidUuid(String),
    UnterminatedUuidLiteral,
    RootMustBeTable,
    InvalidDatetime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "TOML parse error: {error}"),
            Self::Serialize(error) => write!(f, "TOML serialization error: {error}"),
            Self::InvalidUuid(value) => write!(f, "invalid UUID literal: {value}"),
            Self::UnterminatedUuidLiteral => write!(f, "unterminated UUID literal"),
            Self::RootMustBeTable => write!(f, "a MOEL document root must be a table"),
            Self::InvalidDatetime(value) => write!(f, "invalid TOML datetime: {value}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}

impl From<toml::ser::Error> for Error {
    fn from(value: toml::ser::Error) -> Self {
        Self::Serialize(value)
    }
}

/// Parse a MOEL document.
///
/// Documents without MOEL extensions are delegated directly to the TOML parser,
/// which makes TOML compatibility the baseline rather than a parallel grammar.
pub fn parse(source: &str) -> Result<Value, Error> {
    for salt in 0_u32.. {
        let rewritten = rewrite_uuid_literals(source, salt)?;
        let parsed: toml::Value = toml::from_str(&rewritten.source)?;

        if markers_are_unique(&parsed, &rewritten.uuids) {
            return Ok(from_toml(parsed, &rewritten.uuids));
        }
    }

    unreachable!("u32 marker space exhausted")
}

/// Serialize a semantic MOEL document into deterministic, canonical MOEL text.
/// UUIDs are emitted as `uuid"..."`; TOML-compatible values are serialized by
/// the TOML serializer.
pub fn to_string(value: &Value) -> Result<String, Error> {
    if !matches!(value, Value::Table(_)) {
        return Err(Error::RootMustBeTable);
    }

    let string_values = collect_strings(value);
    let (toml_value, markers) = to_toml(value, &string_values)?;
    let mut serialized = toml::to_string_pretty(&toml_value)?;

    for (marker, uuid) in markers {
        let quoted_marker = format!("\"{marker}\"");
        let uuid_literal = format!("uuid\"{}\"", uuid.hyphenated());
        serialized = serialized.replace(&quoted_marker, &uuid_literal);
    }

    Ok(serialized)
}

#[derive(Debug)]
struct RewrittenSource {
    source: String,
    uuids: BTreeMap<String, Uuid>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LexState {
    Normal,
    Comment,
    BasicString,
    LiteralString,
    MultilineBasicString,
    MultilineLiteralString,
}

fn rewrite_uuid_literals(source: &str, salt: u32) -> Result<RewrittenSource, Error> {
    let mut output = String::with_capacity(source.len());
    let mut uuids = BTreeMap::new();
    let mut state = LexState::Normal;
    let mut i = 0;
    let mut marker_index = 0_u32;

    while i < source.len() {
        match state {
            LexState::Normal => {
                if source[i..].starts_with('#') {
                    output.push('#');
                    i += 1;
                    state = LexState::Comment;
                    continue;
                }

                if source[i..].starts_with("\"\"\"") {
                    output.push_str("\"\"\"");
                    i += 3;
                    state = LexState::MultilineBasicString;
                    continue;
                }

                if source[i..].starts_with("'''") {
                    output.push_str("'''");
                    i += 3;
                    state = LexState::MultilineLiteralString;
                    continue;
                }

                if source[i..].starts_with('"') {
                    output.push('"');
                    i += 1;
                    state = LexState::BasicString;
                    continue;
                }

                if source[i..].starts_with('\'') {
                    output.push('\'');
                    i += 1;
                    state = LexState::LiteralString;
                    continue;
                }

                if source[i..].starts_with("uuid\"") && is_value_boundary(source, i) {
                    let body_start = i + 5;
                    let Some(relative_end) = source[body_start..].find('"') else {
                        return Err(Error::UnterminatedUuidLiteral);
                    };
                    let body_end = body_start + relative_end;
                    let body = &source[body_start..body_end];
                    let uuid =
                        Uuid::parse_str(body).map_err(|_| Error::InvalidUuid(body.to_owned()))?;
                    let marker = format!("__MOEL_UUID_{salt}_{marker_index}__");
                    marker_index += 1;
                    output.push('"');
                    output.push_str(&marker);
                    output.push('"');
                    uuids.insert(marker, uuid);
                    i = body_end + 1;
                    continue;
                }

                push_next_char(source, &mut output, &mut i);
            }
            LexState::Comment => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\n' {
                    state = LexState::Normal;
                }
            }
            LexState::BasicString => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\\' && i < source.len() {
                    push_next_char(source, &mut output, &mut i);
                } else if ch == '"' {
                    state = LexState::Normal;
                }
            }
            LexState::LiteralString => {
                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if ch == '\'' {
                    state = LexState::Normal;
                }
            }
            LexState::MultilineBasicString => {
                let quote_run = repeated_ascii_char_len(source, i, '"');
                if quote_run >= 3 {
                    output.push_str(&source[i..i + quote_run]);
                    i += quote_run;
                    state = LexState::Normal;
                } else {
                    let ch = next_char(source, i);
                    output.push(ch);
                    i += ch.len_utf8();
                    if ch == '\\' && i < source.len() {
                        push_next_char(source, &mut output, &mut i);
                    }
                }
            }
            LexState::MultilineLiteralString => {
                let quote_run = repeated_ascii_char_len(source, i, '\'');
                if quote_run >= 3 {
                    output.push_str(&source[i..i + quote_run]);
                    i += quote_run;
                    state = LexState::Normal;
                } else {
                    push_next_char(source, &mut output, &mut i);
                }
            }
        }
    }

    Ok(RewrittenSource {
        source: output,
        uuids,
    })
}

fn is_value_boundary(source: &str, index: usize) -> bool {
    source[..index]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, '=' | '[' | ','))
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

fn markers_are_unique(value: &toml::Value, markers: &BTreeMap<String, Uuid>) -> bool {
    markers
        .keys()
        .all(|marker| count_string(value, marker) == 1)
}

fn count_string(value: &toml::Value, target: &str) -> usize {
    match value {
        toml::Value::String(value) => usize::from(value == target),
        toml::Value::Array(values) => values.iter().map(|value| count_string(value, target)).sum(),
        toml::Value::Table(values) => values
            .values()
            .map(|value| count_string(value, target))
            .sum(),
        _ => 0,
    }
}

fn from_toml(value: toml::Value, uuids: &BTreeMap<String, Uuid>) -> Value {
    match value {
        toml::Value::String(value) => uuids
            .get(&value)
            .copied()
            .map(Value::Uuid)
            .unwrap_or(Value::String(value)),
        toml::Value::Integer(value) => Value::Integer(value),
        toml::Value::Float(value) => Value::Float(value),
        toml::Value::Boolean(value) => Value::Boolean(value),
        toml::Value::Datetime(value) => {
            let rendered = value.to_string();
            if rendered.ends_with('Z') || rendered.ends_with("+00:00") {
                Value::UtcTimestamp(rendered)
            } else {
                Value::TomlDatetime(rendered)
            }
        }
        toml::Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| from_toml(value, uuids))
                .collect(),
        ),
        toml::Value::Table(values) => Value::Table(
            values
                .into_iter()
                .map(|(key, value)| (key, from_toml(value, uuids)))
                .collect(),
        ),
    }
}

fn collect_strings(value: &Value) -> Vec<&str> {
    let mut strings = Vec::new();
    collect_strings_into(value, &mut strings);
    strings
}

fn collect_strings_into<'a>(value: &'a Value, strings: &mut Vec<&'a str>) {
    match value {
        Value::String(value) => strings.push(value),
        Value::Array(values) => {
            for value in values {
                collect_strings_into(value, strings);
            }
        }
        Value::Table(values) => {
            for (key, value) in values {
                strings.push(key);
                collect_strings_into(value, strings);
            }
        }
        _ => {}
    }
}

fn to_toml(
    value: &Value,
    existing_strings: &[&str],
) -> Result<(toml::Value, BTreeMap<String, Uuid>), Error> {
    let mut markers = BTreeMap::new();
    let mut next_marker = 0_u32;
    let toml_value = to_toml_inner(value, existing_strings, &mut markers, &mut next_marker)?;
    Ok((toml_value, markers))
}

fn to_toml_inner(
    value: &Value,
    existing_strings: &[&str],
    markers: &mut BTreeMap<String, Uuid>,
    next_marker: &mut u32,
) -> Result<toml::Value, Error> {
    Ok(match value {
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Integer(value) => toml::Value::Integer(*value),
        Value::Float(value) => toml::Value::Float(*value),
        Value::Boolean(value) => toml::Value::Boolean(*value),
        Value::UtcTimestamp(value) | Value::TomlDatetime(value) => parse_datetime(value)?,
        Value::Uuid(value) => {
            let marker = loop {
                let candidate = format!("__MOEL_SERIALIZED_UUID_{}__", *next_marker);
                *next_marker += 1;
                if !existing_strings.contains(&candidate.as_str())
                    && !markers.contains_key(&candidate)
                {
                    break candidate;
                }
            };
            markers.insert(marker.clone(), *value);
            toml::Value::String(marker)
        }
        Value::Array(values) => toml::Value::Array(
            values
                .iter()
                .map(|value| to_toml_inner(value, existing_strings, markers, next_marker))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Table(values) => toml::Value::Table(
            values
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        to_toml_inner(value, existing_strings, markers, next_marker)?,
                    ))
                })
                .collect::<Result<toml::map::Map<String, toml::Value>, Error>>()?,
        ),
    })
}

fn parse_datetime(value: &str) -> Result<toml::Value, Error> {
    let document: toml::Value = toml::from_str(&format!("value = {value}"))?;
    document
        .get("value")
        .cloned()
        .filter(|value| matches!(value, toml::Value::Datetime(_)))
        .ok_or_else(|| Error::InvalidDatetime(value.to_owned()))
}
