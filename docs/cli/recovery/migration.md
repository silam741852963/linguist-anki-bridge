# Write and recovery algorithm

## ALG-MIGRATE — preserve cards

1. Require tested lab-native-v1 companion, explicit field/card-template mapping, fresh native precondition evidence and accepted schema/full-sync warning. Installed custom updateNoteModel is not sufficient evidence.
2. Capture fresh card IDs, semantic tasks, per-card decks, scheduling and review-history checksums/counts after lease/preflight. Confirm source model/task map against observed manifest.
3. Journal desired target fields and explicit ordinal mappings. Retained tasks keep their card IDs/history; newly enabled tasks create fresh cards. Reject a mapping that removes a retained task without an explicitly scoped destructive decision.
4. Call native mapped migration, reconcile outcome, then read note/cards/history back. Compare retained IDs/task mapping/history/scheduling and decks; normal review activity during mutation cannot be assumed harmless—unexpected differences require recovery.
5. If unsupported, prepare/export/manual native migration remain available; CLI apply for that item is blocked. Do not silently fall back to field-only model reassignment. Filtered-deck membership blocks migration until cards return home; never modify filtered scheduler state automatically.
