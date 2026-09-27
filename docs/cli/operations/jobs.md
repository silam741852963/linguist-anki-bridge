# Jobs operations

Read [shared command rules](README.md) before implementing any handler.

## OP-35 — `jobs create`

Inputs: Plan/input refs; explicit mode prepare/simulate/apply.

Effects: Local immutable job, no execution.

1. Validate mode and freeze ordered selection/settings/items.
2. Apply mode requires ready approved revisions; prepare mode allows raw selected inputs.
3. Allocate job/item IDs and persist pending state.

Result/failure: Job ID and mode; never starts workers. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-36 — `jobs list`

Inputs: Status/mode filters/cursor.

Effects: Local read.

1. Read durable job summaries; annotate stale lease separately from confirmed worker death.

Result/failure: Counts labelled as inputs/items/notes/cards. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-37 — `jobs show JOB`

Inputs: Job ID.

Effects: Local read.

1. Show immutable mode/settings/selection, approvals, controls, lease and aggregate states.
2. List recovery-required items and next commands.

Result/failure: No lease/state mutation. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-38 — `jobs items JOB`

Inputs: State filter/cursor.

Effects: Local read.

1. Query durable item states/errors/plan/receipt refs.
2. Return bounded output and typed retry eligibility.

Result/failure: Unknown effects distinguished from ordinary failed items. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-39 — `jobs run JOB`

Inputs: Optional --apply for apply mode; execution limits.

Effects: Local/provider reads; Anki writes only apply mode+flag.

1. Validate immutable mode and current caller authority.
2. Claim lease and execute ALG-JOB; simulate performs preflight only, no write checkpoints.
3. Persist every item transition and report remaining work.

Result/failure: Apply mode without --apply does not run mutation; actionable authorization error. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-40 — `jobs pause JOB`

Inputs: Job ID.

Effects: Local control request.

1. Persist pause_requested under transaction.
2. Worker stops dispatch, finishes/reconciles active sent step, releases lease and marks paused.
3. Report requested versus confirmed paused; absence of worker does not fake completion.

Result/failure: Idempotent control receipt. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-41 — `jobs resume JOB`

Inputs: Job ID; --apply required each apply invocation.

Effects: Same mode effects as run.

1. Validate frozen settings/approval scope and current authority.
2. Check worker liveness before lease reclaim; reconcile unknown steps.
3. Continue pending items through ALG-JOB without regeneration.

Result/failure: No mode change; no duplicate creations. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-42 — `jobs retry JOB`

Inputs: Explicit eligible item IDs or --failed; --apply for apply mode.

Effects: Local new attempts; mode-dependent effects.

1. Classify failure and require changed input revision for permanent validation errors.
2. Unknown outcomes route to reconcile, not retry.
3. Record retry envelope/attempt count and run eligible items only.

Result/failure: Exhausted/noneligible reasons, not blanket retry. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-43 — `jobs cancel JOB`

Inputs: Job ID.

Effects: Local control request.

1. Persist cancel_requested; stop future dispatch.
2. Finish durable accounting of sent effects; committed items remain committed.
3. Mark cancelled only when worker stopped; surface any needs_recovery.

Result/failure: Cancellation is not rollback. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-44 — `jobs rollback JOB`

Inputs: Receipt-backed selected items; optional --apply.

Effects: Preview or journaled restore.

1. Build grouped restore plans from actual receipts/intermediate journals.
2. Show later edits/reviews/created notes and conflicts.
3. Execute ALG-RESTORE with --apply; unknown outcomes reconcile first.

Result/failure: Per-item restore/recovery receipts; partial jobs explicitly reported. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-45 — `jobs delete JOB`

Inputs: Terminal job; --execute for tombstone.

Effects: Local metadata only.

1. Reject active/unresolved recovery jobs.
2. Preview tombstone; retain referenced snapshots/journals/assets/receipts.
3. With --execute tombstone summary under transaction.

Result/failure: No Anki deletion; no automatic history pruning. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-46 — `jobs audit JOB`

Inputs: Optional --live.

Effects: Local read; optional Anki read.

1. Verify revision hashes, approvals, journal links/state consistency and receipt evidence.
2. With --live compare present collection safely and report drift, not repair.

Result/failure: Audit issues and recover inspect command. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-47 — `jobs migrate`

Inputs: Legacy job files and output.

Effects: Local candidate state only.

1. Read legacy format/version and preserve original copy.
2. Translate states/evidence; legacy committing/unknown creations require review, never assumed absent.
3. Emit imported paused/read-only jobs until settings/identity/approval validate.

Result/failure: Migration report with unsupported/ambiguous records; no resumed writes. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## Current preparation queue commands

OP-35 supports `jobs create --note-id ID [--note-id ID...]` with a required global `--purpose` for one of the four vocabulary/grammar purposes. It accepts only existing-note preparation input at this stage. Validate canonical note IDs, reject duplicates/count exceedance, freeze configured selection order and all settings/paths, allocate ordered item UUIDs and persist an immutable definition. It reads no Anki note, starts no worker and creates no plan. Default enabled enrichment may be queued; worker capability checks remain pending and creation does not claim execution availability.

OP-36 supports `jobs list [--after JOB_UUID] [--limit N]`. OP-37 supports `jobs show JOB_UUID`, exposing the immutable definition and explicitly unverified worker liveness. OP-38 supports `jobs items JOB_UUID [--after-index N] [--limit N]`, returning latest durable item summaries in original input order, including implicit pending items. Summaries include attempt, checkpoint sequence/digest, captured document ID if any, stable error code and bounded retry eligibility. Captured-asset references are verified before reporting captured summaries. A retry flag is classification metadata, not permission to dispatch work.

List/item pages use configured `output.page_size` unless `--limit` overrides it, with a 1–10,000 limit. List cursors are exclusive job UUIDs; item cursors are zero-based positions at which to resume. Responses provide `next_cursor` or `next_index`; an empty subsequent page terminates traversal. Reading absent state returns an empty list without creating directories; show/items require an existing job. Commands read only existing state, send no provider/Anki calls and do not claim worker death or recovery. Query/deck job creation, status/mode filters, history selection, worker execution/control and simulate/apply modes remain pending.
