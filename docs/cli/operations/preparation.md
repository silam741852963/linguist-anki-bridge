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

Current revamp implementation (OP-22/OP-24): one explicit `--note-id`, required matching global `--purpose`, repeated read-only note/model/card capture and local source-draft publication. Select `llm.enabled=false`, `dictionary.provider=authored`, disable image search and select preserve/disabled audio; Japanese vocabulary also requires `kanji.enabled=false` until that enrichment is connected. Requested unavailable enrichment fails rather than being skipped. The result explicitly labels `preparation_stage=source_draft`, enrichment incomplete and writes disabled, and exits 4 for review. Source/media interpretation and native task/history evidence remain pending; no Anki write, binding, render or approval is produced. Query/deck/multiple-note selectors and full pipeline execution remain pending.
