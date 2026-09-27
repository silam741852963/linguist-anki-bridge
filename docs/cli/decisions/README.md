# Final implementation baseline

Design status: **finalized for implementation, 2026-09-26**. User delegated pending design choices. No further preference question is required to begin work. A failing implementation test still blocks its write capability; “finalized” does not mean tests have passed.

Read one decision topic as needed:

- [Native Anki bridge, identity and idempotency](native-bridge.md)
- [Storage, wire formats and lifecycle](storage-and-wire.md)
- [Learning, providers and source interpretation](learning-and-providers.md)
- [Command semantics and editor/setup choices](ux-and-operations.md)
- [Release gates and implementation sequence](release-gates.md)
- [Machine-readable gap/decision register](register.json)

These decisions supersede conflicting older proposals and reference-only review wording. Ordinary source-specific ambiguity remains an app review issue, not an unanswered architecture decision. Empirical uncertainties become named tests with an owner and fail-closed behavior.

Selected baseline: new root Rust workspace; Rust edition 2024 and declared MSRV 1.98.1, verified during initial delivery; separate MIT CLI and explicitly licensed Anki add-on; standard AnkiConnect reads plus a narrow versioned companion for managed writes; SQLite WAL/FULL and content-addressed archives; RFC 8785 canonical JSON; Tesseract baseline; local Ollama generation candidate gemma4:12b; rich Jisho/Wiktionary evidence; preserved media; focused opt-in tasks. No extra app server, automatic sync, collection SQL writes, source-media deletion, implicit resource downloads or destructive force flag.

Implementation may start with WP-01. WP-03 delivers the selected bridge and disposable verification; it no longer chooses between unrelated architectures. All 34 R decisions are closed in the register. All RV risks remain mapped to executable evidence gates. No production write is enabled until the relevant gate passes.

Standards versus choices: [RFC 8785](https://datatracker.ietf.org/doc/html/rfc8785) and [JSON Schema 2020-12](https://json-schema.org/draft/2020-12) define serialization/validation formats. [SQLite WAL](https://www.sqlite.org/wal.html), [Anki operation guidance](https://addon-docs.ankiweb.net/background-ops.html) and [Anki export semantics](https://docs.ankiweb.net/exporting.html) ground durability/native/recovery constraints. Bridge integration, language presets, model candidate, fixture thresholds and feature scope are explicit application decisions, not claims those sources prescribe this exact architecture.
