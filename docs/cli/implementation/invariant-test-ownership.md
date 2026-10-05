# Fixed invariant test ownership

This assigns each [fixed invariant](../contracts/invariants.md) a named test location and work package. Statuses were set by the WP-16 review (2026-10-05) from the actual test suite and the [release check](../evidence/release-2026-10-05/README.md). They mean:

- **local**: proven by Rust tests over local state and the fake native port.
- **real Anki (harness)**: proven against Anki 25.09.2 in disposable collections through the test harness port. This is not the shipped CLI transport.
- **pending native**: needs the verified companion transport, which does not exist. No invariant is complete for live collections until its native evidence exists.

| Invariant | Owner | Status | Evidence | Still required |
| --- | --- | --- | --- | --- |
| INV-01 | WP-09 | local; real Anki (harness) | `release_coverage.rs` and `release_scenarios.rs` record only read actions for read/prepare/approve; `linguist-anki` exposes no mutation action | — |
| INV-02 | WP-11 | local | `apply.rs::missing_apply_flag_and_preview_have_zero_effects`, `jobs_apply.rs::simulate_never_writes_and_apply_jobs_need_the_current_flag`, `apply_commands.rs` | native transport test (EV-03) |
| INV-03 | WP-11 | local | `apply.rs::authority_identity_and_checkpoint_refusals_mutate_nothing`, `apply.rs::weak_binding_stale_revision_and_remote_bridge_are_refused` | native binding (EV-04) |
| INV-04 | WP-01, WP-05 | local; real Anki (harness) | `semantic_corpus.rs` (120 fixtures, no source loss), `source_archive.rs`, scenario captures equal the live note | full media restore through the transport |
| INV-05 | WP-01, WP-09 | local | `domain.rs::review_is_bound_to_content_and_cannot_waive_errors`, `semantic_corpus.rs` | — |
| INV-06 | WP-11 | local | `journal.rs::started_request_survives_restart_and_is_pending_without_blind_rewind`, `journal.rs::crash_boundary_request_started_survives_process_exit` | native boundary injection (EV-07) |
| INV-07 | WP-11 | local | `apply.rs::timeout_after_accepted_create_reconciles_to_one_note`, `apply.rs::unknown_status_without_candidates_never_resubmits` | native timeout/crash (EV-07) |
| INV-08 | WP-11, WP-13 | local | `apply.rs::second_writer_is_rejected_by_the_collection_lease`, `lease.rs`, `jobs_apply.rs::an_expired_lease_of_a_live_worker_is_never_reclaimed` | integrated writer test with native model/backup/restore |
| INV-09 | WP-03, WP-11 | local; real Anki (harness) | `apply.rs::mapped_migration_retains_card_id_history_and_scheduling`, `scripts/verify-native-forward-mapping.py`, `scripts/verify-native-apply.py` (FSRS memory state), revamp scenarios | AnkiConnect transport; other note types |
| INV-10 | WP-11, WP-12 | local; real Anki (harness) | `restore.rs::colliding_original_media_uses_a_safe_alternate_name`, `scripts/verify-native-restore.py` | live conversion through the transport |
| INV-11 | WP-11 | local; real Anki (harness) | `apply.rs::create_commits_with_marker_snapshot_receipt_and_owner_release`, `journal.rs::commit_requires_every_step_verified_and_is_terminal`; scenarios commit only after read-back (an FSRS read-back mismatch produced `needs_recovery` before the deck-move fix) | native receipts (EV-03) |
| INV-12 | WP-11, WP-13 | local | `store.rs` immutable revisions, `jobs_apply.rs::an_unknown_outcome_stops_dispatch_and_a_restart_reconciles_without_duplicates` | interrupted native resume |
| INV-13 | WP-12 | local; real Anki (harness) | `restore.rs::restore_after_later_study_restores_content_and_keeps_new_history`, `restore.rs::every_reverse_effect_crash_boundary_resumes_the_same_restore_journal`, revamp scenarios (later review kept, later edit conflicts) | live restore through the transport |
| INV-14 | WP-14 | local | `gc.rs::unresolved_recovery_blocks_pruning`, `gc.rs::active_readers_block_pruning`, `gc.rs::reachable_and_transitively_referenced_assets_survive_and_orphans_are_tombstoned` | — |
| INV-15 | WP-02, WP-14 | local, partial | `coverage.rs` (156 entries mapped), `config.rs`, targeted nondefault tests | a nondefault consumer test per setting (EV-02) |
| INV-16 | WP-06–WP-08 | local, partial | `generation.rs::request_separates_untrusted_data_preserves_facts_and_caps_supplements`, helper processes without a shell (`linguist-provider/src/process.rs`) | explicit adversarial tool/shell/download injection test |
| INV-17 | WP-03, WP-11 | pending native | `native_intent.rs` structural boundary only; the CLI refuses all writes | verified bridge, dedupe and CAS (EV-03/EV-07) |
| INV-18 | WP-09, WP-11 | local | `apply.rs::authority_identity_and_checkpoint_refusals_mutate_nothing`; `release_ux.rs` shows an approved plan blocked by `APPLY_BINDING_WEAK` | live eligibility through the transport |

WP-16 checked every named test against the current suite (579 passed, 0 failed). The [traceability record](../review/traceability.md) maps the same proofs to algorithms, contracts and failure-matrix rows.
