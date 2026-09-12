# MOEL

MOEL is a small configuration and data language built as a strict superset of TOML.

The core compatibility rule is simple: **every valid TOML document must also be a valid MOEL document with the same TOML meaning**. MOEL then adds a small number of explicit data primitives and optional schema support where TOML would otherwise require conventions encoded as strings.

## Direction

- TOML compatibility first.
- Small, explicit extensions rather than a second unrelated syntax.
- First-class UUID values.
- UTC timestamps retain UTC semantics.
- Optional `schema.moel` validation without requiring schemas for ordinary documents.
- Deterministic parsing and serialization.
- A compact data format; no XML-style ceremony.

The first implementation is a Rust library and CLI-oriented parser core. See `docs/language.md` as the language contract grows.
