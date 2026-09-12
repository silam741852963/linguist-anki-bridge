# Native GUI refactor

The refactor is additive until the native application reaches feature parity.
The Python/Textual application remains the behavioral oracle and safe fallback.

## Chosen foundation

- Rust owns domain rules, orchestration, persistence, and service adapters.
- Qt Quick/QML owns the desktop presentation and normal pointer/text editing.
- CXX-Qt is the narrow bridge between QML and Rust.
- Keyboard navigation remains complete through Qt focus order and shortcuts;
  modal editing is an optional later preference rather than the default input
  model.
- The active Omarchy palette is read from
  `~/.config/omarchy/current/theme/colors.toml`, with an internal dark fallback.

The first native screen is a review workspace because it exercises the most
important product boundary: source data becomes an editable, explainable draft
before a transactional Anki write.

## Repository boundaries

| Path | Responsibility |
| --- | --- |
| `crates/linguist-core` | Versioned card contracts and batch recovery rules; no GUI or I/O |
| `crates/linguist-application` | Provider ports and application use-case types |
| `crates/linguist-desktop` | CXX-Qt bridge, Omarchy palette adapter, and QML shell |
| `contracts/v1` | Language-neutral persisted/API contract definitions |
| `contracts/fixtures` | Golden documents consumed by parity tests |
| `src/linguist_anki_bridge` | Existing Python implementation and migration oracle |

The GUI must never call AnkiConnect, OCR, dictionaries, or Ollama directly.
It sends application commands and renders application state. This prevents a
second copy of processing and recovery logic from growing inside QML.

## Invariants already carried forward

1. Enrichment produces a logical `CardDocument` before Anki field mapping.
2. Modernization and injection share one card contract.
3. Missing expression or unresolved issues make a document unready.
4. Multiple logical values mapped to one Anki field join deterministically.
5. Interrupted processing returns to pending; interrupted commit returns to a
   processed artifact so the original snapshot can be reused.
6. Opening the application never resumes batch writes automatically.
7. Every displayed field can carry original, dictionary, OCR, generated, or
   user provenance.

## Build and verify

Prerequisites are Rust 1.85+, CMake 3.24+, Qt 6 with Qt Quick Controls, and a
discoverable `qmake` executable.

```bash
cargo test --workspace
cargo run -p linguist-desktop
```

The current QML data is intentionally local sample state. It validates the
desktop composition, theme bridge, editing defaults, focus behavior, and the
review-first information layout before adapters can mutate Anki.

## Regenerating card fixtures

Fixture source documents under `contracts/fixture-sources` are deterministic
inputs to the existing Python `build_card_document` function. Generate a
contract only by supplying both paths explicitly:

```bash
.venv/bin/python scripts/export_card_contract.py \
  --input contracts/fixture-sources/modernization.json \
  --output contracts/fixtures/modernization-card.v1.json
```

The exporter has no provider setup and no default output path. Tests require the
checked-in output to match a fresh export byte-for-byte.

## Migration slices

1. Export Python `CardDocument` results into the versioned JSON contract and
   run golden parity tests from both languages.
2. Implement the AnkiConnect adapter, including model validation, media
   transactions, snapshots, backups, and restore.
3. Move the durable SQLite job repository and recovery state machine.
4. Port deterministic processing: cleanup, field mapping, rendering, and image
   classification.
5. Port OCR, dictionaries, Kanji, images, Ollama, and TTS behind individual
   application ports with caching, cancellation, and rate limits.
6. Connect queue, field review, rendered card preview, batch jobs, imports,
   settings, and history to real application state.
7. Add configuration and recovery-data migration, then retire the Python UI
   only after parity and interruption tests pass.
