# Safety review and remaining evidence

Review date: 2026-09-26. Scope: reconciled design plus observed Rust/Python paths; not a security certification or live mutation test. No collection writes were performed for this documentation update. The user's existing code changes are preserved.

## Outcome

The proposed design supplies conservative controls for the main loss/duplication risks: immutable source archives, explicit write authority, digest-bound review, verified checkpoint coverage, native task mapping, per-effect journaling, actual read-back, unknown-outcome reconciliation, later-study-aware restore and protected local retention. The existing implementation still needs the work packages. Documentation alone cannot establish the app is safe.

| Review ID | Evidence gap/risk | Selected requirement | Release gate |
| --- | --- | --- | --- |
| RV-01 | Installed custom updateNoteModel lacks explicit native field/card mapping | Tested native mapped adapter; no silent fallback | WP-03/WP-11, retained card/history/deck proof |
| RV-02 | createBackup absent; Python package export excludes scheduling | Verified supported checkpoint with scheduling/media/schema coverage | WP-03/WP-10, actual disposable package restoration |
| RV-03 | Same-profile collection replacement indistinguishable from profile-only identity | Native incarnation where possible; explicit manifest binding otherwise; uncertain recovery blocks | WP-03/WP-11, replacement fixture |
| RV-04 | Timeout after note creation; app tag might be removed | Stable operation marker plus exact read-back; absent marker is insufficient proof of no write | WP-11, durable native ledger investigation or explicit ambiguity block |
| RV-05 | Old snapshot/original scheduler might overwrite later reviews | Fresh apply scheduler; selective restore preserves retained current study | WP-12, later-review fixture |
| RV-06 | Grammar screenshot contains several patterns; unclear anchor | Reviewed segmentation/use key/one anchor, children fresh | WP-08/WP-12, multi-pattern fixture |
| RV-07 | Native OCR order/dictionary flattening lose generation evidence | Rich evidence and OCR-before-generation | WP-06–WP-08, live adapter parity |
| RV-08 | Generic jobs retry/rewind cannot distinguish accepted writes | Typed retries, immutable modes, unknown reconcile, one writer | WP-13, crash/control/lease fixture |
| RV-09 | Multi-action restore lacks independent recovery journal | Restore journal/checkpoint/intermediate-state matching | WP-12, reverse failure injection |
| RV-10 | Shared field/media/schema changes can erase unrelated material | Full archive; no physical source-media/shared-model deletion | WP-05/WP-10–WP-12, shared-reference fixture |
| RV-11 | Local model capability does not prove Japanese/English quality | Benchmark grounded vocabulary/grammar/OCR/cues, review uncertainty | WP-06–WP-09/WP-15, annotated fixture report |
| RV-12 | Optional OCR/browser/voice downloads and licenses vary | Explicit pinned resource install, checksum/license/capability validation | WP-06/WP-14, offline/missing-resource tests |
| RV-13 | Configuration import drops unsupported keys; runtime ignores options | Per-key accounting and registry-to-consumer coverage | WP-02/WP-14, all-setting nondefault tests |
| RV-14 | Disk failure can split local receipt and accepted Anki effect | Intent fsynced before send; stop writing; same journal recovery | WP-04/WP-11/WP-12, disk-full fault tests |
| RV-15 | External Anki edits can race despite app writer lease | Recheck preconditions and read-back; unexpected state blocks | WP-11/WP-12, external edit fixture |

These are implementation/evidence review items, not requests for 15 separate permissions. Optional resources and defaults can be amended through review. Safety protections cannot be disabled to pass a failed empirical gate.

## Finalized user-oriented defaults

Selected defaults: new vocabulary Comprehension only; new grammar Recognition only; existing tasks retained; images preserved; explanation language en unless explicitly configured; generation candidate gemma4:12b (installed capability/digest required); no automatic resources/downloads; source lookup parentheses retained; batch continues independent ordinary errors but stops shared identity/schema faults. Builtin japanese_grammar chooses vi supporting explanations; other initial presets choose en. Explicit configuration can override these defaults. Do not silently translate all personal source text.

Final command spelling is `vocab add|revamp`, `grammar add|revamp`. Review IDs appear with concrete next commands rather than opaque failure messages. Exact notes/cards/learning units are counted separately. Readiness is distinct from committed; approval is distinct from current invocation authorization. Config describe/show expose effective defaults and their origin.

## Review procedure for a smaller model

1. Read the selected WP and its ALG/OP/contracts; inspect the actual code path and working tree.
2. Trace each Anki effect to current authority, precondition, durable intent, actual postcondition and restart/restore route. Missing any link is a release blocker for that effect.
3. Trace each source field/media/card task to preserved output/archive/history; meaningful loss requires explicit scope and review.
4. Trace every setting to a validated registry entry and consumer; fail unsupported options clearly.
5. Inject failures at effect boundaries and compare exact source/target/receipts. Do not count a mocked success as native history proof.
6. Update review/evidence report with actual test results. Keep unsupported writes disabled and report the blocked capability precisely.

Review record for this documentation pass: command catalogue and ALG references checked; registry uniqueness/types/scopes and generated TOML default coverage checked; local links/fences/whitespace checked; earlier spec/research redirected to authoritative handbook. Application tests and real migration/restoration tests were not run because this task changes documentation only.

Documentation verification: `python3 docs/cli/validate.py` passed with 143 registered settings, 117 emitted concrete TOML defaults (nullable defaults omitted and wildcard mappings documented separately), 61 operations, 18 algorithms and 16 work packages. `git diff --check` passed for tracked changes; the handbook validator also checks new Markdown whitespace and links. These results establish documentation consistency only.

Final design review: [baseline decisions](../decisions/README.md) and [decision register](../decisions/register.json) close all preference/architecture items. Runtime source ambiguity still requires content review. Native correctness remains evidence to collect during implementation, with test owners/failure policies in [release gates](../decisions/release-gates.md).

Finalization consistency validation additionally checks all 34 resolved R decisions, 24 finalized FG approaches, 14 unrun EV gates, 155 settings in 29 groups, and four builtin purpose presets. These counts describe the current design registry; earlier counts record previous documentation passes.
