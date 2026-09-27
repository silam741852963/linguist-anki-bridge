# Write and recovery algorithm

## ALG-JOB — bounded batch execution

1. Create immutable selection/settings and mode prepare/simulate/apply. Creation never runs it. Apply jobs reference approved ready revisions; mode cannot change on resume.
2. Run claims a durable lease. Apply mode additionally requires current `--apply`; preparation can never transition into writing because config changed. One collection writer; parallel preparation uses configured bounded workers/provider semaphores.
3. Item state transitions preserve prepared outputs and approvals. Before dispatch check pause/cancel controls. Pause prevents new dispatch and waits for active step's durable result; cancellation never interrupts journaling by pretending a sent mutation was cancelled.
4. Retry only eligible failed items. Unknown mutations reconcile first. Schema/identity faults halt the group; ordinary item errors obey stop/continue policy. Persist progress after each transition.
5. On restart, expired lease is reclaimed only after proving prior worker is absent; reconcile all mutating/unknown items. Never reset committing to processed blindly.
6. Job rollback creates grouped restore plans from receipts, not reverse replay of unchecked commands. Local job deletion is tombstoning only; referenced history survives retention protections.
