# Domain contracts

## Core records

| Record | Required data |
| --- | --- |
| ResolvedSettings | version, typed values, per-key source, profile/purpose, resource hashes, secret reference names only, effective fingerprint |
| SourceRecord | source ID/kind/location hash, full text/fields, model and template manifest, tags, card→deck memberships, media references, capture time/digest |
| SourceRegion | image digest, rectangle/polygon, reading order, OCR engine/settings/text/confidence, literal source language |
| SourceArchive | immutable original fields/text plus durable referenced image/audio bytes; original names and all associations |
| Evidence | entry/sense/formation/example objects; source ID/region/span; actual provider/language; ambiguity markers |
| LearningDocumentV2 | shared ID/kind/languages; typed vocab or grammar body; evidence/archive refs; user edits; chosen tasks; render settings; issues/media |
| FieldIntent | `Keep` (source exists only), `Set(value)` or `Clear`; omission is not clearing; effective output always computed before apply |
| Issue | code, severity `warning|error|review`, stage/item/field/source refs, actionable message, resolution requirements |
| ReviewDecision | explicit chosen sense/note/anchor/image/cue/interpretation, input fingerprint, actor/time; invalid after input changes |
| MediaAsset | safe intended filename, bytes digest, size, decoded MIME, original filename, owner `source|app|external`, attribution/provider, retention refs |
| PlanRevision | ID/revision/parent/digest, status, frozen settings/binding/selection, item documents/actions, assets, model plans, review decisions |
| Approval | plan digest/revision + item IDs + actor/time + explicitly accepted warning codes; no write permission by itself |
| OperationJournal | operation/group/child IDs, approved digest, source pre-state, desired post-state, backup/snapshot refs, ordered per-step intents/evidence, state/errors |
| Snapshot | immutable original fields/model/tags/decks/media; card-task/scheduler evidence; journal and verified post-state links; restore receipts |
| Job | ID/mode `prepare|simulate|apply`, ordered immutable item refs, frozen settings, approval scope, controls/lease, progress/errors |
| BackupReceipt | file/provider ID, binding, creation time, scope, scheduling/media/schema coverage, bytes/checksum, verification/restore-test evidence |
| NativeOperationReceipt | lineage/operation UUID/payload digest; queued/running/unknown/verified state; observed native IDs/history/manifests; durable evidence hash |
| ResumeBindingDecision | original operation/approval identity, old/new session epoch, matching lineage, current observed-state digest, actor/time and authorization scope |
| CapabilityReport | protocol/build/resource versions, per-action supported/tested gate state, auth/identity/serialization constraints, actionable failure code |
| GateEvidence | gate ID, version matrix, fixture/input hashes, actual commands/assertions/results and artifact references; not_run is never pass |

Secrets never appear in durable settings/logs/export by default. Inputs needed to reproduce generation can contain private material: store them privately in plan data, not routine progress logs.

## Selection receipt

A v2 plan may include `selection` with a version-1 receipt: purpose, tagged `selector` (`note_ids`, `query`, or `deck` with original name and compiled query), normalized `matched_note_ids`, ordered `selected_note_ids`, `order` and `max_notes`. CLI revamp writes it before publishing its revision. Older plans omit the optional field, including during serialization, so their approval projection remains unchanged.

Validate nonempty unique canonical decimal IDs in the Anki v6 safe integer range, supported purpose, version 1, configured order/limit, selector shape and exact selected source correspondence. Numeric order sorts matched IDs; input order preserves them. Captured notes are collected in first-occurrence document/source order, permitting multiple documents to retain one original note while requiring the complete frozen selection. A receipt is part of the approval digest and must survive edits and recovery. It records observed selection; it does not certify native collection identity, atomicity or card history.

Selection receipts may include optional `command_limit` (1–100,000), only for query/deck input. Omit it when absent to preserve older receipt projections. Validate selected IDs as the ordered prefix of all normalized matches with that limit; explicit IDs are never truncated. With no command limit, matched count must fit frozen `max_notes`. With an explicit limit, it overrides the default count while the receipt still binds the unchanged configured value. All match pools have a 100,000-ID safety ceiling.

## Source-media role decision

`ReviewChoice::SourceMediaRole` serializes as `{"decision":"source_media_role","value":{...}}`. The value binds source UUID, exact bytes digest, original filename, inspection evidence UUID, selected role, reviewer attribution and nullable license. A picture/audio choice requires matching successful inspection, supported MIME and, for audio, verified container extent. The application redecodes stored bytes before publication. An archive choice can acknowledge failed format inspection while preserving bytes and omitting rendering. Decisions cannot resolve native history/identity, missing assets or structural errors. Selected rendering names use the full bytes digest and decoded extension; original source names/fields and archive references survive. Changing role/attribution/license invalidates reviews tied to the prior semantic document. Resolved capture observations remain warnings and reopen when the decision fingerprint or evidence/asset no longer matches.

## Read-preparation checkpoints

The store's version-1 `PreparationDefinition` contains `job`, exact `selection`, and `created_at`. This initial boundary accepts only prepare mode, ordered `anki-note:<decimal ID>` input references, nonnil unique item UUIDs, no requested controls, a matching frozen settings fingerprint and valid selection/purpose. Definitions are immutable; identical-ID/identical-content creation returns the same digest, while changed content fails.

`PreparationEvent` contains version 1, job/item UUIDs, global sequence, prior global digest, attempt and tagged stage. Stages are `started`, `captured` with a complete staged learning document, or `failed` with a stable code and explicit retry eligibility. Initial pending state is implicit. Require pending → started at attempt 1; started → captured/failed at the same attempt; only classified retryable failed → started at the next attempt, bounded by frozen `jobs.max_item_attempts`. Captured is terminal. A started record is neither proof of worker liveness nor permission to redispatch.

Before committing a captured event, match the selected note, document kind/language and full source/archive association; verify every retained asset and media size. Publish bytes first, then event and retention references in one transaction. Compare the caller's expected global head under an immediate SQLite transaction; a competing writer must reload rather than overwrite history. Read events with `after_sequence` and page limits 1–1,000. These checkpoints prove local storage linkage, not native history, atomic Anki capture or collection recovery. Worker controls and batch publication are separate unfinished consumers.
