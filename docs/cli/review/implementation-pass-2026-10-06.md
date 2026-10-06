# Implementation pass review (WP-17)

Review date: 2026-10-06. Scope: the write path left open by the [final review](final-review-2026-10-05.md) — the native companion, the Rust transport, collection binding, native-history resolution and the review issues RI-01–RI-08. Package record: [WP-17](../implementation/wp-17.md).

**Outcome.** The CLI writes to Anki. `apply --apply`, `snapshots restore --apply`, `jobs rollback --apply`, `recover reconcile --apply`, `backup create --apply` and `models install --apply` perform verified, recoverable writes through the companion add-on installed next to AnkiConnect. Four workflows and native fault injection were run through CLI commands only, against real Anki 25.09.2 desktop on disposable base folders, with the companion installed through Anki's add-on installer. No user profile was opened.

## What is proven against real Anki

| Claim | Evidence |
| --- | --- |
| Companion registers inside real AnkiConnect, authenticates controls and declares seven variants | every desktop scenario starts only after `labCapabilities` reports them; `test_native_runtime.py` (real collection) |
| vocab add, grammar add, vocab revamp (FSRS) and grammar multi-unit split: prepare → review → bind → approve → `apply --apply` → later study → restore | `desktop_scenarios.rs`: `vocab_add_apply_study_restore`, `grammar_add_apply_study_restore_keeps_history`, `vocab_revamp_migrate_study_restore`, `vocab_revamp_home_deck_edit_conflict_restore`, `grammar_revamp_multi_unit_split_apply_study_rollback` |
| Card IDs, scheduling, FSRS memory state and review logs are kept through migration and restore | revamp and split scenarios compare card IDs, scheduler projections (including FSRS stability/difficulty) and review-row digests before, after and after later study |
| Timeouts, crashes between effect and receipt, duplicate UUIDs, ENOSPC at the ledger boundary, session change and a removed marker recover through `recover reconcile --apply` | `native_faults_recover_through_reconcile` |
| A split group interrupted after its sibling resumes without duplicates | `grammar_split_crash_resume` |
| INV-17: writes need the verified bridge, the current binding and durable dedupe/precondition checks | `native_port.rs`, `native_commands.rs`, `identity_and_preconditions_block_writes`, `native_faults_recover_through_reconcile`, `test_native_runtime.py` ([ownership](../implementation/invariant-test-ownership.md)) |

## Review issues

| ID | Status | Resolution |
| --- | --- | --- |
| RI-01 | resolved | only `LAB_SECTION__KEY` names are overrides; tool variables and credentials named by `*.api_key_env` are ignored (`config.rs::only_section_key_environment_names_are_overrides`) |
| RI-02 | resolved | WP-03 completed by WP-17 with its [completion audit](../implementation/wp-03.md#completion-audit-2026-10-06) |
| RI-03 | resolved | `plans bind` records the verified binding in a new revision before approval |
| RI-04 | resolved | `plans resolve-history` resolves `SOURCE_NATIVE_HISTORY_REVIEW` from companion evidence |
| RI-05 | resolved | one canonical manifest projection for capture, apply and the companion |
| RI-06 | open | model benchmark quality; see the release evidence (EV-11) |
| RI-07 | resolved | `status.md` rewritten as a current-state page |
| RI-08 | partly | `scripts/release-check.py` runs the desktop scenarios; there is still no CI machine with Anki |

## New findings

| ID | Finding | Effect | Suggested resolution |
| --- | --- | --- | --- |
| RI-09 | `jobs run`/`jobs resume` for apply jobs still return `CAPABILITY_UNAVAILABLE` | batch apply is only possible item by item or per split group | wire the job executor to `NativePort` and add a desktop job scenario |
| RI-10 | the companion ledger keeps `unknown` rows after the CLI resolves them | companion status alone cannot tell which unknowns were adopted | add a `resolved` event written by `recover reconcile` |
| RI-11 | session identity is exercised with restarts, a hook-driven close/reopen, a profile switch and a lost sidecar, not with a real `.colpkg` import or AnkiWeb full sync | those replacement paths rely on the same hooks without direct evidence | add disposable import and local sync-server scenarios |

Full limits: [WP-17 known limits](../implementation/wp-17.md#known-limits-revisit-after-all-packages).

## Release check

`python3 scripts/release-check.py --msrv --release-build --benchmark` on a clean tree, 2026-10-06 ([evidence](../evidence/release-2026-10-06/README.md)): every command exits 0; 586 workspace tests pass; the 8 desktop scenarios pass against Anki 25.09.2; MSRV 1.98.1 passes; two clean release builds are byte-equal.

| Gate | Status |
| --- | --- |
| EV-01, EV-05, EV-06, EV-08, EV-10, EV-12, EV-13 | pass (as before, now with desktop-scenario evidence where listed) |
| EV-03, EV-04, EV-07, EV-09, EV-14 | **pass** (blocked on 2026-10-05) |
| EV-02 | blocked: `EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE` |
| EV-11 | blocked: `OCR_BENCHMARK_LANGUAGE_PACKS_MISSING` (jpn/vie Tesseract packs not installed); model schema compliance 53/55 = 96.4% this run, still nondeterministic across runs (RI-06) |

A full CLI release claim therefore still waits on EV-02 and EV-11; the write path is no longer a blocker.
