# Final review and traceability (WP-16)

> Update 2026-10-06: the write path, RI-01–RI-05 and RI-07 were closed by WP-17; see the [implementation pass review](implementation-pass-2026-10-06.md). This report is kept as the WP-16 record.

Review date: 2026-10-05. Scope: the whole repository at the WP-16 commit, against the [reconciliation table](reconciliation.md), [safety review](safety.md), [failure matrix](../recovery/failure-matrix.md), [invariants](../contracts/invariants.md), the setting registry and the 61 operations. This is a fresh traceability and safety pass over code, tests and evidence. It is not a security certification. It does not replace the native evidence the blocked gates require.

**Outcome:**
- The CLI safely prepares, reviews, validates, approves and exports vocabulary and grammar cards. It reads Anki and keeps durable, recoverable local records.
- Every operation, algorithm, setting, invariant, failure-matrix row and unknown-outcome state maps to code and to tests that exist and pass.
- Collection writes are not available: the native companion transport (WP-03) is incomplete. Seven of the fourteen release gates are blocked, so no full CLI release and no safe revamp of a real collection is claimed.
- The [consolidated package record](../implementation/packages.md) collects every package's specification, audit and known limits in one document, for the later implementation pass.

## Package statuses

| Package | Status | Evidence |
| --- | --- | --- |
| WP-01 domain and resources | complete | [audit 2026-10-01](../implementation/wp-01.md#completion-audit-2026-10-01) |
| WP-02 CLI/config shell | complete (audited by this review) | no package audit existed; its done criterion holds: every unfinished operation reports `CAPABILITY_UNAVAILABLE` and cannot mutate. See the limits in the [consolidated record](../implementation/packages.md#package-wp-02-cliconfig-shell) |
| WP-03 native bridge and safety gate | **incomplete**: fallback state | read port, companion ledger/protocol helpers and disposable effect evidence exist. Registered actions, the Rust mutation transport, strong collection identity and native dedupe/CAS are missing, so per its own done criterion migration stays blocked and the full revamp release is not marked complete |
| WP-04 durable store | complete | [audit 2026-10-03](../implementation/wp-04.md#completion-audit-2026-10-03) |
| WP-05 selectors and capture | complete | [audit 2026-10-03](../implementation/wp-05.md#completion-audit-2026-10-03) |
| WP-06 – WP-14 | complete with known limits | audits and limits in each package file |
| WP-15 release/UX | complete with known limits | [audit 2026-10-05](../implementation/wp-15.md#completion-audit-2026-10-05) |
| WP-16 final review | complete | this report, [traceability record](traceability.md), [consolidated record](../implementation/packages.md) |

## Test commands and results (2026-10-05)

| Command | Result |
| --- | --- |
| `cargo test --locked --workspace --no-fail-fast` | 579 passed, 0 failed, 5 ignored (the ignored tests are the real-Anki scenarios) |
| `cargo test --locked -p linguist-cli --test release_scenarios -- --ignored --test-threads 1` | 5 passed against Anki 25.09.2 (release check) |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 -m unittest discover -s addons/linguist_bridge/tests` | 57 passed |
| `python3 docs/cli/validate.py` | pass |
| `python3 scripts/op-coverage.py --check`, `python3 scripts/traceability.py --check`, `python3 scripts/generate-semantic-corpus.py --check`, `python3 scripts/consolidate-packages.py --check` | current |
| `python3 scripts/release-check.py --date 2026-10-05 --msrv --release-build --benchmark` | all 34 commands exit 0 ([evidence](../evidence/release-2026-10-05/README.md)); MSRV 1.98.1 passes; two clean release builds are byte-equal |

The release-check run predates this review's changes. Those changes are module-doc `ALG-*` tags, three new tests, documentation and generators; none changes product behaviour. The workspace test count above is from after them.

## Traceability summary

| Item | Covered | Where |
| --- | --- | --- |
| Operations OP-01–OP-61 | 61/61 with command-level tests, plus library and real-Anki tests for write paths | [operation coverage](../implementation/op-coverage.md) |
| Algorithms (18) | 18/18 tagged in source (`ALG-*` module docs) with named proofs | [traceability](traceability.md#algorithms) |
| Reconciliation contracts D01–D28 | 28/28 with status and proof; D08, D09, D11, D12 and D21 partly gated | [traceability](traceability.md#reconciliation-contracts) |
| Settings (156 registry entries) | 156/156 with a consumer, resolver role or reported unavailable feature | [setting coverage](../configuration/setting-coverage.md) |
| Invariants INV-01–INV-18 | 15 proven locally (6 of them also through real Anki via the harness); INV-15 and INV-16 partial; INV-17 pending native | [invariant ownership](../implementation/invariant-test-ownership.md) |
| Failure matrix (14 rows) | 14/14 with fake-port proofs; real-Anki evidence for 6 | [traceability](traceability.md#failure-acceptance-matrix) |
| Unknown outcomes | 9 states, each with a recovery route and a proof | [traceability](traceability.md#unknown-outcomes-and-recovery-routes) |
| Review risks RV-01–RV-15 | 3 closed for the tested scope, 3 closed locally, 4 with partial evidence, 5 open | [traceability](traceability.md#review-risks) |

## Explicit checks

- **Secrets and private data in outputs.** `crates/linguist-cli/tests/review_privacy.rs` (new) configures `anki.api_key_env` and `llm.api_key_env` with a secret in the environment. Across `config show/describe/validate`, `doctor` (all modes), reads, `vocab add`, `plans list/show/export` and the debug diagnostic log, the secret never appears in stdout, stderr or any file under HOME (config, state database, assets, logs, bundle). Plan listings never print personal notes. These existing tests cover the same ground:
  - `read_port.rs::response_errors_cannot_echo_credentials`;
  - `ollama.rs::bearer_credentials_are_sent_by_reference_and_never_enter_metadata_receipts`;
  - `preparation.rs::export_controls_private_archive_disclosure_and_checksums_every_asset`;
  - `legacy_import.rs::credentials_and_remote_endpoints_are_not_carried`;
  - syntax errors never echo values.

  Release-evidence logs are redacted for repository, home and temporary paths, and no user path or email remains in them.
- **Backups.**
  - No checkpoint is claimed without a verified package: `checkpoint_writes.rs` covers false success, truncation, claim mismatch and missing scope. Real Anki `.colpkg` scope and decode-restore checks pass (EV-05).
  - `backup create --apply` stays unavailable without the native export.
- **Later-study preservation.**
  - Restore keeps later reviews and current scheduling (`restore.rs`, `verify-native-restore.py`, the revamp scenarios).
  - Apply captures study since preparation fresh.
  - Deck moves keep FSRS memory state; this was a real defect found and fixed in WP-15.
- **Source ownership.**
  - Original fields, media bytes and model payloads are archived before any change (`semantic_corpus.rs`, `source_archive.rs`; scenario captures equal the live note).
  - Collection media and shared note types are never deleted: media collisions use alternate names, and the cache prune never touches Anki media.
  - Studied created notes are never removed.

## Capability evidence

| Capability | State |
| --- | --- |
| Read Anki through AnkiConnect (decks, models, notes, media) | available; profile-pinned, read-only |
| Prepare vocab/grammar add (authored, dictionary, local generation) | available; generation is review-required |
| Prepare vocab/grammar revamp | drafts only; stops at `SOURCE_NATIVE_HISTORY_REVIEW` (no resolution yet) |
| Review, edit, resolve, validate, approve, export, split grammar | available |
| Jobs (prepare) with leases, controls and recovery | available; apply/simulate runs refuse after their checks |
| Apply, restore, rollback, reconcile, checkpoint create, model install | **unavailable** (`CAPABILITY_UNAVAILABLE`, exit 3); orchestration is proven over the fake port and, through the harness, over real Anki effects |
| Config import/migrate, resources install, cache prune, legacy job import | available |
| Release build and evidence | reproducible build, checksums, gate records |

## Remaining review issues

These are open findings to address when the system is fully implemented. They come in addition to the blocked gates (EV-02, EV-03, EV-04, EV-07, EV-09, EV-11, EV-14) and the per-package known limits.

| ID | Finding | Effect | Suggested resolution |
| --- | --- | --- | --- |
| RI-01 | Every `LAB_*` environment variable except `LAB_CONFIG`/`LAB_PROFILE` is read as a setting override; unknown ones fail with `UNKNOWN_SETTING` | A credential variable named `LAB_*`, or a tool variable such as `LAB_ANKI_PYTHON` exported in the shell, breaks every command | reserve a distinct prefix for overrides (for example `LAB_SET_`), or ignore names referenced by `*_api_key_env` and document the reserved names |
| RI-02 | WP-03 has no completion audit and is incomplete | No write path; INV-17 pending; EV-03/EV-04/EV-07 blocked | implement registered `lab*` actions, the Rust `labMutate` transport, strong identity, native dedupe/CAS, and capture-identical model manifest digests |
| RI-03 | Plans prepared by the CLI never carry a collection binding | Even with a transport, approved plans fail `APPLY_BINDING_WEAK` | record the verified binding at preparation, or bind explicitly before approval |
| RI-04 | `SOURCE_NATIVE_HISTORY_REVIEW` has no resolution | Revamp workflows cannot reach approval through the CLI | resolve it with native history evidence from the companion |
| RI-05 | Model manifest digests differ between read capture (the raw model JSON) and the managed manifest | A real companion cannot satisfy both `source.model_manifest` checks and `create_note` checks with one function | define one canonical manifest projection used by capture, apply and the companion |
| RI-06 | The model benchmark is below target and nondeterministic (94.55% vs 95%, 54/55 on another run) | EV-11 cannot pass on model quality | raise the output budget for grammar or use structural repair; run repeated benchmarks; install the jpn/vie OCR packs for the OCR benchmark |
| RI-07 | `status.md` keeps earlier progress paragraphs, for example "WP-02 is in progress" and "WP-03 is in progress" | Readers can see stale states | the consolidated record and this report give the current statuses; rewrite `status.md` during the implementation pass |
| RI-08 | The real-Anki scenarios are `#[ignore]` and need Anki's Python | `cargo test --workspace` alone does not prove real-Anki behaviour | run `scripts/release-check.py` in CI on a machine with Anki |

No new safety defect was found that would let the shipped CLI mutate a collection. Every write path refuses before any lease, journal or Anki request.
