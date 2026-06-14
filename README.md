# Linguist Anki Bridge

An open-source Python tool that interfaces with an Anki desktop application to automate and enhance language learning workflows.

## Features

- **Legacy Card Modernization:** Extracts text from screenshot-based flashcards using local or cloud LLMs (vLLM, Gemini) via OCR, fetching definitions, contexts, and regenerating notes.
- **New Vocabulary Ingestion:** Automatically ingests new vocabulary from dictionaries (Jisho, Cambridge) and builds beautiful Anki flashcards.
- **AnkiConnect Integration:** Directly connects to your local Anki application.
- **Safety First:** Includes `--dry-run` and terminal confirmation menus, as well as automatic collection backups.

## Setup

Requires Anki running locally with the [AnkiConnect](https://ankiweb.net/shared/info/2055492159) add-on.

```bash
pip install linguist-anki-bridge
```

Or for Arch Linux:
```bash
makepkg -si
```

## Usage

```bash
# General help
linguist-anki-bridge --help

# Modernize legacy cards in a deck (requires configuration)
linguist-anki-bridge modernize --deck "Japanese Vocab"

# Ingest new words
linguist-anki-bridge ingest --words "hello,world" --language en
```
