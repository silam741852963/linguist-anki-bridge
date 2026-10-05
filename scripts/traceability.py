#!/usr/bin/env python3
"""Write docs/cli/review/traceability.md, the WP-16 traceability record.

It maps every algorithm (ALG-*), reconciliation contract (D01-D28),
failure-matrix row, review risk (RV-01..RV-15) and unknown-outcome state to
the code that implements it, the tests that prove it and the evidence that is
still missing. Algorithm code locations come from `ALG-*` tags in source; every
cited test is checked to exist, so the record fails to generate rather than
cite a missing proof. `--check` fails when the committed file is stale.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/cli/review/traceability.md"
A = "crates/linguist-application/tests/"
C = "crates/linguist-cli/tests/"
S = "crates/linguist-store/tests/"
K = "crates/linguist-core/tests/domain.rs"
SCEN = C + "release_scenarios.rs::"

ALGS = {
    "ALG-CONFIG": ("Typed registry, layered resolution, freezing, legacy import", [
        "crates/linguist-config/tests/config.rs::all_override_layers_follow_precedence",
        "crates/linguist-config/tests/legacy_import.rs::every_legacy_key_is_accounted_and_the_candidate_validates",
        "crates/linguist-config/tests/coverage.rs::every_registry_entry_has_exactly_one_coverage_row"]),
    "ALG-RENDER": ("Fixed v2 field rendering", [K + "::fixtures_render_fixed_models_and_roundtrip",
                                                 K + "::html_and_media_cannot_bypass_typed_controls"]),
    "ALG-CAPTURE": ("Read capture, archives, field mapping", [
        A + "source_archive.rs::raw_note_model_and_card_assets_survive_publication_without_native_history_claims",
        A + "mapping.rs::exact_mapping_preserves_shared_and_unmapped_original_fields_without_claiming_facts",
        A + "semantic_corpus.rs::every_fixture_meets_its_annotation_and_the_corpus_policy"]),
    "ALG-GRAMMAR": ("Grammar segmentation, units, cues", [
        A + "grammar.rs::five_pattern_screenshot_becomes_five_reviewed_units_with_fresh_siblings",
        A + "grammar.rs::recognition_and_application_suggestions_are_focused_and_leak_free"]),
    "ALG-OCR": ("Bounded Tesseract OCR before generation", [
        A + "ocr.rs::recognition_keeps_regions_provenance_and_untrusted_text_verbatim",
        A + "ocr.rs::missing_packs_engines_and_executables_fail_before_recognition"]),
    "ALG-PROVIDER": ("Network/provider boundary, retries, cache", [
        "crates/linguist-provider/tests/reader.rs::transient_reads_retry_but_permanent_faults_do_not",
        "crates/linguist-provider/tests/reader.rs::public_names_resolving_to_private_addresses_are_denied",
        A + "ollama.rs::slow_inference_uses_the_llm_budget_not_the_network_read_timeout"]),
    "ALG-VALIDATE": ("Document, plan and live validation", [
        K + "::task_cues_reject_answer_leakage_and_invalid_task_kinds",
        K + "::review_is_bound_to_content_and_cannot_waive_errors"]),
    "ALG-VOCAB": ("Vocabulary dictionary, enrichment, generation", [
        A + "vocabulary.rs::optional_enrichment_failures_are_warnings_and_authored_cues_are_kept",
        A + "preparation.rs::dictionary_preparation_archives_all_senses_and_requires_explicit_selection",
        A + "generation.rs::output_cannot_override_core_facts_authored_fields_or_example_provenance"]),
    "ALG-APPLY": ("Authorized, journaled, read-back-verified apply", [
        A + "apply.rs::create_commits_with_marker_snapshot_receipt_and_owner_release",
        A + "apply.rs::authority_identity_and_checkpoint_refusals_mutate_nothing",
        SCEN + "vocab_add_prepare_review_apply_restore"]),
    "ALG-BACKUP": ("Verified checkpoints", [
        A + "checkpoint_writes.rs::verified_checkpoint_has_receipt_restoration_and_committed_journal",
        A + "checkpoint_writes.rs::false_api_success_without_file_is_not_a_checkpoint"]),
    "ALG-IDENTITY": ("Collection binding, lineage, rebind", [
        A + "apply.rs::profile_switch_or_session_change_stops_reconciliation_until_rebind",
        "crates/linguist-anki/tests/read_port.rs::native_declarations_are_profile_pinned_read_evidence_and_never_enable_writes"]),
    "ALG-JOB": ("Immutable-mode jobs with leases and controls", [
        A + "jobs_apply.rs::simulate_never_writes_and_apply_jobs_need_the_current_flag",
        A + "jobs_apply.rs::an_unknown_outcome_stops_dispatch_and_a_restart_reconciles_without_duplicates"]),
    "ALG-MIGRATE": ("Mapped note-type migration keeping history", [
        A + "apply.rs::mapped_migration_retains_card_id_history_and_scheduling",
        SCEN + "vocab_revamp_capture_migrate_study_restore"]),
    "ALG-MODEL": ("Journaled managed model installation", [
        A + "checkpoint_writes.rs::created_model_is_journaled_before_call_and_verified",
        A + "checkpoint_writes.rs::partial_model_needs_recovery_and_blocks_new_attempts"]),
    "ALG-RECONCILE": ("Unknown-outcome reconciliation", [
        A + "apply.rs::timeout_after_accepted_create_reconciles_to_one_note",
        A + "apply.rs::unknown_status_without_candidates_never_resubmits"]),
    "ALG-RESTORE": ("Later-study-aware restore with its own journal", [
        A + "restore.rs::restore_after_later_study_restores_content_and_keeps_new_history",
        A + "restore.rs::every_reverse_effect_crash_boundary_resumes_the_same_restore_journal",
        SCEN + "grammar_revamp_capture_migrate_study_restore"]),
    "ALG-GC": ("Mark-and-sweep pruning with protected history", [
        S + "gc.rs::reachable_and_transitively_referenced_assets_survive_and_orphans_are_tombstoned",
        S + "gc.rs::unresolved_recovery_blocks_pruning"]),
    "ALG-SPLIT": ("Grammar split: children first, one anchor", [
        A + "split.rs::children_are_created_first_then_the_anchor_keeps_its_history",
        SCEN + "grammar_revamp_multi_unit_split_apply_rollback"]),
}

# D-ID -> (status, evidence)
CONTRACTS = {
    "D01": ("implemented", [SCEN + "vocab_add_prepare_review_apply_restore", SCEN + "grammar_revamp_capture_migrate_study_restore"]),
    "D02": ("implemented (no aliases)", [C + "release_ux.rs::every_command_and_argument_has_help_text"]),
    "D03": ("implemented", [A + "apply.rs::missing_apply_flag_and_preview_have_zero_effects"]),
    "D04": ("implemented", [A + "jobs_apply.rs::simulate_never_writes_and_apply_jobs_need_the_current_flag"]),
    "D05": ("implemented", [K + "::fixtures_render_fixed_models_and_roundtrip"]),
    "D06": ("implemented", [A + "grammar.rs::exercise_and_recognition_templates_resolve_through_typed_decisions"]),
    "D07": ("implemented", [A + "mapping.rs::exact_mapping_preserves_shared_and_unmapped_original_fields_without_claiming_facts"]),
    "D08": ("implemented; native adapter gated (EV-03)", [A + "checkpoint_writes.rs::false_api_success_without_file_is_not_a_checkpoint"]),
    "D09": ("partial: weak binding refused; native incarnation gated (EV-04)", [A + "apply.rs::weak_binding_stale_revision_and_remote_bridge_are_refused"]),
    "D10": ("implemented", [A + "apply.rs::crash_after_receipt_before_commit_finalizes_with_the_stored_receipt"]),
    "D11": ("implemented over fake port; native ledger gated (EV-07)", [A + "apply.rs::timeout_after_accepted_create_reconciles_to_one_note"]),
    "D12": ("implemented; native crash recovery gated (EV-09)", [A + "split.rs::child_accepted_and_anchor_failed_resumes_without_duplicates"]),
    "D13": ("implemented", [A + "restore.rs::restore_after_later_study_restores_content_and_keeps_new_history", SCEN + "vocab_revamp_capture_migrate_study_restore"]),
    "D14": ("implemented", [A + "restore.rs::every_reverse_effect_crash_boundary_resumes_the_same_restore_journal"]),
    "D15": ("implemented (no shared model deletion)", [A + "checkpoint_writes.rs::exact_model_is_reused_and_same_name_different_manifest_blocks"]),
    "D16": ("implemented", [A + "restore.rs::colliding_original_media_uses_a_safe_alternate_name", A + "maintenance.rs::provider_cache_prunes_by_retention_and_budget_but_never_unmanaged_files"]),
    "D17": ("implemented", ["crates/linguist-config/tests/legacy_import.rs::every_legacy_key_is_accounted_and_the_candidate_validates"]),
    "D18": ("implemented", [A + "preparation.rs::both_authored_workflows_preserve_raw_input_and_freeze_settings"]),
    "D19": ("implemented", [K + "::field_intents_preserve_set_and_clear_distinctly"]),
    "D20": ("implemented", [A + "vocabulary.rs::optional_enrichment_failures_are_warnings_and_authored_cues_are_kept"]),
    "D21": ("implemented; quality benchmark below target (EV-11)", [A + "generation.rs::repair_limit_and_factual_failures_cannot_be_bypassed"]),
    "D22": ("implemented", [A + "mapping.rs::absent_empty_and_wrong_kind_mappings_are_explicit"]),
    "D23": ("implemented", [A + "checkpoint_writes.rs::missing_media_scheduling_or_card_fails_scope"]),
    "D24": ("implemented", [S + "gc.rs::unresolved_recovery_blocks_pruning"]),
    "D25": ("implemented (CLI only, add-on companion)", [C + "release_ux.rs::fresh_home_setup_and_offline_authored_preparation"]),
    "D26": ("implemented", [A + "preparation.rs::both_authored_workflows_preserve_raw_input_and_freeze_settings"]),
    "D27": ("implemented", ["crates/linguist-provider/tests/reader.rs::transient_reads_retry_but_permanent_faults_do_not"]),
    "D28": ("implemented (fixed defaults)", ["crates/linguist-config/tests/presets.rs::each_purpose_resolves_to_its_final_values"]),
}

FAILURES = [
    ("Backup absent/false result/missing scheduling/media", [A + "checkpoint_writes.rs::false_api_success_without_file_is_not_a_checkpoint", A + "checkpoint_writes.rs::missing_media_scheduling_or_card_fails_scope"], "real .colpkg scope checks (EV-05)"),
    ("Source edited between preparation and apply", [A + "apply.rs::source_edit_between_preparation_and_apply_conflicts_and_preserves_original"], "verify-native-apply.py CAS refusal"),
    ("Study since preparation", [A + "apply.rs::study_between_preparation_and_apply_is_captured_fresh_and_preserved"], "verify-native-apply.py; revamp scenarios"),
    ("Timeout after addNote accepted", [A + "apply.rs::timeout_after_accepted_create_reconciles_to_one_note"], "fake port only (EV-07)"),
    ("Crash after any field/model/media/deck effect", [A + "apply.rs::crash_after_receipt_before_commit_finalizes_with_the_stored_receipt", S + "journal.rs::crash_boundary_request_started_survives_process_exit"], "fake port only (EV-07)"),
    ("Disk full after Anki accepts write", [A + "apply.rs::disk_full_after_native_acceptance_stops_and_recovery_discovers_result"], "fake port only (EV-07)"),
    ("Partial native migration/card mismatch", [A + "apply.rs::partial_native_migration_with_replaced_card_needs_recovery"], "real FSRS read-back mismatch observed before the set_deck fix"),
    ("Profile switch or collection replacement", [A + "apply.rs::profile_switch_or_session_change_stops_reconciliation_until_rebind"], "no live fixture (EV-04)"),
    ("Shared media/model or filename collision", [A + "apply.rs::media_collision_never_overwrites_and_identical_media_is_reused", A + "restore.rs::colliding_original_media_uses_a_safe_alternate_name"], "verify-native-apply.py collision refusal"),
    ("Split child succeeds, anchor fails", [A + "split.rs::child_accepted_and_anchor_failed_resumes_without_duplicates"], "fake port only (EV-09)"),
    ("Later source edits/reviews before restore", [A + "restore.rs::later_personal_edits_conflict_until_explicitly_merged", A + "restore.rs::restore_after_later_study_restores_content_and_keeps_new_history"], "grammar/vocab revamp scenarios (EV-10)"),
    ("Crash during restore", [A + "restore.rs::every_reverse_effect_crash_boundary_resumes_the_same_restore_journal"], "fake port only"),
    ("Concurrent apply/restore", [A + "apply.rs::second_writer_is_rejected_by_the_collection_lease", S + "lease.rs::expired_lease_cannot_be_reclaimed_while_owner_is_alive"], "local leases only"),
    ("Cache prune during jobs/recovery", [S + "gc.rs::active_readers_block_pruning", S + "gc.rs::unresolved_recovery_blocks_pruning"], "local store"),
]

UNKNOWN = [
    ("Apply step `request_started`/unknown", "`recover inspect --pending`, `recover reconcile OPERATION` (preview; `--apply` gated)", [A + "apply.rs::unknown_status_without_candidates_never_resubmits"]),
    ("Restore step unknown", "repeat `snapshots restore SNAPSHOT --apply` resumes the same restore journal (gated)", [A + "restore.rs::unknown_reverse_effect_without_evidence_stays_in_recovery"]),
    ("Checkpoint export unknown", "journaled `needs_recovery`; new checkpoint only after evidence", [A + "checkpoint_writes.rs::rejected_and_unknown_exports_are_journaled_differently"]),
    ("Model install unknown", "reconciled only with operation evidence", [A + "checkpoint_writes.rs::lost_response_reconciles_only_with_operation_evidence"]),
    ("Apply job item unknown / worker lost", "`jobs run`/`jobs resume` reconcile before dispatch; `jobs audit`", [A + "jobs_apply.rs::a_worker_lost_mid_item_is_reconciled_from_its_recorded_operation"]),
    ("Preparation job interrupted", "`jobs recover JOB --execute`", ["crates/linguist-store/tests/preparation.rs::worker_checkpoints_reject_wrong_expired_and_released_fencing_tokens"]),
    ("Split child unknown", "`apply PLAN --split-group G --apply` resumes (gated)", [A + "split.rs::unknown_child_outcomes_are_reconciled_on_resume_never_recreated"]),
    ("Prune interrupted", "next `cache prune --execute` completes from tombstones", [S + "gc.rs::interrupted_prune_is_finished_from_its_tombstones"]),
    ("Process interrupted (SIGINT/SIGTERM)", "exit 130/143; `recover inspect --pending`, `jobs list`, `jobs recover`", [C + "release_ux.rs::sigint_exits_130_and_sigterm_exits_143_without_damaging_state"]),
]

RISKS = {
    "RV-01": ("evidence partial", "real-Anki mapped migration via effects (EV-06 pass); no registered companion (EV-03)"),
    "RV-02": ("evidence partial", "real .colpkg scope/restore (EV-05 pass); no native export adapter"),
    "RV-03": ("open", "no collection-replacement fixture (EV-04)"),
    "RV-04": ("open", "marker/ledger rules fake-port only (EV-07)"),
    "RV-05": ("closed for tested scope", "later reviews survive restore in real Anki (EV-10)"),
    "RV-06": ("evidence partial", "two-unit real-Anki split; crash recovery fake-only (EV-09)"),
    "RV-07": ("open", "OCR packs jpn/vie missing; no live parity run (EV-11)"),
    "RV-08": ("closed locally", "typed retries, immutable modes, leases (EV-12, fake port)"),
    "RV-09": ("closed locally", "restore journal crash boundaries (fake port)"),
    "RV-10": ("closed for tested scope", "full archives, no media/model deletion (EV-08, scenarios)"),
    "RV-11": ("open", "deterministic corpus passes; model benchmark 94.55% < 95% (EV-11)"),
    "RV-12": ("closed locally", "pinned resource install tests (EV-13)"),
    "RV-13": ("evidence partial", "per-key accounting complete; nondefault test per setting incomplete (EV-02)"),
    "RV-14": ("open", "disk-full injection fake-port only (EV-07)"),
    "RV-15": ("closed for tested scope", "preconditions and read-back refuse external edits (CAS in real Anki)"),
}


def tagged_files(alg):
    files = []
    for base in ["crates", "addons/linguist_bridge"]:
        for path in sorted((ROOT / base).rglob("*")):
            if path.suffix in (".rs", ".py") and "/tests/" not in str(path) and path.is_file():
                if re.search(rf"\b{alg}\b", path.read_text(errors="ignore")):
                    files.append(str(path.relative_to(ROOT)))
    return files


def check_ref(ref):
    file, function = ref.split("::")
    text = (ROOT / file).read_text()
    if f"fn {function}()" not in text:
        raise SystemExit(f"missing test: {ref}")
    return ref


def short(ref):
    file, function = ref.split("::")
    return f"`{Path(file).name}::{function}`"


def render():
    refs = set()
    out = ["# Traceability record", "",
           "Generated by `python3 scripts/traceability.py` (WP-16). Every cited test is checked to exist; "
           "`crates/linguist-cli/tests/release_evidence.rs` re-checks the list. Operations are traced in "
           "[operation coverage](../implementation/op-coverage.md), settings in "
           "[setting coverage](../configuration/setting-coverage.md), invariants in "
           "[invariant ownership](../implementation/invariant-test-ownership.md) and gates in the "
           "[release check](../evidence/release-2026-10-05/README.md).", "",
           "## Algorithms", "", "| ALG | Scope | Implementation (tagged sources) | Proof |", "| --- | --- | --- | --- |"]
    for alg, (scope, tests) in ALGS.items():
        files = tagged_files(alg)
        if not files:
            raise SystemExit(f"{alg}: no tagged implementation")
        refs.update(check_ref(t) for t in tests)
        out.append(f"| {alg} | {scope} | {'<br>'.join(f'`{f}`' for f in files)} | {'<br>'.join(short(t) for t in tests)} |")
    out += ["", "## Reconciliation contracts", "", "| ID | Status | Proof |", "| --- | --- | --- |"]
    for cid, (status, tests) in CONTRACTS.items():
        refs.update(check_ref(t) for t in tests)
        out.append(f"| {cid} | {status} | {'<br>'.join(short(t) for t in tests)} |")
    out += ["", "## Failure acceptance matrix", "", "| Injected failure | Proof | Real-Anki evidence |", "| --- | --- | --- |"]
    for row, tests, native in FAILURES:
        refs.update(check_ref(t) for t in tests)
        out.append(f"| {row} | {'<br>'.join(short(t) for t in tests)} | {native} |")
    out += ["", "## Unknown outcomes and recovery routes", "", "| State | Route | Proof |", "| --- | --- | --- |"]
    for state, route, tests in UNKNOWN:
        refs.update(check_ref(t) for t in tests)
        out.append(f"| {state} | {route} | {'<br>'.join(short(t) for t in tests)} |")
    out += ["", "## Review risks", "", "| RV | Status | Evidence |", "| --- | --- | --- |"]
    out += [f"| {rv} | {status} | {evidence} |" for rv, (status, evidence) in RISKS.items()]
    out += ["", "## Listed tests", "", "```text", *sorted(refs), "```", ""]
    return "\n".join(out)


def main():
    text = render()
    if "--check" in sys.argv:
        if OUT.read_text() != text:
            raise SystemExit(f"{OUT} is stale; run scripts/traceability.py")
        return
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
