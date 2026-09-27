# Write and recovery algorithm

## ALG-IDENTITY — collection binding and source conflict

1. Read endpoint/bridge capabilities, profile/path/sidecar lineage and collection-session epoch. Every managed write requires tested lab-native-v1 on the same host; no plain AnkiConnect fallback. Compare stable approval binding and current execution binding.
2. Weak/manifest-only bindings permit preparation/export, not writes. Session changes stop dispatch; explicit --rebind with --apply records ResumeBindingDecision after matching lineage/source/post-state evidence. Uncertain replacement/lost lineage remains needs_recovery. Verify creation model/deck and operation scope as well.
3. Compare current source fields/model/tags/card-task/deck membership/media hashes to plan source digest immediately before writing. Content conflict blocks that item. Normal reviews since preparation are permitted: capture fresh scheduling/history at apply, not stale preparation scheduling.
4. External Anki edits remain possible despite the app lease. Bridge compares expected pre-state inside the same serialized native operation as the effect; read-back mismatch enters needs_recovery. See [native protocol](../decisions/native-bridge.md).
