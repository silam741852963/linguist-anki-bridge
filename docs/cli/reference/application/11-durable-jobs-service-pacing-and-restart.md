# Application specification reference

## 11. Durable jobs, service pacing, and restart

SQLite is the orchestration source of truth; large artifacts/media live outside it. Use schema migrations, foreign keys, WAL, full synchronous durability, bounded pages, atomic artifact writes, content verification, and an OS-held runner lease. Persist resolved selector/configuration/approved plan revision and execution policy. Legacy database migrations audit/copy/import into a separate native store, retaining unknown historical fields via contract extensions.

Current durable state vocabulary to preserve or migrate explicitly:

| Entity | States | Recovery |
| --- | --- | --- |
| Job | queued, running, pausing, paused, completed, failed, cancelled, rolling_back, rollback_paused, rolled_back, rollback_partial | running/pausing → paused; rolling_back → rollback_paused |
| Item | pending, processing, processed, committing, completed, failed, skipped, reverted, rollback_failed | processing → pending; committing → processed **plus write reconciliation** |

An artifact is persisted before the item is considered processed. Commit retries reuse the reviewed artifact and original snapshot. Missing/corrupt artifacts stop the affected item or require explicit regeneration, never silently apply a replacement result. Failed items remain visible with stage, attempts, error class, next retry and recovery evidence.

Recommended one ordered collection writer; bounded enrichment concurrency with independent service pacing. Serialize expensive local generation according to actual resources. Retry transient network/429/5xx failures with bounded exponential backoff/jitter and provider delay hints; do not retry schema errors, unresolved conflicts, or unsupported capabilities as transient failures. **REVIEW R31** confirms job concurrency and retry policy. Keep in-memory rate timing monotonic; persisted retry timestamps are UTC.

Pause/cancel finish or reconcile the current mutation boundary. Cancel skips work that has not begun and does not revert completed changes. Delete removes local job metadata/artifacts only after explicit confirmation and cannot mean rollback. Keep snapshot/receipt references needed for recovery even when a job is deleted. **REVIEW R32** defines retention and deletion safeguards.

Jobs complete only when every selected item reaches its intended successful terminal outcome; intentional skips and dry-run completions are separately counted. Partial failures have a non-success aggregate status/exit code. `jobs items` has pagination and stable machine-readable filtering, rather than constructing every row of a large job.
