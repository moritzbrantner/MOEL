use std::collections::btree_map;
use std::fmt;

use serde::Deserialize;
use serde::de::value::StringDeserializer;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde_path_to_error::Segment;
use uuid::Uuid as RawUuid;

use crate::diagnostics::{SourceSpan, document_span_for_path, parse_error_span};
use crate::{Value, parse};

const UUID_MARKER: &str = "\0MOEL::Uuid\0";
const UTC_TIMESTAMP_MARKER: &str = "\0MOEL::UtcTimestamp\0";
const TOML_DATETIME_MARKER: &str = "\0MOEL::TomlDatetime\0";

fn encode_semantic(marker: &str, value: &str) -> String {
    let mut encoded = String::with_capacity(marker.len() + value.len());
    encoded.push_str(marker);
    encoded.push_str(value);
    encoded
}

fn decode_semantic<E>(encoded: String, marker: &str, expected: &str) -> Result<String, E>
where
    E: de::Error,
{
    encoded
        .strip_prefix(marker)
        .map(str::to_owned)
        .ok_or_else(|| E::custom(format!("expected {expected}")))
}

/// A UUID that can only be deserialized from MOEL's explicit UUID primitive.
///
/// This wrapper deliberately does not accept an ordinary string that merely looks
/// like a UUID. Use MoelUuid::into_inner when an API requires uuid::Uuid.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MoelUuid(RawUuid);

impl MoelUuid {
    pub fn as_inner(&self) -> &RawUuid {
        &self.0
    }

    pub fn into_inner(self) -> RawUuid {
        self.0
    }
}

impl fmt::Display for MoelUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<RawUuid> for MoelUuid {
    fn from(value: RawUuid) -> Self {
        Self(value)
    }
}

impl From<MoelUuid> for RawUuid {
    fn from(value: MoelUuid) -> Self {
        value.0
    }
}

impl AsRef<RawUuid> for MoelUuid {
    fn as_ref(&self) -> &RawUuid {
        &self.0
    }
}

impl<'de> Deserialize<'de> for MoelUuid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct UuidVisitor;

        impl<'de> Visitor<'de> for UuidVisitor {
            type Value = MoelUuid;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an explicit MOEL UUID")
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                let encoded = String::deserialize(deserializer)?;
                let value = decode_semantic(encoded, UUID_MARKER, "an explicit MOEL UUID")?;
                RawUuid::parse_str(&value)
                    .map(MoelUuid)
                    .map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(UuidVisitor)
    }
}

/// A TOML offset date-time that was explicitly UTC (Z or +00:00).
///
/// Non-zero offsets and local TOML date/time values do not deserialize into this
/// type, so direct typed deserialization preserves MOEL's UTC distinction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct TimestampVisitor;

        impl<'de> Visitor<'de> for TimestampVisitor {
            type Value = UtcTimestamp;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a MOEL UTC timestamp")
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                let encoded = String::deserialize(deserializer)?;
                decode_semantic(encoded, UTC_TIMESTAMP_MARKER, "a MOEL UTC timestamp")
                    .map(UtcTimestamp)
            }
        }

        deserializer.deserialize_any(TimestampVisitor)
    }
}

/// A valid TOML date/time value that is not explicitly UTC.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TomlDatetime(String);

impl TomlDatetime {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for TomlDatetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for TomlDatetime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct DatetimeVisitor;

        impl<'de> Visitor<'de> for DatetimeVisitor {
            type Value = TomlDatetime;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a non-UTC TOML date/time value")
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                let encoded = String::deserialize(deserializer)?;
                decode_semantic(
                    encoded,
                    TOML_DATETIME_MARKER,
                    "a non-UTC TOML date/time value",
                )
                .map(TomlDatetime)
            }
        }

        deserializer.deserialize_any(DatetimeVisitor)
    }
}

/// Failure to parse MOEL or deserialize its semantic value tree into a Rust type.
#[derive(Debug)]
pub enum DeserializeError {
    Parse {
        error: Box<crate::Error>,
        span: Option<SourceSpan>,
    },
    Data {
        path: String,
        message: String,
        span: Option<SourceSpan>,
    },
}

impl DeserializeError {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Parse { .. } => None,
            Self::Data { path, .. } => Some(path),
        }
    }

    pub fn span(&self) -> Option<&SourceSpan> {
        match self {
            Self::Parse { span, .. } | Self::Data { span, .. } => span.as_ref(),
        }
    }
}

impl fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                error,
                span: Some(span),
            } => write!(
                f,
                "{error} at {}:{} (bytes {}..{})",
                span.line, span.column, span.start, span.end
            ),
            Self::Parse { error, span: None } => error.fmt(f),
            Self::Data {
                path,
                message,
                span: Some(span),
            } => write!(
                f,
                "typed deserialization error at {path}, {}:{} (bytes {}..{}): {message}",
                span.line, span.column, span.start, span.end
            ),
            Self::Data {
                path,
                message,
                span: None,
            } => write!(f, "typed deserialization error at {path}: {message}"),
        }
    }
}

impl std::error::Error for DeserializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse { error, .. } => Some(error.as_ref()),
            Self::Data { .. } => None,
        }
    }
}

/// Parse a MOEL document and deserialize it directly into an owned Rust value.
///
/// This never discovers or loads schema.moel; it has the same parsing boundary
/// as crate::parse. Use MoelUuid, UtcTimestamp, and TomlDatetime when the
/// corresponding MOEL semantic distinctions must remain explicit.
pub fn from_str<T>(source: &str) -> Result<T, DeserializeError>
where
    T: DeserializeOwned,
{
    let value = parse(source).map_err(|error| {
        let span = parse_error_span(source, &error);
        DeserializeError::Parse {
            error: Box::new(error),
            span,
        }
    })?;

    deserialize_tracked(value).map_err(|(path, message)| DeserializeError::Data {
        span: document_span_for_path(source, &path),
        path,
        message,
    })
}

/// Deserialize an already parsed semantic MOEL value into an owned Rust value.
pub fn from_value<T>(value: Value) -> Result<T, DeserializeError>
where
    T: DeserializeOwned,
{
    deserialize_tracked(value).map_err(|(path, message)| DeserializeError::Data {
        path,
        message,
        span: None,
    })
}

fn deserialize_tracked<T>(value: Value) -> Result<T, (String, String)>
where
    T: DeserializeOwned,
{
    match serde_path_to_error::deserialize(ValueDeserializer::new(value)) {
        Ok(value) => Ok(value),
        Err(error) => {
            let path = path_to_moel(error.path());
            let message = error.into_inner().to_string();
            Err((path, message))
        }
    }
}

fn path_to_moel(path: &serde_path_to_error::Path) -> String {
    let mut output = String::from("$");
    for segment in path.iter() {
        match segment {
            Segment::Seq { index } => output.push_str(&format!("[{index}]")),
            Segment::Map { key } => {
                let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
                output.push_str(&format!("[\"{escaped}\"]"));
            }
            Segment::Enum { variant } => {
                let escaped = variant.replace('\\', "\\\\").replace('"', "\\"");
                output.push_str(&format!("[\"{escaped}\"]"));
            }
            Segment::Unknown => {}
        }
    }
    output
}

#[derive(Debug)]
struct ValueError(String);

impl ValueError {
    fn type_mismatch(expected: &str, value: &Value) -> Self {
        Self(format!("expected {expected}, found {}", value_kind(value)))
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for ValueError {}

impl de::Error for ValueError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self(message.to_string())
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::UtcTimestamp(_) => "UTC timestamp",
        Value::TomlDatetime(_) => "TOML date/time",
        Value::Uuid(_) => "UUID",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
    }
}

struct ValueDeserializer {
    value: Value,
}

impl ValueDeserializer {
    fn new(value: Value) -> Self {
        Self { value }
    }
}

impl<'de> de::Deserializer<'de> for ValueDeserializer {
    type Error = ValueError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_string(value),
            Value::Integer(value) => visitor.visit_i64(value),
            Value::Float(value) => visitor.visit_f64(value),
            Value::Boolean(value) => visitor.visit_bool(value),
            Value::Array(values) => visitor.visit_seq(SeqDeserializer::new(values)),
            Value::Table(values) => visitor.visit_map(MapDeserializer::new(values)),
            Value::Uuid(value) => visitor.visit_newtype_struct(
                StringDeserializer::<ValueError>::new(encode_semantic(
                    UUID_MARKER,
                    &value.hyphenated().to_string(),
                )),
            ),
            Value::UtcTimestamp(value) => visitor.visit_newtype_struct(
                StringDeserializer::<ValueError>::new(encode_semantic(
                    UTC_TIMESTAMP_MARKER,
                    &value,
                )),
            ),
            Value::TomlDatetime(value) => visitor.visit_newtype_struct(
                StringDeserializer::<ValueError>::new(encode_semantic(
                    TOML_DATETIME_MARKER,
                    &value,
                )),
            ),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Boolean(value) => visitor.visit_bool(value),
            value => Err(ValueError::type_mismatch("boolean", &value)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Integer(value) => visitor.visit_i64(value),
            value => Err(ValueError::type_mismatch("integer", &value)),
        }
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Integer(value) => visitor.visit_i128(i128::from(value)),
            value => Err(ValueError::type_mismatch("integer", &value)),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Integer(value) if value >= 0 => visitor.visit_u64(value as u64),
            Value::Integer(_) => Err(ValueError(
                "expected unsigned integer, found negative integer".into(),
            )),
            value => Err(ValueError::type_mismatch("unsigned integer", &value)),
        }
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Integer(value) if value >= 0 => visitor.visit_u128(value as u128),
            Value::Integer(_) => Err(ValueError(
                "expected unsigned integer, found negative integer".into(),
            )),
            value => Err(ValueError::type_mismatch("unsigned integer", &value)),
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Float(value) => visitor.visit_f64(value),
            value => Err(ValueError::type_mismatch("float", &value)),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => {
                let mut chars = value.chars();
                let Some(ch) = chars.next() else {
                    return Err(ValueError("expected character, found empty string".into()));
                };
                if chars.next().is_some() {
                    return Err(ValueError(
                        "expected character, found multi-character string".into(),
                    ));
                }
                visitor.visit_char(ch)
            }
            value => Err(ValueError::type_mismatch("character", &value)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_string(value),
            value => Err(ValueError::type_mismatch("string", &value)),
        }
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(ValueError::type_mismatch("byte string", &self.value))
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(ValueError::type_mismatch("byte string", &self.value))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(ValueError::type_mismatch("unit", &self.value))
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(SeqDeserializer::new(values)),
            value => Err(ValueError::type_mismatch("array", &value)),
        }
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) if values.len() == len => {
                visitor.visit_seq(SeqDeserializer::new(values))
            }
            Value::Array(values) => Err(ValueError(format!(
                "expected tuple of length {len}, found array of length {}",
                values.len()
            ))),
            value => Err(ValueError::type_mismatch("array", &value)),
        }
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Table(values) => visitor.visit_map(MapDeserializer::new(values)),
            value => Err(ValueError::type_mismatch("table", &value)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(variant) => visitor.visit_enum(EnumDeserializer {
                variant,
                value: None,
            }),
            Value::Table(values) if values.len() == 1 => {
                let (variant, value) = values
                    .into_iter()
                    .next()
                    .expect("one-entry table checked above");
                visitor.visit_enum(EnumDeserializer {
                    variant,
                    value: Some(value),
                })
            }
            value => Err(ValueError::type_mismatch(
                "string or single-entry table for enum",
                &value,
            )),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

struct SeqDeserializer {
    values: std::vec::IntoIter<Value>,
}

impl SeqDeserializer {
    fn new(values: Vec<Value>) -> Self {
        Self {
            values: values.into_iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = ValueError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.values
            .next()
            .map(|value| seed.deserialize(ValueDeserializer::new(value)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

struct MapDeserializer {
    entries: btree_map::IntoIter<String, Value>,
    value: Option<Value>,
}

impl MapDeserializer {
    fn new(values: std::collections::BTreeMap<String, Value>) -> Self {
        Self {
            entries: values.into_iter(),
            value: None,
        }
    }
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = ValueError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(StringDeserializer::<ValueError>::new(key))
            .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .ok_or_else(|| ValueError("map value requested before map key".into()))?;
        seed.deserialize(ValueDeserializer::new(value))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len() + usize::from(self.value.is_some()))
    }
}

struct EnumDeserializer {
    variant: String,
    value: Option<Value>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = ValueError;
    type Variant = VariantDeserializer;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(StringDeserializer::<ValueError>::new(self.variant))?;
        Ok((variant, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer {
    value: Option<Value>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = ValueError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            None => Ok(()),
            Some(value) => Err(ValueError::type_mismatch("unit enum variant", &value)),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| ValueError("expected value for enum variant".into()))?;
        seed.deserialize(ValueDeserializer::new(value))
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| ValueError("expected tuple value for enum variant".into()))?;
        de::Deserializer::deserialize_tuple(ValueDeserializer::new(value), len, visitor)
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = self
            .value
            .ok_or_else(|| ValueError("expected table value for enum variant".into()))?;
        de::Deserializer::deserialize_struct(
            ValueDeserializer::new(value),
            "enum variant",
            fields,
            visitor,
        )
    }
}
