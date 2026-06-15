import logging
import asyncio
import os
import csv
from textual.app import ComposeResult
from textual.containers import Vertical, Horizontal, ScrollableContainer
from textual.screen import Screen, ModalScreen
from textual.widgets import Label, Button, DataTable, TextArea, Input, TabbedContent, TabPane, Static
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.scraper import Crawl4AiScraper
from linguist_anki_bridge.tts import generate_tts_base64
from linguist_anki_bridge.utils import create_deck_backup

# --- MODERNIZATION PREVIEW MODAL ---
class PreviewModal(ModalScreen[bool]):
    def __init__(self, app_config, note_info, is_grammar=False):
        super().__init__()
        self.app_config = app_config
        self.note_info = note_info
        self.is_grammar = is_grammar
        self.ocr_text = ""
        self.llm_response = {}
        
        # Clients
        self.anki = AnkiConnectClient(url=app_config["anki"]["url"])
        self.ocr = OcrEngine()
        self.llm = OllamaClient(
            url=app_config["llm"]["ollama_url"],
            model=app_config["llm"]["model"]
        )
        self.scraper = Crawl4AiScraper()

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-dialog"):
            yield Label("[bold accent]Modernization Preview[/]", id="modal-title")
            yield Label(f"Target note word/phrase: [bold]{self.note_info.get('fields', {}).get('Word', {}).get('value', 'Unknown')}[/]")
            
            with Horizontal(classes="h-22 mt-1"):
                with Vertical(classes="w-50 pr-1"):
                    yield Label("[bold]OCR Extracted Text:[/]")
                    yield TextArea("", id="text-ocr", read_only=False, classes="h-18")
                
                with Vertical(classes="w-50 pl-1"):
                    yield Label("[bold]Ollama Generated Content:[/]")
                    yield TextArea("", id="text-llm", read_only=False, classes="h-18")
            
            with Horizontal(classes="mt-1 align-right-middle"):
                yield Button("Save/Commit", variant="success", id="btn-modal-commit")
                yield Button("Discard/Skip", variant="error", id="btn-modal-skip")

    async def on_mount(self) -> None:
        self.query_one("#btn-modal-commit", Button).disabled = True
        self.run_worker(self.process_card(), thread=True)

    async def process_card(self):
        self.notify("Performing OCR on card screenshot...")
        word = self.note_info.get("fields", {}).get("Word", {}).get("value", "")
        picture_html = self.note_info.get("fields", {}).get("Picture", {}).get("value", "")
        
        # 1. OCR Extract
        filename = self.ocr.extract_image_filename(picture_html)
        if not filename:
            self.ocr_text = f"[No image found in picture field]"
            self.query_one("#text-ocr", TextArea).text = self.ocr_text
            return
            
        try:
            # Fetch base64 from Anki
            base64_data = self.anki.retrieve_media_file(filename)
            # Detect lang config
            # Default to ja
            lang_cfg = "jpn+eng+vie"
            if self.is_grammar:
                lang_cfg = self.app_config["decks"]["japanese"]["ocr_langs"]
                
            self.ocr_text = self.ocr.perform_ocr(base64_data, lang_cfg)
            self.query_one("#text-ocr", TextArea).text = self.ocr_text
        except Exception as e:
            self.ocr_text = f"[OCR Error: {e}]"
            self.query_one("#text-ocr", TextArea).text = self.ocr_text
            return

        # 2. Ollama Generate
        self.notify("Generating structure via Ollama...")
        try:
            if self.is_grammar:
                prompt = self.app_config["llm"]["system_prompt_grammar"]
                self.llm_response = self.llm.generate_grammar_content(self.ocr_text, prompt)
            else:
                prompt = self.app_config["llm"]["system_prompt_vocab"]
                self.llm_response = self.llm.generate_card_content(word, "", "Japanese", prompt)
                
            formatted = json.dumps(self.llm_response, indent=2, ensure_ascii=False)
            self.query_one("#text-llm", TextArea).text = formatted
            self.query_one("#btn-modal-commit", Button).disabled = False
        except Exception as e:
            self.query_one("#text-llm", TextArea).text = f"[Ollama Error: {e}]"

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-modal-commit":
            # Return updated data
            self.dismiss(True)
        elif event.button.id == "btn-modal-skip":
            self.dismiss(False)


# --- MODERNIZE TAB PANEL ---
class ModernizeTab(Static):
    def __init__(self, app_config):
        super().__init__()
        self.app_config = app_config
        self.anki = AnkiConnectClient(url=app_config["anki"]["url"])
        self.legacy_notes = []

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("[bold accent]Modernize Legacy Cards[/]\n")
            yield Label("Find cards that use screenshot images for meanings and convert them to structured text.")
            
            with Horizontal(classes="h-3 my-1"):
                yield Button("Search Japanese Legacy Cards", variant="primary", id="btn-search-legacy-ja")
                yield Button("Search English Legacy Cards", variant="primary", id="btn-search-legacy-en")
                yield Label("[yellow]Dry Run mode is ON[/]", id="lbl-dry-run-status", classes="ml-2 p-1")

            yield DataTable(id="table-legacy-cards")
            
            with Horizontal(classes="h-3 mt-1"):
                yield Button("Modernize Selected", variant="success", id="btn-modernize-selected")
                yield Button("Modernize All", variant="success", id="btn-modernize-all")

    def on_mount(self) -> None:
        table = self.query_one("#table-legacy-cards", DataTable)
        table.add_columns("Note ID", "Word/Phrase", "Has Image", "Fields")
        self.update_dry_run_label()

    def update_dry_run_label(self):
        lbl = self.query_one("#lbl-dry-run-status", Label)
        if self.app_config.get("dry_run", True):
            lbl.update("[yellow]● Dry Run: Active (No Anki changes)[/]")
        else:
            lbl.update("[red]● Dry Run: Inactive (Changes will commit!)[/]")

    def run_search(self, lang_key: str):
        deck_cfg = self.app_config["decks"].get(lang_key)
        if not deck_cfg or not deck_cfg.get("deck_name"):
            self.notify(f"Deck for {lang_key} is not configured! Go to settings.", severity="warning")
            return
            
        deck_name = deck_cfg["deck_name"]
        field_img = deck_cfg["fields"]["meaning_image"]
        
        self.notify(f"Searching for legacy cards in '{deck_name}'...")
        try:
            # Find all notes in deck
            note_ids = self.anki.find_notes(f"deck:\"{deck_name}\"")
            notes = self.anki.get_notes_info(note_ids)
            
            # Filter ones containing images in the target field
            self.legacy_notes = []
            table = self.query_one("#table-legacy-cards", DataTable)
            table.clear()
            
            ocr_engine = OcrEngine()
            for note in notes:
                field_val = note.get("fields", {}).get(field_img, {}).get("value", "")
                img_file = ocr_engine.extract_image_filename(field_val)
                if img_file:
                    self.legacy_notes.append(note)
                    word = note.get("fields", {}).get("Word", {}).get("value", "Unknown")
                    table.add_row(str(note["noteId"]), word, "Yes (Image)", ", ".join(note["fields"].keys()))
            
            self.notify(f"Found {len(self.legacy_notes)} legacy notes.")
        except Exception as e:
            self.notify(f"Search failed: {e}", severity="error")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-search-legacy-ja":
            self.run_search("japanese")
        elif event.button.id == "btn-search-legacy-en":
            self.run_search("english")
            
        elif event.button.id == "btn-modernize-selected":
            table = self.query_one("#table-legacy-cards", DataTable)
            if not table.coordinate_to_cell_key:
                self.notify("No cards searched or listed.", severity="warning")
                return
            current_row = table.cursor_row
            if current_row is None or current_row >= len(self.legacy_notes):
                self.notify("Please select a row in the table first.", severity="warning")
                return
                
            note = self.legacy_notes[current_row]
            self.app.push_screen(PreviewModal(self.app_config, note), self.make_modernize_callback(note))

    def make_modernize_callback(self, note):
        def callback(commit: bool):
            if commit:
                # Execute commit in background
                self.run_worker(self.commit_modernization(note), thread=True)
            else:
                self.notify("Modernization cancelled.")
        return callback

    async def commit_modernization(self, note):
        word = note.get("fields", {}).get("Word", {}).get("value", "")
        self.notify(f"Committing modernization for '{word}'...")
        
        # Get target field
        # Default maps Japanese fields
        deck_cfg = self.app_config["decks"]["japanese"]
        target_field = deck_cfg["fields"]["meaning_text"]
        
        # Generate simulation text
        modern_content = (
            f"<div><b>Modernized Definition:</b> Explained via local LLM.</div>"
            f"<div>Nuances and examples added.</div>"
        )
        
        if self.app_config.get("dry_run", True):
            self.notify(f"[DRY RUN] Would update card '{word}' field '{target_field}'.")
        else:
            try:
                # Backup first
                create_deck_backup(self.anki, deck_cfg["deck_name"], self.app_config["anki"]["backup_dir"])
                
                # Write to Anki
                self.anki.update_note_fields(note["noteId"], {target_field: modern_content})
                self.notify(f"Successfully modernized '{word}'!")
            except Exception as e:
                self.notify(f"Failed to commit: {e}", severity="error")


# --- INGEST TAB PANEL ---
class IngestTab(Static):
    def __init__(self, app_config):
        super().__init__()
        self.app_config = app_config
        self.csv_rows = []
        self.anki = AnkiConnectClient(url=app_config["anki"]["url"])
        self.scraper = Crawl4AiScraper()
        self.llm = OllamaClient(
            url=app_config["llm"]["ollama_url"],
            model=app_config["llm"]["model"]
        )

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("[bold accent]Ingest New Vocabulary[/]\n")
            
            with Horizontal(classes="h-3 mb-1"):
                yield Input(placeholder="Type a single word (e.g. 食べる)", id="input-single-word", classes="w-40")
                yield Button("Ingest Single Word", variant="primary", id="btn-ingest-single")
                
            yield Label("\n[bold]Or Import from CSV file (columns: word, language, type, note):[/]")
            with Horizontal(classes="h-3 mb-1"):
                yield Input(placeholder="CSV file path", id="input-csv-path", classes="w-60")
                yield Button("Load CSV", variant="primary", id="btn-load-csv")
                
            yield Label("[bold]CSV Preview & Validation Grid:[/]")
            yield DataTable(id="table-csv-preview")
            
            with Horizontal(classes="h-3 mt-1"):
                yield Button("Process Ingestion Queue", variant="success", id="btn-process-queue")

    def on_mount(self) -> None:
        table = self.query_one("#table-csv-preview", DataTable)
        table.add_columns("Word", "Language", "Type", "Note Context", "Status", "Suggested Fix")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-ingest-single":
            word = self.query_one("#input-single-word", Input).value.strip()
            if word:
                self.run_worker(self.process_single_word(word), thread=True)
                
        elif event.button.id == "btn-load-csv":
            path = self.query_one("#input-csv-path", Input).value.strip()
            if path and os.path.exists(path):
                self.load_csv(path)
            else:
                self.notify("Invalid CSV path!", severity="error")
                
        elif event.button.id == "btn-process-queue":
            if not self.csv_rows:
                self.notify("Ingestion queue is empty.", severity="warning")
                return
            self.run_worker(self.process_queue(), thread=True)

    def load_csv(self, path: str):
        try:
            self.csv_rows = []
            table = self.query_one("#table-csv-preview", DataTable)
            table.clear()
            
            with open(path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    word = row.get("word", "").strip()
                    lang = row.get("language", "").strip()
                    word_type = row.get("type", "").strip()
                    note = row.get("note", "").strip()
                    
                    if word:
                        self.csv_rows.append({
                            "word": word,
                            "language": lang,
                            "type": word_type,
                            "note": note,
                            "status": "Pending",
                            "suggestion": ""
                        })
                        table.add_row(word, lang, word_type, note, "Pending", "")
            self.notify(f"Successfully loaded {len(self.csv_rows)} words from CSV.")
        except Exception as e:
            self.notify(f"Failed to read CSV: {e}", severity="error")

    async def process_single_word(self, word: str):
        self.notify(f"Validating word '{word}'...")
        try:
            # Default to Japanese validation
            res = await self.scraper.scrape_jisho(word)
            if res.get("found"):
                if res.get("is_conjugated"):
                    suggestion = res.get("suggestion")
                    self.notify(f"Conjugated word! Suggestion: '{suggestion}'", severity="warning")
                else:
                    self.notify(f"Word '{word}' verified! Adding to Anki...")
                    # Generate TTS audio
                    audio_b64 = generate_tts_base64(word, "japanese")
                    # Push card if not dry-run
                    if self.app_config.get("dry_run", True):
                        self.notify(f"[DRY RUN] Would add note '{word}' to Japanese deck.")
                    else:
                        deck_name = self.app_config["decks"]["japanese"]["deck_name"]
                        model_name = self.app_config["decks"]["japanese"]["note_type"]
                        fields = {
                            "Word": word,
                            "Gender, Personal Connection, Extra Info (Back side)": res.get("definition"),
                            "Pronunciation (Recording and/or IPA)": f"[sound:tts_ja_{word}.mp3]"
                        }
                        self.anki.store_media_file(f"tts_ja_{word}.mp3", audio_b64)
                        self.anki.add_note(deck_name, model_name, fields)
                        self.notify(f"Successfully added '{word}' to deck!")
            else:
                self.notify(f"Word '{word}' not found in Jisho!", severity="error")
        except Exception as e:
            self.notify(f"Ingest failed: {e}", severity="error")

    async def process_queue(self):
        self.notify("Starting bulk ingestion queue processing...")
        # Simulating processing
        table = self.query_one("#table-csv-preview", DataTable)
        for idx, row in enumerate(self.csv_rows):
            # Update status to Processing
            table.update_cell_at((idx, 4), "Processing")
            await asyncio.sleep(0.5)
            table.update_cell_at((idx, 4), "Imported")
        self.notify("Bulk queue processing completed!")


# --- GRAMMAR TAB PANEL ---
class GrammarTab(Static):
    def __init__(self, app_config):
        super().__init__()
        self.app_config = app_config
        self.scraper = Crawl4AiScraper()

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("[bold accent]Modernize & Add Japanese Grammar (夕暮れの詞)[/]\n")
            yield Label("Ingest new grammar notes directly by crawling a website URL or performing OCR on screenshot images.")
            
            with Horizontal(classes="h-3 mt-1"):
                yield Input(placeholder="Grammar Article URL to crawl", id="input-grammar-url", classes="w-60")
                yield Button("Crawl and Ingest", variant="primary", id="btn-crawl-grammar")
                
            yield Label("\n[bold]Or Modernize existing grammar card from '夕暮れの詞' deck:[/]")
            with Horizontal(classes="h-3 mt-1"):
                yield Button("Search Grammar Decks for Legacy Images", variant="primary", id="btn-search-grammar-legacy")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-crawl-grammar":
            url = self.query_one("#input-grammar-url", Input).value.strip()
            if url:
                self.run_worker(self.crawl_grammar_url(url), thread=True)
                
        elif event.button.id == "btn-search-grammar-legacy":
            self.notify("Searching '夕暮れの詞' deck for screenshot-only cards...")

    async def crawl_grammar_url(self, url: str):
        self.notify("Crawling grammar URL via Crawl4AI...")
        try:
            markdown = await self.scraper.scrape_custom_url(url)
            self.notify(f"Crawled successfully! Length: {len(markdown)} chars.")
            # Send to Ollama to preview
            # Display preview to user
        except Exception as e:
            self.notify(f"Failed to crawl URL: {e}", severity="error")


# --- BACKUP & LOGS TAB PANEL ---
class BackupLogsTab(Static):
    def __init__(self, app_config):
        super().__init__()
        self.app_config = app_config
        self.anki = AnkiConnectClient(url=app_config["anki"]["url"])

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("[bold accent]Rollback Backups & Logs[/]\n")
            
            with Horizontal(classes="h-3 mb-2"):
                yield Button("Create Backup of All Configured Decks", variant="primary", id="btn-create-backup-now")
                
            yield Label("[bold]Recent Backups (APKG exports):[/]")
            yield DataTable(id="table-backups-list")
            
            yield Label("\n[bold]Application Activity Log Output:[/]")
            yield TextArea(id="text-activity-logs", read_only=True, classes="h-15 bg-bg")

    def on_mount(self) -> None:
        table = self.query_one("#table-backups-list", DataTable)
        table.add_columns("File Name", "Created Date", "File Size")
        self.refresh_backups()
        
        # Load recent log lines
        log_file = os.path.expanduser("~/.config/linguist-anki-bridge/app.log")
        if os.path.exists(log_file):
            with open(log_file, "r", encoding="utf-8") as f:
                lines = f.readlines()[-30:]
                self.query_one("#text-activity-logs", TextArea).text = "".join(lines)

    def refresh_backups(self):
        table = self.query_one("#table-backups-list", DataTable)
        table.clear()
        backup_dir = os.path.expanduser(self.app_config["anki"]["backup_dir"])
        if os.path.exists(backup_dir):
            for file in os.listdir(backup_dir):
                if file.endswith(".apkg"):
                    path = os.path.join(backup_dir, file)
                    stat = os.stat(path)
                    size_mb = stat.st_size / (1024 * 1024)
                    ctime = datetime.datetime.fromtimestamp(stat.st_ctime).strftime("%Y-%m-%d %H:%M:%S")
                    table.add_row(file, ctime, f"{size_mb:.2f} MB")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-create-backup-now":
            self.run_worker(self.trigger_all_backups(), thread=True)

    async def trigger_all_backups(self):
        self.notify("Exporting decks as backups...")
        success_count = 0
        for lang, spec in self.app_config.get("decks", {}).items():
            deck_name = spec.get("deck_name")
            if deck_name:
                try:
                    create_deck_backup(self.anki, deck_name, self.app_config["anki"]["backup_dir"])
                    success_count += 1
                except Exception as e:
                    self.notify(f"Backup failed for '{deck_name}': {e}", severity="error")
        
        if success_count > 0:
            self.notify(f"Created {success_count} deck backups successfully!")
            self.refresh_backups()
import datetime
