# Linguist Anki Bridge

The new CLI design lives in **[docs/cli/](docs/cli/README.md)**. Start with its reading index, then load only the operation, algorithm, setting group or implementation package needed for the task.

```sh
python3 docs/cli/read.py OP-34
python3 docs/cli/read.py ALG-RESTORE
python3 docs/cli/read.py WP-11
python3 docs/cli/read.py --setting llm.model
python3 docs/cli/validate.py
```

The existing Rust/Python application, tests, contracts, packaging, scripts and historical documentation are preserved in **[legacy/](legacy/README.md)**. Run its commands from that directory. Current implementation files retain their pre-move contents; the CLI specifications describe future behavior, not a completed replacement.

The new Rust workspace builds a CLI (`linguist-anki-bridge` 0.1.0) without Qt or Python. It prepares, reviews, validates, approves and exports vocabulary and grammar cards, reads Anki through AnkiConnect, and keeps durable local plans, jobs, snapshots and recovery records. **Collection writes are not available in 0.1.0**: `apply`, `snapshots restore`, `jobs rollback`, `recover reconcile`, `backup create` and `models install` preview only, and their `--apply` forms exit 3 with `CAPABILITY_UNAVAILABLE`. The native companion transport does not exist yet, so no claim of a safe revamp of a real collection is made. The [release check](docs/cli/evidence/release-2026-10-05/README.md) records which release gates pass (scoped to Anki 25.09.2 disposable collections) and which are blocked. [Command examples for 0.1.0](docs/cli/examples/commands-v0.1.0.md) list what works.

Release build (locked, reproducible, with checksums; nothing is installed):

```sh
python3 scripts/release-build.py --verify-reproducible   # dist/release/
python3 scripts/release-check.py --msrv --release-build --benchmark
```

```sh
cargo test --locked --workspace
cargo run --locked -- --help
cargo run --locked -- completions bash
cargo run --locked -- doctor --local
cargo run --locked -- doctor --ollama --offline
cargo run --locked -- config show --defaults
cargo run --locked -- --purpose japanese_grammar config show learning --provenance
cargo run --locked -- document validate contracts/v2/fixtures/vocabulary.json
cargo run --locked -- document render contracts/v2/fixtures/grammar.json
cargo run --locked -- recover inspect --pending
# Read-only Anki inspection (Anki + AnkiConnect must be running):
cargo run --locked -- doctor
cargo run --locked -- decks list --counts
cargo run --locked -- models list
cargo run --locked -- notes list --query '' --limit 10
# Explicit media inspection: notes show NOTE_ID --media
# Initial revamp source draft (requires configured purpose field mappings):
cargo run --locked -- --purpose english_vocab --set llm.enabled=false \
  --set dictionary.provider=authored --set images.search_when_missing=false \
  vocab revamp --note-id NOTE_ID
```

Structured input can be piped or redirected into `vocab add --document -` or `grammar add --document -`. Supply one v2 JSON record; both commands stage a local plan. Current authored preparation requires disabled generation/image search and `dictionary.provider=authored` as described in the implementation status. `document validate -` and `document digest -` also accept piped JSON.

See [implementation status](docs/cli/implementation/status.md) for package progress and known limitations. Git history, repository metadata and the project license remain at root.
