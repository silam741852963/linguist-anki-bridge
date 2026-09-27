# Write and recovery algorithm

## ALG-SPLIT — grammar group

1. Persist reviewed units and exactly one anchor. Allocate stable child operation IDs before writing; snapshot source once and link every child.
2. Prepare all children and verify checkpoint/model/assets before mutating source. Create non-anchor notes with fresh scheduling and distinct markers first; each receives independent receipt/recovery state.
3. Only after all required children verify, migrate/update anchor while preserving its retained history. Keep source archive linked everywhere. Do not delete source note as cleanup.
4. Any failure yields partial group with exact completed/pending/unknown children. Resume reconciles those children; never recreates verified children. Rollback restores anchor and evaluates created-child removal under ALG-RESTORE.
