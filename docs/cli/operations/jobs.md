# Jobs operations

Read [shared command rules](README.md) before implementing any handler.

## OP-35 — `jobs create`

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

Inputs: Explicit eligible item IDs or --failed; --apply for apply mode.

Effects: Local new attempts; mode-dependent effects.

1. Classify failure and require changed input revision for permanent validation errors.
2. Unknown outcomes route to reconcile, not retry.
3. Record retry envelope/attempt count and run eligible items only.

Result/failure: Exhausted/noneligible reasons, not blanket retry. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-43 — `jobs cancel JOB`

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

Inputs: Terminal job; --execute for tombstone.

Effects: Local metadata only.

1. Reject active/unresolved recovery jobs.
2. Preview tombstone; retain referenced snapshots/journals/assets/receipts.
3. With --execute tombstone summary under transaction.

Result/failure: No Anki deletion; no automatic history pruning. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-46 — `jobs audit JOB`

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

## Current preparation queue commands

OP-35 supports explicit note IDs or query/deck selectors, with a required global `--purpose` for one of the four vocabulary/grammar purposes. It accepts existing-note preparation input. Validate canonical note IDs and selection limits, freeze configured order/settings/paths, allocate ordered item UUIDs and persist an immutable definition. Query/deck creation freezes matches with profile checks; it reads no note content, starts no worker and creates no plan. Default enrichment may be queued; worker capability checks reject unavailable adapters when run.

OP-36 supports `jobs list [--after JOB_UUID] [--limit N]`. OP-37 supports `jobs show JOB_UUID`, exposing the immutable definition and explicitly unverified worker liveness. OP-38 supports `jobs items JOB_UUID [--after-index N] [--limit N]`, returning latest durable item summaries in original input order, including implicit pending items. Summaries include attempt, checkpoint sequence/digest, captured document ID if any, stable error code and bounded retry eligibility. Captured-asset references are verified before reporting captured summaries. A retry flag is classification metadata, not permission to dispatch work.

List/item pages use configured `output.page_size` unless `--limit` overrides it, with a 1–10,000 limit. List cursors are exclusive job UUIDs; item cursors are zero-based positions at which to resume. Responses provide `next_cursor` or `next_index`; an empty subsequent page terminates traversal. Reading absent state returns an empty list without creating directories; show/items require an existing job. Inspection reads only existing state, sends no provider/Anki calls and does not claim worker death or recovery. Status/mode filters, history selection, durable stop acknowledgement and simulate/apply modes remain pending.
