# Preparation operations

Read [shared command rules](README.md) before implementing any handler.

## OP-21 — `vocab add`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate add input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-VOCAB and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-22 — `vocab revamp`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate revamp input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-VOCAB and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-23 — `grammar add`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate add input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-GRAMMAR and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-24 — `grammar revamp`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate revamp input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-GRAMMAR and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current revamp implementation (OP-22/OP-24): repeat explicit `--note-id` for multiple existing notes, with a required matching global `--purpose`. Validate IDs, reject duplicate IDs/selection counts above `selection.max_notes`, and freeze order using `selection.order=note_id` (numeric ascending) or `input` (argument order) before repeated read-only note/model/card capture. The aggregate raw capture assets must fit `input.max_file_mb`; exceedance fails without truncation or local plan creation. Validate every capture before creating local state, publish all assets first, then one immutable revision containing all documents in frozen order. A failed capture rejects this batch; partial failed-item publication remains pending. Single-note output retains `result`; multi-note output uses `result.items` and `item_count`, with the same plan ID/revision/digest on every item. Select `llm.enabled=false`, `dictionary.provider=authored`, disable image search and select preserve/disabled audio; Japanese vocabulary also requires `kanji.enabled=false` until that enrichment is connected. Requested unavailable enrichment fails rather than being skipped. Output labels `preparation_stage=source_draft`, enrichment incomplete and writes disabled, and exits 4 for review. Source/media interpretation and native task/history evidence remain pending; no Anki write, binding, render or approval is produced. Alternatively select `--query QUERY` or `--deck NAME`; selector families conflict. Deck names use the existing quoted Anki query compiler. A query/deck search runs once with profile checks before and after; returned IDs are normalized to unique numeric order by the read port, then pass through the same batch limits/capture/publication. Empty searches return `result.items=[]`, `item_count=0` and exit 0 without creating state. Query text must be nonempty and fit `input.max_record_chars`. Explicit selector limits, separate durable query receipts, partial item outcomes and full pipeline execution remain pending.
