# Application specification reference

## 3. Current architecture and evidence map

These sources were inspected for responsibilities, data shapes, pipeline orchestration, and transaction/recovery behavior. Tests were read as supporting contracts; live providers and the user's Anki collection were not exercised for this specification.

| Concern | Python source | Rust source | Implication for the CLI |
| --- | --- | --- | --- |
| Entry and packaging | `main.py`, `pyproject.toml` | workspace `Cargo.toml`, desktop `main.rs` | Python has template-install flags; ordinary startup launches Textual. No general command-only app exists. |
| Configuration | `config.py`, `tui/setup.py` | `linguist-config/src/lib.rs`, desktop settings helpers | Python YAML v2 and native JSON v1 expose different settings. Import is not complete parity. |
| Input and duplicate routing | `tui/setup.py`, `tui/app.py` | application `ingestion.rs`, exact resolution in `lib.rs` | Existing expressions route to modernization; Rust represents ambiguous matches explicitly. |
| Selection and browsing | `tui/app.py`, `tui/batch_screen.py` | application `selector.rs`, Anki metadata transport | Reuse indexed Anki search and bounded metadata reads. |
| Canonical document and rendering | `card_model.py`, `markdown_text.py`, `contracts.py` | core `card.rs`, `expression.rs`, `persistence.rs` | The shared card document is the enrichment/write boundary. Rust's intermediate dictionary data is less rich than Python's. |
| Pipeline composition | processing functions in `tui/screens.py` | `linguist-pipeline/src/lib.rs`, desktop live generation/enrichment adapters | Move live adapters out of UI code. Fix OCR ordering before claiming parity. |
| Dictionaries | `scraper.py` | dictionary `lib.rs`, `cambridge.rs`, `moedict.rs`, `dictcc.rs`, `custom.rs` | Multiple providers exist; a parser is not proof of a working live fallback. |
| Kanji | `scraper.py`, `fetch_kanji_construction_if_needed`, LLM helpers | dictionary `kanji.rs` | Keep ordered per-character sources and stroke-order media. |
| OCR and classification | `ocr.py` | OCR `lib.rs`, media classification helpers, desktop image policy | Python has calibrated layout features and local feedback; desktop Rust uses different decision logic. |
| Ollama | `llm.py` | `linguist-ollama/src/lib.rs` | Structured generation, normalization, grammar, vision, and mapping suggestions are distinct capabilities. |
| Images | `fetch_web_image`, media cache helpers in `utils.py` | `linguist-media/src/lib.rs` | Internet discovery, bounded validation, RGB JPEG normalization, and preservation policy. |
| Audio | `tts.py`, audio helpers in `tui/screens.py` | `linguist-audio/src/lib.rs` | Dictionary audio, online Google TTS, and native local eSpeak paths differ. |
| Commit and templates | `anki.py`, `card_model.py`, `card_templates.py`, split commit helpers | `linguist-anki/src/lib.rs`, application commit use case, core `managed_template.rs` | Recoverable multi-action writes, schema checks, managed model upgrades. |
| Snapshots and restore | `snapshots.py`, snapshot orchestration in `tui/app.py` | `linguist-snapshots/src/lib.rs`, application restore use case | Native snapshots add post-write conflict evidence. Historical snapshots have weaker evidence. |
| Durable jobs | `batch_jobs.py`, runners in `tui/app.py` | `linguist-jobs/src/lib.rs`, core `jobs.rs` | SQLite WAL, artifacts, leases, pause/retry/recovery, rollback and legacy migration. |
| Review and edits | TUI preview/editor/image/audio/snapshot screens | desktop `draft.rs`, `controller.rs`, `review_model.rs`, `preview_model.rs`, `commit_model.rs`, `batch_model.rs`, `backend.rs` | Preserve draft acceptance and pending edits via durable artifacts and commands. UI behavior itself is not the target. |
| GUI-only integration | theme loading and runtime CSS | desktop `theme.rs`, `accessibility_contract.rs`, `build.rs`, QML | Exclude from CLI build/runtime dependency graph. |
| Compatibility verification | `legacy/tests/test_core.py`, `test_batch_jobs.py`, `test_native_contracts.py`, `test_batch_ui.py`, exporter script | per-crate tests, contract schemas and fixtures | Reuse meaningful fixture contracts; UI tests do not prove CLI behavior. |

All Python module paths in this table are relative to `legacy/src/linguist_anki_bridge/`; all Rust crate paths are below `crates/` unless stated otherwise. HTML/CSS assets are currently under the Python package and embedded by Rust with `include_str!`; remove package-layout coupling before retiring Python.


Subsections:

- [3.1 Known discrepancies that the new specification must resolve](03-01-31-known-discrepancies-that-the-new-specification-must-resolve.md)
- [3.2 Grounding from the live collection](03-02-32-grounding-from-the-live-collection.md)
