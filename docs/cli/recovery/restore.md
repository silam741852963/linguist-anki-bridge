# Write and recovery algorithm

## ALG-RESTORE — selective recovery

1. Inspect snapshot plus verified post-state and all intermediate journals. Produce field/model/tag/deck/media diff and card/history consequences. Default is preview; actual restore requires `--apply` and current observed-state-bound decision.
2. Acquire lease, verify collection identity and current state. Differences from recorded post-state are conflicts; never force-overwrite later user edits. Permit an explicit reviewed field merge in a new restore plan; there is no global force flag.
3. Create a new restore journal/checkpoint and capture current state before effects. Restore referenced original media bytes before fields reference them; use alternate safe filenames if collisions occur.
4. Restore fields/model with supported mapped migration, tags and per-card decks. Preserve retained card IDs and current scheduling/review history, including reviews after conversion. Original scheduling is restored only in separately reviewed disaster-recovery procedure, never ordinary restore.
5. Created child/new notes: default preview keeps them. Deletion requires explicit reviewed note list, unchanged content, and verified no later reviews/history; studied notes are kept by the first-release CLI and require a separately authorized manual disaster-recovery procedure if removal is desired. If deletion is unsupported by release policy, leave them and report manual next steps.
6. Do not delete shared models or physically remove original/shared media. Newly created task cards with later study cannot be removed by first-release reverse mapping; preserve them or stop and offer separate manual disaster recovery. If native reverse mapping cannot preserve retained history, stop.
7. Verify observed restored state and history; write restore receipt. Failure enters needs_recovery on restore journal; repeated restore resumes that journal, never assumes a transactional rollback.
