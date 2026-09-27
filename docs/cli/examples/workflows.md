# Workflow examples and state traces

These are proposed CLI examples, not commands to run during documentation review. Replace uppercase placeholders with IDs returned by the previous command. The executable is `linguist-anki-bridge`; `lab` is an optional shell alias. See operation IDs for exact failure branches. Never paste example `--apply` commands against the user's collection without task authorization.

## First setup

```sh
lab config init
lab config describe llm.model
lab resources list
lab config set llm.model INSTALLED_MODEL
lab doctor --purpose japanese_vocab
lab decks list
lab models inspect SOURCE_MODEL
lab decks map japanese_vocab --source-deck SOURCE_DECK --target-deck TARGET_DECK --source-model SOURCE_MODEL
lab config validate
```

Mapping also needs verified field/task mappings from the inspected model. `decks map` cannot infer arbitrary templates or create a target deck without a reviewed action. Missing native migration capability allows preparation; it blocks affected apply. Generation model choice is required only for generation, not authored/dictionary-only material.

## Add vocabulary

```sh
lab vocab add --purpose japanese_vocab --input words.jsonl
lab plans show PLAN
lab plans diff PLAN
lab plans resolve PLAN ISSUE --decision decision.json
lab plans validate PLAN --revision REVISION
lab plans approve PLAN --revision REVISION --digest DIGEST
lab apply PLAN --revision REVISION
lab apply PLAN --revision REVISION --apply
```

The first apply is a preview. Decision files must match the issue's schema, such as reviewed sense/cue/duplicate choice; arbitrary free text is not a waiver. Resolve only when issues exist. PLAN/REVISION/DIGEST must be taken from the latest output after edits; readiness and approvals for an older revision cannot authorize the new one.

Trace: input row → archived source/evidence → selected expression/sense → generated or authored annotations → validated prompts/media → ready revision → digest approval → preflight/checkpoint/snapshot/journal → new note with operation marker → read-back → receipt. Duplicate candidates stop at review or are explicitly skipped according to policy; they never silently become an update.

## Revamp vocabulary

```sh
lab notes show NOTE_ID
lab vocab revamp --purpose japanese_vocab --note-id NOTE_ID
lab plans show PLAN
lab plans diff PLAN --live
lab plans validate PLAN --revision REVISION --live
lab apply PLAN --revision REVISION
```

Review all source content and retained task cues, approve the current digest, then explicitly apply as above when authorized. Trace: full old note/cards/media archive → OCR before dictionary/generation → retain reviewed tasks/decks → native explicit mapping → same retained card IDs/history/scheduling → observed receipt. If the source changed, revise/reprepare; never force-overwrite. Normal reviews since preparation are captured fresh at apply.

## Add grammar

```sh
lab grammar add --purpose english_grammar --document grammar.json
lab plans show PLAN
lab plans edit PLAN --base-revision REVISION --patch grammar-edit.json
lab plans validate PLAN --revision NEW_REVISION
lab plans export PLAN --revision NEW_REVISION --output grammar-plan.bundle
```

A structured authored pattern/formation/examples source can avoid OCR/Ollama. Recognition is default; Application requires explicit opt-in and an answerable exercise. Export is an alternative to apply, not an implicit collection write. Review/approve/apply follows the shared sequence.

## Revamp grammar with several units

```sh
lab grammar revamp --purpose japanese_grammar --note-id NOTE_ID
lab plans show PLAN
lab plans resolve PLAN SEGMENTATION_ISSUE --decision units.json
lab plans resolve PLAN ANCHOR_ISSUE --decision anchor.json
lab plans diff PLAN
lab plans validate PLAN --revision REVISION
```

Trace: front/back images → ordered OCR regions → reviewed units/use keys → one history anchor → independently journaled fresh siblings → verified siblings → anchor mapped/updated → group receipt. Keep Vietnamese explanation language explicitly through purpose settings if desired. If one sibling succeeded and another failed, resume/reconcile exact child IDs; do not regenerate/recreate the whole group.

## Batch

```sh
lab jobs create --mode prepare --purpose japanese_vocab --query QUERY
lab jobs run JOB
lab jobs items JOB
lab jobs pause JOB
lab jobs show JOB
lab jobs resume JOB
```

Review produced plans, then create a separate immutable apply job from exact approved revisions. `jobs run APPLY_JOB --apply` and every later resume require current write authorization. Prepare/simulate modes never change into apply. Pause_requested and paused are different states; cancellation keeps successful writes.

## Unknown response and restore

```sh
lab recover inspect --pending
lab recover inspect OPERATION --live
lab recover reconcile OPERATION
lab snapshots show SNAPSHOT
lab snapshots restore SNAPSHOT
```

These inspect/preview. `recover reconcile OPERATION --apply` continues only proven-safe effects; absent creation marker alone leaves ambiguity. `snapshots restore SNAPSHOT --apply` executes a conflict-checked restore journal and preserves later retained-card reviews. It does not delete studied created notes or reset old scheduling automatically. Package disaster recovery is a separate manual last resort.

## Expected smaller-model implementation report

For the chosen WP, report: implemented OP/ALG IDs; files changed; setting consumers covered; tests actually run/results; native/fake evidence distinction; remaining RV/issue IDs; next eligible package. Do not report “safe” or “done” solely from compile success or this handbook.
