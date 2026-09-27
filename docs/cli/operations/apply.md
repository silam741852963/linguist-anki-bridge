# Apply operations

Read [shared command rules](README.md) before implementing any handler.

## OP-34 — `apply PLAN`

Inputs: Revision/digest, optional approved item subset; --apply mandatory for mutation.

Effects: Preview without flag; journaled Anki/local writes with flag.

1. Without --apply show preflight/diff only; no backups/schema/media writes.
2. With --apply invoke ALG-APPLY, or ALG-SPLIT for reviewed groups.
3. Report per-item committed/failed/recovery states and receipt/snapshot IDs.

Result/failure: Nonzero if any selected item unresolved; --yes cannot resolve review choices. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
