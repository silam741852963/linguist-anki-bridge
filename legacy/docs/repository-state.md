# Repository state — version 0.0.1

This document describes the implemented state of Linguist Anki Bridge at the
0.0.1 baseline. It is a local-first Python application that inspects Anki
notes, enriches Japanese learning material, previews field-level changes, and
commits updates through AnkiConnect.

## Runtime and packaging

- Python 3.11 or newer; the current development environment also supports
  Python 3.14.
- The Textual terminal UI is the primary interface.
- `linguist-anki-bridge` starts the application.
- `linguist-anki-bridge --install-japanese-template` installs or upgrades the
  managed Japanese note type while Anki and AnkiConnect are running.
- `pyproject.toml` is the Python package source of truth. `PKGBUILD` provides
  the Arch Linux development package recipe.
- User configuration and logs live under
  `~/.config/linguist-anki-bridge`; generated preview media lives under the
  XDG cache directory. Neither is stored in the repository.

## Main architecture

| Module | Responsibility |
| --- | --- |
| `anki.py` | AnkiConnect requests, exact-note lookup, model installation and migration, field/media writes |
| `card_templates.py` | Managed Japanese model specification and three card templates |
| `card_model.py` | Canonical card document, rendering, schema mapping, media transactions, commit policy |
| `scraper.py` | Jisho, Kanji, Cambridge, Moedict, dict.cc and custom extraction paths |
| `ocr.py` | Multilingual OCR, image features, classification confidence and correction data |
| `llm.py` | Ollama model discovery plus structured nuance/example and grammar generation |
| `snapshots.py` | Word-level pre-write snapshots and restoration |
| `batch_jobs.py` | SQLite/WAL job state, artifacts, retry pacing and crash recovery |
| `markdown_text.py` | Safe Markdown editing and Anki HTML rendering |
| `tui/app.py` | Application state, panes, queue orchestration and commands |
| `tui/screens.py` | Processing pipelines, previews, editors, media selection and commits |
| `tui/batch_screen.py` | Batch creation, monitoring, control and mass rollback UI |
| `tui/setup.py` | Initial setup, settings and manual/CSV injection interfaces |

The canonical `CardDocument` is the boundary between enrichment and Anki. Both
modernization and injection build the same logical fields before mapping them
to a managed note schema. Media is staged and validated before a note write;
failed writes restore overwritten media.

## Japanese managed note type

`Linguist Japanese Vocabulary` contains five fields:

1. `Expression`
2. `Picture`
3. `Meaning`
4. `Kanji`
5. `Audio`

One note produces three stable card instances:

- **Comprehension:** expression to meaning, reading and supporting material.
- **Spelling:** audio/reading prompt with a typed Japanese answer.
- **Production:** picture prompt to spoken Japanese recall.

The installer preserves the ordinal-zero card when upgrading the earlier
managed one-card version, then adds Spelling and Production. Modernization can
migrate a supported legacy note in place through AnkiConnect. Commit performs
the same safe managed-template upgrade when necessary. Unexpected managed
schemas are rejected before media is written.

See [japanese-card-template.md](japanese-card-template.md) for review layouts
and field mapping.

## Modernization pipeline

1. Fetch the selected legacy note and its media once.
2. Extract Japanese, English and Vietnamese OCR where configured.
3. Classify the image using OCR, layout and visual features.
4. Preserve visual-recall images. Treat high-confidence dictionary screenshots
   as context rather than final card artwork.
5. Parse dictionary entries and preserve exact and related results.
6. Ask Ollama only for structured nuance and example pairs; dictionary senses
   remain authoritative.
7. Fetch Kanji construction with per-source fallbacks.
8. Replace removed/missing artwork with a validated illustrative image when
   available and generate pronunciation audio when needed.
9. Render the canonical fields and show a before/after comparison.
10. Capture a snapshot, migrate the note type if required, then commit the note
    and media transaction.

Downloaded artwork is converted to baseline RGB JPEG before preview or commit.
This avoids CMYK, alpha, embedded-profile and progressive-JPEG differences in
lightweight Linux image viewers.

## Injection pipeline

Manual input accepts multiple words before enqueueing; CSV input supports
`word`, `language`, `type` and `note`. Each candidate first performs an exact
Anki expression lookup:

- an existing note enters the modernization path;
- a new expression enters injection and targets the managed note type.

Vocabulary and grammar injection share processing, queue, preview, snapshot
and commit infrastructure. Author notes and type tags are retained as source
context. Injection does not require a successful dictionary lookup when the
user supplied enough context.

## Dictionary, Kanji and image resilience

- Jisho uses bounded direct API retries with incremental backoff.
- After direct failures, Crawl4AI tries the JSON endpoint through Chromium and
  then parses the rendered search result page.
- Wikipedia-labelled senses remain dictionary entries, but transport labels
  and link lines are omitted from card text.
- Kanji lookup falls back per character to KanjiAPI when the primary source is
  unavailable. Embedded stroke-order images remain part of the Anki field but
  are abbreviated in terminal comparisons.
- Illustrative image search checks multiple Wikipedia results and Wikimedia
  Commons, rejects invalid, nearly black and low-information candidates, then
  normalizes accepted images.

## TUI state

The main interface has Status, Decks, Cards, Preview and Log panes. Arrow keys
and Tab are the primary navigation controls. Preview owns card editing, image
selection, voice selection and snapshot access. Its sections independently
show comparison fields, image classification/OCR evidence, dictionary details,
LLM annotations and Kanji details.

The field editor uses modal NORMAL/INSERT behavior and explicit field commands.
Search mode supports next/previous matches and returns to editing with Escape.
The Preview remains populated when focus moves outside the application.

## Safety and persistence

- Dry-run is enabled by default.
- Every real commit captures a word-level snapshot before mutation.
- Snapshots include note fields, model/deck information and relevant media;
  injected-note snapshots record that a new note was created.
- Snapshot restoration can restore prior fields/media or remove a note created
  by injection.
- Writes validate managed schemas and conflicting media before changing Anki.
- Original dictionary screenshots are removed from the resulting field only
  after a replacement image succeeds; uncertain classifications never trigger
  automatic removal.

## Configuration surface

Configuration groups AnkiConnect, Ollama, OCR, image classification, dictionary
retry/fallback behavior, Kanji sources, image search, filters, dry-run and
per-language vocabulary/grammar deck mappings. Japanese OCR defaults to
`jpn+eng+vie`. Ollama is optional for deterministic image classification but is
used for nuance/examples and can adjudicate uncertain images when enabled.

## Current external constraints

- Anki Desktop and AnkiConnect must be running for reads, migration and commits.
- Ollama generation quality depends on the active model returning the required
  structured fields; response normalization handles common nested envelopes.
- Jisho and Kanji web sources can return upstream 502 responses. Retries and
  fallbacks reduce failures but cannot guarantee third-party availability.
- Google TTS and illustrative web image retrieval require network access.
- Crawl4AI browser fallback requires its Chromium browser installation.
- Japanese is the first fully managed card template. Other configured languages
  still use generic mappings pending dedicated templates.

## Verification baseline

The 0.0.1 repository test suite covers configuration migration, managed model
installation and migration, transaction rollback, snapshot restoration,
dictionary parsing, Jisho retry/fallback parsing, image normalization,
classification, LLM normalization, Markdown rendering and injection behavior.
Run it with:

```bash
python -m pytest -q
```
