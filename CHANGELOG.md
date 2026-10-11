# Changelog

## 0.2.1 — 2026-10-11

- `--help` and error guidance describe companion writes as available: `apply`, `snapshots`, `jobs`, `recover`, `plans`, `doctor`, `models` and `backup` no longer say writes are "unavailable until the native adapter is verified". The `CAPABILITY_UNAVAILABLE` hint names the released `linguist-bridge-*.ankiaddon` and Anki 25.09.2.
- Work-package records name the release-check, traceability and check-dev scripts on the `cli-dev` branch.
- Full release check rerun on the split repository.

## 0.2.0 — 2026-10-11

First release with collection writes. Companion add-on 0.4.0.

- **Writes through the companion add-on** (`lab-native-v1`): `apply --apply` for items and split groups, `snapshots restore --apply`, `jobs rollback --apply`, `recover reconcile --apply [--rebind]`, `backup create --apply` and `models install --apply`, each journaled under the collection-writer lease after a verified native checkpoint. `plans bind` records the collection binding before approval (WP-17).
- **Revamp review**: `plans resolve-history` maps source cards from live companion evidence; `plans resolve-batch` reviews a whole plan from one decisions file (WP-17, WP-21).
- **Jobs**: full-pipeline prepare jobs with per-item checkpoints; apply jobs over the companion with one checkpoint per job, crash resume through reconcile and group rollback (WP-21).
- **Card models**: Linguist Vocabulary v3 (stroke GIFs, dictionary audio, comprehension/production/spelling), English Vocabulary v1, and Linguist Grammar v4 (Usage and Nuance fields, per-example audio, no exercise card) (WP-19, WP-22, WP-23).
- **Splits**: vocabulary and grammar splits; new split units inherit the source card's schedule (WP-20, WP-23).
- **Local engine certification**: `doctor --ollama --certify` (WP-19).
- **Release check 2026-10-11**: 13 of 14 gates pass on Anki 25.09.2 disposable collections; EV-02 is blocked (`EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE`).
- **Repository**: the archived GUI/TUI code moved to the `legacy` branch; release evidence, reviews, decisions and research moved to the `cli-dev` branch. The gate registry is now `resources/release-gates.json`.

## 0.1.0 — 2026-10-05

Headless CLI: prepare, review, validate, approve and export vocabulary and grammar cards; read Anki through AnkiConnect; durable local plans, jobs, snapshots and recovery records. Collection writes were preview-only.
