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

## 4. Schema direction

Schema support is optional. Ordinary MOEL and ordinary TOML documents must remain usable without a schema.

The intended convention is a nearby `schema.moel` file. The reference/discovery mechanism will be specified separately so it does not reserve an ordinary TOML data key by accident.

## 5. Determinism

Parsing and canonical serialization must be deterministic. Canonical serialization is semantic, not source-preserving: comments and original formatting are not part of the value tree.

## 6. Compatibility tests

The implementation should continuously test three boundaries:

1. representative TOML documents parse unchanged in meaning;
2. MOEL-only values round-trip through parse and canonical serialization;
3. extension-looking text inside TOML strings and comments is never treated as MOEL syntax.
