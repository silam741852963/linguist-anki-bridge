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
