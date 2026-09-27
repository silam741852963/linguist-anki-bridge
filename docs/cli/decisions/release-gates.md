# Final release and verification gates

Design finalized; implementation evidence is not yet collected. Gates are proof requirements, not further user preference questions. Gate metadata: [release-gates.json](release-gates.json).

## Start order and baseline scope

Start WP-01 domain/contracts, then WP-02 CLI/config and WP-03 native bridge/isolated compatibility fixtures. WP-04 durability follows WP-02. Complete read/capture/providers and both preparation workflows before write handlers. Deliver checkpoints/apply/reconcile/restore before applying jobs. WP-14 closes migrations/retention/optional resource interfaces; WP-15 and WP-16 establish complete release evidence. No architecture spike is left undecided.

Use a new root workspace with focused CLI/core/application/storage/Anki/provider crates. Build default members include no legacy Qt/TUI crate. Pin Rust 1.98.1, edition 2024, Cargo.lock and minimum feature sets; test actual MSRV with Cargo rather than inheriting stale 1.85 claims. Keep tested existing dependency lines initially, add clap/serde/schema/JCS/rendering test dependencies with locked versions, then isolate upgrades into compatibility changes. The [Cargo rust-version contract](https://doc.rust-lang.org/cargo/reference/rust-version.html) supports declaring/test-driving MSRV; it does not prove dependencies work merely from a version label.

Baseline required: all four workflows, Japanese/English, CLI setup/read/prepare/review/export/apply/restore, native safe mapped migration, bounded jobs, typed complete settings, fixed v2 models, backups and recovery. Optional engines/remote providers/browser fallback are explicit unsupported feature gates until delivered; their absence cannot prevent authored/dictionary-only baseline use. Registered optional settings are validated and rejected clearly when activated without capability.

## Gate table

| Gate | Owner | Evidence | Failure policy |
| --- | --- | --- | --- |
| EV-01 | WP-01/09 | v2 schemas, JCS cross-language vectors, intent/render/task fixtures | No approval/apply of incompatible document |
| EV-02 | WP-02/14 | Every setting nondefault consumer or unavailable-feature rejection; precedence/redaction | Reject bad/ignored config |
| EV-03 | WP-03 | Companion registration/version/auth/serialized native worker fixtures | All managed writes unavailable |
| EV-04 | WP-03/11 | Epoch/profile/import/sidecar lineage/rebind fixtures | No automatic uncertain continuation |
| EV-05 | WP-03/10 | Real scheduling/history/media/schema package restore in disposable collection | Checkpoint-dependent writes unavailable |
| EV-06 | WP-03/11/12 | Forward/reverse task map retains IDs, decks, scheduler/history/FSRS evidence | Mapped migration/restore unavailable |
| EV-07 | WP-04/11 | UUID dedupe, delayed duplicate, removed marker, crash/disk-full boundary injection | Unknown remains needs_recovery |
| EV-08 | WP-05/10/12 | Full raw source/shared-field/media/model preservation | No lossy source/schema action |
| EV-09 | WP-08/12 | Multi-unit grammar anchor/fresh siblings/partial crash recovery | Unreviewed split never applied |
| EV-10 | WP-11/12 | Later reviews/edits and filtered/home-deck handling | Conflict or explicit supported restore only |
| EV-11 | WP-06–09/15 | 120 semantic fixtures and pinned local-model/OCR benchmark | Unreliable content needs review; failing capability blocked |
| EV-12 | WP-04/13 | Lease/liveness/control/retry/mode/restart tests | No concurrent writer or mode escalation |
| EV-13 | WP-06/14 | Resource hashes/licenses/safe paths/limits/offline/private-network tests | Invalid/unavailable resources rejected |
| EV-14 | WP-15/16 | Headless build and four full disposable end-to-end scenarios | No full CLI release claim |

Gate passes are scoped to tested protocol/Anki/AnkiConnect/model/resource versions, not forever. Artifacts record schema_version, gate_id, version matrix, fixture manifest hashes, commands, result, observed assertions and evidence paths. Never invent pass results or substitute mocked history tests for real native proof.

## Input and process resource limits

Assets/source archives use configurable registered limits; failures block the affected item rather than truncate evidence. Source GIFs are allowed validated raster assets. Resource installs use manifest-declared sizes/checksums plus configurable download/unpacked limits, with archive traversal/symlink/expansion rejection. OCR/browser/voice processes have bounded deadlines and memory through configured helper limits. Unsupported engine-specific settings fail before invocation.

Schema migration needs backup and explicit warning acceptance; normal restore preserves later study. Full disaster recovery, history deletion, custom target models, active-state relocation, automatic Anki sync and other-language production support are **deferred scope**, not design questions to resolve during baseline coding.

## Done means

All R/FG choices are decided. RV risks have EV tests/owners/failure policy. All 61 OP handlers map to algorithms/tests; all settings map to consumers or explicit feature rejection; read.py routes include relevant decision pages. Gate results are collected during implementation. Implementation starts without waiting for a new design approval, within the user's authorized task scope.
