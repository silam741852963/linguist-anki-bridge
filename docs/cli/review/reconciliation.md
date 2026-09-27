# Reconciliation and design review

This document closes gaps between the earlier specification and research plan. Decisions are finalized implementation contracts under the user's delegation; the [final baseline](../decisions/README.md) closes all R decisions and newly found gaps. Empirical gates remain gates; ordinary choices are no longer unanswered approval questions.

| ID | Gap | Final contract | Implementation location |
| --- | --- | --- | --- |
| D01 | Earlier vocabulary-first scope conflicts with grammar request | All four workflows required; Japanese/English initial validated targets. | OP-21–OP-24 |
| D02 | `ingest/modernize` versus `add/revamp` | Canonical `vocab add|revamp`, `grammar add|revamp`; aliases use identical handlers, require explicit kind. | WP-02 |
| D03 | “Reviewed plan” lacks authorization semantics | `plans approve` approves a digest/revision only; `apply --apply` explicitly authorizes it and may record approval for ready items. Human-reviewed decisions cannot be bypassed with flags. | OP-33, ALG-APPLY |
| D04 | Dry-run job might become a write job | Job mode immutable; creating apply job records approval but does not authorize every later invocation. `jobs run/resume --apply` is required for write mode. | ALG-JOB |
| D05 | Production/spelling cue has no v2 field | Vocabulary adds ProductionPrompt, SpellingPrompt, ExplanationLanguage. Fixed final fields documented once in data contracts. | ALG-RENDER |
| D06 | Grammar use identity and precise recognition cue absent | Grammar adds UseKey, RecognitionPrompt, ExplanationLanguage. Contextual Application remains a regular card. | ALG-GRAMMAR |
| D07 | Stage source preservation versus legacy field loss | Every old field goes to source archive; personal content is mapped to PersonalNotes when appropriate. Unmapped meaningful content blocks conversion until acknowledged/preserved. | ALG-CAPTURE, ALG-VALIDATE |
| D08 | Missing `createBackup`; weak `updateNoteModel` | No action-name-based safety inference. Scheduled export/verified native backup path; native mapped migration with compatible adapter, otherwise blocked. | ALG-BACKUP, ALG-MIGRATE |
| D09 | Profile hash mistaken for collection ID | Strong adapter collection incarnation plus endpoint/profile when available. Weak/manifest-only binding permits reads/drafts/export; every managed write requires verified bridge session/lineage and explicit rebind after epoch change. | ALG-IDENTITY |
| D10 | Snapshot after-state saved before cleanup/read-back | Journal intent before request; verified actual after-state after every effect; completion only after final read-back and fsynced receipt. No routine physical source-media deletion. | ALG-APPLY |
| D11 | Injection retry might duplicate accepted write | Stable UUID client/server dedupe + native receipt and marker; exact reconciliation under writer lease before retry. Lost tag/ambiguous candidates cause conflict, never blind re-add. | ALG-RECONCILE |
| D12 | Multi-note split is not transactional | Parent recovery group, child operation IDs, anchor explicitly selected; journal each child. Crash rolls forward/reconciles first; compensation is journaled, not best-effort silence. | ALG-SPLIT |
| D13 | Restore might erase later Anki reviews | Content and scheduler evidence separated. Restore content/model while preserving current retained-card scheduling; no old-counter rewind after intervening reviews. Studied created notes are retained by baseline CLI; removal is separately authorized manual disaster recovery. | ALG-RESTORE |
| D14 | Restore interrupted halfway looks like a new conflict | Restore has its own journal and per-step expected states. Match pre/post/known intermediate states; resume recorded intent, do not re-run all steps unconditionally. | ALG-RESTORE |
| D15 | Shared model rollback can delete mature/new cards | Automatic note recovery does not delete/reorder shared templates or drop fields. Safe model reversion requires affected-note/card inventory and unchanged scope; otherwise backup/manual recovery. | ALG-MODEL |
| D16 | Old physical media can be shared | Remove references in rendered note only. Retain original bytes; no collection media deletion in ordinary CLI conversion. Cache cleanup only removes proven disposable app data. | ALG-APPLY, OP-56 |
| D17 | Config setting could silently enable writes or disable recovery | All behavior knobs configurable through typed registry; authorization/journal/snapshot/conflict invariants are fixed, not off-switches. Legacy `dry_run=false` never authorizes CLI writes. | ALG-CONFIG |
| D18 | Config edits mutate plans mid-run | Resolve/freeze settings before preparation; settings change creates next plan revision/job, never changes running work. Endpoints/identity changes require explicit rebind. | ALG-CONFIG, OP-28 |
| D19 | Null, empty and omitted update can collapse | Explicit `Keep/Set/Clear` intent; archived original values remain. Old v1 null→Keep, empty→Clear only with migration rules. | data contracts |
| D20 | Optional provider error blocks every note or silently empties it | Structured warning/error/review; optional stage fails preserve old value; missing meaningful answer/evidence is blocking. | ALG-PROVIDER |
| D21 | Schema compliance mistaken for language correctness | Validate both structure and evidence; uncertain grammar operators/formation and misleading cues require review. Models have no tool/write privileges. | ALG-GRAMMAR, ALG-VALIDATE |
| D22 | Generic target mapping could discard fields | Managed v2 mappings fixed to their declared fields. Custom output models are deferred; source models require explicit schema/task map validation. | ALG-CAPTURE |
| D23 | Backup existence mistaken for restore guarantee | Record scope, content, scheduling/media coverage, checksum and compatibility test evidence. Verify before protected batch. Archive is protection, not transparent one-note scheduling restore. | ALG-BACKUP |
| D24 | Cache/retention could erase accepted media or history | Durable content references + lease-aware marking; no automatic snapshot pruning. Export/verify retention requires separate implemented policy. | ALG-GC |
| D25 | CLI-only could require a new app server | Rust foreground commands; optional subprocess helpers. Native Anki extension belongs to existing add-on process; no independently hosted application API. | WP-03 |
| D26 | Dictionary-only/author-created notes prohibited accidentally | Meaningful dictionary or explicit author answer can be ready without Ollama; generated vocabulary definitions never masquerade as dictionary evidence. | ALG-VOCAB |
| D27 | Unlimited retries hide permanent/unknown failures | Central retry budgets and classification; unknown mutation→reconcile; content conflict/schema/capability→no automatic retry. | ALG-PROVIDER |
| D28 | Every safety option treated as user permission request | Fixed safe implementation defaults; user review amends design. Only explicit session authorization controls real external actions. | handbook |

## Design safety review outcome

The two earlier documents supplied useful goals but did not fully specify authorization, source/asset ownership, model field prerequisites, unknown-write outcomes or restore after intervening study. These gaps are closed as design rules here and in recovery algorithms. The existing source still has incompatible paths: uniform issue strings, partial config import, UI-bound adapters, `createBackup` assumption, unmapped model update, generic retry and multi-action restore. They are implementation work, not verified fixes.

**Evidence still required:** native extension integration/version handshake; true collection-incarnation identity; supported scheduled backup creation/restoration; retained-card and review-log preservation through forward/reverse model changes; lost-response creation reconciliation; resource/model quality on Japanese/English grammar and vocabulary. WP-03/WP-11/WP-12 gate these. An unsupported live collection is allowed to prepare/export plans but must not silently perform an unsafe model migration.

Finalization audit: 34 R decisions resolved and 24 additional gaps finalized in [register.json](../decisions/register.json). Compatibility/history/backup claims remain 14 named evidence gates; no unresolved preference blocks implementation.
