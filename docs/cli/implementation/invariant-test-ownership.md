# Fixed invariant test ownership

This assigns each [fixed invariant](../contracts/invariants.md) a named test location and work package. Statuses were set by the WP-16 review (2026-10-05) and updated by WP-17 (2026-10-06) from the actual test suite and the real-Anki desktop scenarios. They mean:

- **local**: proven by Rust tests over local state and the fake native port.
- **real Anki (CLI)**: proven by `crates/linguist-cli/tests/desktop_scenarios.rs`, where every product step is a CLI command against Anki 25.09.2 desktop on a disposable base folder, with AnkiConnect and the companion installed through Anki's add-on installer.
- **real Anki (effects)**: the companion's native effect functions exercised on disposable collections by the `scripts/verify-native-*.py` probes.

| Invariant | Owner | Status | Evidence | Still required |
| --- | --- | --- | --- | --- |
| INV-01 | WP-09 | local; real Anki (CLI) | `release_coverage.rs` records only read actions for read/prepare/approve; desktop scenarios prepare, review and approve before any write; `linguist-anki` writes only through `lab*` controls | — |
| INV-02 | WP-11 | local; real Anki (CLI) | `apply.rs::missing_apply_flag_and_preview_have_zero_effects`, `jobs_apply.rs::simulate_never_writes_and_apply_jobs_need_the_current_flag`, `apply_commands.rs`; every desktop write is an explicit `--apply` | — |
| INV-03 | WP-11, WP-17 | local; real Anki (CLI) | `apply.rs::authority_identity_and_checkpoint_refusals_mutate_nothing`, `native_commands.rs::plans_bind_records_the_verified_binding_in_a_new_revision`, `desktop_scenarios.rs::identity_and_preconditions_block_writes` (profile switch, lost lineage) | — |
| INV-04 | WP-01, WP-05 | local; real Anki (CLI) | `semantic_corpus.rs` (120 fixtures, no source loss), `source_archive.rs`; revamp scenarios restore the exact original fields | full media restore through the transport for media-bearing notes |
| INV-05 | WP-01, WP-09 | local | `domain.rs::review_is_bound_to_content_and_cannot_waive_errors`, `semantic_corpus.rs`, `revamp.rs::native_history_evidence_resolves_review_and_maps_every_card` | — |
| INV-06 | WP-11 | local; real Anki (CLI) | `journal.rs::started_request_survives_restart_and_is_pending_without_blind_rewind`; `desktop_scenarios.rs::native_faults_recover_through_reconcile` (crash, ENOSPC) | — |
| INV-07 | WP-11 | local; real Anki (CLI) | `apply.rs::timeout_after_accepted_create_reconciles_to_one_note`; `native_faults_recover_through_reconcile` (timeout, crash, duplicate UUID, removed marker) | — |
| INV-08 | WP-11, WP-13, WP-17 | local; real Anki (CLI) | `apply.rs::second_writer_is_rejected_by_the_collection_lease`, `lease.rs`; companion owner fences (`test_native_runtime.py::test_stale_fence_session_change_and_in_flight_owner`); every desktop write holds the collection-writer lease | concurrent CLI writers against real Anki |
| INV-09 | WP-03, WP-11 | local; real Anki (CLI) | `apply.rs::mapped_migration_retains_card_id_history_and_scheduling`, `scripts/verify-native-forward-mapping.py`; `vocab_revamp_migrate_study_restore` (card ID, review log, FSRS state) | note types other than Basic and the managed models |
| INV-10 | WP-11, WP-12 | local; real Anki (effects) | `restore.rs::colliding_original_media_uses_a_safe_alternate_name`, `scripts/verify-native-restore.py` | a media-bearing restore through the CLI |
| INV-11 | WP-11, WP-17 | local; real Anki (CLI) | `apply.rs::create_commits_with_marker_snapshot_receipt_and_owner_release`, `journal.rs::commit_requires_every_step_verified_and_is_terminal`; companion `verified` only after read-back; desktop scenarios commit only after CLI read-back | — |
| INV-12 | WP-11, WP-13 | local; real Anki (CLI) | `store.rs` immutable revisions, `jobs_apply.rs::an_unknown_outcome_stops_dispatch_and_a_restart_reconciles_without_duplicates`; `grammar_split_crash_resume` | interrupted apply-job resume against real Anki |
| INV-13 | WP-12 | local; real Anki (CLI) | `restore.rs::restore_after_later_study_restores_content_and_keeps_new_history`; revamp and split scenarios keep the later review; `vocab_revamp_home_deck_edit_conflict_restore` (later edit conflicts until a field decision) | — |
| INV-14 | WP-14 | local | `gc.rs::unresolved_recovery_blocks_pruning`, `gc.rs::active_readers_block_pruning`, `gc.rs::reachable_and_transitively_referenced_assets_survive_and_orphans_are_tombstoned` | — |
| INV-15 | WP-02, WP-14 | local, partial | `coverage.rs` (156 entries mapped), `config.rs`, targeted nondefault tests | a nondefault consumer test per setting (EV-02) |
| INV-16 | WP-06–WP-08 | local, partial | `generation.rs::request_separates_untrusted_data_preserves_facts_and_caps_supplements`, helper processes without a shell (`linguist-provider/src/process.rs`) | explicit adversarial tool/shell/download injection test |
| INV-17 | WP-03, WP-11, WP-17 | **proven: local; real Anki (CLI)** | verified bridge: `native_port.rs::unverified_builds_and_missing_credentials_keep_writes_unavailable`, `native_commands.rs::an_unverified_companion_cannot_bind_or_write`; current binding: `identity_and_preconditions_block_writes` (profile switch, lost lineage), `native_faults` (session change needs `--rebind`); durable deduplication: `test_operations.py`, `native_faults` (duplicate UUID returns the stored state, changed payload refused); precondition checks: `test_native_runtime.py` (stale CAS refused before write), `identity_and_preconditions_block_writes` (source edited after approval) | — |
| INV-18 | WP-09, WP-11 | local; real Anki (CLI) | `apply.rs::authority_identity_and_checkpoint_refusals_mutate_nothing`; `vocab_add_apply_study_restore` (an unbound approved plan is blocked by `APPLY_BINDING_WEAK`) | — |

The [traceability record](../review/traceability.md) maps the same proofs to algorithms, contracts and failure-matrix rows.
