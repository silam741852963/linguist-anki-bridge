# Final storage and wire decisions

Owner: WP-01/WP-02/WP-04/WP-09/WP-13. These complete previously underspecified serialization, persistence and state rules.

## Wire and hashing

Use JSON Schema 2020-12 for documents, requests, decisions, exports and machine output. Rust tagged unions reject unknown fields; known legacy extensions are archived separately. All Anki IDs and operation UUIDs serialize as strings; ordinal/revision counters are bounded integers. UTF-8 input rejects invalid bytes and duplicate object keys. Floating parameters are finite, range-checked IEEE-754 values; no arbitrary-precision numeric payloads.

Digest format is **`lab-jcs-v1`**: SHA-256 over RFC 8785 JCS bytes of a documented semantic projection. Keep exact array order, raw strings and FieldIntent tags. No implicit Unicode normalization occurs during canonicalization. Volatile timestamps, execution leases, secrets and scheduler changes since preparation are excluded from the content approval projection; identity/model/deck/source intents and accepted warnings are included. Distinct content/source/settings/asset hashes are never substituted for one another. This replaces the earlier loosely specified lab-canonical-v1. [RFC 8785](https://datatracker.ietf.org/doc/html/rfc8785) defines interoperable canonicalization; schema/version prefix prevents accidental old-hash comparison.

Request envelope: schema_version, request_id, operation_uuid, action, expected_binding, plan_digest, payload_digest, precondition_digest, payload. Response: schema_version, request_id, operation_uuid, status, result or typed error, evidence_digest. Create payload includes exact fixed model fields, reviewed task/deck map, safe staged media and operation marker. Native adapters reject case-insensitive guessing of target fields. Source archive stores original bytes/field names separately from rendered intent.

Structured add input v2: one JSON object with kind, target_language, explanation_language, optional context, body, sources and requested_tasks. Vocab body has expression plus optional reading/pronunciation/selected senses/meaning/examples; grammar body has pattern/use_key/meaning/formation/examples and optional exercise. Unknown/absent required content fails validation. JSONL carries one such object per line; CSV has distinct explicit schemas/headers for simple vocabulary and grammar; multiline plain text is one record unless explicit record format is selected. Patch envelope: base_revision, base_digest, item_id, field_intents and requested_tasks. Review envelope: issue_id, input_digest, decision tagged by the issue schema, actor. Never interpret arbitrary free text as error waiver.

## Local durability

SQLite settings are fixed safety controls: WAL, synchronous=FULL, foreign_keys=ON, transactional schema migrations, integrity checks after migration, busy timeout from registry. No option downgrades journaling/durability. FULL WAL syncs commits; network filesystems are unsupported for active state. [SQLite WAL documentation](https://www.sqlite.org/wal.html) explains those durability/concurrency constraints.

Source role extraction supports pronunciation/kanji/identity/cue/language/task flags as well as content/media. Legacy combined audio+IPA/personal fields can feed several roles; parse sound references separately and preserve every original byte/field in the archive.

Private state directories use mode 0700 and files 0600 subject to platform support. Publish asset bytes first via temp file/fsync/rename/directory sync, then commit metadata refs; crashes can leave unreferenced bytes, never metadata pointing to unpublished bytes. Native sidecar follows the same intent durability. Database backups use SQLite's [online backup API](https://www.sqlite.org/backup.html), not a casual main-file copy while WAL is live. Active store backups are checksummed but are not Anki checkpoints.

One process locks each collection writer; worker lease includes PID, host, process-start identity, boot ID and heartbeat. Expired heartbeat alone never proves worker death. If identity cannot be checked, refuse takeover. Reader/cache-prune locks protect open evidence. State schema downgrade is export-only; unknown future versions never mutate existing state.

## Lifecycle semantics

Plan content readiness, approval and apply eligibility are separate: content-ready can be exported despite unavailable native capability. Apply eligibility additionally needs current bridge/model/checkpoint/resources/identity and exact source manifests. Structural/content errors cannot be waived; capability absence belongs to preflight, not an endlessly editable content issue.

Prepare item success means a persisted plan revision, even if needs_review; completed prepare jobs can contain reviewed/ready outputs and report their counts. Simulate success means preflight report, never committed receipt. Apply success means verified receipt only. Pausing/cancel controls are durable control flags; stale/unknown writes enter needs_recovery. Job terminal completion does not imply every input became a note: skipped/review/failed counts remain explicit. No replay rewinds unknown to processed.

Normal cache pruning never deletes history/backups. History cleanup and state relocation are deferred standalone future operations; changing storage paths with nonempty state is rejected until explicit export/import migration exists. Legacy snapshot import creates inspect/export-only records unless source binding and post-state are verifiable; there is no weak force restore.
