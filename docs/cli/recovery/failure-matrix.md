# Write and recovery algorithm

## Failure acceptance matrix

| Injected failure | Required outcome |
| --- | --- |
| Backup absent/false result/missing scheduling/media | No dependent mutation |
| Source edited between preparation and apply | Conflict, original preserved |
| Study since preparation | Fresh scheduler captured; retained history preserved |
| Timeout after addNote accepted | One note after reconcile; no duplicate |
| Crash after any field/model/media/deck effect | Durable partial state; resume/restore from same journal |
| Disk full after Anki accepts write | Stop writes; recovery discovers result; snapshot untouched |
| Partial native migration/card mismatch | needs_recovery; no claim of success |
| Profile switch or collection replacement | Stop; require trustworthy rebinding evidence |
| Shared media/model or filename collision | No overwrite/deletion |
| Split child succeeds, anchor fails | Exact child receipts; resume no duplicates |
| Later source edits/reviews before restore | Conflict-aware merge; retained later study preserved |
| Crash during restore | Restore journal resumes safely |
| Concurrent apply/restore | Exactly one app writer; rejected second writer |
| Cache prune during jobs/recovery | Reachable evidence survives |

Release tests must exercise every row on fake ports and all migration/history/package claims in disposable real Anki collections. Fake-only tests do not prove native scheduling preservation.
