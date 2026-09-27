# Application specification reference

## 10. Apply transaction and safety boundaries

AnkiConnect has separate actions, not a cross-action transaction. The target is a **recoverable operation with reconciliation**, not a claim of database-level atomicity.

For each approved item:

1. Acquire a collection-aware writer lease shared with jobs and restore. Bind the plan to the intended Anki profile/collection; verify identity or refuse unresolved identity mismatch. **REVIEW R26** defines the available identity mechanism.
2. Revalidate plan version/revision/approval, readiness, model/schema/action capabilities, and target deck. Fetch current source note/media and compare against reviewed before-state. Recheck injection duplicates and current model templates. Conflicts block the affected item.
3. Run the chosen backup policy. Persist full pre-write note fields/model/tags/deck and all media targets' actual before-state. Unknown media retrieval is an error, not `None`. Fsync the recovery record before any mutation.
4. Save an operation journal/intended after-state and stable operation ID. Stage media, verify what was stored, and record progress.
5. Apply a validated managed-template plan, including durable prior template/CSS evidence when changed. Apply the note update/migration or creation. Record newly returned note IDs immediately, including split siblings.
6. Read back note/media state and save post-write evidence. Only after the note points at valid replacements may obsolete references be removed; physical media deletion follows the policy below.
7. Mark the operation/snapshot/item complete durably and report its receipt. On failure, compensate reversible changes and record any failed compensation. Preserve all recovery evidence for manual or automatic reconciliation.

Before-state comparison must include tags/model/fields/deck membership and affected media; do not use only expression text. An external Anki edit between comparison and mutation remains possible because the API lacks a compare-and-swap operation. Document that limit and minimize the window; never promise total exclusion of Anki's own edits.

**SELECTED media ownership policy:** remove obsolete references from this note but retain physical originals by default. Anki media can be shared by other notes. Delete a file only if ownership/reference safety is established and before-data is recoverable. Existing code can delete obsolete files after replacement without a collection-wide reference proof: **REVIEW R27**.

Deterministic names must avoid unrelated-content collisions. Python names use sanitized expression/purpose/index and can collide across notes; Rust caches use various non-cryptographic keys. Recommend content-addressed media with a readable prefix and SHA-256 manifest, while preserving legacy filenames on import. **REVIEW R28**.

When an Anki write times out, its outcome is **unknown**. Reconcile recorded intended state before retry. For updates, compare current fields/model/media to before/after. For creations, use a stable, queryable operation marker or another reliable receipt strategy; ambiguous results must require review. Never blindly repeat `addNote` after an unknown outcome. **REVIEW R29** selects the marker strategy and its effect on tags.

Template mutation is collection-wide. Store a separate model recovery record; ordinary note rollback must not overwrite later template edits or delete templates used by other notes. **REVIEW R30** defines scope and backup frequency. Recommended first release: explicit model install/update transactions, with apply using already-reviewed compatible models when feasible.
