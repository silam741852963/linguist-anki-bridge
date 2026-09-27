# Application specification reference

## 16. User review register

All R01–R34 now have finalized choices in the [decision register](../../decisions/register.json). The table below preserves historical suggestions for traceability; it is not a pending approval queue. Current approaches and scope take priority in the [final baseline](../../decisions/README.md). Native evidence gates remain unrun until implementation.

| ID | Decision or uncertainty | Suggested default |
| --- | --- | --- |
| R01 | First-release languages and grammar scope | Resolved workflow scope: revamp/add vocabulary and grammar; Japanese/English validated first. |
| R02 | CLI implementation language and Python retirement | Rust CLI reusing libraries; Python retained during parity/migration. |
| R03 | Required backup type and failure policy | Durable per-operation snapshots; required collection backup for destructive migrations. |
| R04 | Executable name and coexistence with old Python command | Preserve canonical name at cutover; separately named preview during migration. |
| R05 | Write flags, plan approval, and prompts | `--apply` explicit intent; approval tied to revision; avoid redundant prompts. |
| R06 | Public exit codes and partial-result behavior | Adopt proposed stable code table and versioned JSON statuses. |
| R07 | Canonical config, precedence, prompt parity and model choice | Versioned TOML; complete read-only import with warnings; evaluate installed Gemma candidates and freeze chosen model/digest. |
| R08 | Automatic tessdata/resource downloads | Explicit installation/remedy; no hidden downloads during processing. |
| R09 | Notes whose cards belong to several decks | Require explicit destination and preserve observed memberships. |
| R10 | Review-state versus modernization-completeness filters | Rename current new/reviewed predicate; define modernization separately. |
| R11 | Expression cleanup/equality rules | HTML/entities/whitespace normalization only by default; preview destructive filters. |
| R12 | Meaning of Taiwanese language support | Label current capability Taiwanese Mandarin; decide Hokkien separately. |
| R13 | Duplicate creation, ambiguous matches and invalid input rows | Ambiguity blocks affected row; repeated rows skip; no implicit duplicates. |
| R14 | Lemma/correction acceptance | Explicit acceptance followed by lookup and duplicate re-resolution. |
| R15 | Multi-expression splitting and audio assignment | Explicit reviewed split; journal sibling creation and preserve original ID. |
| R16 | Classification implementation/thresholds/feedback | Conservative preservation; validated corpus and inspectable operator overrides. |
| R17 | Related-only dictionary results and sense choice | Review rather than silently treating first result as exact. |
| R18 | CLI browser fallback and optional Python helper | Optional headless adapter; direct-only limitation clearly reported until implemented. |
| R19 | Minimum content and partial-provider failure policy | Structured warning/error severity; meaningful dictionary-only cards may be accepted. |
| R20 | Kanji translation, LLM summary and embedded media | Preserve actual source language; deterministic details; track all media. |
| R21 | Image relevance, attribution and mandatory-image rules | Candidate review, provenance retention, preserve originals when uncertain. |
| R22 | Audio provider/voice priority | User recording → dictionary audio → selected local TTS → optional online fallback. |
| R23 | CLI field editor and Markdown parity | External editor plus structured patch; port safe Markdown/media placeholders. |
| R24 | Card templates with missing picture/audio and scheduling impact | v2 models with conditional opt-in tasks; retained legacy task mappings/history verified; no automatic card deletion. |
| R25 | Preview fidelity required in CLI | Field diff first; optional HTML export; no mandatory browser. |
| R26 | Collection/profile identity and writer lease scope | Active profile available but not unique; stronger adapter identity and before/after manifests; refuse unresolved mismatches. |
| R27 | Deleting media potentially shared by other notes | Retain physical originals unless reference-safe deletion is established. |
| R28 | Media naming and content collision strategy | Readable content-addressed names; hashes in manifests; preserve legacy imports. |
| R29 | Injection/split idempotency after unknown write outcomes | Persisted operation UUID/tag and intended state; query/reconcile before creation retry. |
| R30 | Shared-template recovery and backup frequency | Separate reviewed model transactions and conflict-aware model recovery. |
| R31 | Batch concurrency, pacing and retries | One ordered writer; bounded enrichment; retry transient failures only. |
| R32 | Job deletion, retention and cache cleanup | No implicit rollback; retain referenced snapshot/receipt/media evidence. |
| R33 | Legacy snapshot restore and force semantics | Explicit weaker-safety review; capture current state before any force. |
| R34 | XDG paths and storage migration | Separate state/cache/config; explicit migration, originals remain read-only. |
