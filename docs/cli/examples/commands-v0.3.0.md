# Command examples for linguist-anki-bridge 0.3.0

These examples match release 0.3.0 (see the release check on the `cli-dev` branch). Every command path and flag below is checked against the built binary by `crates/linguist-cli/tests/release_ux.rs`. Replace uppercase placeholders with values printed by an earlier command. [Workflow examples](workflows.md) describe the target design. This page lists only what 0.3.0 does.

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

A revamp draft stops at `SOURCE_NATIVE_HISTORY_REVIEW` until `plans resolve-history` maps every source card from live companion evidence (see below).

## Install the companion add-on

Collection writes go through the companion add-on `linguist-bridge-0.4.0.ankiaddon`, published with this release. Install it in Anki (Tools > Add-ons > Install from file) next to AnkiConnect, set an AnkiConnect API key and restart Anki. The companion is pinned to Anki 25.09.2; on any other build it registers nothing and every write stays unavailable.

```sh
linguist-anki-bridge doctor --bridge
linguist-anki-bridge models install japanese_vocab
linguist-anki-bridge models install japanese_vocab --apply
```

## Bind, approve and apply

```sh
linguist-anki-bridge plans resolve-history PLAN --item ITEM --map 0=comprehension --actor NAME
linguist-anki-bridge plans resolve-batch PLAN --template --actor NAME
linguist-anki-bridge plans resolve-batch PLAN --decisions decisions.json
linguist-anki-bridge plans bind PLAN --digest DIGEST
linguist-anki-bridge plans validate PLAN --revision REVISION
linguist-anki-bridge plans approve PLAN --revision REVISION --digest DIGEST --actor NAME
linguist-anki-bridge apply PLAN --revision REVISION
linguist-anki-bridge apply PLAN --revision REVISION --digest DIGEST --apply
linguist-anki-bridge apply PLAN --revision REVISION --digest DIGEST --split-group GROUP --apply
```

`plans bind` publishes a child revision that carries the collection binding; validate and approve that revision before `apply --apply`. Every write runs under the collection-writer lease after a verified native checkpoint, and is journaled. Without a verified companion, API key, loopback endpoint and open profile, `--apply` exits 3 with `CAPABILITY_UNAVAILABLE` before any lease, journal or Anki request.

## Jobs over many notes

```sh
linguist-anki-bridge jobs create --purpose japanese_vocab --deck SOURCE_DECK --limit 50
linguist-anki-bridge jobs run JOB
linguist-anki-bridge jobs create --mode apply --plan PLAN --revision REVISION --digest DIGEST --create-checkpoint
linguist-anki-bridge jobs run JOB --apply
linguist-anki-bridge jobs show JOB
linguist-anki-bridge jobs rollback JOB
linguist-anki-bridge jobs rollback JOB --apply
```

## Restore, recover and checkpoints

```sh
linguist-anki-bridge snapshots list
linguist-anki-bridge snapshots restore SNAPSHOT
linguist-anki-bridge snapshots restore SNAPSHOT --apply
linguist-anki-bridge recover inspect --pending
linguist-anki-bridge recover reconcile OPERATION
linguist-anki-bridge recover reconcile OPERATION --apply --rebind
linguist-anki-bridge backup create --scope collection
linguist-anki-bridge backup create --scope collection --apply
linguist-anki-bridge backup verify package.colpkg --restore-test-target /tmp/empty-dir
```

An uncertain native outcome is never re-sent blindly: `recover reconcile OPERATION --apply` resolves it from the companion ledger. After an Anki restart, sync or profile switch, continuing needs `--rebind`.

## Maintenance

```sh
linguist-anki-bridge cache status
linguist-anki-bridge cache prune
linguist-anki-bridge resources list --required
linguist-anki-bridge resources install tesseract --source jpn.traineddata --version 4.1.0 --sha256 SHA256 --license Apache-2.0
linguist-anki-bridge config import --file old-config.yaml --output candidate.toml
linguist-anki-bridge jobs migrate --legacy batch_jobs.sqlite3
```

## Collection revamp campaign (WP-24)

```sh
linguist-anki-bridge campaign create --spec campaign-spec.json --dry-run
linguist-anki-bridge campaign create --spec campaign-spec.json
linguist-anki-bridge campaign status
linguist-anki-bridge campaign run CAMPAIGN --max-seconds 3000
linguist-anki-bridge campaign batch CAMPAIGN 1
linguist-anki-bridge campaign item CAMPAIGN 3 NOTE --settled
linguist-anki-bridge campaign item CAMPAIGN 3 NOTE --defer "homograph"
linguist-anki-bridge campaign approve CAMPAIGN 3 --actor claude-agent
linguist-anki-bridge campaign asset DIGEST --output picture.png
linguist-anki-bridge campaign events CAMPAIGN --after 0 --limit 100
linguist-anki-bridge campaign install-timer CAMPAIGN --script scripts/campaign-tick.sh --workdir . --execute
linguist-anki-bridge campaign resume CAMPAIGN --by-user
linguist-anki-bridge campaign close CAMPAIGN
linguist-anki-bridge plans resolve-batch PLAN --policy builtin
linguist-anki-bridge plans generate PLAN --all-items --pending --base-revision 3 --digest DIGEST --use-current-settings
linguist-anki-bridge jobs publish JOB --skip-failed
linguist-anki-bridge notes restore-merged NOTE --apply
```

`campaign run` advances every batch without judgment and exits; `scripts/campaign-tick.sh` runs it from a systemd user timer and starts the `campaign-tick` agent skill only when a batch needs it. Only the user resumes a paused campaign. After `campaign close`, sync to AnkiWeb and choose **Upload**.
