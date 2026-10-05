# Command examples for linguist-anki-bridge 0.1.0

These examples match release 0.1.0 (see [release check](../evidence/release-2026-10-05/README.md)). Every command path and flag below is checked against the built binary by `crates/linguist-cli/tests/release_ux.rs`. Replace uppercase placeholders with values printed by an earlier command. [Workflow examples](workflows.md) describe the target design. This page lists only what 0.1.0 does.

## Setup and diagnostics

```sh
linguist-anki-bridge --help
linguist-anki-bridge config init
linguist-anki-bridge config validate
linguist-anki-bridge config describe llm.model
linguist-anki-bridge config set llm.model gemma4:12b
linguist-anki-bridge doctor --local
linguist-anki-bridge doctor --ollama
linguist-anki-bridge doctor
linguist-anki-bridge completions bash
```

`doctor --local` contacts nothing. `doctor` reads AnkiConnect, and `doctor --ollama` reads local Ollama model metadata. Each report includes `release_gates`, the recorded gate statuses of this build.

## Prepare without Anki (offline, authored)

```sh
linguist-anki-bridge --offline --purpose japanese_vocab --set dictionary.provider=authored --set llm.enabled=false vocab add --expression 食べる --meaning "to eat" --sense-key eat --target-language ja
linguist-anki-bridge --offline --purpose english_grammar --set llm.enabled=false grammar add --document grammar.json
linguist-anki-bridge --offline --set dictionary.provider=authored --set llm.enabled=false vocab add --format jsonl --document words.jsonl
linguist-anki-bridge --output jsonl plans list
```

With generation enabled (the default), a local Ollama model such as `gemma4:12b` fills allowed fields only. It never overwrites authored ones, and every generated fact still needs review.

## Review, approve and export

```sh
linguist-anki-bridge plans show PLAN
linguist-anki-bridge plans show PLAN --issues-only
linguist-anki-bridge plans diff PLAN --from-revision 1
linguist-anki-bridge plans edit PLAN --base-revision REVISION --patch edit.json
linguist-anki-bridge plans resolve PLAN ISSUE --decision decision.json
linguist-anki-bridge plans validate PLAN --revision REVISION
linguist-anki-bridge plans approve PLAN --revision REVISION --digest DIGEST --actor NAME
linguist-anki-bridge plans export PLAN --revision REVISION --output plan.bundle.json
```

## Read Anki and prepare revamps (Anki with AnkiConnect running)

```sh
linguist-anki-bridge decks list --counts
linguist-anki-bridge models inspect "Basic"
linguist-anki-bridge notes count --deck "Japanese::Vocab"
linguist-anki-bridge notes show NOTE_ID --media
linguist-anki-bridge decks map japanese_vocab --source-deck SOURCE_DECK --target-deck TARGET_DECK --source-model SOURCE_MODEL --fields fields.json
linguist-anki-bridge --purpose japanese_vocab vocab revamp --note-id NOTE_ID
linguist-anki-bridge jobs create --purpose japanese_vocab --deck SOURCE_DECK --limit 50
linguist-anki-bridge jobs run JOB
```

A revamp draft stops at `SOURCE_NATIVE_HISTORY_REVIEW` in 0.1.0. Native history verification has no resolution yet.

## Apply, restore and checkpoints: previews only

```sh
linguist-anki-bridge apply PLAN --revision REVISION
linguist-anki-bridge snapshots list
linguist-anki-bridge snapshots restore SNAPSHOT
linguist-anki-bridge backup create --scope collection
linguist-anki-bridge backup verify package.colpkg --restore-test-target /tmp/empty-dir
linguist-anki-bridge recover inspect --pending
```

Each `--apply` form (`apply`, `snapshots restore`, `jobs rollback`, `recover reconcile`, `backup create`, `models install`) exits 3 with `CAPABILITY_UNAVAILABLE` and a `next` hint. Collection writes need the native companion transport, which 0.1.0 does not have.

## Maintenance

```sh
linguist-anki-bridge cache status
linguist-anki-bridge cache prune
linguist-anki-bridge resources list --required
linguist-anki-bridge resources install tesseract --source jpn.traineddata --version 4.1.0 --sha256 SHA256 --license Apache-2.0
linguist-anki-bridge config import --file old-config.yaml --output candidate.toml
linguist-anki-bridge jobs migrate --legacy batch_jobs.sqlite3
```
