# Write and recovery algorithm

## ALG-GC — storage pruning

1. Compute reachable objects from plans, active jobs, approvals, snapshots, journals, backups, receipts and active reader leases. Unresolved recovery objects are permanent roots until resolved.
2. Preview candidates, bytes and retention policy. `--execute` recomputes reachability under storage lock and rechecks readers before deletion. Anki media is outside this operation.
3. Delete only unreferenced local cache/temporary assets; use tombstone then unlink with recoverable bookkeeping. Never prune accepted assets/source archives solely by age. History deletion requires separate explicit policy/export and is out of automatic cache pruning.
