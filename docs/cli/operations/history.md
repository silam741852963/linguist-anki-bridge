# History operations

Read [shared command rules](README.md) before implementing any handler.

## OP-48 — `snapshots list`

Inputs: Note/job/status filters/cursor.

Effects: Local read.

1. List immutable snapshots and linked post-state/restore receipts.

Result/failure: Unknown post-state clearly labelled. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-49 — `snapshots show SNAPSHOT`

Inputs: Snapshot ID.

Effects: Local read.

1. Verify stored checksums and archive presence.
2. Show original model/fields/tags/decks/cards/scheduler plus observed post-state/intermediate effects.

Result/failure: Missing evidence blocks dependent restore. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-50 — `snapshots restore SNAPSHOT`

Inputs: Optional --apply; conflict decisions bound to current digest.

Effects: Preview or journaled restore.

1. Run ALG-RESTORE preview/conflict checks.
2. With --apply create or resume restore journal and execute guarded effects. Imported legacy snapshots lacking verifiable binding/post-state are inspect/export only; no force fallback.

Result/failure: Restore receipt or needs_recovery; later study preserved. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-51 — `snapshots export SNAPSHOT`

Inputs: Create-new output bundle path.

Effects: Local file.

1. Gather immutable source/post-state/journal links and media bytes.
2. Export checksummed versioned bundle with private-content notice and secret redaction.

Result/failure: Recoverable evidence, not claimed full collection backup. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-52 — `backup create`

Inputs: Explicit scope/output; --apply required for Anki export/checkpoint action.

Effects: Preview or external checkpoint/local artifact.

1. Preview coverage and path without mutation endpoint.
2. With --apply bind identity/lease and execute ALG-BACKUP with journaled export intent.

Result/failure: Verified coverage receipt; unsupported/missing package fails. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-53 — `backup list`

Inputs: Scope/date filters.

Effects: Local read.

1. List receipts/checksum/coverage/restore-test status.

Result/failure: Unverified artifact is not a valid checkpoint. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-54 — `backup verify BACKUP`

Inputs: Receipt/file; optional disposable restore-test target.

Effects: Read file; explicit disposable-target test separately authorized.

1. Check checksum/package metadata/scope/assets.
2. Compare receipt coverage; mark verified only evidence supports it.
3. Never import backup into active user collection; restoration test requires isolated disposable environment.

Result/failure: Verification report; no claim of scheduler recovery from file presence alone. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
