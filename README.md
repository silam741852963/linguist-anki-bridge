# Linguist Anki Bridge

Open-source Python application to bridge local Anki Desktop and Ollama with Crawl4AI web scrapers.
Automates legacy card modernization (screenshot OCR -> Ollama explanations) and new vocabulary ingestion (web scrapers -> Ollama examples -> gTTS audio fallback).

## Features
- **TUI & CLI**: Beautiful terminal user interface themed with Omarchy system colors (optional).
- **Legay Card Modernization**: Run multi-language OCR on screenshot images inside Anki cards and generate context/meanings via local Ollama.
- **New Vocab Ingestion**: Ingest words interactively or via CSV (`word,language,type,note`). Passes custom contextual notes to Ollama.
- **Grammar Modernization (夕暮れの詞)**: Extracts grammar from textbook screenshots (OCR) or grammar web pages (Crawl4AI) and builds standardized cards.
- **Safety First**: Optional dry run mode and LLM preview dialog before updating notes. Auto backup via `exportPackage`.

## Installation
Requires `tesseract` and optional language packs (e.g. `tesseract-data-jpn`, `tesseract-data-vie`, `tesseract-data-deu`, `tesseract-data-chi_tra`).

```bash
# PKGBUILD will be published to AUR
makepkg -si
```
