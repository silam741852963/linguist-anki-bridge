# Application specification reference

## 12. Snapshots, rollback, and backup

Every real write records original note state, before-media, intended/verified after-state, created-note IDs, configuration/plan/job/operation identifiers, timestamps and error history. Injection restoration deletes only the note(s) demonstrably created by that operation. Modernization restores original fields/model/tags/deck as supported. Split restoration deletes proven siblings and restores the original source note. Preserve current card scheduling where possible; disclose what is outside snapshot scope.

Native current snapshots are atomic per-file records under `snapshots-v1`, combined with read-only Python `card_snapshots.json` history. Native `native_post_write_v1` records enable conflict checks. Never silently replace corrupt history with an empty array; show recoverable records and corruption warnings, retaining damaged files.

Before restore, compare current affected state with the operation's after-state, including media. A newer batch, manual Anki edit, changed model, deck or tags is a conflict. For legacy records without after-state, label the missing protection and require deliberate review. A force option, if introduced, must save the current state as a new recovery record first: **REVIEW R33**.

Multi-snapshot/job rollback runs reverse commit order. Continue independent restores after item conflicts, report each result, and finish with `rollback_partial` if any failed. Journal restore steps so a partial restore can resume; current restore APIs are multi-action and can fail midway. Detect already-restored state idempotently rather than assuming the original operation still matches after the first restore action.

Backups are distinct: collection backup, exported `.apkg`, and per-operation snapshots. Say exactly what each contains and whether scheduling/media/model changes are recoverable. Rust currently calls `createBackup`; Python manual export omits scheduling. **REVIEW R03/R30** select required protection before writes and model changes. Recommended: durable snapshots always; a verified collection backup before destructive migration/model changes; no repeated full export for every ordinary note.
