# Writes, jobs and recovery

Status: required target algorithms, not a claim that current adapters satisfy them. Safety gates are fixed invariants, not settings. Read [contracts](../contracts/README.md). All mutations, including restore and model installation, use a durable journal and collection writer lease.

Lease ownership rule: orchestration acquires one writer lease and passes its ownership token to nested backup/model/migration/restore helpers; helpers validate the token rather than reacquiring a lock and deadlocking. Job-worker lease and collection-writer lease are distinct. An unclassified unknown collection effect blocks further collection mutations until reconciliation; known isolated item failures may continue only when shared model/identity/assets are proven unaffected.


Read only the relevant section:

- [ALG-IDENTITY — collection binding and source conflict](identity.md)
- [ALG-BACKUP — verified checkpoint](backup.md)
- [ALG-MODEL — model installation/change](models.md)
- [ALG-MIGRATE — preserve cards](migration.md)
- [ALG-APPLY — one item](apply.md)
- [Journal state and step protocol](journal.md)
- [ALG-RECONCILE — unknown outcome/restart](reconcile.md)
- [ALG-SPLIT — grammar group](split.md)
- [ALG-RESTORE — selective recovery](restore.md)
- [ALG-JOB — bounded batch execution](jobs.md)
- [ALG-GC — storage pruning](retention.md)
- [Failure acceptance matrix](failure-matrix.md)
