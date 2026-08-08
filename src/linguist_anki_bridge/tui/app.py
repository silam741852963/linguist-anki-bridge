import asyncio
import logging
import os
import csv
import datetime
import time
from pathlib import Path
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Vertical, Horizontal
from textual.widgets import Label, Header, Footer, DataTable, ListItem, ListView, Static, RichLog
from textual.reactive import reactive
from rich.table import Table
from rich.panel import Panel
from textual import on
from textual.events import Focus
from textual.message import Message

from linguist_anki_bridge.config import ConfigManager, load_omarchy_theme
from linguist_anki_bridge.card_templates import japanese_vocab_template
from linguist_anki_bridge.card_model import sanitize_generated_text
from linguist_anki_bridge.markdown_text import html_to_markdown, render_markdown
from linguist_anki_bridge.tui.setup import SetupScreen, ConfigScreen
from linguist_anki_bridge.tui.screens import (
    PreviewPane, LogPane, InputDialog, SelectionListModal, process_legacy_card, process_inject_item,
    commit_card_modernization, commit_card_injection, make_parallel_comparison, make_inject_comparison_table,
    clean_html_for_tui, kanji_summary_for_tui, build_card_document, BottomRightPaletteModal, SpacebarMenuModal, FieldEditModal,
    LogViewerModal, UniversalSearchModal, SnapshotManagementScreen
)
from linguist_anki_bridge.snapshots import SnapshotManager

def get_log_days() -> list[tuple[str, Path]]:
    log_dir = Path.home() / ".config" / "linguist-anki-bridge"
    if not log_dir.exists():
        return []

    log_days = []

    # 1. Check current app.log
    app_log = log_dir / "app.log"
    if app_log.exists() and app_log.stat().st_size > 0:
        import datetime
        today_str = datetime.date.today().strftime("%Y-%m-%d")
        log_days.append((today_str, app_log))

    # 2. Check daily files app_YYYY-MM-DD.log
    import re
    for p in log_dir.glob("app_*.log"):
        m = re.match(r"app_(\d{4}-\d{2}-\d{2})\.log", p.name)
        if m and p.stat().st_size > 0:
            date_str = m.group(1)
            if not any(d[0] == date_str for d in log_days):
                log_days.append((date_str, p))

    log_days.sort(key=lambda x: x[0], reverse=True)
    return log_days[:5]


def dictionary_detail_markup(scraped: dict) -> str:
    """Render Jisho's structured entries without transport/link noise."""
    lines = ["[bold accent]DICTIONARY SCRAPE DETAILS[/]", "-------------------------"]
    entries = scraped.get("entries") or []
    if not entries:
        if not scraped.get("found"):
            return "\n".join(lines + ["(No dictionary scrape entry found)"])
        heading = " / ".join(filter(None, (scraped.get("reading"), scraped.get("word"))))
        return "\n".join(lines + [f"[bold]{heading}[/]", str(scraped.get("definition", ""))])
    target_word = scraped.get("input_word") or scraped.get("word")
    exact_index = 0
    for candidate_index, candidate in enumerate(entries):
        values = {candidate.get("word"), candidate.get("reading")}
        values.update(
            value for form in candidate.get("forms", []) if isinstance(form, dict)
            for value in (form.get("word"), form.get("reading"))
        )
        if target_word in values:
            exact_index = candidate_index
            break
    ordered_entries = [entries[exact_index], *(entry for index, entry in enumerate(entries) if index != exact_index)]
    for index, entry in enumerate(ordered_entries):
        if index:
            lines.extend(("", "─────────────────────────"))
        heading = " / ".join(filter(None, (entry.get("reading"), entry.get("word"))))
        lines.append(f"[bold]{heading}[/]")
        metadata = (["Common word"] if entry.get("is_common") else [])
        metadata += [*entry.get("jlpt", []), *entry.get("tags", [])]
        if metadata:
            lines.append(" · ".join(map(str, metadata)))
        for sense in entry.get("senses", []):
            labels = [str(value) for value in sense.get("parts_of_speech", []) + sense.get("tags", [])
                      if "wikipedia definition" not in str(value).lower()]
            if labels:
                lines.append(f"[bold]{' · '.join(labels)}[/]")
            definitions = "; ".join(map(str, sense.get("definitions", [])))
            lines.append(f"{sense.get('number', '')}. {definitions}")
            if sense.get("see_also"):
                lines.append(f"See also: {'; '.join(map(str, sense['see_also']))}")
    return "\n".join(lines)


def llm_detail_markup(result: dict) -> str:
    llm = result.get("llm_response") or {}
    lines = ["[bold accent]LLM GENERATED CONTENT[/]", "-------------------------"]
    if result.get("meaning_override_markdown") is not None:
        lines.append(f"[bold]Meaning Override (Markdown):[/] {result.get('meaning_override_markdown', '')}")
    elif result.get("meaning_override") is not None:
        lines.append(f"[bold]Meaning Override:[/] {result.get('meaning_override', '')}")
    if llm.get("nuances"):
        lines.append(f"[bold]Nuance:[/] {llm['nuances']}")
    examples = llm.get("examples") or []
    if examples:
        lines.append("[bold]Examples:[/]")
        for number, example in enumerate(examples, 1):
            if isinstance(example, dict):
                sentence = sanitize_generated_text(example.get("sentence"))
                translation = sanitize_generated_text(example.get("translation"))
                separator = " — " if sentence and translation else ""
                if sentence or translation:
                    lines.append(f"{number}. {sentence}{separator}{translation}")
    if len(lines) == 2:
        lines.append("(No LLM content generated)")
    return "\n".join(lines)

class StatusDataTable(DataTable):
    BINDINGS = list(DataTable.BINDINGS) + [
        Binding("tab", "next_pane", "Next Pane", priority=True),
        Binding("shift+tab", "prev_pane", "Previous Pane", priority=True),
        ("r", "refresh_status", "Refresh Status"),
    ]
    def action_refresh_status(self) -> None:
        self.app.action_refresh_status()
    def action_next_pane(self) -> None:
        self.app.action_next_pane()
    def action_prev_pane(self) -> None:
        self.app.action_prev_pane()

class DecksListView(ListView):
    BINDINGS = list(ListView.BINDINGS) + [
        Binding("tab", "next_pane", "Next Pane", priority=True),
        Binding("shift+tab", "prev_pane", "Previous Pane", priority=True),
        ("s", "toggle_deck_select", "Toggle Selection"),
        ("f", "fetch_active_decks", "Fetch Cards"),
        ("c", "map_deck", "Map Deck"),
        ("b", "backup_decks", "Backup Selected"),
        ("a", "toggle_all_decks", "Select/Deselect All Decks")
    ]
    def action_toggle_deck_select(self) -> None:
        self.app.action_toggle_deck_select()
    def action_next_pane(self) -> None:
        self.app.action_next_pane()
    def action_prev_pane(self) -> None:
        self.app.action_prev_pane()
    def action_fetch_active_decks(self) -> None:
        self.app.action_fetch_active_decks()
    def action_map_deck(self) -> None:
        self.app.action_map_deck()
    def action_backup_decks(self) -> None:
        self.app.action_backup_decks()
    def action_toggle_all_decks(self) -> None:
        self.app.action_toggle_all_decks()

class CardsDataTable(DataTable):
    BINDINGS = list(DataTable.BINDINGS) + [
        Binding("tab", "next_pane", "Next Pane", priority=True),
        Binding("shift+tab", "prev_pane", "Previous Pane", priority=True),
        ("s", "toggle_card_select", "Toggle Card"),
        ("a", "toggle_all_cards", "Toggle All"),
        ("c", "commit_batch", "Commit Batch"),
        ("p", "preview_card", "Preview Card"),
        ("r", "return_deck_cards", "Return to Deck"),
    ]
    def action_toggle_card_select(self) -> None:
        self.app.action_toggle_card_select()
    def action_next_pane(self) -> None:
        self.app.action_next_pane()
    def action_prev_pane(self) -> None:
        self.app.action_prev_pane()
    def action_toggle_all_cards(self) -> None:
        self.app.action_toggle_all_cards()
    def action_commit_batch(self) -> None:
        self.app.action_commit_batch()
    def action_preview_card(self) -> None:
        self.app.action_preview_card()
    def action_return_deck_cards(self) -> None:
        self.app.action_return_deck_cards()
from linguist_anki_bridge.utils import check_health, create_deck_backup
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.scraper import Crawl4AiScraper

class TuiLogMessage(Message):
    def __init__(self, text: str) -> None:
        super().__init__()
        self.text = text


class TuiLogHandler(logging.Handler):
    def __init__(self, app):
        super().__init__()
        self.app = app

    def emit(self, record):
        try:
            msg = self.format(record)
            # post_message is safe from both Textual's UI thread and executor
            # threads; call_from_thread raises when invoked on the UI thread.
            self.app.post_message(TuiLogMessage(msg))
        except Exception:
            self.handleError(record)

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
        ("3", "focus_queue", "Cards Pane"),
        ("4", "focus_preview", "Preview Pane"),
        ("5", "focus_log", "Log Pane"),
        ("space", "open_space_menu", "Palette Menu"),
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
        self.active_deck_key = "japanese_vocab"
        self.selected_decks = set()  # Set of checked deck keys
        self.queue_items = []
        self.active_row_idx = None
        self.processed_cache = {} # key: noteId (or stable injection id) -> processed source data
        self.deck_queues = {}     # key: lang_key -> list of queue items
        self.preview_task = None
        self.is_bulk_processing = False
        self.running_scans = set()
        self.raw_logs = []

        # Health & Logs
        self.health_status = {
            "anki": False, "ollama": False, "jisho": False,
            "cambridge": False, "moedict": False, "dict_cc": False
        }
        self.initial_scan_done = False
        self.models_list = []
        self.log_lines = []
        self._inject_sequence = 0
        self._manual_queue_view = False
        self.snapshot_manager = SnapshotManager()

    def cache_key_for(self, item: dict, row_idx: int):
        deck_key = item.get("deck_key") or item.get("language") or self.active_deck_key
        item_id = item.get("note_id") if item.get("type") == "modernize" else item.get("inject_id", item.get("ingest_id", row_idx))
        return deck_key, item_id

    def normalize_deck_key(self, value: str, default: str | None = None) -> str | None:
        key = (value or default or "").strip().lower().replace(" ", "_")
        aliases = {lang: f"{lang}_vocab" for lang in ("japanese", "english", "taiwanese", "german")}
        key = aliases.get(key, key)
        return key if key in self.config_manager.config.get("decks", {}) else None

    def note_image_filename(self, note: dict, preferred_field: str = "") -> str:
        fields = note.get("fields", {})
        names = [preferred_field, "Picture", "Image", "Meaning Image", *fields.keys()]
        for name in dict.fromkeys(filter(None, names)):
            filename = self.ocr.extract_image_filename(fields.get(name, {}).get("value", ""))
            if filename:
                return filename
        return ""

    @staticmethod
    def note_expression_value(note: dict, preferred_field: str = "") -> str:
        fields = note.get("fields", {})
        names = [preferred_field, "Expression", "Word", "Front", "Vocabulary"]
        for name in dict.fromkeys(filter(None, names)):
            value = fields.get(name, {}).get("value", "")
            if value:
                return value
        return "Unknown"

    def compose(self) -> ComposeResult:
        yield Header(show_clock=False)

        with Horizontal():
            # Left Column (35% width)
            with Vertical(id="left-column"):
                # Status Panel
                with StatusPanel(id="panel-status", classes="pane"):
                    yield Label("[bold accent]● STATUS [1][/]", classes="pane-title")
                    yield StatusDataTable(id="table-status")

                # Decks Panel
                with Vertical(id="panel-decks", classes="pane"):
                    yield Label("[bold accent]● DECKS [2][/]", classes="pane-title")
                    yield DecksListView(
                        ListItem(Label("Japanese Vocabulary"), id="item-japanese_vocab"),
                        ListItem(Label("Japanese Grammar"), id="item-japanese_grammar"),
                        ListItem(Label("English Vocabulary"), id="item-english_vocab"),
                        ListItem(Label("English Grammar"), id="item-english_grammar"),
                        ListItem(Label("Taiwanese Vocabulary"), id="item-taiwanese_vocab"),
                        ListItem(Label("Taiwanese Grammar"), id="item-taiwanese_grammar"),
                        ListItem(Label("German Vocabulary"), id="item-german_vocab"),
                        ListItem(Label("German Grammar"), id="item-german_grammar"),
                        id="list-decks"
                    )

                # Queue Panel
                with Vertical(id="panel-queue", classes="pane"):
                    yield Label("[bold accent]● CARDS [3][/]", classes="pane-title")
                    yield CardsDataTable(id="table-queue")

            # Right Column (65% width)
            with Vertical(id="right-column"):
                yield PreviewPane(id="panel-preview", classes="pane")
                yield LogPane(id="panel-log", classes="pane")

        yield Footer()

    async def on_mount(self) -> None:
        # Register TUI log handler
        handler = TuiLogHandler(self)
        handler.setFormatter(logging.Formatter("%(asctime)s [%(levelname)s] %(message)s", datefmt="%H:%M:%S"))
        handler.setLevel(logging.DEBUG if self.debug_mode else logging.INFO)

        root_logger = logging.getLogger()
        # Remove StreamHandler to avoid terminal corruption
        for h in list(root_logger.handlers):
            if isinstance(h, logging.StreamHandler):
                root_logger.removeHandler(h)
        root_logger.addHandler(handler)

        table_status = self.query_one("#table-status", DataTable)
        table_status.cursor_type = "row"
        table_status.add_columns("Service / Setting", "Status / Value")

        table = self.query_one("#table-queue", DataTable)
        table.cursor_type = "row"
        table.add_columns("Queue Empty")
        self.update_status_table()
        self.update_deck_list_labels()

        # Log init
        self.log_action("Linguist Anki Bridge initialized.")

        if not self.config_manager.is_setup_completed():
            self.notify("First run detected. Please configure deck mappings.", severity="warning")
            self.log_action("First run setup mapping triggered.")
            self.push_screen(SetupScreen(self.config_manager, self.on_setup_completed))
        else:
            from linguist_anki_bridge.tui.screens import LoadingScreen
            self.push_screen(LoadingScreen(self))

        self.update_details()

    def on_setup_completed(self) -> None:
        self.pop_screen()
        self.notify("Setup completed! Mappings loaded.", severity="information")
        self.log_action("Deck setup mappings loaded.")
        self.config_manager.load()
        self.anki = AnkiConnectClient(url=self.config_manager.config["anki"]["url"])
        self.ollama = OllamaClient(
            url=self.config_manager.config["llm"]["ollama_url"],
            model=self.config_manager.config["llm"].get("model"),
        )
        self.deck_queues.clear()
        self.processed_cache.clear()
        self.update_deck_list_labels()
        self.update_details()
        from linguist_anki_bridge.tui.screens import LoadingScreen
        self.push_screen(LoadingScreen(self))

    def log_action(self, msg: str) -> None:
        logging.info(f"[UI] {msg}")

    def on_tui_log_message(self, message: TuiLogMessage) -> None:
        self.write_to_tui_log(message.text)

    async def health_loop(self):
        while True:
            loop = asyncio.get_event_loop()
            status = await loop.run_in_executor(
                None, check_health,
                self.config_manager.config["anki"]["url"],
                self.config_manager.config["llm"]["ollama_url"]
            )
            self.health_status = status
            self.update_status_table()

            # Start initial scan when Anki connects for the first time
            if status["anki"] and not self.initial_scan_done and self.config_manager.is_setup_completed():
                self.initial_scan_done = True
                self.run_worker(self.run_card_search(self.active_deck_key))

            if self.focused and self.focused.id == "table-status":
                self.update_details()

            await asyncio.sleep(10)

    def update_status_table(self):
        try:
            table = self.query_one("#table-status", DataTable)
            table.clear()

            # 1. AnkiConnect
            anki_online = self.health_status.get("anki", False)
            anki_val = "[green]Online[/]" if anki_online else "[red]Offline[/]"
            table.add_row("AnkiConnect", anki_val)

            # 2. Ollama
            ollama_online = self.health_status.get("ollama", False)
            ollama_val = "[green]Online[/]" if ollama_online else "[red]Offline[/]"
            table.add_row("Ollama", ollama_val)

            # 3. Dictionaries
            dicts_online = all([
                self.health_status.get("jisho", False),
                self.health_status.get("cambridge", False),
                self.health_status.get("moedict", False),
                self.health_status.get("dict_cc", False)
            ])
            if dicts_online:
                dicts_val = "[green]Online[/]"
            else:
                offline = [k for k, v in self.health_status.items() if k not in ("anki", "ollama") and not v]
                if offline:
                    dicts_val = f"[yellow]Some Offline ({', '.join(offline)})[/]"
                else:
                    dicts_val = "[red]Offline[/]"
            table.add_row("Dictionaries", dicts_val)

            # 4. Ollama Model
            model = self.config_manager.config.get("llm", {}).get("model", "None") or "None"
            table.add_row("Ollama Model", f"[cyan]{model}[/]")

            # 5. Dry Run
            dry_run = self.config_manager.config.get("dry_run", True)
            dry_val = "[yellow]ON (Dry Run)[/]" if dry_run else "[red]OFF (Write mode)[/]"
            table.add_row("Dry Run Mode", dry_val)
        except Exception as e:
            logging.error(f"Failed to update status table: {e}")

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
            self.update_status_table()
        except Exception as e:
            logging.error(f"Failed to load Ollama models: {e}")

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
            self.update_status_table()
            self.notify(f"Active model set to '{choice}'")
            self.log_action(f"Ollama model changed to '{choice}'")
            self.update_details()

    def update_deck_list_labels(self) -> None:
        try:
            deck_keys = [
                "japanese_vocab", "japanese_grammar",
                "english_vocab", "english_grammar",
                "taiwanese_vocab", "taiwanese_grammar",
                "german_vocab", "german_grammar"
            ]
            for key in deck_keys:
                deck_name = self.config_manager.config["decks"].get(key, {}).get("deck_name")
                val_str = f" ({deck_name})" if deck_name else " [Unmapped]"
                display_name = key.replace("_", " ").title()
                is_sel = key in self.selected_decks
                if is_sel:
                    label_text = f"[green]{display_name}{val_str}[/]"
                else:
                    label_text = f"{display_name}{val_str}"
                self.query_one(f"#item-{key} Label", Label).update(label_text)
        except Exception as e:
            logging.error(f"Failed to update deck list labels: {e}")

    # --- FOCUS CYCLE & NAVIGATION ---
    def action_focus_status(self) -> None:
        self.query_one("#table-status").focus()

    def action_focus_decks(self) -> None:
        self.query_one("#list-decks").focus()

    def action_focus_queue(self) -> None:
        self.query_one("#table-queue").focus()

    def action_focus_preview(self) -> None:
        self.query_one("#panel-preview").focus()

    def action_focus_log(self) -> None:
        self.query_one("#details-logs").focus()

    def action_next_pane(self) -> None:
        pane_ids = ["panel-status", "panel-decks", "panel-queue", "panel-preview", "panel-log"]
        current_id = self._focused_pane_id()
        next_idx = 0 if current_id not in pane_ids else (pane_ids.index(current_id) + 1) % len(pane_ids)
        self._focus_pane(pane_ids[next_idx])

    def action_prev_pane(self) -> None:
        pane_ids = ["panel-status", "panel-decks", "panel-queue", "panel-preview", "panel-log"]
        current_id = self._focused_pane_id()
        prev_idx = -1 if current_id not in pane_ids else (pane_ids.index(current_id) - 1) % len(pane_ids)
        self._focus_pane(pane_ids[prev_idx])

    def _focused_pane_id(self) -> str | None:
        node = self.focused
        pane_ids = {"panel-status", "panel-decks", "panel-queue", "panel-preview", "panel-log"}
        while node is not None:
            if getattr(node, "id", None) in pane_ids:
                return node.id
            node = getattr(node, "parent", None)
        return None

    def _focus_pane(self, pane_id: str) -> None:
        targets = {
            "panel-status": "#table-status",
            "panel-decks": "#list-decks",
            "panel-queue": "#table-queue",
            "panel-preview": "#panel-preview",
            "panel-log": "#details-logs",
        }
        self.query_one(targets[pane_id]).focus()

    def on_descendant_focus(self, event: Focus) -> None:
        self.update_details()

    # --- LIST / TABLE EVENTS ---
    # --- LIST / TABLE EVENTS ---
    def on_focus(self, event: Focus) -> None:
        if event.widget and event.widget.id == "list-decks":
            if self.health_status.get("anki") and self.config_manager.is_setup_completed():
                self.run_worker(self.run_card_search(self.active_deck_key))
        self.update_details()

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

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if event.list_view.id == "list-decks":
            if event.item and event.item.id:
                lang = event.item.id.replace("item-", "")
                self.active_deck_key = lang
                if self.health_status.get("anki") and self.config_manager.is_setup_completed():
                    self.run_worker(self.run_card_search(lang))

    async def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "table-status":
            self.update_details()
        elif event.data_table.id == "table-queue":
            row_idx = event.cursor_row
            if row_idx is not None and row_idx < len(self.queue_items):
                self.active_row_idx = row_idx
                self.update_details()

    # --- DATATABLE MULTI-SELECT SPACE BAR TOGGLE ---
    def update_row_selection_visuals(self, row_idx: int, is_selected: bool) -> None:
        table = self.query_one("#table-queue", DataTable)
        item = self.queue_items[row_idx]

        mode = "Modernize" if item["type"] == "modernize" else "Inject"
        if item["type"] == "modernize":
            # Strip HTML tags from displayed word
            from bs4 import BeautifulSoup
            clean_word = BeautifulSoup(item["word"], "html.parser").get_text().strip()
            raw_values = [clean_word, item.get("filename", "") or "—"]
        else:
            result = self.processed_cache.get(self.cache_key_for(item, row_idx), {})
            media = result.get("new_image_filename") or result.get("audio_filename") or "—"
            raw_values = [item["word"], media]
        if self._manual_queue_view:
            raw_values.append(mode)

        for col_idx, val in enumerate(raw_values):
            if is_selected:
                formatted_val = f"[green]{val}[/]"
            else:
                formatted_val = str(val)
            table.update_cell_at((row_idx, col_idx), formatted_val)

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

        columns = ["Word", "Media"]
        if self._manual_queue_view:
            columns.append("Mode")
        table.add_columns(*columns)

        for idx in range(len(self.queue_items)):
            item = self.queue_items[idx]
            if item["type"] == "modernize":
                from bs4 import BeautifulSoup
                clean_word = BeautifulSoup(item["word"], "html.parser").get_text().strip()
                values = [clean_word, item.get("filename", "") or "—"]
            else:
                result = self.processed_cache.get(self.cache_key_for(item, idx), {})
                media = result.get("new_image_filename") or result.get("audio_filename") or "—"
                values = [item["word"], media]
            if self._manual_queue_view:
                values.append("Modernize" if item["type"] == "modernize" else "Inject")
            table.add_row(*values)

            if item.get("selected", False):
                self.update_row_selection_visuals(idx, True)

    # --- ACTIONS & OPERATIONS ---
    def action_toggle_dry_run(self) -> None:
        current = self.config_manager.config.get("dry_run", True)
        new_val = not current
        self.config_manager.config["dry_run"] = new_val
        self.config_manager.save()
        self.update_status_table()
        self.log_action(f"Dry Run toggled to {new_val}")
        self.update_details()

    def action_configure_setup(self) -> None:
        self.log_action("Settings screen loaded.")
        self.push_screen(ConfigScreen(self.config_manager, self.on_settings_closed))

    def on_settings_closed(self) -> None:
        """Return from Settings without invoking the first-run setup workflow."""
        self.pop_screen()
        self.config_manager.load()
        self.anki = AnkiConnectClient(url=self.config_manager.config["anki"]["url"])
        self.ollama = OllamaClient(
            url=self.config_manager.config["llm"]["ollama_url"],
            model=self.config_manager.config["llm"].get("model"),
        )
        self.update_deck_list_labels()
        self.update_status_table()
        self.update_details()
        self.notify("Settings closed.", severity="information")

    def action_search_cards(self) -> None:
        deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key)
        if not deck_cfg or not deck_cfg.get("deck_name"):
            self.notify(f"Deck '{self.active_deck_key}' is not mapped/configured. Press 'u' to map.", severity="error")
            return

        self.log_action(f"Searching deck: {deck_cfg['deck_name']}")
        self._manual_queue_view = False
        self.run_worker(self.run_card_search(self.active_deck_key, force_refresh=True))

    async def run_card_search(self, lang_key: str, force_refresh: bool = False):
        self._manual_queue_view = False
        if lang_key in self.running_scans:
            return

        deck_cfg = self.config_manager.config["decks"][lang_key]
        deck_name = deck_cfg["deck_name"]
        field_img = deck_cfg["fields"]["meaning_image"]

        if not force_refresh and lang_key in self.deck_queues:
            self.queue_items = self.deck_queues[lang_key]
            self.log_action(f"Loaded {len(self.queue_items)} cards for '{deck_name}' from cache.")
            self.rebuild_queue_table()
            self.update_details()
            return

        self.running_scans.add(lang_key)
        self.log_action(f"Auto-scanning deck: '{deck_name}'...")

        # Display scanning message in PreviewPane
        try:
            preview = self.query_one("#panel-preview", PreviewPane)
            preview.update_comparison(
                f"[bold yellow]● Auto-scanning deck: '{deck_name}' for legacy cards...[/]\n\n"
                "Querying local AnkiConnect instance to retrieve note listings. This may take a moment."
            )
            preview.update_dict_scrape("")
            preview.update_kanji_scrape("")
        except Exception:
            pass

        try:
            loop = asyncio.get_running_loop()
            note_ids = await loop.run_in_executor(None, self.anki.find_notes, f"deck:\"{deck_name}\"")
            notes = await loop.run_in_executor(None, self.anki.get_notes_info, note_ids)

            found_items = []

            word_field = deck_cfg.get("fields", {}).get("expression", "Word")
            for note in notes:
                img_file = self.note_image_filename(note, field_img)
                if img_file:
                    found_items.append({
                        "type": "modernize",
                        "deck_key": lang_key,
                        "note": note,
                        "word": self.note_expression_value(note, word_field),
                        "status": "Scanned",
                        "note_id": note["noteId"],
                        "filename": img_file,
                        "selected": False  # Default to deselected!
                    })

            self.deck_queues[lang_key] = found_items
            if self.active_deck_key == lang_key:
                self.queue_items = found_items
                self.rebuild_queue_table()

            self.log_action(f"Scan complete. Found {len(found_items)} legacy screenshot cards.")
            self.update_details()
        except Exception as e:
            self.log_action(f"Scan failed: {e}")
        finally:
            self.running_scans.discard(lang_key)

    async def run_multi_card_search(self, lang_keys: list[str]):
        self._manual_queue_view = False
        self.queue_items = []
        self.processed_cache = {}

        # Display scanning message
        try:
            preview = self.query_one("#panel-preview", PreviewPane)
            preview.update_comparison("[bold yellow]● Scanning selected decks for legacy cards...[/]")
            preview.update_dict_scrape("")
            preview.update_kanji_scrape("")
        except Exception:
            pass

        loop = asyncio.get_running_loop()
        semaphore = asyncio.Semaphore(4)

        async def scan_deck(lk: str) -> tuple[str, list]:
            if lk in self.running_scans:
                return lk, []
            self.running_scans.add(lk)
            try:
                deck_cfg = self.config_manager.config["decks"][lk]
                deck_name = deck_cfg["deck_name"]
                if not deck_name:
                    return lk, []
                field_img = deck_cfg["fields"]["meaning_image"]
                word_field = deck_cfg.get("fields", {}).get("expression", "Word")
                async with semaphore:
                    note_ids = await loop.run_in_executor(None, self.anki.find_notes, f"deck:\"{deck_name}\"")
                    notes = await loop.run_in_executor(None, self.anki.get_notes_info, note_ids)

                lk_items = []
                for note in notes:
                    img_file = self.note_image_filename(note, field_img)
                    if img_file:
                        lk_items.append({
                            "type": "modernize",
                            "deck_key": lk,
                            "note": note,
                            "word": self.note_expression_value(note, word_field),
                            "status": "Scanned",
                            "note_id": note["noteId"],
                            "filename": img_file,
                            "selected": False
                        })
                return lk, lk_items
            except Exception as e:
                self.log_action(f"Failed to scan {lk}: {e}")
                return lk, []
            finally:
                self.running_scans.discard(lk)

        results = await asyncio.gather(*(scan_deck(lk) for lk in lang_keys))
        for lk, lk_items in results:
            self.deck_queues[lk] = lk_items
            self.queue_items.extend(lk_items)

        self.rebuild_queue_table()
        self.log_action(f"Scan complete. Found {len(self.queue_items)} cards across selected decks.")
        self.update_details()

    def action_open_file(self, filename: str) -> None:
        import subprocess
        import sys
        from linguist_anki_bridge.tui.screens import find_anki_media_path

        media_path = find_anki_media_path(filename)
        if not media_path:
            self.notify(f"Image file not found: {filename}", severity="error")
            self.log_action(f"Image file not found: {filename}")
            return

        try:
            if sys.platform == "darwin":
                subprocess.run(["open", media_path])
            elif sys.platform == "win32":
                os.startfile(media_path)
            else:
                subprocess.run(["xdg-open", media_path])
            self.log_action(f"Opened image: {filename}")
        except Exception as e:
            self.log_action(f"Failed to open image: {e}")
            self.notify(f"Failed to open image: {e}", severity="error")

    def action_open_image(self) -> None:
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected to open image.", severity="warning")
            return

        item = self.queue_items[self.active_row_idx]
        fields = item.get("note", {}).get("fields", {}) if item.get("note") else {}
        deck_cfg = self.config_manager.config["decks"].get(item.get("deck_key", self.active_deck_key))
        picture_field = deck_cfg["fields"].get("meaning_image", "Picture") if deck_cfg else "Picture"
        picture_html = fields.get(picture_field, {}).get("value", "") if fields else ""

        # Extract all image tags in HTML
        filenames = []
        if picture_html:
            from bs4 import BeautifulSoup
            soup = BeautifulSoup(picture_html, "html.parser")
            for img in soup.find_all("img"):
                src = img.get("src", "")
                if src and not src.startswith("http"):
                    filenames.append(src)

        # Extract original/fallback filename if not already in list
        fallback_fn = self.ocr.extract_image_filename(picture_html)
        if fallback_fn and fallback_fn not in filenames:
            filenames.append(fallback_fn)

        # Get from processed cache
        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)
        if res:
            if res.get("filename") and res.get("filename") not in filenames:
                filenames.append(res.get("filename"))
            if res.get("new_image_filename") and res.get("new_image_filename") not in filenames:
                filenames.append(res.get("new_image_filename"))

        if not filenames:
            self.notify("No image filename found for this card.", severity="warning")
            return

        for fn in filenames:
            self.action_open_file(fn)

    def action_open_audio(self) -> None:
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected to play audio.", severity="warning")
            return

        item = self.queue_items[self.active_row_idx]
        fields = item.get("note", {}).get("fields", {}) if item.get("note") else {}
        deck_cfg = self.config_manager.config["decks"].get(item.get("deck_key", self.active_deck_key))
        audio_field = deck_cfg["fields"].get("audio", "Audio") if deck_cfg else "Audio"
        audio_val = fields.get(audio_field, {}).get("value", "") if fields else ""

        filenames = []
        import re
        if audio_val:
            match = re.search(r"\[sound:([^\]]+)\]", audio_val)
            if match:
                filenames.append(match.group(1))

        # Get from processed cache
        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)
        if res:
            if res.get("audio_filename") and res.get("audio_filename") not in filenames:
                filenames.append(res.get("audio_filename"))

        if not filenames:
            self.notify("No audio filename found for this card.", severity="warning")
            return

        for fn in filenames:
            self.action_open_file(fn)

    def action_inject_word(self) -> None:
        deck_cfg = self.config_manager.config.get("decks", {}).get(self.active_deck_key, {})
        if not deck_cfg.get("deck_name"):
            self.notify(f"Deck '{self.active_deck_key}' is not mapped.", severity="error")
            return
        from linguist_anki_bridge.tui.setup import InputManagementScreen
        self.push_screen(
            InputManagementScreen(self.config_manager, self.active_deck_key),
            self.on_input_management_complete
        )

    # Keep existing Textual bindings and third-party callers working.
    action_ingest_word = action_inject_word

    def action_return_deck_cards(self) -> None:
        """Leave a manual result queue and return to the active scanned deck."""
        self._manual_queue_view = False
        cached = self.deck_queues.get(self.active_deck_key)
        if cached is None:
            self.run_worker(self.run_card_search(self.active_deck_key))
            return
        self.queue_items = list(cached)
        self.active_row_idx = 0 if self.queue_items else None
        self.rebuild_queue_table()
        self.query_one("#table-queue", DataTable).focus()
        self.log_action(f"Returned to scanned cards for {self.active_deck_key}.")
        self.update_details()

    def on_input_management_complete(self, result: dict) -> None:
        if not result:
            return

        action = result.get("action")
        if action in {"manual", "single"}:
            words = result.get("words") or [result.get("word", "")]
            self.log_action(f"Checking {len(words)} manual expressions against Anki...")
            self.run_worker(self.resolve_injection_candidates([{
                "word": word,
                "language": self.active_deck_key,
                "type_tag": result.get("word_type", ""),
                "note": result.get("note", ""),
            } for word in words if str(word).strip()], replace_queue=True))

        elif action == "csv":
            csv_path = result["path"]
            self.on_csv_load_complete(csv_path)

        elif action == "grammar":
            url = result["url"]
            grammar_key = f"{self.active_deck_key.split('_')[0]}_grammar"
            grammar_cfg = self.config_manager.config["decks"].get(grammar_key, {})
            if not grammar_cfg.get("deck_name"):
                self.notify(f"Grammar deck '{grammar_key}' is not mapped.", severity="error")
                return
            self.log_action(f"Crawling grammar URL: {url}")
            self.run_worker(self.run_grammar_crawl(url, grammar_key))

    def on_csv_load_complete(self, csv_path: str):
        if not csv_path or not os.path.exists(csv_path):
            self.notify("Invalid CSV path!", severity="error")
            return

        try:
            candidates = []
            with open(csv_path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    word = row.get("word", "").strip()
                    lang = self.normalize_deck_key(row.get("language"), self.active_deck_key)
                    word_type = row.get("type", "").strip()
                    note = row.get("note", "").strip()

                    if word and lang and self.config_manager.config.get("decks", {}).get(lang, {}).get("deck_name"):
                        candidates.append({
                            "word": word,
                            "language": lang,
                            "type_tag": word_type,
                            "note": note,
                        })
                    elif word:
                        self.log_action(f"Skipped CSV word '{word}': unknown or unmapped language.")

            self.log_action(f"Checking {len(candidates)} CSV expressions against Anki...")
            self.run_worker(self.resolve_injection_candidates(candidates, replace_queue=True))
        except Exception as e:
            self.log_action(f"Failed to load CSV: {e}")

    async def resolve_injection_candidates(self, candidates: list[dict], replace_queue: bool = False) -> None:
        """Resolve requested expressions to modernization or injection items."""
        loop = asyncio.get_running_loop()
        semaphore = asyncio.Semaphore(4)

        async def resolve(candidate: dict) -> dict:
            lang_key = candidate["language"]
            deck_cfg = self.config_manager.config["decks"][lang_key]
            expression_field = deck_cfg.get("fields", {}).get("expression", "Expression")
            async with semaphore:
                notes = await loop.run_in_executor(
                    None,
                    self.anki.find_exact_expression,
                    deck_cfg["deck_name"],
                    candidate["word"],
                    (expression_field, "Expression", "Word", "Front", "Vocabulary"),
                )
            if not notes:
                self._inject_sequence += 1
                return {
                    "type": "inject",
                    "deck_key": lang_key,
                    "inject_id": f"inject-{self._inject_sequence}",
                    **candidate,
                    "status": "Pending",
                    "selected": False,
                }

            note = dict(notes[0])
            note["_linguist_expression"] = candidate["word"]
            filename = self.note_image_filename(note, deck_cfg.get("fields", {}).get("meaning_image", ""))
            if len(notes) > 1:
                self.log_action(
                    f"Found {len(notes)} exact notes for '{candidate['word']}'; using note {note['noteId']}."
                )
            return {
                "type": "modernize",
                "deck_key": lang_key,
                "note": note,
                "word": candidate["word"],
                "status": "Existing",
                "note_id": note["noteId"],
                "filename": filename,
                "selected": False,
            }

        try:
            resolved = await asyncio.gather(*(resolve(candidate) for candidate in candidates))
        except Exception as exc:
            self.log_action(f"Anki expression lookup failed: {exc}")
            self.notify("Could not check Anki before injection.", severity="error")
            return

        if replace_queue:
            self.queue_items = []
            self.processed_cache = {}
        self._manual_queue_view = True
        self.queue_items.extend(resolved)
        existing_count = sum(item["type"] == "modernize" for item in resolved)
        self.rebuild_queue_table()
        if resolved:
            table = self.query_one("#table-queue", DataTable)
            table.focus()
            table.move_cursor(row=len(self.queue_items) - len(resolved))
            self.active_row_idx = len(self.queue_items) - len(resolved)
        self.log_action(
            f"Resolved {len(resolved)} expressions: {existing_count} existing → Modernize, "
            f"{len(resolved) - existing_count} new → Inject."
        )
        self.update_details()

    async def run_grammar_crawl(self, url: str, grammar_key: str):
        try:
            markdown = await self.scraper.scrape_custom_url(url)
            self.log_action("Scraped grammar page markdown content successfully.")

            if not self.queue_items or self.queue_items[0].get("type") != "inject":
                self.queue_items = []
                self.processed_cache = {}

            grammar_name = "Grammar Point"
            if self.ollama.model:
                try:
                    res = await asyncio.get_running_loop().run_in_executor(
                        None,
                        self.ollama.generate_grammar_content,
                        markdown[:3000],
                        self.config_manager.config["llm"].get("translation_language", "English"),
                        "Identify grammar point name. Return strictly as JSON: {\"grammar_point\": \"name\"}",
                    )
                    grammar_name = res.get("grammar_point", "Grammar Point")
                except Exception:
                    pass

            new_item = {
                "type": "inject",
                "deck_key": grammar_key,
                "inject_id": f"grammar-{len(self.queue_items)}",
                "word": grammar_name,
                "language": grammar_key,
                "type_tag": "Grammar",
                "note": f"Crawled from {url}",
                "status": "Pending",
                "raw_text": markdown,
                "selected": False
            }
            self.queue_items.append(new_item)
            self._manual_queue_view = True
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
        preview = self.query_one("#panel-preview", PreviewPane)

        cache_key = self.cache_key_for(item, row_idx)

        # Instantly show the BEFORE panel and intermediate LOADING AFTER panel
        self.update_details()

        if cache_key in self.processed_cache:
            return

        try:
            if item["type"] == "modernize":
                item_deck_key = item.get("deck_key", self.active_deck_key)
                is_grammar = item_deck_key.endswith("grammar")
                res = await process_legacy_card(
                    self.anki, self.ocr, self.scraper,
                    self.config_manager.config, item["note"], is_grammar,
                    log_cb=self.log_action, llm_client=self.ollama, deck_key=item.get("deck_key")
                )
                self.processed_cache[cache_key] = res
            else:
                res = await process_inject_item(
                    self.scraper, self.ollama, item,
                    item["language"], self.config_manager.config,
                    log_cb=self.log_action,
                )
                self.processed_cache[cache_key] = res
            self.update_row_selection_visuals(row_idx, item.get("selected", False))
            logging.info(
                "Loaded preview result for '%s' (dictionary_entries=%d, images=%d, audio=%s)",
                item["word"],
                len((res.get("scraped") or {}).get("entries") or []),
                len(res.get("renamed_images") or []),
                bool(res.get("audio_b64")),
            )
            self.update_details()
        except asyncio.CancelledError:
            raise
        except Exception as e:
            logging.error(f"Failed to process preview: {e}")
            preview.update_comparison(f"[bold red]Failed to process preview:[/] {e}")

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

    @staticmethod
    def _snapshot_media_names(item: dict, processed: dict) -> set[str]:
        import re

        names: set[str] = set()
        for field in (item.get("note") or {}).get("fields", {}).values():
            value = str(field.get("value", "") if isinstance(field, dict) else field)
            names.update(re.findall(r"<img[^>]+src=[\"']([^\"']+)", value, flags=re.IGNORECASE))
            names.update(re.findall(r"\[sound:([^\]]+)\]", value))
        for key in ("filename", "new_image_filename", "audio_filename"):
            if processed.get(key):
                names.add(str(processed[key]))
        for image in processed.get("renamed_images") or []:
            if isinstance(image, dict) and image.get("new_name"):
                names.add(str(image["new_name"]))
        return {
            name for name in names
            if name and not name.startswith(("http://", "https://", "data:"))
        }

    async def _create_card_snapshot(
        self, item: dict, processed: dict, dry_run: bool,
    ) -> tuple[str, dict[str, str | None]]:
        """Capture fields and every media target before a write can mutate Anki."""
        names = self._snapshot_media_names(item, processed)

        def capture() -> tuple[str, dict[str, str | None]]:
            filenames = sorted(names)
            media_before: dict[str, str | None]
            try:
                media_before = self.anki.retrieve_media_files(filenames)
            except (AttributeError, NotImplementedError):
                media_before = {}
                for filename in filenames:
                    try:
                        previous = self.anki.retrieve_media_file(filename)
                        media_before[filename] = str(previous) if previous else None
                    except Exception as exc:
                        logging.warning("Snapshot could not inspect media '%s': %s", filename, exc)
            snapshot_id = self.snapshot_manager.create(
                word=str(processed.get("word") or item.get("word") or ""),
                mode=item.get("type", "inject"),
                deck_key=item.get("deck_key") or item.get("language") or self.active_deck_key,
                note=item.get("note") if item.get("type") == "modernize" else None,
                processed=processed,
                dry_run=dry_run,
                media_before=media_before,
            )
            return snapshot_id, media_before

        return await asyncio.get_running_loop().run_in_executor(None, capture)

    async def commit_selected_batch(self, indices: list):
        table = self.query_one("#table-queue", DataTable)
        dry_run = self.config_manager.config.get("dry_run", True)

        # Mark all selected cards as Processing first, update table row visuals
        valid_indices = []
        for idx in indices:
            item = self.queue_items[idx]
            if item["status"] not in ("Modernized", "Injected", "DryRun", "Processing"):
                item["status"] = "Processing"
                self.update_row_selection_visuals(idx, item.get("selected", False))
                valid_indices.append(idx)

        if not valid_indices:
            return

        # Refresh details panel so that the currently highlighted card (if processing) shows loading preview
        self.update_details()

        for idx in valid_indices:
            item = self.queue_items[idx]
            cache_key = self.cache_key_for(item, idx)

            if cache_key not in self.processed_cache:
                self.log_action(f"Processing '{item['word']}' before committing...")
                try:
                    if item["type"] == "modernize":
                        item_deck_key = item.get("deck_key", self.active_deck_key)
                        is_grammar = item_deck_key.endswith("grammar")
                        res = await process_legacy_card(
                            self.anki, self.ocr, self.scraper,
                            self.config_manager.config, item["note"], is_grammar,
                            log_cb=self.log_action, llm_client=self.ollama, deck_key=item.get("deck_key")
                        )
                    else:
                        res = await process_inject_item(
                            self.scraper, self.ollama, item,
                            item["language"], self.config_manager.config,
                            log_cb=self.log_action
                        )
                    self.processed_cache[cache_key] = res

                except Exception as e:
                    self.log_action(f"Failed to process '{item['word']}': {e}")
                    item["status"] = "Error"
                    self.update_row_selection_visuals(idx, item.get("selected", False))
                    continue

            processed = self.processed_cache[cache_key]

            # Query Ollama if we haven't generated suggestions yet for modernization
            if item["type"] == "modernize" and (not processed.get("llm_response")):
                self.log_action(f"Querying local Ollama model '{self.ollama.model}' for modernization suggestions...")
                try:
                    loop = asyncio.get_running_loop()
                    item_deck_key = item.get("deck_key", self.active_deck_key)
                    is_grammar = item_deck_key.endswith("grammar")
                    if is_grammar:
                        prompt = self.config_manager.config["llm"]["system_prompt_grammar"]
                        llm_res = await loop.run_in_executor(
                            None, self.ollama.generate_grammar_content, processed.get("ocr_text", ""), self.config_manager.config["llm"].get("translation_language", "English"), prompt
                        )
                    else:
                        prompt = self.config_manager.config["llm"]["system_prompt_vocab"]
                        lang_target = item_deck_key.split("_")[0].capitalize()
                        llm_res = await loop.run_in_executor(
                            None, self.ollama.generate_card_content, item["word"], "", lang_target, self.config_manager.config["llm"].get("translation_language", "English"), prompt
                        )
                    processed["llm_response"] = llm_res
                    self.log_action("Ollama modernization suggestion received.")
                except Exception as e:
                    self.log_action(f"Ollama suggestion generation failed: {e}")
                    item["status"] = "Error"
                    self.update_row_selection_visuals(idx, item.get("selected", False))
                    continue

            logging.info(
                "Prepared '%s' for %s commit (images=%d, audio=%s)",
                item["word"], item["type"],
                len(processed.get("renamed_images") or []),
                bool(processed.get("audio_b64")),
            )

            snapshot_id = ""
            try:
                loop = asyncio.get_running_loop()
                commit_started = time.perf_counter()
                snapshot_id, media_before = await self._create_card_snapshot(item, processed, dry_run)
                logging.info(
                    "Captured card snapshot for '%s' in %.2fs",
                    item["word"], time.perf_counter() - commit_started,
                )
                result_note_id = None
                if item["type"] == "modernize":
                    if not dry_run:
                        result_note_id = await loop.run_in_executor(
                            None, commit_card_modernization, self.anki, item["note"], processed,
                            item.get("deck_key", self.active_deck_key), self.config_manager.config,
                            media_before,
                        )
                    item["status"] = "DryRun" if dry_run else "Modernized"
                else:
                    if not dry_run:
                        result_note_id = await loop.run_in_executor(
                            None, commit_card_injection, self.anki, processed, item["language"],
                            self.config_manager.config, media_before,
                        )
                    item["status"] = "DryRun" if dry_run else "Injected"
                await loop.run_in_executor(
                    None,
                    lambda: self.snapshot_manager.finalize(snapshot_id, result_note_id=result_note_id),
                )

                item["selected"] = False
                self.update_row_selection_visuals(idx, False)

                elapsed = time.perf_counter() - commit_started
                self.log_action(f"Successfully committed card: '{item['word']}' ({elapsed:.2f}s)")
            except Exception as e:
                if snapshot_id:
                    await asyncio.get_running_loop().run_in_executor(
                        None, lambda: self.snapshot_manager.finalize(snapshot_id, error=str(e))
                    )
                self.log_action(f"Failed to commit card '{item['word']}': {e}")
                item["status"] = "Error"
                self.update_row_selection_visuals(idx, item.get("selected", False))
                self.update_details()

    # --- DYNAMIC DETAILS RENDERER ---
    def update_details(self) -> None:
        preview = self.query_one("#panel-preview", PreviewPane)
        focused = self.focused

        if focused is None:
            # Losing terminal/application focus must not destroy the last preview.
            return

        # 1. Status Panel Focused (Show overall configuration details)
        if focused.id == "table-status":
            import shutil
            import subprocess
            active_model = self.config_manager.config["llm"]["model"] or "None"
            dry_run = "Active (No changes to Anki)" if self.config_manager.config.get("dry_run", True) else "Inactive (CHANGES WILL WRITE TO ANKI!)"
            anki_lat = f"{self.health_status.get('anki_latency', -1)} ms" if self.health_status.get('anki_latency', -1) >= 0 else "N/A"
            ollama_lat = f"{self.health_status.get('ollama_latency', -1)} ms" if self.health_status.get('ollama_latency', -1) >= 0 else "N/A"
            models_str = ", ".join(self.models_list) if self.models_list else "None found"
            row = self.query_one("#table-status", DataTable).cursor_row or 0
            if row == 0:
                markup = (
                    f"[bold accent]ANKICONNECT[/]\n-------------------------\n"
                    f"● [bold]API URL[/]: {self.config_manager.config['anki']['url']}\n"
                    f"● [bold]Status[/]: {'[green]Online[/]' if self.health_status['anki'] else '[red]Offline[/]'}\n"
                    f"● [bold]Latency[/]: {anki_lat}\n"
                )
            elif row in (1, 3):
                vram = "Not detected"
                if shutil.which("nvidia-smi"):
                    try:
                        raw = subprocess.check_output(
                            ["nvidia-smi", "--query-gpu=memory.total,memory.free", "--format=csv,noheader,nounits"],
                            text=True, timeout=1,
                        ).strip()
                        vram = "; ".join(f"{free.strip()} / {total.strip()} MiB (free / total)" for total, free in (line.split(",", 1) for line in raw.splitlines()))
                    except Exception:
                        pass
                markup = (
                    f"[bold accent]OLLAMA[/]\n-------------------------\n"
                    f"● [bold]API URL[/]: {self.config_manager.config['llm']['ollama_url']}\n"
                    f"● [bold]Status[/]: {'[green]Online[/]' if self.health_status['ollama'] else '[red]Offline[/]'}\n"
                    f"● [bold]Latency[/]: {ollama_lat}\n"
                    f"● [bold]Active Model[/]: {active_model}\n"
                    f"● [bold]Models List[/]: {models_str}\n"
                    f"● [bold]System VRAM[/]: {vram}\n"
                )
            elif row == 2:
                markup = "[bold accent]DICTIONARIES[/]\n-------------------------\n" + "\n".join(
                    f"● [bold]{label}[/]: {'[green]Online[/]' if self.health_status.get(key) else '[red]Offline[/]'}"
                    for label, key in (("Jisho", "jisho"), ("Cambridge", "cambridge"), ("MoeDict", "moedict"), ("Dict.cc", "dict_cc"))
                )
            else:
                markup = f"[bold accent]DRY RUN MODE[/]\n-------------------------\n● [bold]Status[/]: {dry_run}\n"
            preview.update_comparison(markup)
            preview.update_dict_scrape("")
            preview.update_llm("")
            preview.update_kanji_scrape("")
            return

        # 2. Decks or Queue panel focused:
        # If queue has items, show card details comparison! (Keep details visible when moving around decks/queue)
        if self.queue_items:
            if self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
                preview.update_comparison("[bold]Select an item in the queue to preview.[/]")
                preview.update_dict_scrape("")
                preview.update_llm("")
                preview.update_kanji_scrape("")
                return

            item = self.queue_items[self.active_row_idx]
            cache_key = self.cache_key_for(item, self.active_row_idx)
            res = self.processed_cache.get(cache_key)

            if item["type"] == "modernize":
                item_deck_key = item.get("deck_key", self.active_deck_key)
                is_grammar = item_deck_key.endswith("grammar")
                deck_cfg = self.config_manager.config["decks"].get(item_deck_key)
                if item_deck_key == "japanese_vocab" and deck_cfg:
                    spec = japanese_vocab_template()
                    deck_cfg = {**deck_cfg, "note_type": spec.model_name, "fields": spec.field_mapping()}
                comp_table = make_parallel_comparison(
                    item["note"].get("fields", {}), res, deck_cfg, is_grammar
                )
                preview.update_comparison(comp_table)
            else:
                deck_cfg = self.config_manager.config["decks"].get(item["language"])
                if item["language"] == "japanese_vocab" and deck_cfg:
                    spec = japanese_vocab_template()
                    deck_cfg = {**deck_cfg, "note_type": spec.model_name, "fields": spec.field_mapping()}
                comp_table = make_inject_comparison_table(res, deck_cfg)
                preview.update_comparison(comp_table)

            if res:
                scraped = res.get("scraped")
                classification = res.get("classification_result") or {}
                classification_markup = ""
                if classification:
                    state = res.get("classification", "uncertain")
                    probability = float(classification.get("probability", 0.5))
                    prompt = "\n[bold yellow]Press x to confirm this image class.[/]" if state == "uncertain" else ""
                    classification_markup = (
                        "[bold accent]IMAGE CLASSIFICATION[/]\n-------------------------\n"
                        f"● [bold]Result[/]: {state}\n"
                        f"● [bold]Dictionary probability[/]: {probability:.0%}\n"
                        f"● [bold]Evidence[/]: {classification.get('reason', '')}\n"
                        f"● [bold]Source[/]: {classification.get('source', '')}{prompt}\n\n"
                        "─────────────────────────\n\n"
                    )
                preview.update_dict_scrape(classification_markup + dictionary_detail_markup(scraped or {}))
                preview.update_llm(llm_detail_markup(res))
                kanji_html = res.get("kanji_construction", "")
                if res.get("kanji_override_markdown") is not None:
                    kanji_html = render_markdown(
                        res.get("kanji_override_markdown", ""), res.get("kanji_override_media")
                    )
                if kanji_html:
                    kanji_markup = "[bold accent]KANJI CONSTRUCTION SCRAPE[/]\n-------------------------\n" + kanji_summary_for_tui(kanji_html)
                else:
                    kanji_markup = "[bold accent]KANJI CONSTRUCTION SCRAPE[/]\n-------------------------\n(No Kanji details extracted)"
                preview.update_kanji_scrape(kanji_markup)
            else:
                preview.update_dict_scrape("[bold accent]DICTIONARY SCRAPE DETAILS[/]\n-------------------------\n(Awaiting dictionary lookup...)")
                preview.update_llm("[bold accent]LLM GENERATED CONTENT[/]\n-------------------------\n(Awaiting LLM generation...)")
                preview.update_kanji_scrape("[bold accent]KANJI CONSTRUCTION SCRAPE[/]\n-------------------------\n(Awaiting Kanji construction lookup...)")

        # If queue is empty, and Decks is focused, show Deck mappings details
        elif focused.id == "list-decks":
            deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key)
            if not deck_cfg:
                preview.update_comparison("[red]Unknown deck mapping.[/]")
                preview.update_dict_scrape("")
                preview.update_kanji_scrape("")
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
            preview.update_comparison(markup)
            preview.update_dict_scrape("")
            preview.update_kanji_scrape("")

        else:
            preview.update_comparison(
                "[bold]Queue is currently empty.[/]\n\n"
                "Select a mapped deck to scan its cards, or open [bold]Manual Input[/] from the Space menu to stage one or more expressions."
            )
            preview.update_dict_scrape("")
            preview.update_kanji_scrape("")

    def action_process_preview(self) -> None:
        if self.active_row_idx is not None and self.active_row_idx < len(self.queue_items):
            item = self.queue_items[self.active_row_idx]
            cache_key = self.cache_key_for(item, self.active_row_idx)
            if cache_key in self.processed_cache:
                del self.processed_cache[cache_key]
            if self.preview_task:
                self.preview_task.cancel()
            self.preview_task = asyncio.create_task(self.load_active_item_preview(self.active_row_idx))

    def write_to_tui_log(self, msg: str) -> None:
        self.raw_logs.append(msg)
        try:
            log_widget = self.query_one("#details-logs", RichLog)
            from rich.markup import escape
            if "[DEBUG]" in msg or "DEBUG" in msg:
                formatted = f"[dim white]{escape(msg)}[/]"
            elif "[ERROR]" in msg or "ERROR" in msg:
                formatted = f"[bold red]{escape(msg)}[/]"
            elif "[WARNING]" in msg or "WARNING" in msg:
                formatted = f"[bold yellow]{escape(msg)}[/]"
            else:
                formatted = f"[green]{escape(msg)}[/]"
            log_widget.write(formatted)
        except Exception:
            pass

    def action_copy_logs(self) -> None:
        self.action_copy_log_clipboard()

    # --- PANE SPECIFIC ACTIONS ---
    def action_toggle_deck_select(self) -> None:
        list_decks = self.query_one("#list-decks", ListView)
        if list_decks.highlighted_child and list_decks.highlighted_child.id:
            key = list_decks.highlighted_child.id.replace("item-", "")
            if key in self.selected_decks:
                self.selected_decks.remove(key)
            else:
                self.selected_decks.add(key)
            self.update_deck_list_labels()

    def action_fetch_active_decks(self) -> None:
        self.run_worker(self.run_card_search(self.active_deck_key, force_refresh=True))

    def action_map_deck(self) -> None:
        if not self.health_status.get("anki"):
            self.notify("Anki is offline!", severity="error")
            return
        self.run_worker(self.open_deck_mapping())

    async def open_deck_mapping(self) -> None:
        try:
            decks = await asyncio.get_running_loop().run_in_executor(None, self.anki.get_decks)
            self.push_screen(SelectionListModal("Select Anki Deck", decks), self.on_deck_selected_for_mapping)
        except Exception as e:
            self.notify(f"Failed to fetch decks: {e}", severity="error")

    def on_deck_selected_for_mapping(self, selected_deck: str) -> None:
        if selected_deck:
            self.config_manager.config["decks"][self.active_deck_key]["deck_name"] = selected_deck
            self.config_manager.save()
            self.update_deck_list_labels()
            self.notify(f"Mapped {self.active_deck_key} to {selected_deck}")
            self.run_worker(self.run_card_search(self.active_deck_key, force_refresh=True))

    def action_backup_decks(self) -> None:
        if not self.health_status.get("anki"):
            self.notify("Anki is offline!", severity="error")
            return

        keys = list(self.selected_decks)
        if not keys:
            keys = [self.active_deck_key]

        decks_to_backup = []
        for key in keys:
            deck_name = self.config_manager.config["decks"].get(key, {}).get("deck_name")
            if deck_name:
                decks_to_backup.append(deck_name)

        if not decks_to_backup:
            self.notify("No mapped decks to back up.", severity="warning")
            return

        backup_dir = self.config_manager.config["anki"].get("backup_dir", "./backups")

        async def do_backups():
            self.notify(f"Backing up {len(decks_to_backup)} deck(s)...")
            loop = asyncio.get_running_loop()
            success_count = 0
            for d in decks_to_backup:
                try:
                    await loop.run_in_executor(None, create_deck_backup, self.anki, d, backup_dir)
                    self.log_action(f"Backup created for deck '{d}'")
                    success_count += 1
                except Exception as e:
                    self.notify(f"Failed to back up {d}: {e}", severity="error")
            self.notify(f"Successfully backed up {success_count} deck(s)!")

        self.run_worker(do_backups())

    def action_toggle_all_decks(self) -> None:
        deck_keys = [
            "japanese_vocab", "japanese_grammar",
            "english_vocab", "english_grammar",
            "taiwanese_vocab", "taiwanese_grammar",
            "german_vocab", "german_grammar"
        ]
        if all(k in self.selected_decks for k in deck_keys):
            self.selected_decks.clear()
            self.notify("Deselected all decks.")
        else:
            self.selected_decks.update(deck_keys)
            self.notify("Selected all decks.")
        self.update_deck_list_labels()

    def action_toggle_card_select(self) -> None:
        if self.active_row_idx is not None and self.active_row_idx < len(self.queue_items):
            item = self.queue_items[self.active_row_idx]
            item["selected"] = not item.get("selected", False)
            self.update_row_selection_visuals(self.active_row_idx, item["selected"])

    def action_toggle_all_cards(self) -> None:
        self.action_toggle_select_all()

    def action_commit_batch(self) -> None:
        self.action_commit_item()

    def action_preview_card(self) -> None:
        self.action_process_preview()

    def action_select_audio(self) -> None:
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected.", severity="warning")
            return

        item = self.queue_items[self.active_row_idx]
        choices = []

        fields = item.get("note", {}).get("fields", {}) if item.get("note") else {}
        deck_cfg = self.config_manager.config["decks"].get(item.get("deck_key", self.active_deck_key))
        audio_field = deck_cfg["fields"].get("audio", "Audio") if deck_cfg else "Audio"
        audio_val = fields.get(audio_field, {}).get("value", "") if fields else ""

        import re
        if audio_val:
            match = re.search(r"\[sound:([^\]]+)\]", audio_val)
            if match:
                choices.append(("Play Current Anki Audio", f"play:{match.group(1)}"))

        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)
        if res and res.get("audio_filename"):
            choices.append(("Play Generated/TTS Audio", f"play:{res['audio_filename']}"))

        if not choices:
            self.notify("No audio options found for this card.", severity="warning")
            return

        self.push_screen(BottomRightPaletteModal("Select Audio Track", choices), self.on_audio_selected)

    def on_audio_selected(self, result: str) -> None:
        if result and result.startswith("play:"):
            fn = result.split("play:")[-1]
            self.action_open_file(fn)

    def action_select_image(self) -> None:
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected.", severity="warning")
            return

        choices = [
            ("Old Images (Original)", "old"),
            ("New Images (Proposed/Renamed)", "new")
        ]
        self.push_screen(BottomRightPaletteModal("Select Image Set", choices), self.on_image_set_selected)

    def action_confirm_image_classification(self) -> None:
        if not self.queue_items or self.active_row_idx is None:
            self.notify("No card selected.", severity="warning")
            return
        item = self.queue_items[self.active_row_idx]
        result = self.processed_cache.get(self.cache_key_for(item, self.active_row_idx))
        if not result or not result.get("classification_result"):
            self.notify("Process the card preview before classifying its image.", severity="warning")
            return
        self.push_screen(
            BottomRightPaletteModal(
                "Confirm Image Class",
                [("Dictionary screenshot", "dictionary"), ("Photograph / visual recall", "visual_recall")],
            ),
            self.on_image_classification_confirmed,
        )

    def on_image_classification_confirmed(self, classification: str) -> None:
        if classification not in ("dictionary", "visual_recall") or self.active_row_idx is None:
            return
        item = self.queue_items[self.active_row_idx]
        result = self.processed_cache.get(self.cache_key_for(item, self.active_row_idx))
        if not result:
            return
        image = next((entry.get("b64") for entry in result.get("renamed_images", []) if entry.get("b64")), None)
        if image:
            try:
                self.ocr.record_classification_feedback(image, result.get("classification_result", {}), classification)
            except Exception as exc:
                self.log_action(f"Could not store image classification feedback: {exc}")
        result["classification"] = classification
        result["classification_result"] = {
            **result.get("classification_result", {}),
            "source": "user-confirmed",
            "reason": f"Explicitly confirmed as {classification}",
            "probability": 1.0 if classification == "dictionary" else 0.0,
        }
        if classification == "dictionary" and not result.get("new_image_b64"):
            self.notify(
                "Confirmed. No replacement image is cached, so the original will be preserved safely.",
                severity="warning",
            )
        else:
            self.notify(f"Image confirmed as {classification}.", severity="information")
        self.update_details()

    def on_image_set_selected(self, set_key: str) -> None:
        if not set_key:
            return

        item = self.queue_items[self.active_row_idx]
        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)

        filenames = []
        if set_key == "old":
            fields = item.get("note", {}).get("fields", {}) if item.get("note") else {}
            deck_cfg = self.config_manager.config["decks"].get(item.get("deck_key", self.active_deck_key))
            picture_field = deck_cfg["fields"].get("meaning_image", "Picture") if deck_cfg else "Picture"
            picture_html = fields.get(picture_field, {}).get("value", "") if fields else ""

            from bs4 import BeautifulSoup
            if picture_html:
                soup = BeautifulSoup(picture_html, "html.parser")
                for img in soup.find_all("img"):
                    src = img.get("src", "")
                    if src and not src.startswith("http"):
                        filenames.append(src)
                fallback_fn = self.ocr.extract_image_filename(picture_html)
                if fallback_fn and fallback_fn not in filenames:
                    filenames.append(fallback_fn)
        else:
            if res:
                # Renamed originals remain useful for visual-recall cards, but
                # are not a proposed image when they were classified as a
                # dictionary screenshot.
                if res.get("classification") != "dictionary":
                    for img in res.get("renamed_images", []):
                        filenames.append(img["new_name"])
                if res.get("new_image_filename"):
                    filenames.append(res["new_image_filename"])

        if not filenames:
            self.notify(f"No images found in {set_key} set.", severity="warning")
            return

        if len(filenames) == 1:
            self.action_open_file(filenames[0])
        else:
            choices = [(fn, fn) for fn in filenames]
            self.push_screen(BottomRightPaletteModal("Select Image Index", choices), self.action_open_file)

    def action_edit_word(self) -> None:
        self.on_field_selected_for_edit("word")

    def action_edit_meaning(self) -> None:
        self.on_field_selected_for_edit("meaning")

    def action_edit_kanji(self) -> None:
        self.on_field_selected_for_edit("kanji")

    def action_manage_snapshots(self) -> None:
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected.", severity="warning")
            return
        item = self.queue_items[self.active_row_idx]
        from bs4 import BeautifulSoup
        word = BeautifulSoup(str(item.get("word", "")), "html.parser").get_text().strip()
        self.push_screen(SnapshotManagementScreen(self.snapshot_manager, self.anki, word))

    def on_field_selected_for_edit(self, field_key: str) -> None:
        if not field_key:
            return

        if self._focused_pane_id() != "panel-preview":
            self.notify("Focus Preview [4] before editing card fields.", severity="warning")
            return
        if not self.queue_items or self.active_row_idx is None or self.active_row_idx >= len(self.queue_items):
            self.notify("No card selected.", severity="warning")
            return

        item = self.queue_items[self.active_row_idx]
        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)
        if not res:
            self.notify("Card must be processed (Press 'p') before editing fields.", severity="warning")
            return

        current_val = ""
        self.editing_field_media = {}
        if field_key == "word":
            current_val = res.get("word", "")
        elif field_key == "meaning":
            if res.get("meaning_override_markdown") is not None:
                current_val = str(res.get("meaning_override_markdown", ""))
                self.editing_field_media = dict(res.get("meaning_override_media") or {})
            else:
                lang_key = item.get("deck_key") or item.get("language") or self.active_deck_key
                editable_res = {**res, "type_tag": "", "source_note": ""}
                current_val, self.editing_field_media = html_to_markdown(
                    build_card_document(
                        editable_res, lang_key, "modernize" if item["type"] == "modernize" else "inject"
                    ).values.get("meaning_text", "") or ""
                )
        elif field_key == "kanji":
            if res.get("kanji_override_markdown") is not None:
                current_val = str(res.get("kanji_override_markdown", ""))
                self.editing_field_media = dict(res.get("kanji_override_media") or {})
            else:
                current_val, self.editing_field_media = html_to_markdown(res.get("kanji_construction", ""))

        self.editing_field_key = field_key
        title = field_key.capitalize() if field_key == "word" else f"{field_key.capitalize()} (Markdown)"
        self.push_screen(FieldEditModal(title, current_val), self.on_field_edit_submitted)

    def on_field_edit_submitted(self, new_val: str) -> None:
        if new_val is None:
            return

        item = self.queue_items[self.active_row_idx]
        cache_key = self.cache_key_for(item, self.active_row_idx)
        res = self.processed_cache.get(cache_key)

        if self.editing_field_key == "word":
            res["word"] = new_val.strip()
            item["word"] = new_val.strip()
        elif self.editing_field_key == "meaning":
            res["meaning_override_markdown"] = new_val
            res["meaning_override_media"] = dict(self.editing_field_media)
            res.pop("meaning_override", None)
        elif self.editing_field_key == "kanji":
            res["kanji_override_markdown"] = new_val
            res["kanji_override_media"] = dict(self.editing_field_media)

        self.update_details()
        self.notify(f"Updated {self.editing_field_key} successfully!")

    def action_date_select(self) -> None:
        days = get_log_days()
        if not days:
            self.notify("No log files found.", severity="warning")
            return

        choices = []
        for date_str, path in days:
            choices.append((date_str, f"open:{date_str}:{path}"))

        self.push_screen(BottomRightPaletteModal("Select Log Date", choices), self.on_log_date_selected)

    def on_log_date_selected(self, result: str) -> None:
        if result and result.startswith("open:"):
            parts = result.split("open:")[-1].split(":")
            date_str = parts[0]
            path = Path(":".join(parts[1:]))
            self.push_screen(LogViewerModal(date_str, path))

    def action_copy_log_clipboard(self) -> None:
        text = "\n".join(self.raw_logs)
        if not text:
            try:
                log_file = Path.home() / ".config" / "linguist-anki-bridge" / "app.log"
                if log_file.exists():
                    with open(log_file, "r", encoding="utf-8") as f:
                        text = f.read()
            except Exception:
                pass

        if not text:
            self.notify("No logs to copy.", severity="warning")
            return

        # Try wl-copy (Wayland)
        try:
            import subprocess
            process = subprocess.Popen(['wl-copy'], stdin=subprocess.PIPE)
            process.communicate(input=text.encode('utf-8'))
            self.notify("Logs copied to clipboard via wl-copy!")
            return
        except Exception:
            pass

        # Try xclip (X11)
        try:
            import subprocess
            process = subprocess.Popen(['xclip', '-selection', 'clipboard'], stdin=subprocess.PIPE)
            process.communicate(input=text.encode('utf-8'))
            self.notify("Logs copied to clipboard via xclip!")
            return
        except Exception:
            pass

        # Try xsel (X11)
        try:
            import subprocess
            process = subprocess.Popen(['xsel', '--clipboard', '--input'], stdin=subprocess.PIPE)
            process.communicate(input=text.encode('utf-8'))
            self.notify("Logs copied to clipboard via xsel!")
            return
        except Exception:
            pass

        # Fallback to local workspace file
        try:
            out_file = Path.cwd() / "copied_logs.txt"
            with open(out_file, "w", encoding="utf-8") as f:
                f.write(text)
            self.notify("Clipboard tools missing. Logs saved to copied_logs.txt", severity="warning")
        except Exception as e:
            self.notify(f"Could not copy logs: {e}", severity="error")

    def action_open_space_menu(self) -> None:
        # Two modal screens are opened before a universal search is submitted.
        # Textual restores focus asynchronously while they dismiss, so remember
        # the real pane now instead of consulting ``self.focused`` later.
        self._search_origin_widget = self.focused
        self.push_screen(SpacebarMenuModal(), self.on_space_menu_result)

    def on_space_menu_result(self, result: str) -> None:
        if result in {"inject", "ingest"}:
            self.call_after_refresh(self.action_inject_word)
        elif result == "settings":
            self.call_after_refresh(self.action_configure_setup)
        elif result == "search":
            self.call_after_refresh(self.action_universal_search)

    def action_universal_search(self) -> None:
        self.push_screen(UniversalSearchModal(), self.on_search_query_submitted)

    def _resolve_search_target(self):
        target = getattr(self, "_search_origin_widget", None)
        supported_ids = {"table-status", "list-decks", "table-queue"}
        if target is not None and getattr(target, "id", None) in supported_ids:
            return target

        pane_targets = {
            "panel-status": "#table-status",
            "panel-decks": "#list-decks",
            "panel-queue": "#table-queue",
        }
        selector = pane_targets.get(getattr(target, "id", None))
        if selector:
            try:
                return self.query_one(selector)
            except Exception:
                return None
        return None

    def on_search_query_submitted(self, query: str) -> None:
        if not query:
            return

        self.search_mode = True
        self.search_query = query
        self.search_matches = []
        self.search_current_idx = 0

        focused = self._resolve_search_target()
        if focused is None:
            logging.debug(
                "Universal search has no supported origin widget (origin=%r)",
                getattr(self, "_search_origin_widget", None),
            )
            self.notify(
                "Search is available in Status, Decks, and Cards panes.",
                severity="warning",
            )
            self.search_mode = False
            return

        self.search_target_widget = focused

        if focused.id == "table-status":
            table = focused
            for r in range(table.row_count):
                row_data = table.get_row_at(r)
                for c, cell_val in enumerate(row_data):
                    if query.lower() in str(cell_val).lower():
                        self.search_matches.append((r, c))

        elif focused.id == "list-decks":
            lv = focused
            for idx, child in enumerate(lv.children):
                if hasattr(child, "query_one"):
                    try:
                        lbl = child.query_one(Label).renderable
                        if query.lower() in str(lbl).lower():
                            self.search_matches.append(idx)
                    except Exception:
                        pass

        elif focused.id == "table-queue":
            table = focused
            for r in range(table.row_count):
                row_data = table.get_row_at(r)
                for cell_val in row_data:
                    if query.lower() in str(cell_val).lower():
                        if r not in self.search_matches:
                            self.search_matches.append(r)

        if self.search_matches:
            self.search_current_idx = 0
            self.jump_to_search_match()
            from linguist_anki_bridge.tui.screens import SearchNavigationScreen
            self.push_screen(SearchNavigationScreen())
        else:
            self.notify(f"No matches found for '{query}'", severity="warning")
            self.search_mode = False

    def jump_to_search_match(self) -> None:
        if not self.search_matches:
            return
        match = self.search_matches[self.search_current_idx]
        widget = self.search_target_widget

        if widget.id == "table-status":
            widget.cursor_coordinate = match
        elif widget.id == "list-decks":
            widget.index = match
        elif widget.id == "table-queue":
            widget.move_cursor(row=match)

    def search_next_match(self) -> None:
        if not self.search_matches:
            return
        self.search_current_idx = (self.search_current_idx + 1) % len(self.search_matches)
        self.jump_to_search_match()

    def search_prev_match(self) -> None:
        if not self.search_matches:
            return
        self.search_current_idx = (self.search_current_idx - 1) % len(self.search_matches)
        self.jump_to_search_match()

    def exit_search_mode(self) -> None:
        self.search_mode = False

    from textual import events
    def on_key(self, event: events.Key) -> None:
        if self.debug_mode:
            logging.debug(f"[TUI Debug] Key pressed: {event.key} (focused: {self.focused})")
