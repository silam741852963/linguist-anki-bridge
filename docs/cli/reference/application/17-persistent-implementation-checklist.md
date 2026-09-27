# Application specification reference

## 17. Persistent implementation checklist

- [x] Resolve four-workflow release scope and select reviewable implementation proposals through research/read-only collection inspection.
- [ ] Prove native card-preserving migration and available backup/recovery paths in a disposable profile.
- [ ] Implement source-linked grammar OCR/layout, learning-unit segmentation and optional contextual exercises alongside vocabulary.
- [ ] Add CLI composition without pulling desktop/TUI dependencies into its build.
- [ ] Move reusable Rust live provider/application adapters out of desktop backend.
- [ ] Port rich dictionary evidence and safe authoring behavior without silent data loss.
- [ ] Fix OCR/generation dependency order and per-purpose provider routing.
- [ ] Define plan/issue/journal schemas, approval revisions and compatibility migrations.
- [ ] Implement review commands before apply commands; preserve user edits and media.
- [ ] Implement conflict checks, collection identity, backups/snapshots and unknown-outcome reconciliation.
- [ ] Implement durable job/restore control, leases, pagination, retry and partial-result contracts.
- [ ] Verify all first-release acceptance scenarios in disposable collections where mutation is needed.
- [ ] Update README/install help with implemented commands only; mark deferred features clearly.

This checklist tracks target work, not completed implementation. The source review establishes current behavior and gaps; it does not certify provider uptime, model quality, scheduling preservation, or release readiness.
