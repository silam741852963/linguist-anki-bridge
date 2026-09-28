# Plans operations

Read [shared command rules](README.md) before implementing any handler.

## OP-25 — `plans list`

Inputs: Status/workflow/purpose filters; cursor.

Effects: Local read.

1. Query state indexes, stable ordering/pagination.
2. Display revisions/status/issues without materializing private payloads.

Result/failure: Plan summaries. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-26 — `plans show PLAN`

Inputs: Optional revision/item.

Effects: Local read.

1. Load exact revision or latest and show digest/frozen settings/provenance/tasks.
2. Show issues and review decisions, assets/model/deck actions and source archive links.

Result/failure: Human/machine inspection; missing archived asset reported. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current focused inspection: `plans show PLAN --item DOCUMENT_ID` returns only that
saved document with the exact plan revision and approval digest. It contains the
original archive fields, so use it when source detail is needed.

`plans show PLAN --issues-only [--item DOCUMENT_ID] [--revision N]
[--after-index N] [--limit N]` returns a bounded page (1–1000 issues, configured
page size by default). Pin `--revision` when paginating; latest may change after
review. Results revalidate the saved revision and include only non-warning issues,
exact base/document/issue/input digests and actor requirement. Supported dictionary
sense/reading choices, authored cue/exercise skeletons, source-content evidence
choices and generated-fact evidence choices are shown as typed templates. Each
choice is revalidated when submitted; empty authored text must be filled and no
actor is invented. Source-media decisions need manual typed attributes and are
marked as such. Unsupported or blocked issues have no template and are explicitly
marked unavailable. This page omits raw source/archive fields, credentials and
card media; `plans show` without flags retains the full private view. Read-only
inspection does not approve or enable native writes.

## OP-27 — `plans diff PLAN`

Inputs: Revision or captured source comparison.

Effects: Local read; explicit --live requests Anki reads.

1. Render effective fields and compare model/tags/per-card decks/tasks/media/source.
2. Separate captured diff from current live conflict report.
3. Report new/deleted cards and history consequences; do not regenerate.

Result/failure: Exact proposed changes and conflict warnings. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-28 — `plans edit PLAN`

Inputs: Structured --patch file OR explicit --editor; --base-revision/base digest required.

Effects: Local new revision.

1. Validate base revision/digest against current branch; conflict on stale editor.
2. For --editor, create private typed draft and launch editing.editor_argv or safely parsed VISUAL/EDITOR without shell; abort preserves parent. Apply typed FieldIntent patch, not arbitrary rendered HTML substitutions.
3. Run dependency invalidation; preserve user ownership and archive; validate/render.
4. Persist child revision, invalidate old approval, retain parent.

Result/failure: New revision; invalid edits saved only as explicitly marked draft, never ready. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-29 — `plans resolve PLAN ISSUE`

Implemented dictionary decisions use `{"decision":"sense","value":"SENSE_KEY"}`
when the written form has one reading or the draft already contains a verified
reading. To resolve Japanese entries with several readings, use
`{"decision":"sense_with_reading","value":{"key":"SENSE_KEY","reading":"なま"}}`
inside the fingerprint-bound resolution request. The selected sense must be unique
and the reading must belong to the draft's expression in that entry's archived
written-form pairs. Readings from other forms, invented readings and empty
selections are rejected. Selecting a different valid reading requires a new
review. This decision publishes a child and preserves the original source plan;
it does not authorize collection writes. See the generated
[resolution schema](../../../contracts/v2/resolution-request.schema.json).

Inputs: Typed decision and current input digest.

Typed content repairs use the same fingerprint-bound request. For a missing or
leaking vocabulary cue, use `{"decision":"cue","value":{"task":"production","text":"Say the verb for consuming food."}}`
(or `task=spelling`). A missing grammar RecognitionPrompt accepts
`task=recognition`. A missing/leaking Application exercise accepts
`{"decision":"exercise","value":{"prompt":"Complete the supplied context: ___","answer":"Expected completion"}}`.
The task must already be requested, the issue must target the corresponding field,
and frozen character limits apply. Repair changes that content and removes its
field override; it preserves tasks, sources and archives. Revalidation must remove
the targeted error without introducing a missing-content/leakage error on that
field. Invalid repairs publish nothing. This is a typed repair, never an error
waiver. Other errors and native reviews remain blocking. Content changes clear
prior reviews and create a new immutable child; no approval or apply authorization
is inherited.

Effects: Local new revision.

1. Load issue/resolution schema; reject error waiver.
2. Record explicit sense/segmentation/anchor/duplicate/media/cue choice with actor/fingerprint.
3. Recompute only affected stages and validate; source changes invalidate decision.

Result/failure: Remaining issues and new digest. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current source-media resolution: OP-29 accepts `choice.decision=source_media_role`. Its `value` must name `source_id`, `asset_digest`, `original_filename`, `evidence_id`, `role` (`picture|audio|archive`), explicit nonempty `attribution`, and nullable `license`. Use the normal resolution envelope with exact base revision/digest, document/input digest, issue ID and actor. Resolve `SOURCE_MEDIA_CONTENT_REVIEW` to select a role; format/completeness review can only select `archive` to omit rendering while retaining bytes.

1. Match the issue/source/name, one source-owned asset, its immutable archive reference and one digest-linked `media_format` evidence record. Reject stale input, mismatches, structural-error waivers and MIME/role conflicts.
2. Validate frozen media limits/allowlists and image/audio policies. `images.existing_policy=omit_reference` forbids picture selection; `audio.provider=disabled` forbids audio selection. Changed current configuration does not override frozen policy.
3. Read the asset from the private store, verify size, and for picture/audio decode again under frozen limits. Require the exact successful inspection receipt; audio additionally requires verified container extent. Failure or forged inspection publishes nothing.
4. Set the role and explicit attribution/license. Rendering references use `lab_<full SHA-256>.<decoded extension>`; retain `original_filename`, source fields, archives and exact bytes. Archive selection restores the original reference name and emits no card media. This plans a reference; it creates no Anki media file.
5. Invalidate prior reviews if semantic content changed, add the fingerprint-bound decision, retain resolved observations as warnings, revalidate and attempt rendering in a new immutable child revision. Other source/native/enrichment issues remain unresolved; this never authorizes apply.

Malformed files may remain archived after explicit archive decisions for their content and format issues. Missing source bytes and structural errors cannot be waived. Role changes that need a different frozen policy require a new preparation; generic field edits cannot change media roles.

## OP-30 — `plans regenerate PLAN`

Inputs: Explicit stage/fields and base revision.

Effects: Provider reads/local new revision.

1. Compute invalidation graph; preview user-edited fields at risk.
2. Require explicit overwrite list for user edits; default preserves them.
3. Run selected pipeline branches with new frozen fingerprint and validate/render.

Result/failure: New revision; no approved outputs overwritten in place. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-31 — `plans validate PLAN`

Inputs: Exact revision; optional --live.

Effects: Local validation; optional Anki read.

1. Run ALG-VALIDATE, ALG-RENDER consistency.
2. With --live inspect conflicts/capabilities but do not checkpoint/write.
3. Persist validation evidence linked to digest without changing semantic content.

Result/failure: ready/needs_review/invalid; live check is time-bound, apply rechecks. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current `--live` path performs bounded, profile-pinned read-only recapture of
revamp source notes. It verifies saved source/archive/payload links, then compares
current fields, tags, model manifest content, card IDs/ordinals and deck IDs with
the archived capture. Card payload changes such as ordinary scheduling progress
are reported separately from content conflicts; they do not by themselves mark
source content drift. Missing card ordinal/deck data fail closed. A repeated
capture match is optimistic, never an atomic native snapshot or history proof.
Media bytes are not rechecked. Authored add plans have no captured source to
compare; this command does not perform collection duplicate search.

Use `plans validate PLAN --live --revision N [--after-index N] [--limit N]`
for bounded pages (1–1000 source notes; configured page size by default). The
first page may omit `--revision`, but later pages require it. Source note IDs are
ordered numerically; the response gives `next_index`. A page is not a whole-plan
live clearance: only an unpaginated complete scan can report
`all_sources_checked=true`, and `apply_eligible` remains false. Live transport
failure publishes no validation receipt. Once live reads finish, the ordinary
immutable local content-validation receipt is persisted and returned alongside
the time-bound live report. Apply must repeat source/identity checks through the
native bridge.

## OP-32 — `plans export PLAN`

Inputs: Output path/format, optional explicit private archive inclusion.

Effects: Local output file.

1. Load immutable revision; validate target export format.
2. Export versioned manifest and selected assets, redacting secrets; private content disclosure explicit.
3. Use create-new/atomic file rules; v1 export fails if v2 semantics are not representable.

Result/failure: Portable bundle with checksums; export never applies. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-33 — `plans approve PLAN`

Inputs: Exact revision/digest; accepted warning codes.

Effects: Local approval only.

1. Require ready state and valid review decisions.
2. Display/bind intended fields/media/models/tasks/decks/source/settings digest.
3. Persist actor/time/item scope/accepted warnings; any semantic revision invalidates approval.

Result/failure: Approval ID; invocation --apply still required later. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
