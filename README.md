# Linguist Anki Bridge

A headless command-line tool that prepares, reviews and applies Anki vocabulary and grammar cards, with a companion Anki add-on for safe, journaled collection writes. Four workflows: vocabulary add, vocabulary revamp, grammar add and grammar revamp, for Japanese and English.

- **Prepare and review.** Authored, dictionary and local-LLM (Ollama) sources; capture of existing notes with full archives; typed edits, grammar and vocabulary splits, validation, digest-bound approval and export.
- **Read Anki** through AnkiConnect: decks, models, notes, cards and media, profile-pinned.
- **Write Anki** through the companion add-on: apply, split groups, restore, job rollback, reconcile, checkpoints and model installation run under a collection-writer lease after a verified native checkpoint. Without a verified companion every write stops with `CAPABILITY_UNAVAILABLE` before touching Anki.
- **Recover.** Every uncertain native outcome is journaled and resolved by `recover reconcile`; nothing is re-sent blindly.

## Branches

| Branch | Contents |
| --- | --- |
| `main` | Rust CLI (`crates/`), Python companion add-on (`addons/`), contracts, resources, packaging, and the operational and implementation documentation (`docs/cli/`) |
| [`cli-dev`](https://github.com/silam741852963/linguist-anki-bridge/tree/cli-dev) | Development records: release evidence, reviews, design decisions, research, and the release-check and traceability tooling |
| [`legacy`](https://github.com/silam741852963/linguist-anki-bridge/tree/legacy) | The archived Rust/Qt GUI and Python/TUI applications |

## Install

Download from the [releases page](https://github.com/silam741852963/linguist-anki-bridge/releases):

1. `linguist-anki-bridge-VERSION-x86_64-unknown-linux-gnu.tar.gz` — verify it with `SHA256SUMS`, then copy the binary onto your `PATH`. It needs neither Qt nor Python.
2. `linguist-bridge-COMPANION.ankiaddon` — install in Anki (Tools > Add-ons > Install from file) next to AnkiConnect, and set an AnkiConnect API key. The companion is pinned to Anki 25.09.2; on any other build it registers nothing and writes stay unavailable.

Optional: Ollama (default model `gemma4:12b`) for generation, Tesseract with `jpn`/`vie` data for OCR. An Arch recipe is in [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD).

```sh
linguist-anki-bridge config init
linguist-anki-bridge doctor --local
linguist-anki-bridge doctor --bridge
```

[Command examples for 0.3.0](docs/cli/examples/commands-v0.3.0.md) list what works; every command and flag there is checked against the binary by the test suite. [Implementation status](docs/cli/implementation/status.md) states what is implemented and links each package's known limits.

## Documentation

The handbook is [docs/cli/](docs/cli/README.md): operations (`OP-*`), contracts, pipelines, recovery algorithms (`ALG-*`), configuration and the implementation packages (`WP-*`). Read one record at a time:

```sh
python3 docs/cli/read.py OP-34
python3 docs/cli/read.py ALG-RESTORE
python3 docs/cli/read.py WP-23
python3 docs/cli/read.py --setting llm.model
python3 docs/cli/validate.py
```

## Build and test

Rust 1.98.1 (edition 2024). Everything is locked.

```sh
cargo test --locked --workspace
cargo run --locked -- --help
python3 -m unittest discover -s addons/linguist_bridge/tests
python3 addons/build_addon.py "$(pwd)/dist/linguist-bridge.ankiaddon"
python3 scripts/release-build.py --verify-reproducible   # dist/release/, with SHA256SUMS
```

The real-Anki desktop scenarios are ignored tests that need Anki 25.09.2 and an AnkiConnect package (`scripts/disposable-anki-desktop.py`). The full release check that runs them and records gate evidence lives on the `cli-dev` branch.

## License

[MIT](LICENSE).
