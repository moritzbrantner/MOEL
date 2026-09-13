use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

use toml_edit::{Item, Table, Value as EditValue};

use crate::Error;
use crate::schema::{Diagnostic, DiagnosticKind, SchemaError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceSpan {
    fn from_range(source: &str, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end > source.len() {
            return None;
        }
        if !source.is_char_boundary(range.start) || !source.is_char_boundary(range.end) {
            return None;
        }

        let (line, column) = line_column(source, range.start);
        let (end_line, end_column) = line_column(source, range.end);
        Some(Self {
            start: range.start,
            end: range.end,
            line,
            column,
            end_line,
            end_column,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSource {
    Document,
    Schema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticSpan {
    pub source: DiagnosticSource,
    pub span: SourceSpan,
}

pub fn parse_error_span(source: &str, error: &Error) -> Option<SourceSpan> {
    let range = match error {
        Error::Toml(_) => {
            let masked = mask_uuid_literals(source);
            toml::from_str::<toml::Value>(&masked).err()?.span()?
        }
        Error::InvalidUuid(value) => locate_uuid_error(source, Some(value))?,
        Error::UnterminatedUuidLiteral => locate_uuid_error(source, None)?,
        _ => return None,
    };

    SourceSpan::from_range(source, range)
}

pub fn schema_error_span(source: &str, error: &SchemaError) -> Option<SourceSpan> {
    if let SchemaError::Parse(parse_error) = error {
        return match parse_error {
            Error::InvalidUuid(value) => {
                SourceSpan::from_range(source, locate_uuid_error(source, Some(value))?)
            }
            Error::UnterminatedUuidLiteral => {
                SourceSpan::from_range(source, locate_uuid_error(source, None)?)
            }
            Error::Toml(_) => {
                let rewritten = rewrite_optional_keys(source);
                let masked = mask_uuid_literals(&rewritten.source);
                let range = toml::from_str::<toml::Value>(&masked).err()?.span()?;
                SourceSpan::from_range(source, rewritten.original_range(range))
            }
            _ => None,
        };
    }

    let path = schema_error_path(error)?;
    schema_span_for_path(source, &path)
}

pub fn validation_span(
    document_source: &str,
    schema_source: &str,
    diagnostic: &Diagnostic,
) -> Option<DiagnosticSpan> {
    if !matches!(diagnostic.kind, DiagnosticKind::MissingField)
        && let Some(span) = document_span_for_path(document_source, &diagnostic.path)
    {
        return Some(DiagnosticSpan {
            source: DiagnosticSource::Document,
            span,
        });
    }

    if let Some(span) = schema_span_for_path(schema_source, &diagnostic.path) {
        return Some(DiagnosticSpan {
            source: DiagnosticSource::Schema,
            span,
        });
    }

    document_span_for_path(document_source, &diagnostic.path).map(|span| DiagnosticSpan {
        source: DiagnosticSource::Document,
        span,
    })
}

fn document_span_for_path(source: &str, path: &str) -> Option<SourceSpan> {
    let masked = mask_uuid_literals(source);
    let document = masked.parse::<toml_edit::DocumentMut>().ok()?;
    let mut spans = BTreeMap::new();
    collect_item(document.as_item(), "$", &BTreeMap::new(), None, &mut spans);
    SourceSpan::from_range(source, spans.get(path)?.clone())
}

fn schema_span_for_path(source: &str, path: &str) -> Option<SourceSpan> {
    let rewritten = rewrite_optional_keys(source);
    let masked = mask_uuid_literals(&rewritten.source);
    let document = masked.parse::<toml_edit::DocumentMut>().ok()?;
    let mut spans = BTreeMap::new();
    collect_item(
        document.as_item(),
        "$",
        &rewritten.optional_fields,
        Some(&rewritten),
        &mut spans,
    );
    SourceSpan::from_range(source, spans.get(path)?.clone())
}

fn collect_item(
    item: &Item,
    path: &str,
    optional_fields: &BTreeMap<String, String>,
    rewrite: Option<&SchemaRewrite>,
    spans: &mut BTreeMap<String, Range<usize>>,
) {
    record_span(item.span(), path, rewrite, spans);
    match item {
        Item::None => {}
        Item::Value(value) => collect_value(value, path, optional_fields, rewrite, spans),
        Item::Table(table) => collect_table(table, path, optional_fields, rewrite, spans),
        Item::ArrayOfTables(tables) => {
            for (index, table) in tables.iter().enumerate() {
                collect_table(
                    table,
                    &format!("{path}[{index}]"),
                    optional_fields,
                    rewrite,
                    spans,
                );
            }
        }
    }
}

fn collect_table(
    table: &Table,
    path: &str,
    optional_fields: &BTreeMap<String, String>,
    rewrite: Option<&SchemaRewrite>,
    spans: &mut BTreeMap<String, Range<usize>>,
) {
    record_span(table.span(), path, rewrite, spans);
    for (key, item) in table.iter() {
        let key = normalize_key(key, optional_fields);
        let child_path = field_path(path, &key);
        collect_item(item, &child_path, optional_fields, rewrite, spans);
    }
}

fn collect_value(
    value: &EditValue,
    path: &str,
    optional_fields: &BTreeMap<String, String>,
    rewrite: Option<&SchemaRewrite>,
    spans: &mut BTreeMap<String, Range<usize>>,
) {
    record_span(value.span(), path, rewrite, spans);
    match value {
        EditValue::Array(array) => {
            for (index, value) in array.iter().enumerate() {
                collect_value(
                    value,
                    &format!("{path}[{index}]"),
                    optional_fields,
                    rewrite,
                    spans,
                );
            }
        }
        EditValue::InlineTable(table) => {
            for (key, value) in table.iter() {
                let key = normalize_key(key, optional_fields);
                collect_value(
                    value,
                    &field_path(path, &key),
                    optional_fields,
                    rewrite,
                    spans,
                );
            }
        }
        _ => {}
    }
}

fn record_span(
    span: Option<Range<usize>>,
    path: &str,
    rewrite: Option<&SchemaRewrite>,
    spans: &mut BTreeMap<String, Range<usize>>,
) {
    let Some(span) = span else {
        return;
    };
    let span = rewrite.map_or(span.clone(), |rewrite| rewrite.original_range(span));
    spans.insert(path.to_owned(), span);
}

fn normalize_key<'a>(key: &'a str, optional_fields: &'a BTreeMap<String, String>) -> Cow<'a, str> {
    optional_fields
        .get(key)
        .map(|key| Cow::Owned(key.clone()))
        .unwrap_or_else(|| Cow::Borrowed(key))
}

fn schema_error_path(error: &SchemaError) -> Option<String> {
    match error {
        SchemaError::Parse(_) => None,
        SchemaError::RootMustBeTable => Some("$".to_owned()),
        SchemaError::UnknownType { path, .. }
        | SchemaError::EmptyArray { path }
        | SchemaError::ArrayMustHaveSingleElement { path, .. }
        | SchemaError::InvalidSchemaValue { path, .. } => Some(path.clone()),
        SchemaError::EmptyEnum { path }
        | SchemaError::EnumMustBeArray { path, .. }
        | SchemaError::DuplicateEnumValue { path, .. } => Some(field_path(path, "enum")),
        SchemaError::EnumValueMustBeString { path, index, .. } => {
            Some(format!("{}[{index}]", field_path(path, "enum")))
        }
        SchemaError::DuplicateFieldDeclaration { path, name } => Some(field_path(path, name)),
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, line)| line)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn field_path(parent: &str, key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{parent}[\"{escaped}\"]")
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

fn mask_uuid_literals(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut state = LexState::Normal;
    let mut expects_value = false;
    let mut i = 0;

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
                    expects_value = false;
                    output.push_str("\"\"\"");
                    i += 3;
                    state = LexState::MultilineBasicString;
                    continue;
                }
                if source[i..].starts_with("'''") {
                    expects_value = false;
                    output.push_str("'''");
                    i += 3;
                    state = LexState::MultilineLiteralString;
                    continue;
                }
                if source[i..].starts_with('"') {
                    expects_value = false;
                    output.push('"');
                    i += 1;
                    state = LexState::BasicString;
                    continue;
                }
                if source[i..].starts_with('\'') {
                    expects_value = false;
                    output.push('\'');
                    i += 1;
                    state = LexState::LiteralString;
                    continue;
                }
                if source[i..].starts_with("uuid\"") && expects_value {
                    let body_start = i + 5;
                    if let Some(relative_end) = source[body_start..].find('"') {
                        let literal_end = body_start + relative_end + 1;
                        push_string_placeholder(&mut output, literal_end - i);
                        i = literal_end;
                        expects_value = false;
                        continue;
                    }
                }

                let ch = next_char(source, i);
                output.push(ch);
                i += ch.len_utf8();
                if !ch.is_whitespace() {
                    expects_value = match ch {
                        '=' | ',' => true,
                        '[' => expects_value,
                        _ => false,
                    };
                }
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

    output
}

fn locate_uuid_error(source: &str, invalid_value: Option<&str>) -> Option<Range<usize>> {
    let mut state = LexState::Normal;
    let mut expects_value = false;
    let mut i = 0;

    while i < source.len() {
        match state {
            LexState::Normal => {
                if source[i..].starts_with('#') {
                    i += 1;
                    state = LexState::Comment;
                    continue;
                }
                if source[i..].starts_with("\"\"\"") {
                    expects_value = false;
                    i += 3;
                    state = LexState::MultilineBasicString;
                    continue;
                }
                if source[i..].starts_with("'''") {
                    expects_value = false;
                    i += 3;
                    state = LexState::MultilineLiteralString;
                    continue;
                }
                if source[i..].starts_with('"') {
                    expects_value = false;
                    i += 1;
                    state = LexState::BasicString;
                    continue;
                }
                if source[i..].starts_with('\'') {
                    expects_value = false;
                    i += 1;
                    state = LexState::LiteralString;
                    continue;
                }
                if source[i..].starts_with("uuid\"") && expects_value {
                    let body_start = i + 5;
                    let Some(relative_end) = source[body_start..].find('"') else {
                        return invalid_value.is_none().then_some(i..source.len());
                    };
                    let body_end = body_start + relative_end;
                    if invalid_value.is_some_and(|value| value == &source[body_start..body_end]) {
                        return Some(i..body_end + 1);
                    }
                    i = body_end + 1;
                    expects_value = false;
                    continue;
                }

                let ch = next_char(source, i);
                i += ch.len_utf8();
                if !ch.is_whitespace() {
                    expects_value = match ch {
                        '=' | ',' => true,
                        '[' => expects_value,
                        _ => false,
                    };
                }
            }
            LexState::Comment => {
                let ch = next_char(source, i);
                i += ch.len_utf8();
                if ch == '\n' {
                    state = LexState::Normal;
                }
            }
            LexState::BasicString => {
                let ch = next_char(source, i);
                i += ch.len_utf8();
                if ch == '\\' && i < source.len() {
                    i += next_char(source, i).len_utf8();
                } else if ch == '"' {
                    state = LexState::Normal;
                }
            }
            LexState::LiteralString => {
                let ch = next_char(source, i);
                i += ch.len_utf8();
                if ch == '\'' {
                    state = LexState::Normal;
                }
            }
            LexState::MultilineBasicString => {
                let quote_run = repeated_ascii_char_len(source, i, '"');
                if quote_run >= 3 {
                    i += quote_run;
                    state = LexState::Normal;
                } else {
                    let ch = next_char(source, i);
                    i += ch.len_utf8();
                    if ch == '\\' && i < source.len() {
                        i += next_char(source, i).len_utf8();
                    }
                }
            }
            LexState::MultilineLiteralString => {
                let quote_run = repeated_ascii_char_len(source, i, '\'');
                if quote_run >= 3 {
                    i += quote_run;
                    state = LexState::Normal;
                } else {
                    i += next_char(source, i).len_utf8();
                }
            }
        }
    }

    None
}

fn push_string_placeholder(output: &mut String, literal_len: usize) {
    output.push('"');
    for _ in 0..literal_len.saturating_sub(2) {
        output.push('_');
    }
    output.push('"');
}

#[derive(Debug)]
struct SchemaRewrite {
    source: String,
    optional_fields: BTreeMap<String, String>,
    replacements: Vec<Replacement>,
}

#[derive(Debug)]
struct Replacement {
    rewritten: Range<usize>,
    original: Range<usize>,
}

impl SchemaRewrite {
    fn original_range(&self, range: Range<usize>) -> Range<usize> {
        self.original_offset(range.start, false)..self.original_offset(range.end, true)
    }

    fn original_offset(&self, offset: usize, end_bias: bool) -> usize {
        let mut delta = 0_isize;
        for replacement in &self.replacements {
            if offset < replacement.rewritten.start {
                break;
            }
            if offset <= replacement.rewritten.end {
                if offset == replacement.rewritten.start {
                    return replacement.original.start;
                }
                if offset == replacement.rewritten.end {
                    return replacement.original.end;
                }
                return if end_bias {
                    replacement.original.end
                } else {
                    replacement.original.start
                };
            }
            delta += replacement.original.len() as isize - replacement.rewritten.len() as isize;
        }
        (offset as isize + delta) as usize
    }
}

fn rewrite_optional_keys(source: &str) -> SchemaRewrite {
    let mut output = String::with_capacity(source.len());
    let mut optional_fields = BTreeMap::new();
    let mut replacements = Vec::new();
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

                if source[i..].starts_with('?')
                    && let Some((key_start, key)) = optional_bare_key_before(source, i)
                    && optional_key_delimiter_after(source, i + 1)
                {
                    let key_len = i - key_start;
                    let rewritten_start = output.len() - key_len;
                    output.truncate(rewritten_start);
                    let marker = next_optional_marker(source, &optional_fields, &mut marker_index);
                    let replacement = format!("\"{marker}\"");
                    output.push_str(&replacement);
                    replacements.push(Replacement {
                        rewritten: rewritten_start..rewritten_start + replacement.len(),
                        original: key_start..i + 1,
                    });
                    optional_fields.insert(marker, key.to_owned());
                    i += 1;
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

    SchemaRewrite {
        source: output,
        optional_fields,
        replacements,
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
        let marker = format!("__MOEL_DIAGNOSTIC_OPTIONAL_{}__", *marker_index);
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
