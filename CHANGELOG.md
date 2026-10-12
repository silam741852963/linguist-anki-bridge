# Changelog

## 0.3.0 — 2026-10-11

Unattended collection revamp campaigns (WP-24). Companion add-on 0.5.0; local store schema 15.

- **Campaigns**: `campaign create|list|status|batch|events|run|approve|item|skip|pause|resume|cancel|close|agent-slot|asset|install-timer`. A campaign freezes a selection into cohorts and batches and advances them one tick at a time through prepare jobs, policy review, generation, approval, apply jobs with their own checkpoint, verification of every card and review row, and checkpoint retention. All state is in the store; a killed tick, a restarted Anki or a new agent session continues where the last stopped. Only the user resumes a paused campaign.
- **Decision policy**: `plans resolve-batch --policy FILE|builtin` decides what the user's standing preferences settle (native history map, sense by reading, pictures by the OCR image classification, own recordings, generated facts) and lists every issue it leaves open; `resources/policies/campaign-default.json`.
- **One revision per pass**: `plans resolve-batch` publishes one revision per pass instead of one per decision.
- **Resumable generation**: `plans generate --all-items --pending` skips items with nothing to generate.
- **Partial preparation**: `jobs publish JOB --skip-failed` publishes a prepare job's plan without its failed notes.
- **Duplicate merge**: `plans merge-notes` archives duplicates on their survivor's item; `notes delete-merged` deletes a duplicate after its survivor is applied (companion `delete_merged_note`); `notes restore-merged` recreates it with its note, card and review IDs (`recreate_deleted_note`).
- **Native history**: a uniform history map drops entries for templates a note has no card of.
- **Agent driver**: `scripts/campaign-tick.sh` (systemd user timer) and the `campaign-tick` project skill.
- Release check 2026-10-12: every command passes, including all eleven desktop scenarios; 13 of 14 gates pass, EV-02 stays blocked.

## 0.2.1 — 2026-10-11

- `--help` and error guidance describe companion writes as available: `apply`, `snapshots`, `jobs`, `recover`, `plans`, `doctor`, `models` and `backup` no longer say writes are "unavailable until the native adapter is verified". The `CAPABILITY_UNAVAILABLE` hint names the released `linguist-bridge-*.ankiaddon` and Anki 25.09.2.
- Work-package records name the release-check, traceability and check-dev scripts on the `cli-dev` branch.
- Full release check rerun on the split repository ([evidence](https://github.com/silam741852963/linguist-anki-bridge/blob/cli-dev/docs/cli/evidence/release-2026-10-11-v0.2.1/README.md)): every command passes, 633 workspace tests and all ten desktop scenarios; 13 of 14 gates pass, EV-02 stays blocked (`EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE`).

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
