# Write and recovery algorithm

## ALG-BACKUP — verified checkpoint

1. Under writer lease, enumerate exact collection/scope and affected notes/models/media. Determine whether checkpoint covers scheduling, media and schema actions.
2. Use tested native export_checkpoint. Model creation/migration/reverse migration requires collection-wide .colpkg with scheduling/media at the scope-closed group boundary. Content-only updates/adds use verified affected-scope scheduling/media packages plus per-item snapshots. Native automatic backups lacking media are insufficient. Do not export a full collection per note.
3. Write a temporary local artifact, check API result, file existence/size/checksum and package structure, then atomically finalize receipt. Never equate a successful API response with a verified file.
4. Record coverage, adapter versions and restoration-test evidence. Unsupported createBackup is an error; do not invoke a nonexistent endpoint. Native migration remains gated until disposable restoration demonstrates scheduling/history coverage.
5. Failed/missing coverage blocks the dependent write. Items of one frozen scope-closed group share its checkpoint; each still captures fresh current pre-state. Cross-group reuse requires unchanged protected manifests and configured age; record why. Full collection restoration is a manual last resort because it can discard unrelated later changes.
