# Domain contracts

## States

Plan status: `preparing|draft|needs_review|ready|approved|applying|applied|partial|abandoned`. Revision content immutable after publication; status/receipts can change in separate records. Editing/regeneration creates next revision. A successful item receipt does not imply all items applied.

Job status: `queued|running|pausing|paused|completed|failed|cancelled|rolling_back|rollback_paused|rolled_back|rollback_partial|needs_recovery`. Item status: `pending|processing|processed|needs_review|ready|committing|completed|failed|skipped|reverted|rollback_failed|needs_recovery`. Legacy states import through explicit mapping, retaining unknowns in extensions. Unknown mutation never rewinds straight to generic `processed` without reconciliation.

Source/evidence versus rendered model data is deliberately separate. The five-field v1 contract remains readable; v2 projection to v1 is exported only if explicitly representable. Otherwise fail `CONTRACT_NOT_REPRESENTABLE`, never silently lose cues/grammar/evidence.

Final mode-specific meanings and capability separation are in [storage/lifecycle decisions](../decisions/storage-and-wire.md). Control flags pause_requested/cancel_requested are distinct from durable confirmed status. Prepare completion persists plans; simulation persists preflight; apply completion requires verified receipt.
