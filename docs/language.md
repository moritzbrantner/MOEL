# MOEL language contract

This document records the compatibility boundary before the language grows.

## 1. TOML is the base language

Every valid TOML document is valid MOEL. If a document uses no MOEL-only syntax, parsing it as MOEL preserves its TOML value meaning.

MOEL extensions must therefore be explicit. A new feature must not reinterpret an existing valid TOML token sequence as a different value.

## 2. UUID primitive

MOEL adds an explicitly tagged UUID literal:

```moel
request_id = uuid"550e8400-e29b-41d4-a716-446655440000"
```

The tag is deliberately outside TOML syntax, so a normal TOML string remains a string even if it happens to look like a UUID:

```toml
request_id = "550e8400-e29b-41d4-a716-446655440000"
```

UUIDs serialize canonically in lowercase hyphenated form.

## 3. UTC timestamp semantics

TOML already has date/time syntax. MOEL does not add a competing timestamp spelling. Instead, TOML offset date-times that are explicitly UTC (`Z` or `+00:00`) are represented by the semantic `UtcTimestamp` value variant.

Other valid TOML date/time forms remain valid and are represented as TOML date/time values. MOEL must not silently change a non-zero offset into UTC.

## 4. `schema.moel`

Schemas are optional. Calling the ordinary MOEL parser never requires or implicitly loads a schema.

The conventional schema for `path/to/config.moel` is the sibling file `path/to/schema.moel`. Discovery is deliberately limited to the same directory; implementations do not walk parent directories. A file named `schema.moel` does not discover itself.

This convention does not reserve any key in the data document. In particular, a normal document may contain a `schema`, `$schema`, or any other TOML key without changing MOEL behavior.

### 4.1 Schema syntax

A schema is itself a MOEL document. Its shape mirrors the shape of the data document:

```moel
name = "string"
id = "uuid"
created = "utc"
score = "number"
tags = ["string"]

[profile]
age = "integer"
active = "boolean"
```

Leaf strings are type names. Nested tables describe nested tables. A one-element array describes a homogeneous array whose every item must match the contained schema.

The initial scalar type set is:

- `any`: any MOEL value;
- `string`: TOML/MOEL string;
- `integer`: integer only;
- `float`: floating-point value only;
- `number`: integer or floating-point value;
- `boolean`: boolean;
- `uuid`: the explicit MOEL UUID primitive, not a UUID-looking string;
- `utc`: a date-time explicitly carrying UTC semantics;
- `datetime`: any TOML date/time value, including UTC values.

Unknown type names, empty schema arrays, arrays with more than one schema item, and ordinary data literals used as schema declarations fail closed as invalid schemas.

### 4.2 Initial table semantics

Version 1 table schemas are exact:

- every declared field is required;
- every undeclared field is rejected;
- nested tables follow the same rule;
- validation diagnostics identify the exact field or array item path.

Optional fields and explicitly open tables are intentionally deferred until their syntax can be added without weakening the simple shape model.

### 4.3 Validation boundary

Schema parsing and document parsing are distinct from validation. Implementations should preserve that distinction in diagnostics: malformed MOEL, malformed `schema.moel`, and a well-formed document that violates its schema are different failures.

A consumer that wants schema support resolves the sibling `schema.moel`, checks whether it exists, loads it, and validates. A consumer that does not opt into schema discovery can continue parsing ordinary TOML or MOEL exactly as before.

## 5. Determinism

Parsing and canonical serialization must be deterministic. Canonical serialization is semantic, not source-preserving: comments and original formatting are not part of the value tree.

Schema validation is also deterministic. Table traversal and diagnostic ordering must not depend on hash iteration order.

## 6. Compatibility tests

The implementation should continuously test these boundaries:

1. representative TOML documents parse unchanged in meaning;
2. MOEL-only values round-trip through parse and canonical serialization;
3. extension-looking text inside TOML strings and comments is never treated as MOEL syntax;
4. schema-less parsing never starts requiring `schema.moel`;
5. schema validation preserves UUID and UTC semantic distinctions;
6. malformed schemas fail closed rather than silently weakening validation.
