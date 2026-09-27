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

The new Rust workspace builds a CLI without Qt or Python. Implementation has started with domain contracts and offline document tooling; the complete workflows, configuration, persistence and native bridge remain in progress. Anki writes are unavailable.

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
```

Structured input can be piped or redirected into `vocab add --document -` or `grammar add --document -`. Supply one v2 JSON record; both commands stage a local plan. Current authored preparation requires disabled generation/image search and `dictionary.provider=authored` as described in the implementation status. `document validate -` and `document digest -` also accept piped JSON.

See [implementation status](docs/cli/implementation/status.md) for package progress and known limitations. Git history, repository metadata and the project license remain at root.
