# Collection operations

Read [shared command rules](README.md) before implementing any handler.

## OP-11 — `decks list`

Inputs: Optional filters/cursor.

Effects: Anki read only.

1. Resolve Anki read dependency.
2. Fetch deck names/IDs and exact requested counts with bounded calls.
3. Paginate deterministic output; do not infer purposes from deck names.

Result/failure: Deck IDs/names and note/card counts labelled separately. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-12 — `decks show DECK`

Inputs: Exact deck name/ID.

Effects: Anki read only.

1. Resolve one existing deck or fail ambiguity.
2. Fetch models/counts and stored purpose mappings.
3. Report mixed models/subdecks explicitly.

Result/failure: Inspectable deck manifest. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-13 — `decks map PURPOSE`

Inputs: Source/target deck, source model, fields/task map, optional OCR packs.

Effects: Local config only.

1. Validate language/kind/purpose and existing regular source/target identifiers. No implicit deck creation. Revamp may omit target to preserve retained card home decks; add requires an existing target.
2. Inspect source model fields/templates; reject missing fields and duplicate task assignments.
3. Do not guess grammar anchor or destination; validate mapping plus purpose overrides. Filtered decks are not migration destinations; cards currently filtered need normal Anki return-home before migration.
4. Atomically save mapping; offline mapping stays unverified and cannot authorize writes.

Result/failure: Mapping fingerprint, validation evidence and unsupported models. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-14 — `decks unmap PURPOSE`

Inputs: Existing purpose.

Effects: Local config only.

1. Remove mapping from candidate, not Anki deck.
2. Validate/save atomically; existing frozen jobs retain original mapping.

Result/failure: No-op if absent; no collection deletion. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-15 — `models list`

Inputs: Optional purpose/managed filter.

Effects: Anki read only.

1. List model IDs/names.
2. Classify managed only after manifest match; name alone insufficient.

Result/failure: Model manifest summaries. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-16 — `models inspect MODEL`

Inputs: Exact name/ID.

Effects: Anki read only.

1. Fetch field order/templates/CSS and representative task/card capability.
2. Compare desired v2 manifest and report explicit mapping differences.

Result/failure: Compatibility report; no automatic modifications. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-17 — `models install PURPOSE`

Inputs: Versioned manifest; optional --apply.

Effects: Preview by default; Anki schema write only with --apply.

1. Resolve ALG-MODEL proposal and compatibility.
2. Preview exact fields/templates and shared-model conflicts.
3. With --apply use identity/lease/checkpoint/journal then install/verify; ambiguous outcomes reconcile.

Result/failure: Verified model receipt or recovery ID; never overwrites same-name different model. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current preview implementation supports the four built-in Japanese/English
vocabulary/grammar purposes, and `japanese_drill`/`english_drill` for
`Linguist Drill v1` (WP-28; installing it needs companion 0.7.0). Without `--apply`, it performs profile-pinned
read-only model inventory/inspection and returns the exact v2 field, template and
CSS manifest. It reports `create` when the name is absent,
`reuse_requires_native_order_verification` when field order and template/CSS
bytes match, or `name_collision` when a same-name model differs. Template order
and managed provenance are not established by AnkiConnect's read actions, so
even matching content is only a reuse candidate. A collision exits review-needed
and is never auto-overwritten. The preview lists `apply_requires`: a restored,
collection-scope checkpoint with scheduling, media and schema, a journal before
the call, and reconciliation by name, exact manifest and operation evidence.
`--apply` still fails with `CAPABILITY_UNAVAILABLE` before any lease, journal or
Anki request, because no tested native `install_model`/`export_checkpoint`
adapter exists. Preview never claims apply eligibility or mutates the collection.

The ALG-MODEL installer behind it (`linguist_application::model_install`) is
complete over an injected port and tested with fakes. It reuses only an exact
single same-name model (fields in order, template names/ordinals/bytes, CSS),
blocks a differing or duplicated name, requires the checkpoint gate before
creation, records pre-state and an `install_model` intent before calling, and
verifies the read-back. A partial model or lost response enters
`needs_recovery`, blocks new attempts for that name and is never deleted.
Reconciliation adopts a result only when the exact name, exact manifest and
companion evidence naming that model agree; it closes an uncertain request as
not created only when the name is absent and the companion reports
`failed_before_write`. It never re-sends.

## OP-87 — `models update PURPOSE`

Inputs: Purpose; optional --apply.

Effects: Preview by default; with --apply, the managed note type's templates and CSS change in place. Fields, template names and ordinals stay, so no full sync is needed.

1. Read the one note type named like the purpose's managed model, through the verified companion.
2. Preview `current` (it matches the manifest), `update` (it matches a known earlier manifest), `unknown_prior` (edited outside the bridge), `absent` or `ambiguous`, and whether the companion declares `update_model` (0.6.0).
3. With --apply: lease, collection-scope checkpoint, model-operation record (with `update_of`: model id and prior manifest digest), journal `update_model` intent, the companion call, read-back.

Result/failure: `updated` or `current` with the model id; `MODEL_UPDATE_ABSENT`, `MODEL_UPDATE_SHAPE_CHANGED`, `MODEL_UPDATE_UNKNOWN_PRIOR` or `MODEL_NAME_AMBIGUOUS` before any lease or write; `CAPABILITY_UNAVAILABLE` without companion 0.6.0. A lost response enters `needs_recovery`; `recover reconcile` adopts the update only with the companion's receipt and an exact read-back, and closes it as `not_updated` when the companion reports `failed_before_write` and the prior manifest is still there. A note type the user edited is never overwritten.

WP-27 uses it to ship the part-of-speech colours: `prior_manifests` in `linguist_core::model` lists the manifests each managed model may be updated from.

## OP-88 — `notes retag [NOTE_ID...] [--all] [--limit N] [--apply]`

Inputs: Anki note IDs, or --all for every note an apply operation wrote; optional --limit (note ID order), --fields (the preview shows each changed field before and after) and --apply.

Effects: Preview by default; with --apply, the fields and tags of unedited applied notes are rewritten in place with the current renderer. Cards, scheduling and the note type stay.

1. Find each note's latest apply operation in the store (a retag is one too). A note whose latest operation was restored or is unresolved is skipped.
2. Read the note through the verified companion. It must still equal the fields and tags that operation wrote; a note edited since is skipped (`RETAG_NOTE_EDITED`) and never written.
3. Render the operation's stored document with the current renderer: fields, and tags as apply makes them (the user's own tags kept, `lab::` derived). Preview `change` (fields and tags that differ), `current` or the skip code.
4. With --apply: lease, one collection-scope checkpoint for the changed notes, then per note an apply-operation record for the same plan item and approval, a fresh snapshot, a journaled exact `restore_note` intent, the companion call, read-back and receipt. The note is read again first; a change since the preview refuses before the journal (`RETAG_NOTE_CHANGED`).

Result/failure: counts by verdict, the proposals, and with --apply the committed operations and refusals; the run stops at the first operation that is not committed. `RETAG_NOT_APPLIED`, `RETAG_NOTE_MISSING`, `RETAG_RENDER_BLOCKED` and `RETAG_MODEL_CHANGED` skip a note without writing. A lost response enters `needs_recovery` for `recover reconcile`; `snapshots restore` of a retag puts back what was there. No LLM is called, so there is no review.

WP-27 uses it to bring notes applied before the part-of-speech tags and colours up to the current render.

## OP-18 — `notes list`

Inputs: Exactly one selector family: IDs, query, deck/purpose; limit/cursor.

Effects: Anki read only.

1. Validate selector grammar and limit.
2. Freeze IDs for this response; fetch bounded summaries.
3. Distinguish note count/card count and exclude private full text by default.

Result/failure: Deterministic results; zero results success. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-19 — `notes show NOTE_ID`

Inputs: Existing note ID; optional media metadata.

Effects: Anki read only.

1. Resolve identity; fetch all fields/model/tags/cards/decks.
2. Report raw values and parsed media references with missing assets.
3. When `--purpose` is selected, consume its exact source field map and expected source model; reject model mismatch or missing explicitly mapped fields. Report raw role values, shared/unmapped fields and missing required roles. No aliases or normalized learning facts are inferred; no mapping/archive/state is written. Without purpose, `source_mapping` is null.

Result/failure: Private explicit inspection, not routine logging. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-20 — `notes count`

Inputs: Selector, no provider generation.

Effects: Anki read only.

1. Compile selector consistently with list.
2. Count unique notes and corresponding cards separately.

Result/failure: Exact labelled totals; no mutation or content generation. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
