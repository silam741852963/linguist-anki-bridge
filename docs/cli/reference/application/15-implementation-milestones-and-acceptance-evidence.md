# Application specification reference

## 15. Implementation milestones and acceptance evidence

1. **CLI foundation:** command tree, help/output/exit codes, configuration migration, storage, doctor, deck/model inspection. Build without Qt, Textual, Omarchy or mandatory browser dependencies.
2. **Durable preparation:** selectors/input parsing, duplicate decisions, source capture, versioned plans, conflict fingerprints, structured issues, reusable review/edit/export operations.
3. **Vocabulary and grammar parity:** Japanese/English dictionary structure, OCR-before-generation, user context, grammar screenshot/article extraction and unit segmentation, examples/exercises, media preservation, pronunciations and v2 models; prove live adapters as well as fixtures.
4. **Safe single-item apply/restore:** snapshots, collection identity, template policy, media verification, unknown-outcome reconciliation, before/after checks, recoverable restore.
5. **Jobs and migration:** bounded concurrency, leases, service pacing, pause/resume/retry, durable artifacts, reverse rollback, partial outcomes, read-only legacy audit/import.
6. **Release:** packaging, completion, manuals and migration guide, with all four vocabulary/grammar workflows verified; additional languages remain separately validated extensions.

Release gates must demonstrate:

| Scenario | Required result |
| --- | --- |
| Help/version, empty deck, missing optional provider | No unnecessary connections/mutations; useful output and accurate exit status. |
| Valid/invalid CSV, BOM/CRLF/quoted newline, aliases and repeated rows | Stable parsing, source locations, visible skips/errors and deterministic routing. |
| Exact/substring/HTML-wrapped/multiple existing matches | Correct final equality; ambiguity never selects a note automatically. |
| Rich Japanese dictionary and English lookup | Preserve source structure; no LLM definitions replacing dictionaries. |
| Revamp/add grammar, including screenshot-only front/back and articles | Preserve Japanese patterns/formation and original explanations; generate source-linked units/examples and optional useful application exercises. |
| Several grammar patterns in one screenshot | Reviewed unit expansion with an explicit anchor; no automatic cloning of mature card history. |
| Legacy vocabulary card migration | Match Comprehension/Production/Spelling by task; verify retained card IDs/history/scheduling with the native adapter. |
| Missing `createBackup` and unverified custom migration action | Use the verified backup/migration path or fail before mutation; action names alone do not prove semantics. |
| Screenshot with recoverable examples | OCR evidence is present before generation; examples retain source-language text and proper translations. |
| Mixed/uncertain images and replacement failure | Original useful images remain; per-image decisions are inspectable. |
| Multiple pronunciations and user recordings | Preserve tracks and locale/reading alignment; no automatic clearing on failure. |
| Shared logical mapping, null/empty and user Markdown | Accurate physical-field diff; stable media placeholders; edits survive restart. |
| Dry-run of apply/models/restore/jobs | No Anki write actions, including template/media actions. Local plan persistence is allowed. |
| Anki edit after preparation or before restore | Conflict; no silent overwriting of unrelated/newer changes. |
| Missing migration action or unexpected managed schema | Fail before note/media mutation; no delete/recreate fallback. |
| Failure after each commit/restore stage | Durable evidence and correctly reported compensation or resumable partial state. |
| Timeout/crash after `addNote` or split creation | Reconciliation prevents unreviewed duplicate creation; created IDs remain recoverable. |
| Restart during processing/commit/rollback | Safe paused/reconciled state; no automatic write resume or snapshot replacement. |
| Two processes, large jobs, stale/corrupt artifacts | One collection writer, bounded pages/memory, precise errors, retained recovery history. |
| JSON/JSONL, redirected output, SIGINT | Clean parseable stdout, stable records/codes, safe termination and next action. |
| Legacy config/jobs/snapshots import | Originals untouched; unsupported data visible; extensions preserved. |

Use deterministic provider fixtures and fake ports for failure injection, contract fixtures for cross-language parity, and CLI integration tests for subprocess/output behavior. Live Japanese/English provider checks prove transport/prompt/parser integration. Mutation and scheduling tests use a disposable Anki profile/collection. Do not create test notes in the user's real collection. Unit tests and compilation alone are not evidence of end-to-end provider success or Anki rendering/scheduling fidelity.
