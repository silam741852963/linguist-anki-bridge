# Write and recovery algorithm

## ALG-JOB — bounded batch execution

1. Create immutable selection/settings and mode prepare/simulate/apply. Creation never runs it. Apply jobs reference approved ready revisions; mode cannot change on resume.
2. Run claims a durable lease. Apply mode additionally requires current `--apply`; preparation can never transition into writing because config changed. One collection writer; parallel preparation uses configured bounded workers/provider semaphores.
3. Item state transitions preserve prepared outputs and approvals. Before dispatch check pause/cancel controls. Pause prevents new dispatch and waits for active step's durable result; cancellation never interrupts journaling by pretending a sent mutation was cancelled.
4. Retry only eligible failed items. Unknown mutations reconcile first. Schema/identity faults halt the group; ordinary item errors obey stop/continue policy. Persist progress after each transition.
5. On restart, expired lease is reclaimed only after proving prior worker is absent; reconcile all mutating/unknown items. Never reset committing to processed blindly.
6. Job rollback creates grouped restore plans from receipts, not reverse replay of unchecked commands. Local job deletion is tombstoning only; referenced history survives retention protections.

## ALG-CAMPAIGN — collection revamp campaign

1. A campaign definition (spec, frozen policy path, the creating command's global arguments, selection report) and its batches are immutable records; only a batch's phase and working data change, each change a compare-and-set on the batch version with an appended event. No campaign state lives in a process or an agent session.
2. One tick at a time holds the campaign lease (a dead tick's lease is reclaimed only after its process is proven absent). The write lane and `prepare_parallel` prep workers loop on their own (prep workers claim distinct batches; every step reads its batch fresh and saves with compare-and-set; `needs_agent` batches wait while an agent run is active); every effect goes through an existing command run as a child process, so each effect has its own lease, journal and recovery (ALG-JOB, ALG-APPLY, ALG-SPLIT).
3. Steps are repeatable or intent-first. Prepare jobs, `resolve-batch --policy`, `generate --all-items --pending`, `jobs run|resume --apply`, verification and retention repeat safely after a kill. An apply job is created only after its intent time is recorded; checkpoints created since that time are recorded for the batch, so a job orphaned by a kill is never run and its checkpoint is still pruned. A merged note's delete records its operation ID and evidence before the send and is settled from the companion ledger.
4. Phases: `selected → preparing → reviewing ⇄ needs_agent → ready → approved → applying → verifying → [merging →] done`; `skipped` (nothing captured), `deferred` (skipped batch). A deferred note is left out of the batch and unchanged in Anki.
5. Approval is a separate command (the agent's). It refuses, and pauses the campaign, when the share of agent-decided notes exceeds the limit.
6. Before an apply job, the approved notes' card facts are read (card IDs, repetitions, review row IDs). Verification requires every card, every review row and every referenced media file, and the target note type; new reviews are allowed. A failure pauses the campaign.
7. A session end (`JOB_CHECKPOINT_SESSION_ENDED`) creates the follow-up job for the pending items with a new checkpoint; unknown outcomes are reconciled (`--rebind`) and pause the campaign when they cannot be settled; any other halt pauses it.
8. Pauses (guardrail, pilot review, repeated step errors) are cleared only by the user (`campaign resume` from a terminal or `--by-user`). Retention deletes only checkpoint files the campaign created, of `done` batches beyond the newest N.
