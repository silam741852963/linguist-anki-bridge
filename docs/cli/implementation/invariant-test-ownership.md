# Fixed invariant test ownership

This assigns each [fixed invariant](../contracts/invariants.md) a named test location and work package. An existing local test is partial evidence unless it covers the invariant's full Anki or recovery scope. Pending tests are requirements, not passes.

| Invariant | Owner | Named test evidence or required test |
| --- | --- | --- |
| INV-01 | WP-09 | `crates/linguist-cli/tests/config_commands.rs`; complete read/prepare/edit/validate/approve action audit pending |
| INV-02 | WP-11 | Disposable native test: stale approval, config value and `--yes` each fail without current `--apply` |
| INV-03 | WP-11 | Disposable native test: mutate one revision/source/binding at a time and reject every mismatch |
| INV-04 | WP-01, WP-05 | `crates/linguist-core/tests/domain.rs` rich dictionary/source types; `crates/linguist-application/tests/source_archive.rs`; full media restore proof pending |
| INV-05 | WP-01, WP-09 | `crates/linguist-core/tests/domain.rs` readiness, cue and review cases; full workflow review acceptance pending |
| INV-06 | WP-11 | `crates/linguist-store/tests/journal.rs` local request-started durability; disposable native boundary injection pending |
| INV-07 | WP-11 | Disposable native timeout/crash test: unknown effect reconciles before any retry or compensation |
| INV-08 | WP-11, WP-13 | `crates/linguist-store/tests/lease.rs` lease behavior; `crates/linguist-application/tests/jobs_apply.rs` job lease of a live owner never reclaimed and stale tokens fenced; canonical collection-writer key and integrated apply/model/backup/restore writer test pending |
| INV-09 | WP-03, WP-11 | `scripts/verify-native-forward-mapping.py`, `scripts/verify-native-apply.py` (FSRS memory state kept through deck moves) and `crates/linguist-cli/tests/release_scenarios.rs` revamp scenarios (Basic→v2 keeps card ID, scheduling, FSRS state and review log in real Anki 25.09.2 through the harness port); AnkiConnect transport and other mappings pending |
| INV-10 | WP-11, WP-12 | `crates/linguist-application/tests/restore.rs` (media collision never overwritten, uploaded media kept); `scripts/verify-native-restore.py` note types and media survive restore and deletion; live conversion/restore through the native transport pending |
| INV-11 | WP-11 | Disposable native test: successful completion requires read-back and durable matching receipt |
| INV-12 | WP-11, WP-13 | `crates/linguist-store/tests/store.rs` immutable revisions; `crates/linguist-application/tests/jobs_apply.rs` resume after pause, unknown outcome and lost worker keeps the revision and operation IDs with no duplicate creation (fake port); interrupted native resume test pending |
| INV-13 | WP-12 | `crates/linguist-application/tests/restore.rs` later study kept, later edits conflict, every reverse crash boundary resumes; `scripts/verify-native-restore.py` effect-level later-review preservation; live restore through the native transport pending; `release_scenarios.rs` restores after later study and refuses until a field decision on a later edit, in real Anki through the harness port |
| INV-14 | WP-14 | Retention test: unresolved journal, snapshot, accepted asset, backup and reader references resist pruning |
| INV-15 | WP-02, WP-14 | `crates/linguist-config/tests/config.rs`; nondefault downstream consumer coverage pending |
| INV-16 | WP-06–WP-08 | `crates/linguist-application/tests/generation.rs` protected merge; tool/shell/network invocation rejection pending |
| INV-17 | WP-03, WP-11 | `crates/linguist-anki/tests/native_intent.rs` structural boundary; verified bridge/native dedupe and CAS tests pending |
| INV-18 | WP-09, WP-11 | `crates/linguist-core/tests/domain.rs` content approval; live apply eligibility and checkpoint rejection tests pending |

Release-check evidence for these rows is in [release-2026-10-05](../evidence/release-2026-10-05/README.md); rows needing the native transport stay pending. WP-16 audits this table against actual test results and release evidence before any invariant is claimed complete.
