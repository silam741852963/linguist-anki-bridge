# Implementation status

Current state on 2026-10-06. Earlier progress notes were removed from this page (review issue RI-07); every package's specification, completion audit and known limits are in the [consolidated package record](packages.md), and the history is in git.

## What the CLI does

- **Prepare and review.** Vocabulary and grammar add and revamp (authored, dictionary and local generation sources), capture of existing notes with full archives, review decisions (including `plans resolve-history` for native card history), typed edits, grammar splits, validation, approval and export.
- **Read Anki.** Decks, models, notes, cards and media through AnkiConnect, profile-pinned.
- **Write Anki.** Through the verified `lab-native-v1` companion add-on (`dist/linguist-bridge.ankiaddon`, installed next to AnkiConnect): `plans bind` records the collection binding before approval; `apply --apply` (items and split groups), `snapshots restore --apply`, `jobs rollback --apply`, `recover reconcile --apply [--rebind]`, `backup create --apply` and `models install --apply` run their journaled orchestration under the collection-writer lease after a verified native checkpoint. Without a verified companion, API key, loopback endpoint and open profile, every write stops with `CAPABILITY_UNAVAILABLE` before any lease, journal or Anki request.
- **Recover.** Every uncertain native outcome is journaled and resolved by `recover reconcile OPERATION --apply`; nothing is re-sent blindly.
- **Jobs.** Prepare jobs capture and enrich every note with per-item checkpoints, and `plans resolve-batch` reviews a whole plan in one call; simulate/apply jobs run over the companion with one checkpoint per job, resume after a crash through reconcile and a follow-up job, and roll back as a group ([WP-21](wp-21.md)).

## Packages

All work packages WP-01–WP-17 are complete; WP-03 was completed by [WP-17](wp-17.md). Statuses, audits and known limits: [consolidated record](packages.md).

Later packages, each with its completion audit and known limits: [WP-18](wp-18.md), [WP-19](wp-19.md), [WP-20](wp-20.md) (flagged-card revamp, vocabulary split, companion 0.3.1), [WP-21](wp-21.md) (apply jobs over the companion, full-pipeline prepare jobs, `plans resolve-batch`) and [WP-22](wp-22.md) (Linguist Grammar v3; grammar add and revamp on the user's notes; companion 0.3.3).

## Evidence

- [Release check 2026-10-06](../evidence/release-2026-10-06/README.md) and the gate registry [release-gates.json](../decisions/release-gates.json).
- [Implementation pass review](../review/implementation-pass-2026-10-06.md): review issues RI-01–RI-08 and what is proven against real Anki.
- [Invariant ownership](invariant-test-ownership.md), [operation coverage](op-coverage.md), [traceability](../review/traceability.md), [setting coverage](../configuration/setting-coverage.md).
