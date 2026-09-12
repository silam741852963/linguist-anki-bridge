# Linguist Anki Bridge

Version **0.0.1**. See
[`docs/repository-state.md`](docs/repository-state.md) for the implemented
architecture, workflows, storage boundaries, and current limitations.

Open-source Python application to bridge local Anki Desktop and Ollama with Crawl4AI web scrapers.
Automates legacy card modernization (screenshot OCR -> Ollama annotations) and new vocabulary injection (dictionary parsing -> Ollama examples -> TTS audio fallback).

## Features
- **TUI & CLI**: Beautiful terminal user interface themed with Omarchy system colors (optional).
- **Legacy Card Modernization**: Run multi-language OCR on screenshot images inside Anki cards and generate context/meanings via local Ollama.
- **New Vocab Ingestion**: Ingest words interactively or via CSV (`word,language,type,note`). Passes custom contextual notes to Ollama.
- **Grammar Modernization**: Extracts grammar from textbook screenshots (OCR) or grammar web pages (Crawl4AI) and builds standardized cards.
- **Safety First**: Dry-run mode and previews are enabled by default. In write mode, each affected deck must be backed up successfully before its batch is changed.

## Installation
Requires `tesseract` and optional language packs (e.g. `tesseract-data-jpn`, `tesseract-data-vie`, `tesseract-data-deu`, `tesseract-data-chi_tra`).

```bash
# PKGBUILD will be published to AUR
makepkg -si
```

For development, create a Python 3.11+ virtual environment and install the
project with its test extras:

```bash
python -m pip install -e '.[test]'
pytest
linguist-anki-bridge --debug
```

Python 3.14 requires Crawl4AI 0.9.2 or newer so that pip can use lxml 6.
If an earlier failed install cached dependency metadata, rerun the install with
`--upgrade`.

## Configuration and workflow

Configuration is stored at
`$XDG_CONFIG_HOME/linguist-anki-bridge/config.yaml` (or `~/.config/...`).
Generated preview media is stored below
`$XDG_CACHE_HOME/linguist-anki-bridge/media` and is not part of the project.

1. Start Anki with AnkiConnect and start Ollama with at least one installed model.
2. Map one or more deck keys in the setup screen. Supported keys are
   `japanese_vocab`, `japanese_grammar`, `english_vocab`, `english_grammar`,
   `taiwanese_vocab`, `taiwanese_grammar`, `german_vocab`, and
   `german_grammar`.
3. Preview individual cards, keep dry-run enabled while checking output, then
   disable dry-run when ready. A failed pre-write backup aborts writes for that
   deck.

Single-word and CSV injection check the mapped Anki deck before doing any
enrichment. An exact existing expression is fetched and routed through the
same preview/modernization pipeline as a scanned legacy card; only a genuinely
new expression becomes an Inject item. The unified queue's Mode column makes
that decision visible before processing or committing.

The input screen uses NORMAL/INSERT navigation: `i` or `a` enters INSERT,
`Esc` returns to NORMAL, `j`/`k` changes fields, `h`/`l` changes tabs, `:w`
resolves and enqueues, and `:q` cancels.

For large migrations press `b` to open the durable Batch Jobs screen. Jobs can
be created from the visible Cards list or the entire active deck, paused,
resumed after a crash, retried, cancelled, and reverted as one operation. See
the [batch modernization operations manual](docs/batch-modernization.md).

CSV injection requires a `word` header and accepts optional `language`, `type`,
and `note` columns. Language aliases such as `japanese` select the corresponding
vocabulary deck; explicit deck keys may also be used.

If startup services are offline, verify the configured AnkiConnect and Ollama
URLs. An empty mapped deck is valid and opens as an empty queue. Dictionary,
image, and TTS failures are reported per card and do not imply that Anki itself
is offline.

## Japanese vocabulary card template

The project includes a responsive Japanese-first Anki note type designed for
the five values the bridge produces. With Anki and AnkiConnect running, install
or safely refresh it with:

```bash
linguist-anki-bridge --install-japanese-template
```

The command creates `Linguist Japanese Vocabulary` with **Comprehension**,
**Spelling**, and **Production** cards; it will not overwrite an unrelated note
type with different fields. Map Japanese Vocabulary in Settings
to `Expression`, `Picture`, `Meaning`, `Kanji`, and `Audio` as printed by the
installer. Existing notes are not converted automatically. See
[`docs/japanese-card-template.md`](docs/japanese-card-template.md) for the
layout and migration guidance.
