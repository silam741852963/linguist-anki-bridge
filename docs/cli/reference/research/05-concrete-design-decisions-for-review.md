# Research reference

## 5. Concrete design decisions for review

These are selected proposals, not questions that must all be answered before work. The user can amend them; correctness limitations still block unsafe writes.

1. Rust owns CLI/domain/application/jobs; optional Python helpers only for advanced OCR/browser integrations.
2. Two new regular model families, Vocabulary v2 and Grammar v2, with language-specific rendering. Existing managed v1 schemas are retained/imported; no silent in-place field-order overhaul of old models.
3. Old vocab card order is preserved and explicitly mapped; grammar Basic recognition history maps to the chosen anchor only. No maturity transfer to new sibling notes.
4. Primary workflows use `revamp vocab|grammar` and `add vocab|grammar`; preparation is write-free. `apply ... --apply` is explicit authorization, with no redundant routine confirmation.
5. Plans are immutable revisions with a content fingerprint; applied values cannot differ from the reviewed revision. External-editor changes create a new revision and invalidate old approval.
6. Canonical CLI config is versioned TOML; import Python YAML/native JSON read-only and emit a complete conversion report. Preserve custom prompts/settings, reject silent unsupported conversions. Runtime flags/environment overrides are not saved as edits.
7. Collection state/config/cache use XDG separation; no automatic historical-store rewrite. Read-only compatibility views and explicit migration are supported.
8. One collection writer; default at most four read/enrichment workers, one local model request, bounded Anki metadata pages of 200. Service start limits/retries remain configurable and measured.
9. Physical source media is retained; replacement concerns note references. Recovery artifacts/accepted media never live solely in disposable cache.
10. Every write uses a durable pre-state and operation journal; migration uses a tested scheduling-aware native adapter. Existing `updateNoteModel` reflection is insufficient.
11. Before first migration/model-change batch, require a verified recoverable backup with scheduling, plus app media snapshots. Reuse a batch checkpoint rather than exporting the deck per note.
12. Creation adds an operation tag `linguist::op::<uuid>`; retry searches it and checks intended state before adding again. Tags persist for audit and are removable only after retained receipt evidence supports safe cleanup. Duplicate expression resolution is separate from operation idempotency.
13. A restored note must still match its recorded post-state; conflicting current content blocks affected restoration. No automatic force; an explicit future override captures the current state first.
14. Images/audio are optional enrichment. Grammar needs no decorative picture. Meaning/pattern correctness and unresolved loss/conflict are blocking; missing optional enrichment is a warning.
15. Exact/fuzzy language handling is conservative. BCP-47 `ja`, `en`, `zh-TW`, `de`; current Taiwanese support means Taiwanese Mandarin. Grammar identity adds its reviewed use key; expression equality alone cannot identify all grammar uses.
16. English/Japanese vocabulary and grammar are complete first-release targets. Additional language adapters remain configurable but are not claimed production-ready until tested.

User review should focus on **field/model shape, default review tasks, explanation language, and acceptable split behavior**. The most important technical gate is native card-preserving migration. Implementational library choices are assistant-owned and reviewable through code/tests.
