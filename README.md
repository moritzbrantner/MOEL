# MOEL

MOEL is a small configuration and data language built as a strict superset of TOML.

The core compatibility rule is simple: **every valid TOML document must also be a valid MOEL document with the same TOML meaning**. MOEL then adds a small number of explicit data primitives and optional schema support where TOML would otherwise require conventions encoded as strings.

## Direction

- TOML compatibility first.
- Small, explicit extensions rather than a second unrelated syntax.
- First-class UUID values.
- UTC timestamps retain UTC semantics.
- Optional sibling `schema.moel` validation without reserving a data-document key.
- Deterministic parsing, serialization, and validation.
- A compact data format; no XML-style ceremony.

## Schema example

A data document:

```moel
name = "MOEL"
id = uuid"550e8400-e29b-41d4-a716-446655440000"
created = 2026-09-12T20:20:00Z
status = "draft"
```

Its optional sibling `schema.moel`:

```moel
name = "string"
id = "uuid"
created = "utc"
nickname? = "string"
status? = { enum = ["draft", "published", "archived"] }
tags? = ["string"]
```

Tables remain closed by default: undeclared fields are rejected. Fields are required unless their bare schema key ends in `?`; if an optional field is present, its declared type, enum, array, or nested-table schema is still enforced normally. Ordinary parsing never requires a schema.

The implementation is a Rust library and CLI-oriented parser core. See `docs/language.md` for the language and schema contract.
