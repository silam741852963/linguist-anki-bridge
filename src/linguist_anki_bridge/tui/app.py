import asyncio
import logging
import os
import csv
import datetime
from pathlib import Path
from textual.app import App, ComposeResult
from textual.containers import Vertical, Horizontal
from textual.widgets import Label, Header, Footer, DataTable, ListItem, ListView, Static
from textual.reactive import reactive
from rich.table import Table
from rich.panel import Panel

from linguist_anki_bridge.config import ConfigManager, load_omarchy_theme
from linguist_anki_bridge.tui.setup import SetupScreen
from linguist_anki_bridge.tui.screens import (
    DetailsPane, InputDialog, SelectionListModal, process_legacy_card, process_ingest_item,
    commit_card_modernization, commit_card_ingestion, make_side_by_side_comparison, make_ingest_side_by_side
)
from linguist_anki_bridge.utils import check_health, create_deck_backup
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.scraper import Crawl4AiScraper

def generate_runtime_css(theme: dict):
    base_css_file = Path(__file__).parent / "styles.css"
    base_css = ""
    if base_css_file.exists():
        with open(base_css_file, "r", encoding="utf-8") as f:
            base_css = f.read()
            
    define_block = f"$bg: {theme.get('background', '#121212')};\n"
    define_block += f"$fg: {theme.get('foreground', '#bebebe')};\n"
    define_block += f"$accent: {theme.get('accent', '#e68e0d')};\n"
    define_block += f"$selection-bg: {theme.get('selection_background', '#333333')};\n"
    define_block += f"$active-border: {theme.get('active_border_color', '#595959')};\n"
    define_block += f"$active-tab: {theme.get('active_tab_background', '#121212')};\n\n"
    
    dest_dir = Path.home() / ".config" / "linguist-anki-bridge"
    dest_dir.mkdir(parents=True, exist_ok=True)
    custom_css = dest_dir / "custom_styles.css"
    with open(custom_css, "w", encoding="utf-8") as f:
        f.write(define_block + base_css)

class StatusPanel(Vertical):
    can_focus = True

class AnkiBridgeApp(App):
    CSS_PATH = str(Path.home() / ".config" / "linguist-anki-bridge" / "custom_styles.css")
    
    BINDINGS = [
        ("q", "quit", "Quit"),
        ("tab", "next_pane", "Next Pane"),
        ("shift+tab", "prev_pane", "Prev Pane"),
        ("1", "focus_status", "Status Pane"),
        ("2", "focus_decks", "Decks Pane"),
        ("3", "focus_queue", "Queue Pane"),
        ("s", "search_cards", "Search Legacy"),
        ("i", "ingest_word", "Ingest Word"),
        ("l", "load_csv", "Load CSV"),
        ("g", "crawl_grammar", "Crawl URL"),
        ("d", "toggle_dry_run", "Toggle Dry Run"),
        ("b", "backup_deck", "Backup Deck"),
        ("c", "commit_item", "Commit Selected"),
        ("a", "toggle_select_all", "Select/Deselect All"),
        ("u", "configure_setup", "Setup Mappings"),
        ("m", "select_model", "Select Model"),
    ]

    def __init__(self, theme=None, debug=False):
        super().__init__()
        self.debug_mode = debug
        self.custom_theme = theme
        self.config_manager = ConfigManager()
        
        # Clients
        self.anki = AnkiConnectClient(url=self.config_manager.config["anki"]["url"])
        self.ollama = OllamaClient(url=self.config_manager.config["llm"]["ollama_url"])
        self.ocr = OcrEngine()
        self.scraper = Crawl4AiScraper()
        
        # UI State
        self.active_deck_key = "japanese"
        self.queue_items = []
        self.active_row_idx = None
        self.processed_cache = {} # key: noteId (or row index for ingest) -> processed dict
        self.deck_queues = {}     # key: lang_key -> list of queue items
        self.preview_task = None
        self.is_bulk_processing = False
        
        # Health & Logs
        self.health_status = {
            "anki": False, "ollama": False, "jisho": False,
            "cambridge": False, "moedict": False, "dict_cc": False
        }
        self.models_list = []
        self.log_lines = []

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        
        with Horizontal():
            # Left Column (35% width)
            with Vertical(id="left-column"):
                # Status Panel
                with StatusPanel(id="panel-status", classes="pane"):
                    yield Label("[bold accent]● STATUS [1][/]", classes="pane-title")
                    yield Label("● AnkiConnect: Checking...", id="lbl-status-anki")
                    yield Label("● Ollama: Checking...", id="lbl-status-ollama")
                    yield Label("● Dictionaries: Checking...", id="lbl-status-dicts")
                    yield Label("Ollama Model: None", id="lbl-active-model")
                    yield Label("Dry Run: ON", id="lbl-dry-run-val")
                
                # Decks Panel
                with Vertical(id="panel-decks", classes="pane"):
                    yield Label("[bold accent]● DECKS & MODES [2][/]", classes="pane-title")
                    yield ListView(
                        ListItem(Label("Japanese"), id="item-japanese"),
                        ListItem(Label("English"), id="item-english"),
                        ListItem(Label("Taiwanese"), id="item-taiwanese"),
                        ListItem(Label("German"), id="item-german"),
                        ListItem(Label("Grammar (夕暮れの詞)"), id="item-grammar"),
                        id="list-decks"
                    )
                
                # Queue Panel
                with Vertical(id="panel-queue", classes="pane"):
                    yield Label("[bold accent]● QUEUE [3][/]", classes="pane-title")
                    yield DataTable(id="table-queue")
            
            # Right Column (65% width)
            with Vertical(id="right-column"):
                yield DetailsPane(id="panel-details", classes="pane")
                    
        yield Footer()

    async def on_mount(self) -> None:
        table = self.query_one("#table-queue", DataTable)
        table.cursor_type = "row"
        table.add_columns("Queue Empty")
        self.update_dry_run_label()
        self.update_deck_list_labels()
        
        # Log init
        self.log_action("Linguist Anki Bridge initialized.")
        
        if not self.config_manager.is_setup_completed():
            self.notify("First run detected. Please configure deck mappings.", severity="warning")
            self.log_action("First run setup mapping triggered.")
            self.push_screen(SetupScreen(self.config_manager, self.on_setup_completed))
            
        self.run_worker(self.health_loop())
        self.run_worker(self.load_ollama_models())
        self.update_details()

    def on_setup_completed(self) -> None:
        self.pop_screen()
        self.notify("Setup completed! Mappings loaded.", severity="information")
        self.log_action("Deck setup mappings loaded.")
        self.config_manager.load()
        self.update_deck_list_labels()
        self.update_details()

    def log_action(self, msg: str) -> None:
        t = datetime.datetime.now().strftime("%H:%M:%S")
        formatted = f"[{t}] {msg}"
        self.log_lines.append(formatted)
        if len(self.log_lines) > 20:
            self.log_lines.pop(0)
            
        # Write to persistent log file
        logging.info(f"[UI] {msg}")
            
        # Compile logs layout
        log_markup = "[bold accent]SYSTEM CONSOLE LOG[/]\n" + "-"*40 + "\n" + "\n".join(self.log_lines)
        try:
            self.query_one("#panel-details", DetailsPane).update_logs(log_markup)
        except Exception:
            pass

    async def health_loop(self):
        while True:
            loop = asyncio.get_event_loop()
            status = await loop.run_in_executor(
                None, check_health, 
                self.config_manager.config["anki"]["url"],
                self.config_manager.config["llm"]["ollama_url"]
            )
            self.health_status = status
            
            # Update labels
            self.update_status_label("#lbl-status-anki", "AnkiConnect", status["anki"])
            self.update_status_label("#lbl-status-ollama", "Ollama", status["ollama"])
            
            dicts_online = all([status["jisho"], status["cambridge"], status["moedict"], status["dict_cc"]])
            if dicts_online:
                self.query_one("#lbl-status-dicts", Label).update("[green]● Dictionaries: Online[/]")
            else:
                offline = [k for k, v in status.items() if k not in ("anki", "ollama") and not v]
                self.query_one("#lbl-status-dicts", Label).update(f"[yellow]● Dictionaries: Some Offline ({', '.join(offline)})[/]")
                
            if self.focused and self.focused.id == "panel-status":
                self.update_details()
                
            await asyncio.sleep(10)

    def update_status_label(self, label_id: str, name: str, online: bool):
        try:
            lbl = self.query_one(label_id, Label)
            if online:
                lbl.update(f"[green]● {name}: Online[/]")
            else:
                lbl.update(f"[red]● {name}: Offline[/]")
        except Exception:
            pass

    async def load_ollama_models(self):
        try:
            loop = asyncio.get_running_loop()
            models = await loop.run_in_executor(None, self.ollama.get_available_models)
            self.models_list = models
            
            config_model = self.config_manager.config["llm"]["model"]
            if config_model in models:
                self.ollama.set_model(config_model)
            elif models:
                self.config_manager.config["llm"]["model"] = models[0]
                self.config_manager.save()
                self.ollama.set_model(models[0])
            self.update_model_label()
        except Exception as e:
            logging.error(f"Failed to load Ollama models: {e}")

    def update_model_label(self):
        model = self.config_manager.config["llm"]["model"] or "None (Press 'm')"
        self.query_one("#lbl-active-model", Label).update(f"Ollama Model: [cyan]{model}[/]")

    def action_select_model(self) -> None:
        if not self.models_list:
            self.notify("No Ollama models available or Ollama offline.", severity="error")
            return
            
        self.push_screen(
            SelectionListModal("Select Active Ollama Model", self.models_list),
            self.on_model_selected
        )
        
    def on_model_selected(self, choice: str):
        if choice:
            self.config_manager.config["llm"]["model"] = choice
            self.config_manager.save()
            self.ollama.set_model(choice)
            self.update_model_label()
            self.notify(f"Active model set to '{choice}'")
            self.log_action(f"Ollama model changed to '{choice}'")
            self.update_details()

    def update_dry_run_label(self):
        val = "ON" if self.config_manager.config.get("dry_run", True) else "OFF"
        color = "yellow" if val == "ON" else "red"
        self.query_one("#lbl-dry-run-val", Label).update(f"Dry Run: [{color}]{val}[/]")

    def update_deck_list_labels(self) -> None:
        try:
            for lang in ["japanese", "english", "taiwanese", "german"]:
                deck_name = self.config_manager.config["decks"][lang].get("deck_name")
                val_str = f" ({deck_name})" if deck_name else " [Unmapped]"
                self.query_one(f"#item-{lang} Label", Label).update(f"{lang.capitalize()}{val_str}")
            
            grammar_name = self.config_manager.config["decks"].get("grammar", {}).get("deck_name") or "夕暮れの詞"
            self.query_one("#item-grammar Label", Label).update(f"Grammar ({grammar_name})")
        except Exception as e:
            logging.error(f"Failed to update deck list labels: {e}")

    # --- FOCUS CYCLE & NAVIGATION ---
    def action_focus_status(self) -> None:
        self.query_one("#panel-status").focus()
        
    def action_focus_decks(self) -> None:
        self.query_one("#list-decks").focus()
        # Auto-scan immediately when explicitly focusing!
        if self.health_status.get("anki") and self.config_manager.is_setup_completed():
            self.run_worker(self.run_card_search(self.active_deck_key))
        
    def action_focus_queue(self) -> None:
        self.query_one("#table-queue").focus()

    def action_next_pane(self) -> None:
        current = self.focused
        if current is None or current.id == "table-queue":
            self.query_one("#panel-status").focus()
        elif current.id == "panel-status":
            self.query_one("#list-decks").focus()
        elif current.id == "list-decks":
            self.query_one("#table-queue").focus()
        else:
            self.query_one("#panel-status").focus()

    def action_prev_pane(self) -> None:
        current = self.focused
        if current is None or current.id == "panel-status":
            self.query_one("#table-queue").focus()
        elif current.id == "table-queue":
            self.query_one("#list-decks").focus()
        elif current.id == "list-decks":
            self.query_one("#panel-status").focus()
        else:
            self.query_one("#table-queue").focus()

    def on_focus(self, event) -> None:
        self.update_details()
        # Trigger scan immediately when Decks List gains focus via keyboard cycling
        if event.node and event.node.id == "list-decks":
            if self.health_status.get("anki") and self.config_manager.is_setup_completed():
                self.run_worker(self.run_card_search(self.active_deck_key))

    # --- LIST / TABLE EVENTS ---
    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        if event.list_view.id == "list-decks":
            if event.item and event.item.id:
                lang = event.item.id.replace("item-", "")
                self.active_deck_key = lang
                self.update_details()
                
                # Auto scan when highlighted item changes
                if self.health_status.get("anki") and self.config_manager.is_setup_completed():
                    deck_cfg = self.config_manager.config["decks"].get(lang)
                    if deck_cfg and deck_cfg.get("deck_name"):
                        self.run_worker(self.run_card_search(lang))

    async def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "table-queue":
            row_idx = event.cursor_row
            if row_idx is not None and row_idx < len(self.queue_items):
                self.active_row_idx = row_idx
                self.update_details()
                
                if self.preview_task:
                    self.preview_task.cancel()
                self.preview_task = asyncio.create_task(self.load_active_item_preview(row_idx))

    # --- DATATABLE MULTI-SELECT SPACE BAR TOGGLE ---
    def update_row_selection_visuals(self, row_idx: int, is_selected: bool) -> None:
        table = self.query_one("#table-queue", DataTable)
        item = self.queue_items[row_idx]
        
        if item["type"] == "modernize":
            cache_key = item.get("note_id")
            class_str = "Checking..."
            if cache_key in self.processed_cache:
                class_str = "Dict" if self.processed_cache[cache_key]["classification"] == "dictionary" else "Recall"
            raw_values = [
                str(item["note_id"]),
                item["word"],
                item["filename"],
                class_str,
                item["status"]
            ]
        else:
            raw_values = [
                item["word"],
                item["language"],
                item.get("type_tag", ""),
                item.get("note", ""),
                item["status"]
            ]
            
        for col_idx, val in enumerate(raw_values):
            if is_selected:
                formatted_val = f"[green]{val}[/]"
            else:
                formatted_val = str(val)
            table.update_cell_at((row_idx, col_idx), formatted_val)

    def key_space(self) -> None:
        focused = self.focused
        if focused and focused.id == "table-queue":
            table = self.query_one("#table-queue", DataTable)
            row_idx = table.cursor_row
            if row_idx is not None and row_idx < len(self.queue_items):
                item = self.queue_items[row_idx]
                is_selected = not item.get("selected", False)
                item["selected"] = is_selected
                self.update_row_selection_visuals(row_idx, is_selected)
                self.log_action(f"Toggled selection for '{item['word']}': {is_selected}")

    def action_toggle_select_all(self) -> None:
        if not self.queue_items:
            return
            
        any_unselected = any(not item.get("selected", False) for item in self.queue_items)
        target_state = any_unselected
        
        for idx, item in enumerate(self.queue_items):
            item["selected"] = target_state
            self.update_row_selection_visuals(idx, target_state)
            
        self.log_action(f"Set all items selected: {target_state}")

    def rebuild_queue_table(self) -> None:
        table = self.query_one("#table-queue", DataTable)
        table.clear(columns=True)
        if not self.queue_items:
            table.add_columns("Queue Empty")
            return
            
        first_item = self.queue_items[0]
        if first_item["type"] == "modernize":
            table.add_columns("Note ID", "Word", "Image File", "Classification", "Status")
        else:
            table.add_columns("Word", "Language", "Type", "Context Note", "Status")
            
        for idx in range(len(self.queue_items)):
            item = self.queue_items[idx]
            if item["type"] == "modernize":
                cache_key = item.get("note_id")
                class_str = "Checking..."
                if cache_key in self.processed_cache:
                    class_str = "Dict" if self.processed_cache[cache_key]["classification"] == "dictionary" else "Recall"
                table.add_row(str(item["note_id"]), item["word"], item["filename"], class_str, item["status"])
            else:
                table.add_row(item["word"], item["language"], item.get("type_tag", ""), item.get("note", ""), item["status"])
            
            if item.get("selected", False):
                self.update_row_selection_visuals(idx, True)

    # --- ACTIONS & OPERATIONS ---
    def action_toggle_dry_run(self) -> None:
        current = self.config_manager.config.get("dry_run", True)
        new_val = not current
        self.config_manager.config["dry_run"] = new_val
        self.config_manager.save()
        self.update_dry_run_label()
        self.log_action(f"Dry Run toggled to {new_val}")
        self.update_details()

    def action_configure_setup(self) -> None:
        self.log_action("Setup mapped deck wizard loaded.")
        self.push_screen(SetupScreen(self.config_manager, self.on_setup_completed))

    def action_search_cards(self) -> None:
        deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key)
        if not deck_cfg or not deck_cfg.get("deck_name"):
            self.notify(f"Deck '{self.active_deck_key}' is not mapped/configured. Press 'u' to map.", severity="error")
            return
            
        self.log_action(f"Searching deck: {deck_cfg['deck_name']}")
        self.run_worker(self.run_card_search(self.active_deck_key, force_refresh=True))

    async def run_card_search(self, lang_key: str, force_refresh: bool = False):
        deck_cfg = self.config_manager.config["decks"][lang_key]
        deck_name = deck_cfg["deck_name"]
        field_img = deck_cfg["fields"]["meaning_image"]
        
        if not force_refresh and lang_key in self.deck_queues:
            self.queue_items = self.deck_queues[lang_key]
            self.log_action(f"Loaded {len(self.queue_items)} cards for '{deck_name}' from cache.")
            self.rebuild_queue_table()
            self.update_details()
            if self.queue_items:
                table = self.query_one("#table-queue", DataTable)
                table.focus()
                table.move_cursor(row=0)
            return

        self.log_action(f"Auto-scanning deck: '{deck_name}'...")
        try:
            loop = asyncio.get_running_loop()
            note_ids = await loop.run_in_executor(None, self.anki.find_notes, f"deck:\"{deck_name}\"")
            notes = await loop.run_in_executor(None, self.anki.get_notes_info, note_ids)
            
            self.queue_items = []
            self.processed_cache = {}
            
            for note in notes:
                field_val = note.get("fields", {}).get(field_img, {}).get("value", "")
                img_file = self.ocr.extract_image_filename(field_val)
                if img_file:
                    self.queue_items.append({
                        "type": "modernize",
                        "note": note,
                        "word": note.get("fields", {}).get("Word", {}).get("value", "Unknown"),
                        "status": "Scanned",
                        "note_id": note["noteId"],
                        "filename": img_file,
                        "selected": False  # Default to deselected!
                    })
                    
            self.deck_queues[lang_key] = self.queue_items
            self.rebuild_queue_table()
                
            self.log_action(f"Scan complete. Found {len(self.queue_items)} legacy screenshot cards.")
            self.update_details()
            if self.queue_items:
                table = self.query_one("#table-queue", DataTable)
                table.focus()
                table.move_cursor(row=0)
        except Exception as e:
            self.log_action(f"Scan failed: {e}")

    def action_ingest_word(self) -> None:
        self.push_screen(InputDialog("Ingest Single Word", "Type word (e.g. 食べる)"), self.on_word_input_dialog_complete)

    def on_word_input_dialog_complete(self, word: str):
        if not word:
            return
        
        table = self.query_one("#table-queue", DataTable)
        if not self.queue_items or self.queue_items[0].get("type") != "ingest":
            self.queue_items = []
            self.processed_cache = {}
            
        new_item = {
            "type": "ingest",
            "word": word,
            "language": self.active_deck_key,
            "type_tag": "",
            "note": "",
            "status": "Pending",
            "selected": False  # Default to deselected
        }
        self.queue_items.append(new_item)
        self.rebuild_queue_table()
        self.query_one("#table-queue").focus()
        
        table.move_cursor(row=len(self.queue_items) - 1)
        self.log_action(f"Added ingest word '{word}' to queue.")

    def action_load_csv(self) -> None:
        self.push_screen(InputDialog("Load Ingestion CSV File", "Path to CSV file"), self.on_csv_load_dialog_complete)

    def on_csv_load_dialog_complete(self, csv_path: str):
        if not csv_path or not os.path.exists(csv_path):
            self.notify("Invalid CSV path!", severity="error")
            return
            
        try:
            self.queue_items = []
            self.processed_cache = {}
            
            with open(csv_path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    word = row.get("word", "").strip()
                    lang = row.get("language", self.active_deck_key).strip().lower()
                    word_type = row.get("type", "").strip()
                    note = row.get("note", "").strip()
                    
                    if word:
                        self.queue_items.append({
                            "type": "ingest",
                            "word": word,
                            "language": lang,
                            "type_tag": word_type,
                            "note": note,
                            "status": "Pending",
                            "selected": False  # Default to deselected
                        })
            
            self.rebuild_queue_table()
            self.log_action(f"Loaded {len(self.queue_items)} items from CSV file: {Path(csv_path).name}")
            self.query_one("#table-queue").focus()
            self.update_details()
        except Exception as e:
            self.log_action(f"Failed to load CSV: {e}")

    def action_crawl_grammar(self) -> None:
        self.push_screen(InputDialog("Crawl Grammar URL", "URL to grammar page"), self.on_crawl_grammar_complete)

    def on_crawl_grammar_complete(self, url: str):
        if not url:
            return
        self.log_action(f"Crawling grammar URL: {url}")
        self.run_worker(self.run_grammar_crawl(url))

    async def run_grammar_crawl(self, url: str):
        try:
            markdown = await self.scraper.scrape_custom_url(url)
            self.log_action("Scraped grammar page markdown content successfully.")
            
            if not self.queue_items or self.queue_items[0].get("type") != "ingest":
                self.queue_items = []
                self.processed_cache = {}
                
            grammar_name = "Grammar Point"
            if self.ollama.model:
                try:
                    res = self.ollama.generate_grammar_content(markdown[:3000], "Identify grammar point name. Return strictly as JSON: {\"grammar_point\": \"name\"}")
                    grammar_name = res.get("grammar_point", "Grammar Point")
                except Exception:
                    pass
                    
            new_item = {
                "type": "ingest",
                "word": grammar_name,
                "language": "grammar",
                "type_tag": "Grammar",
                "note": f"Crawled from {url}",
                "status": "Pending",
                "raw_text": markdown,
                "selected": False  # Default to deselected
            }
            self.queue_items.append(new_item)
            self.rebuild_queue_table()
            
            table = self.query_one("#table-queue", DataTable)
            table.move_cursor(row=len(self.queue_items) - 1)
            self.query_one("#table-queue").focus()
            self.update_details()
        except Exception as e:
            self.log_action(f"Crawl/Extraction failed: {e}")

    def action_backup_deck(self) -> None:
        deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key)
        if not deck_cfg or not deck_cfg.get("deck_name"):
            self.notify("Deck is not configured for backup.", severity="error")
            return
        self.log_action(f"Backing up deck '{deck_cfg['deck_name']}'...")
        self.run_worker(self.run_backup(deck_cfg["deck_name"]))

    async def run_backup(self, deck_name: str):
        try:
            loop = asyncio.get_running_loop()
            backup_file = await loop.run_in_executor(
                None, create_deck_backup, self.anki, deck_name, self.config_manager.config["anki"]["backup_dir"]
            )
            self.log_action(f"Backup saved: {Path(backup_file).name}")
        except Exception as e:
            self.log_action(f"Backup failed: {e}")

    # --- ITEM DETAIL PREVIEW & BATCH COMMIT ---
    async def load_active_item_preview(self, row_idx: int):
        if row_idx >= len(self.queue_items):
            return
            
        item = self.queue_items[row_idx]
        details = self.query_one("#panel-details", DetailsPane)
        
        cache_key = item.get("note_id") if item["type"] == "modernize" else row_idx
        
        # Instantly show the BEFORE panel and intermediate LOADING AFTER panel
        self.update_details()
        
        if cache_key in self.processed_cache:
            return
            
        try:
            if item["type"] == "modernize":
                is_grammar = (self.active_deck_key == "grammar")
                res = await process_legacy_card(
                    self.anki, self.ocr, self.scraper,
                    self.config_manager.config, item["note"], is_grammar,
                    log_cb=self.log_action
                )
                self.processed_cache[cache_key] = res
                
                table = self.query_one("#table-queue", DataTable)
                class_str = "Dict" if res["classification"] == "dictionary" else "Recall"
                # Update cell visuals at col index 3 (Classification)
                cell_val = f"[green]{class_str}[/]" if item.get("selected") else class_str
                table.update_cell_at((row_idx, 3), cell_val)
            else:
                if "raw_text" in item and self.ollama.model:
                    prompt = self.config_manager.config["llm"]["system_prompt_grammar"]
                    loop = asyncio.get_event_loop()
                    llm_res = await loop.run_in_executor(
                        None, self.ollama.generate_grammar_content, item["raw_text"], prompt
                    )
                    res = {
                        "word": item["word"],
                        "scraped": {"found": True, "definition": "Grammar explanation"},
                        "suggestion": "",
                        "is_conjugated": False,
                        "llm_response": llm_res,
                        "audio_b64": None
                    }
                else:
                    res = await process_ingest_item(
                        self.scraper, self.ollama, item,
                        item["language"], self.config_manager.config
                    )
                self.processed_cache[cache_key] = res
                
            logging.info(f"Loaded preview modernization result for '{item['word']}': {res}")
            self.update_details()
        except asyncio.CancelledError:
            raise
        except Exception as e:
            logging.error(f"Failed to process preview: {e}")
            details.update_comparison(f"[bold red]Failed to process preview:[/] {e}")

    def action_commit_item(self) -> None:
        selected_indices = [idx for idx, item in enumerate(self.queue_items) if item.get("selected", False)]
        
        if not selected_indices:
            if self.active_row_idx is not None and self.active_row_idx < len(self.queue_items):
                selected_indices = [self.active_row_idx]
            else:
                self.notify("No items selected or highlighted to commit.", severity="warning")
                return
                
        self.log_action(f"Committing {len(selected_indices)} selected cards...")
        self.run_worker(self.commit_selected_batch(selected_indices))

    async def commit_selected_batch(self, indices: list):
        table = self.query_one("#table-queue", DataTable)
        dry_run = self.config_manager.config.get("dry_run", True)
        
        # Mark all selected cards as Processing first, update table row visuals
        valid_indices = []
        for idx in indices:
            item = self.queue_items[idx]
            if item["status"] not in ("Modernized", "Ingested", "DryRun", "Processing"):
                item["status"] = "Processing"
                self.update_row_selection_visuals(idx, item.get("selected", False))
                cell_val = f"[green]Processing[/]" if item.get("selected") else "Processing"
                table.update_cell_at((idx, 4), cell_val)
                valid_indices.append(idx)
                
        if not valid_indices:
            return
            
        # Refresh details panel so that the currently highlighted card (if processing) shows loading preview
        self.update_details()
        
        for idx in valid_indices:
            item = self.queue_items[idx]
            cache_key = item.get("note_id") if item["type"] == "modernize" else idx
            
            if cache_key not in self.processed_cache:
                self.log_action(f"Processing '{item['word']}' before committing...")
                try:
                    if item["type"] == "modernize":
                        is_grammar = (self.active_deck_key == "grammar")
                        res = await process_legacy_card(
                            self.anki, self.ocr, self.scraper,
                            self.config_manager.config, item["note"], is_grammar,
                            log_cb=self.log_action
                        )
                    else:
                        res = await process_ingest_item(
                            self.scraper, self.ollama, item,
                            item["language"], self.config_manager.config,
                            log_cb=self.log_action
                        )
                    self.processed_cache[cache_key] = res
                    
                    if item["type"] == "modernize":
                        class_str = "Dict" if res["classification"] == "dictionary" else "Recall"
                        cell_val = f"[green]{class_str}[/]" if item.get("selected") else class_str
                        table.update_cell_at((idx, 3), cell_val)
                except Exception as e:
                    self.log_action(f"Failed to process '{item['word']}': {e}")
                    item["status"] = "Error"
                    self.update_row_selection_visuals(idx, item.get("selected", False))
                    cell_val = "[green]Error[/]" if item.get("selected") else "Error"
                    table.update_cell_at((idx, 4), cell_val)
                    continue
                    
            processed = self.processed_cache[cache_key]
            
            # Query Ollama if we haven't generated suggestions yet for modernization
            if item["type"] == "modernize" and (not processed.get("llm_response")):
                self.log_action(f"Querying local Ollama model '{self.ollama.model}' for modernization suggestions...")
                try:
                    loop = asyncio.get_running_loop()
                    is_grammar = (self.active_deck_key == "grammar")
                    if is_grammar:
                        prompt = self.config_manager.config["llm"]["system_prompt_grammar"]
                        llm_res = await loop.run_in_executor(
                            None, self.ollama.generate_grammar_content, processed.get("ocr_text", ""), prompt
                        )
                    else:
                        prompt = self.config_manager.config["llm"]["system_prompt_vocab"]
                        lang_target = "Japanese"
                        llm_res = await loop.run_in_executor(
                            None, self.ollama.generate_card_content, item["word"], "", lang_target, prompt
                        )
                    processed["llm_response"] = llm_res
                    self.log_action("Ollama modernization suggestion received.")
                except Exception as e:
                    self.log_action(f"Ollama suggestion generation failed: {e}")
                    item["status"] = "Error"
                    self.update_row_selection_visuals(idx, item.get("selected", False))
                    cell_val = "[green]Error[/]" if item.get("selected") else "Error"
                    table.update_cell_at((idx, 4), cell_val)
                    continue

            logging.info(f"Committed card data for '{item['word']}': {processed}")
            
            try:
                loop = asyncio.get_running_loop()
                if item["type"] == "modernize":
                    if not dry_run:
                        await loop.run_in_executor(
                            None, commit_card_modernization, self.anki, item["note"], processed, self.active_deck_key, self.config_manager.config
                        )
                    item["status"] = "DryRun" if dry_run else "Modernized"
                else:
                    if not dry_run:
                        await loop.run_in_executor(
                            None, commit_card_ingestion, self.anki, processed, item["language"], self.config_manager.config
                        )
                    item["status"] = "DryRun" if dry_run else "Ingested"
                    
                item["selected"] = False
                self.update_row_selection_visuals(idx, False)
                
                # Update status cell at index 4 (Status)
                cell_val = f"[green]{item['status']}[/]" if item.get("selected") else item["status"]
                table.update_cell_at((idx, 4), cell_val)
                self.log_action(f"Successfully committed card: '{item['word']}'")
            except Exception as e:
                self.log_action(f"Failed to commit card '{item['word']}': {e}")
                item["status"] = "Error"
                self.update_row_selection_visuals(idx, item.get("selected", False))
                cell_val = "[green]Error[/]" if item.get("selected") else "Error"
                table.update_cell_at((idx, 4), cell_val)
                
        self.update_details()

    # --- DYNAMIC DETAILS RENDERER ---
    def update_details(self) -> None:
        details = self.query_one("#panel-details", DetailsPane)
        focused = self.focused
        
        # Always update logs sub-pane
        log_markup = "[bold accent]SYSTEM CONSOLE LOG[/]\n" + "-"*40 + "\n" + "\n".join(self.log_lines)
        details.update_logs(log_markup)
        
        if focused is None:
            details.update_comparison("[bold]Use Tab or keys 1/2/3 to focus panels and start using the app.[/]")
            return
            
        # 1. Status Panel Focused (Show overall configuration details)
        if focused.id == "panel-status":
            dry_run = "Active (No changes to Anki)" if self.config_manager.config.get("dry_run", True) else "Inactive (CHANGES WILL WRITE TO ANKI!)"
            active_model = self.config_manager.config["llm"]["model"] or "None"
            
            markup = (
                f"[bold accent]Linguist Anki Bridge - Configuration Details[/]\n"
                f"----------------------------------------------------\n"
                f"● [bold]AnkiConnect URL[/]: {self.config_manager.config['anki']['url']}\n"
                f"● [bold]Ollama API URL[/]: {self.config_manager.config['llm']['ollama_url']}\n"
                f"● [bold]Active Ollama Model[/]: {active_model}\n"
                f"● [bold]Global Dry Run Status[/]: {dry_run}\n"
                f"● [bold]Local Health Statuses[/]:\n"
                f"  - AnkiConnect: {'[green]ONLINE[/]' if self.health_status['anki'] else '[red]OFFLINE[/]'}\n"
                f"  - Ollama: {'[green]ONLINE[/]' if self.health_status['ollama'] else '[red]OFFLINE[/]'}\n"
                f"  - Dictionary APIs (Jisho, Cambridge, MoeDict, dict.cc): {'[green]ALL ONLINE[/]' if all([self.health_status['jisho'], self.health_status['cambridge'], self.health_status['moedict'], self.health_status['dict_cc']]) else '[yellow]SOME OFFLINE[/]'}\n\n"
                f"[bold accent]Application Keyboard Help & Shortcuts[/]\n"
                f"  - [bold]Tab / Shift+Tab[/]: Navigate between panes\n"
                f"  - [bold]1 / 2 / 3[/]: Direct focus to Status / Decks / Queue Panel\n"
                f"  - [bold]Space[/]: Toggle selection of highlighted queue item\n"
                f"  - [bold]a[/]: Toggle select/deselect all queue items\n"
                f"  - [bold]c[/]: Commit all selected cards to Anki (or highlighted card if none selected)\n"
                f"  - [bold]s[/]: Search/refresh legacy cards containing screenshots in active deck\n"
                f"  - [bold]i[/]: Ingest single word interactively\n"
                f"  - [bold]l[/]: Load a vocabulary CSV file\n"
                f"  - [bold]g[/]: Crawl a grammar URL to extract rules/examples\n"
                f"  - [bold]d[/]: Toggle Dry Run mode\n"
                f"  - [bold]b[/]: Export APKG backup of currently highlighted deck\n"
                f"  - [bold]u[/]: Configure first-run deck mappings\n"
                f"  - [bold]m[/]: Select active Ollama model\n"
                f"  - [bold]q[/]: Quit application"
            )
            details.update_comparison(markup)
            return

        # 2. Decks or Queue panel focused:
        # If queue has items, show card details comparison! (Keep details visible when moving around decks/queue)
        if self.queue_items:
            if self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
                details.update_comparison("[bold]Select an item in the queue to preview.[/]")
                return
                
            item = self.queue_items[self.active_row_idx]
            cache_key = item.get("note_id") if item["type"] == "modernize" else self.active_row_idx
            res = self.processed_cache.get(cache_key)
            
            if item["type"] == "modernize":
                is_grammar = (self.active_deck_key == "grammar")
                is_committed = item["status"] in ("Modernized", "DryRun", "Processing")
                comp_table = make_side_by_side_comparison(
                    item["word"], str(item["note_id"]),
                    item["note"].get("fields", {}), res, is_grammar,
                    committed=is_committed
                )
                details.update_comparison(comp_table)
            else:
                comp_table = make_ingest_side_by_side(item, res)
                details.update_comparison(comp_table)
                
        # If queue is empty, and Decks is focused, show Deck mappings details
        elif focused.id == "list-decks":
            deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key)
            if not deck_cfg:
                details.update_comparison("[red]Unknown deck mapping.[/]")
                return
                
            deck_name = deck_cfg.get("deck_name") or "[red]Not configured[/]"
            note_type = deck_cfg.get("note_type") or "Not configured"
            fields = deck_cfg.get("fields", {})
            ocr_langs = deck_cfg.get("ocr_langs", "None")
            
            markup = (
                f"[bold accent]Deck Mapping Configuration: {self.active_deck_key.upper()}[/]\n"
                f"-------------------------------------------------------\n"
                f"● [bold]Anki Target Deck[/]: {deck_name}\n"
                f"● [bold]Target Note Type[/]: {note_type}\n"
                f"● [bold]OCR Language Pack Config[/]: {ocr_langs}\n\n"
                f"[bold]Configured Field Mappings for Notes[/]:\n"
                f"  - Expression (Word) Field: [cyan]{fields.get('expression')}[/]\n"
                f"  - Screenshot (Picture) Field: [cyan]{fields.get('meaning_image')}[/]\n"
                f"  - Structured Meaning Field: [cyan]{fields.get('meaning_text')}[/]\n"
                f"  - Pronunciation (Audio) Field: [cyan]{fields.get('audio')}[/]\n\n"
                f"[bold yellow]Deck Quick Actions[/]:\n"
                f"  - Selection highlights automatically triggers search for legacy screenshot cards\n"
                f"  - Press [bold]b[/] to create a backup export (.apkg) in your backup directory"
            )
            details.update_comparison(markup)
            
        else:
            details.update_comparison(
                "[bold]Queue is currently empty.[/]\n\n"
                "Ready to process. Shortcuts:\n"
                "  - Highlighting a deck in Decks pane automatically scans deck for legacy screenshot cards\n"
                "  - Press [bold]i[/] to add a single word to queue\n"
                "  - Press [bold]l[/] to import a CSV list of words\n"
                "  - Press [bold]g[/] to crawl a grammar page URL"
            )
