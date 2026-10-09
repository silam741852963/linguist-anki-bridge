# Jobs operations

Read [shared command rules](README.md) before implementing any handler.

## OP-35 — `jobs create`

Simulate/apply modes (WP-13): `jobs create --mode simulate|apply --plan PLAN
--digest DIGEST [--revision N] [--approval ID] [--item-id ID]...` freezes an
approved, ready revision's items in plan order with the effective settings. Each
item passes apply authorization at creation; nothing reads Anki or runs. Apply
jobs also need `--checkpoint ID --protected-manifest DIGEST`; a checkpoint created
for a group makes that group the job ID. Grammar split units are refused.

WP-21: `--create-checkpoint` (apply mode, instead of `--checkpoint`) first exports
one verified full-collection checkpoint through the companion under the
collection-writer lease, scoped to every selected item's source notes and created
for a new group that the job then adopts as its ID. This is the only Anki effect
of `jobs create`; it writes no note.

Current implementation: prepare mode for existing notes. A supported `--purpose`
and exactly one of repeated `--note-id`, `--query` or `--deck` are required.
`--limit N` is supported for query/deck only, from 1 through 100000. Validate
purpose, settings, path expansion, limit and selector before opening local state.
Explicit IDs require no Anki traffic. Query/deck selectors call only
`getActiveProfile`, `findNotes`, `getActiveProfile`; a profile change rejects the
selection before any job is saved. Deck names use the existing escaped exact-deck
query builder. Searches keep the read port's normalized unique match set (numeric
ID order); `selection.order` then selects the frozen order and the optional limit
takes its prefix. Without `--limit`, excess matches fail with
`JOB_INPUT_LIMIT_EXCEEDED` (exit 2) and guidance; never truncate implicitly.
An explicit query/deck limit may override the configured default maximum, within
the hard 100000-input bound. Preserve the full matched set, selected subset,
original selector, order, configured maximum and explicit limit in the receipt.
Allocate IDs and save the validated immutable definition only after selection
succeeds. Empty searches return `job_id=null`, zero counts and no new state.
Results separate `matched_count` from selected `input_count`; workers never start
and no note-content, generation or media reads occur. Queuing can freeze requested
enrichment that execution will later reject until its adapter is implemented.
Runs use the retained note IDs; they never rerun the selection query. Add-input,
simulate/apply modes and approval-backed job creation remain pending.

Inputs: Plan/input refs; explicit mode prepare/simulate/apply.

Effects: Local immutable job, no execution.

1. Validate mode and freeze ordered selection/settings/items.
2. Apply mode requires ready approved revisions; prepare mode allows raw selected inputs.
3. Allocate job/item IDs and persist pending state.

Result/failure: Job ID and mode; never starts workers. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-36 — `jobs list`

Current implementation lists prepare, simulate and apply jobs by ID with `kind`,
`item_count`, `event_count` and `tombstoned`; `--mode` filters by kind and
tombstoned jobs appear only with `--include-deleted`. A status filter is pending.

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

Simulate/apply jobs (WP-13, wired in WP-21): `jobs run` of a simulate job refuses
`--apply` and runs the full preflight read-only; an apply job requires `--apply`
and runs the executor over the verified companion (`NativePort`) while holding the
collection-writer lease, renewed with the job lease before each item. Every item
reuses the job's checkpoint. Items are dispatched in frozen order;
`jobs.on_item_error` decides whether an item failure stops the job, and identity,
session, checkpoint and lease faults always halt it. Unknown outcomes stop with
`recovery_required` and are resolved with `recover reconcile OPERATION --apply`
before a resume. A checkpoint authorizes writes only in the Anki session it was
taken in: after a restart or a full sync the job stops before dispatch with
`JOB_CHECKPOINT_SESSION_ENDED`, and `next_commands` names a follow-up
`jobs create --mode apply ... --item-id ... --create-checkpoint` for the
remaining items. Without a verified companion every run stops with
`CAPABILITY_UNAVAILABLE` before any lease. Prepare jobs refuse `--apply`.

Current implementation: prepare-mode source capture and complete draft publication,
with concurrency controlled by frozen `jobs.prepare_workers`. No apply flag or collection writes.
Read the immutable definition from existing state; reject a different frozen storage
root, unsupported requested enrichment or invalid lease settings before acquiring a
lease. Use frozen settings for the Anki client, capture limits and failure policy.
Claim the job lease; scan checkpoint states and refuse any interrupted `started`
item with `PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY` (exit 7), before dispatching
any other pending item. Do not infer death from expiry. Read the verified newest
checkpoint directly and scan item states in bounded pages before constructing an
eligible input list; do not materialize every historical captured document.
Skip captured items and nonretryable/exhausted failures. Each eligible item gets a
CAS `started` event before dispatch. Both started and result checkpoints recheck the matching job
lease inside the same immediate SQLite transaction as their head comparison;
wrong-resource, expired or released tokens cannot append progress. Renew during reads
at the frozen heartbeat interval. Capture and decode/stage in read workers, then
the coordinator revalidates ownership, publishes
original assets and append a CAS `captured` event. Recognized transport failures
receive stable retry codes; other failures require review and halt the run.
Dispatch eligible inputs in frozen order in groups of at most `jobs.prepare_workers`.
Each started checkpoint precedes its worker's first read. Checkpoint results as
workers finish; final plan order still follows the frozen selection. Drain the
entire dispatched group before starting another one. The bounded result channel
can hold one outcome per worker, so a coordinator failure cannot leave a sender
blocked waiting for channel space. Lease/storage failures stop coordination and
leave any unrecorded started outcomes for explicit recovery.
`jobs.on_item_error=stop` also stops after transport failure, preventing the next
group; already-dispatched workers retain their durable results. A later invocation may
retry an eligible failure once, within the frozen attempt ceiling. No retry loop
runs automatically within one invocation. Release the lease on ordinary completion
or error; crashes retain their durable checkpoints and strong process identity.
After capture, publish a draft only if every frozen item is captured. Construct it
from retained documents in selection order with the exact frozen settings and
selection receipt. Verify all original bytes, enforce aggregate unique-asset and
plan-body limits, and require the current job head and worker lease. The initial
plan ID equals the job UUID and its revision is one. Publication rechecks the lease
inside the SQLite transaction. A rerun compares full canonical revision-one bytes
before reusing its receipt; a different existing plan fails with
`PREPARATION_PLAN_CONFLICT`, even if its approval projection happens to match.
Later review revisions remain untouched. Interrupted publication retries from
retained captures without rereading Anki. A failed/pending item yields no plan;
successful checkpoints survive for later recovery. Publication limit failures
also retain captured checkpoints. JSON reports capture/failure counts for this
invocation, checkpoint digest and optional plan receipt, `ready=false` and
`writes_enabled=false`. `plan_published` reports whether a complete initial draft exists.
Aggregate item/error counts include earlier checkpoints, not just this invocation.
Review-required drafts exit 4; persisted dependency failures exit 3 and other read
execution failures exit 6. Exhausted failures never turn into exit 0 on a no-dispatch rerun.
Durable pause/cancel requests now prevent new dispatch. Workers poll at frozen
`jobs.pause_poll_ms` while waiting for results, renew at the heartbeat interval,
and record every already-dispatched outcome before returning. Dispatch checks the
latest request inside its checkpoint transaction, including a request arriving
between the coordinator check and dispatch. Pause/cancel also blocks new plan
publication inside its transaction; captured results remain retained. Confirmed
worker-stop acknowledgements, interrupted-item reconciliation, provider pacing, enrichment and
partial batch plan publication remain pending.

Inputs: Optional --apply for apply mode; execution limits.

Effects: Local/provider reads; Anki writes only apply mode+flag.

1. Validate immutable mode and current caller authority.
2. Claim lease and execute ALG-JOB; simulate performs preflight only, no write checkpoints.
3. Persist every item transition and report remaining work.

Result/failure: Apply mode without --apply does not run mutation; actionable authorization error. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-40 — `jobs pause JOB`

WP-13: a pause on an idle job (no active job lease, nothing in flight) is
confirmed in the same transaction (`worker_stopped_confirmed=true`); otherwise
the worker acknowledges after the current item's durable result. Apply/simulate
job controls are events in the job's hash-linked history.

Current prepare-mode behavior: require an existing job, append a digest-linked
pause request to schema-seven control history, and return its receipt. Repeating
the current action reuses that receipt without another event. Do not change the
immutable definition's initial flags or item outcomes. `jobs show` includes the
current request separately. Return `worker_stopped_confirmed=false`; this handler
does not infer liveness, wait for drain, or claim a confirmed paused state.
Requests are serialized in an immediate transaction. Unknown jobs/absent state
never create storage. Cancel is terminal; subsequent pause/resume requests fail.

Inputs: Job ID.

Effects: Local control request.

1. Persist pause_requested under transaction.
2. Worker stops dispatch, finishes/reconciles active sent step, releases lease and marks paused.
3. Report requested versus confirmed paused; absence of worker does not fake completion.

Result/failure: Idempotent control receipt. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-41 — `jobs resume JOB`

WP-13/WP-21: an apply job needs `--apply` on every resume. The resume request is
recorded locally first (a cancelled job is refused without contacting Anki), then
the job runs as for `jobs run`. Prepare jobs refuse `--apply`.

Current prepare-mode behavior: append/reuse a resume request, then invoke the same
worker as `jobs run` with immutable inputs/settings. The current request is saved
even if execution later fails capability checks or lease contention. Resume does
not reclaim a live worker, reset interrupted items, regenerate captured documents,
or clear failure/attempt history. Cancelled jobs cannot resume (exit 5).

Inputs: Job ID; --apply required each apply invocation.

Effects: Same mode effects as run.

1. Validate frozen settings/approval scope and current authority.
2. Check worker liveness before lease reclaim; reconcile unknown steps.
3. Continue pending items through ALG-JOB without regeneration.

Result/failure: No mode change; no duplicate creations. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-42 — `jobs retry JOB`

Current implementation (WP-13): for simulate/apply jobs, record a retry envelope
for eligible items only (transient or shared-fault halt classes within
`jobs.max_item_attempts`); unknown outcomes return `JOB_RETRY_RECONCILE_FIRST`
and permanent validation failures `JOB_RETRY_REQUIRES_NEW_REVISION`. Apply jobs
need `--apply`. When at least one item is accepted the job then runs (WP-21);
nothing contacts Anki when no item is eligible (exit 4). Prepare jobs get a
classification only; `jobs run` retries every eligible failure.

Inputs: Explicit eligible item IDs or --failed; --apply for apply mode.

Effects: Local new attempts; mode-dependent effects.

1. Classify failure and require changed input revision for permanent validation errors.
2. Unknown outcomes route to reconcile, not retry.
3. Record retry envelope/attempt count and run eligible items only.

Result/failure: Exhausted/noneligible reasons, not blanket retry. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-43 — `jobs cancel JOB`

WP-13: cancel is terminal for every kind; the stop is confirmed immediately on
an idle job or by the worker's acknowledgement after its current item.

Current prepare-mode behavior: append/reuse a terminal cancel request. Prevent new
dispatch and plan publication; dispatched reads can still checkpoint their result.
Keep original assets, captures, errors and earlier plans. Return a request receipt
with `worker_stopped_confirmed=false`. Nothing is deleted or rolled back. Durable
stop acknowledgement and native-mutation cancellation remain pending.

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

Current implementation (WP-13): preview by default; `--execute` records an
immutable local tombstone. Only terminal jobs qualify (apply/simulate: completed,
cancelled, halted or finished with failures with nothing started or unresolved;
prepare: cancelled, or draft published with every item captured), and an active
job lease refuses. Tombstoned jobs refuse runs, controls, retries and events and
are hidden from `jobs list`; every referenced record remains.

Inputs: Terminal job; --execute for tombstone.

Effects: Local metadata only.

1. Reject active/unresolved recovery jobs.
2. Preview tombstone; retain referenced snapshots/journals/assets/receipts.
3. With --execute tombstone summary under transaction.

Result/failure: No Anki deletion; no automatic history pruning. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-46 — `jobs audit JOB`

Simulate/apply jobs (WP-13): verify the definition, the whole event chain, the
plan digest and approval scope, and per item the recorded operations' plan item
and job group, journal agreement and stored receipts. A partial batch is reported
as `partial=true`; `native_verified=false`; exit 7 when issues are found.

Current prepare-mode implementation audits a bounded local history page.
`--after-checkpoint N` and `--after-control N` are independent exclusive sequence
cursors (default zero). `--limit` or configured `output.page_size` supplies a
1–1000 page size; larger configured pages require an explicit supported limit.
Verify the immutable definition, record identities and hashes, consecutive
sequences and parent-digest links, including the preceding boundary anchor. Reject
checkpoint/anchor bodies above frozen `input.max_file_mb` and controls above 4 KiB
before materializing them from SQLite. Corrupt preparation history exits 7 with
structured diagnostics and no success result on stdout.
History earlier than the boundary anchor is outside this page's verification scope.
Checkpoint storage streams each captured document through verification rather
than retaining full bodies for the whole page. Captured rows verify original
archive/media bytes and report document/semantic/source/asset digests without
document content. Failed rows report stable codes and recorded retry classification,
which does not authorize retry. Controls report their immutable request receipts.
Return separate next sequence cursors; empty subsequent pages terminate traversal.
If an initial source plan exists, verify its stored body, approval digest, retained
assets and exact frozen settings/selection. Its complete checkpoint binding and
later review/approval/native histories are not checked by this page command.
Output says `scope=local_history_page`, `full_history_checked=false`,
`native_verified=false` and `worker_liveness=unverified`; it cannot certify whole-job
recovery or a stopped worker. Concurrent append-only history may advance between
checkpoint/control reads; no cross-stream snapshot claim is made. Invalid hashes,
anchors, gaps or links fail closed. `--live` explicitly reports unavailable native
verification. Unknown/absent jobs never initialize state. Full consistency/approval
and native collection audit remain pending.

Inputs: Optional --live.

Effects: Local read; optional Anki read.

1. Verify revision hashes, approvals, journal links/state consistency and receipt evidence.
2. With --live compare present collection safely and report drift, not repair.

Result/failure: Audit issues and recover inspect command. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-47 — `jobs migrate`

Current behavior upgrades an existing local state database to schema seven; it
does not import legacy job files. Refuse absent state rather than initialize it.
Use the standard verified private backup before upgrade, preserve definitions,
checkpoints and revisions, and add append-only control storage. Current-schema
state is a no-op. Read-only job commands never upgrade: existing schema-six users
run `jobs migrate` explicitly before reading/running controls. No Anki calls occur.

Inputs: Legacy job files and output.

Effects: Local candidate state only.

1. Read legacy format/version and preserve original copy.
2. Translate states/evidence; legacy committing/unknown creations require review, never assumed absent.
3. Emit imported paused/read-only jobs until settings/identity/approval validate.

Result/failure: Migration report with unsupported/ambiguous records; no resumed writes. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current legacy import (WP-14): `jobs migrate --legacy PATH` previews a translation of a legacy Python or desktop `batch_jobs.sqlite3` (and its `-wal`, copied into a private scratch directory; the original is never opened for writing). Job states map to paused, completed, failed, cancelled, rolled_back or rollback_incomplete; items to pending, prepared_not_applied (artifact not imported), applied_historical, failed, skipped, reverted or `requires_review` for `committing` and `rollback_failed`; unknown states are `unsupported`. Every imported job is `runnable: false`. `--execute` stores the original database (and WAL) as assets and the report in the immutable `legacy_job_imports` table (schema 13), idempotently per database digest. `--list-imports` and `--show-import ID` read them back. Exit 4 when any item requires review.

## Current preparation queue commands

OP-35 supports explicit note IDs or query/deck selectors, with a required global `--purpose` for one of the four vocabulary/grammar purposes. It accepts existing-note preparation input. Validate canonical note IDs and selection limits, freeze configured order/settings/paths, allocate ordered item UUIDs and persist an immutable definition. Query/deck creation freezes matches with profile checks; it reads no note content, starts no worker and creates no plan. Default enrichment may be queued; worker capability checks reject unavailable adapters when run.

OP-36 supports `jobs list [--after JOB_UUID] [--limit N]`. OP-37 supports `jobs show JOB_UUID`, exposing the immutable definition and explicitly unverified worker liveness. OP-38 supports `jobs items JOB_UUID [--after-index N] [--limit N]`, returning latest durable item summaries in original input order, including implicit pending items. Summaries include attempt, checkpoint sequence/digest, captured document ID if any, stable error code and bounded retry eligibility. Captured-asset references are verified before reporting captured summaries. A retry flag is classification metadata, not permission to dispatch work.

List/item pages use configured `output.page_size` unless `--limit` overrides it, with a 1–10,000 limit. List cursors are exclusive job UUIDs; item cursors are zero-based positions at which to resume. Responses provide `next_cursor` or `next_index`; an empty subsequent page terminates traversal. Reading absent state returns an empty list without creating directories; show/items require an existing job. Inspection reads only existing state, sends no provider/Anki calls and does not claim worker death or recovery. Status/mode filters, history selection, durable stop acknowledgement and simulate/apply modes remain pending.

Current interrupted-read recovery: `jobs recover JOB` previews one item page.
Use `--after-index N` and `--limit N` (1–1000) to bound work; `next_index` advances
through the frozen item order. Preview leaves state and leases unchanged and
labels worker liveness unverified. `--execute` requires `--actor NAME`, acquires
the existing strong worker lease and re-reads the page. Live/unverified owners
block takeover, including after expiry. For each started item, renew ownership
and append an operator-attributed interruption under checkpoint CAS and lease
fencing. Preserve attempt counts; exhausted items remain ineligible. Preserve
other item checkpoints, assets/plans and pause/cancel controls. Results contain
no dispatch or collection-write claim. A later explicit `jobs run` retries eligible
read captures. Repeat recovery appends nothing when that page has no started
items. This operation cannot reconcile ambiguous native writes or claim a stopped
worker solely from a preview or deadline. Earlier checkpoints remain auditable.
