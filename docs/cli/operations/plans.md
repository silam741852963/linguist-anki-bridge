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

Inputs: Typed decision and current input digest.

Effects: Local new revision.

1. Load issue/resolution schema; reject error waiver.
2. Record explicit sense/segmentation/anchor/duplicate/media/cue choice with actor/fingerprint.
3. Recompute only affected stages and validate; source changes invalidate decision.

Result/failure: Remaining issues and new digest. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

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
