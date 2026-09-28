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
vocabulary/grammar purposes. Without `--apply`, it performs profile-pinned
read-only model inventory/inspection and returns the exact v2 field, template and
CSS manifest. It reports `create` when the name is absent,
`reuse_requires_native_order_verification` when field order and template/CSS
bytes match, or `name_collision` when a same-name model differs. Template order
and managed provenance are not established by AnkiConnect's read actions, so
even matching content is only a reuse candidate. A collision exits review-needed
and is never auto-overwritten. `--apply` fails before any Anki request until the
verified native bridge, checkpoint and journal executor are implemented. Preview
never claims apply eligibility or mutates the collection.

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
