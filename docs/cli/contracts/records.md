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
