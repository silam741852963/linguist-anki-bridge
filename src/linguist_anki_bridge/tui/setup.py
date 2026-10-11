import logging
import json
import asyncio
import urllib.request
import urllib.parse
import copy
from textual.app import ComposeResult
from textual.containers import Vertical, Horizontal, ScrollableContainer
from textual.screen import Screen
from textual.widgets import Label, ListView, ListItem, Header, Footer, Switch, Select, TextArea, Button, Input, TabbedContent, TabPane, Static, ContentSwitcher
from textual import events
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.tui.screens import SelectionListModal

class SetupScreen(Screen):
    BINDINGS = [
        ("s", "save_setup", "Save Settings"),
        ("q", "quit_setup", "Cancel/Quit"),
        ("escape", "quit_setup", "Cancel/Quit"),
    ]

    def __init__(self, config_manager, on_complete_callback):
        super().__init__()
        self.config_manager = config_manager
        self.original_config = copy.deepcopy(config_manager.config)
        self.on_complete_callback = on_complete_callback

        # Connect to Anki to fetch decks
        self.anki_client = AnkiConnectClient(url=config_manager.config["anki"]["url"])
        self.deck_choices = []
        self.anki_online = False

        # Temporary configuration state
        self.temp_decks = {
            "japanese_vocab": config_manager.config["decks"].get("japanese_vocab", {}).get("deck_name"),
            "english_vocab": config_manager.config["decks"].get("english_vocab", {}).get("deck_name"),
            "taiwanese_vocab": config_manager.config["decks"].get("taiwanese_vocab", {}).get("deck_name"),
            "german_vocab": config_manager.config["decks"].get("german_vocab", {}).get("deck_name"),
        }

        self.deck_choices = ["[None / Unmapped]"]

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)

        with Vertical(id="main-content"):
            yield Label("[bold accent]Linguist Anki Bridge - Keyboard-Only Setup Wizard[/]", id="setup-title")
            yield Label("\nUse arrow keys to navigate. Press [bold]Enter[/] to select mapping for language.")
            yield Label("Press [bold]s[/] to Save configuration, or [bold]q[/]/[bold]Esc[/] to Quit.\n")

            yield Label("[yellow]Connecting to AnkiConnect...[/]", id="setup-anki-status")
            yield ListView(
                ListItem(Label(""), id="setup-japanese_vocab"),
                ListItem(Label(""), id="setup-english_vocab"),
                ListItem(Label(""), id="setup-taiwanese_vocab"),
                ListItem(Label(""), id="setup-german_vocab"),
                id="setup-langs-list"
            )

        yield Footer()

    async def on_mount(self) -> None:
        self.update_list_labels()
        try:
            decks = await asyncio.get_running_loop().run_in_executor(None, self.anki_client.get_decks)
            self.deck_choices = ["[None / Unmapped]", *decks]
            self.anki_online = True
            self.query_one("#setup-anki-status", Label).update("[green]AnkiConnect online.[/]")
            self.query_one("#setup-langs-list", ListView).focus()
        except Exception as e:
            logging.error(f"SetupScreen failed to query Anki: {e}")
            self.query_one("#setup-anki-status", Label).update(
                "[bold red]AnkiConnect is offline. Start Anki and reopen setup.[/]"
            )

    def update_list_labels(self):
        # Update text labels inside ListItems
        def get_desc(lang: str, val: str) -> str:
            val_str = f"[green]{val}[/]" if val else "[yellow][Unmapped / Configured Later][/]"
            return f"{lang.replace('_', ' ').title()}: {val_str}"

        self.query_one("#setup-japanese_vocab Label").update(get_desc("japanese_vocab", self.temp_decks["japanese_vocab"]))
        self.query_one("#setup-english_vocab Label").update(get_desc("english_vocab", self.temp_decks["english_vocab"]))
        self.query_one("#setup-taiwanese_vocab Label").update(get_desc("taiwanese_vocab", self.temp_decks["taiwanese_vocab"]))
        self.query_one("#setup-german_vocab Label").update(get_desc("german_vocab", self.temp_decks["german_vocab"]))

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if not event.item or not event.item.id:
            return

        lang_key = event.item.id.replace("setup-", "")
        # Trigger deck selection modal
        self.app.push_screen(
            SelectionListModal(f"Map {lang_key.replace('_', ' ').title()} Target Deck", self.deck_choices),
            lambda choice: self.on_deck_chosen(lang_key, choice)
        )

    def on_deck_chosen(self, lang_key: str, choice: str):
        if not choice:
            return

        if choice == "[None / Unmapped]":
            self.temp_decks[lang_key] = None
        else:
            self.temp_decks[lang_key] = choice

        self.update_list_labels()
        self.notify(f"Mapped {lang_key.replace('_', ' ').title()} to deck '{choice}'")

    def action_save_setup(self) -> None:
        # Check at least one selected
        if not any(self.temp_decks.values()):
            self.notify("Error: You must map at least one language deck to save!", severity="error")
            return

        # Write back to config_manager
        for lang_key, val in self.temp_decks.items():
            self.config_manager.config["decks"][lang_key]["deck_name"] = val

        if not self.config_manager.save():
            self.notify("Failed to save configuration. Check the log and file permissions.", severity="error")
            return
        self.notify("Setup saved successfully!", severity="information")
        self.on_complete_callback()

    def action_quit_setup(self) -> None:
        if not self.config_manager.is_setup_completed():
            # Force quit app if cancelled and not configured
            self.app.exit()
        else:
            self.on_complete_callback()


class ConfigScreen(Screen):
    SETTINGS_SECTIONS = (
        "general", "connections", "decks", "generation",
        "kanji", "dictionary", "filters",
    )
    BINDINGS = [
        ("s", "save_config", "Save Config"),
        ("escape", "quit_config", "Cancel/Back"),
        ("1", "settings_section_1", "Workflow"),
        ("2", "settings_section_2", "Connections"),
        ("3", "settings_section_3", "Decks"),
        ("4", "settings_section_4", "Generation"),
        ("5", "settings_section_5", "Kanji"),
        ("6", "settings_section_6", "Dictionary"),
        ("7", "settings_section_7", "Filters & Media"),
    ]

    def __init__(self, config_manager, on_complete_callback):
        super().__init__()
        self.config_manager = config_manager
        self.original_config = copy.deepcopy(config_manager.config)
        self.on_complete_callback = on_complete_callback

        self.anki_client = AnkiConnectClient(url=config_manager.config["anki"]["url"])
        self.anki_decks = []
        self.anki_online = False
        configured_decks = {
            cfg.get("deck_name") for cfg in config_manager.config.get("decks", {}).values()
            if cfg.get("deck_name")
        }
        self.anki_decks = sorted(configured_decks)

        configured_model = config_manager.config["llm"].get("model")
        self.model_choices = [("None", Select.BLANK)]
        if configured_model:
            self.model_choices.append((configured_model, configured_model))

        self.editing_deck_key = "japanese_vocab"
        self.deck_keys_choices = [
            ("Japanese Vocabulary", "japanese_vocab"),
            ("Japanese Grammar", "japanese_grammar"),
            ("English Vocabulary", "english_vocab"),
            ("English Grammar", "english_grammar"),
            ("Taiwanese Vocabulary", "taiwanese_vocab"),
            ("Taiwanese Grammar", "taiwanese_grammar"),
            ("German Vocabulary", "german_vocab"),
            ("German Grammar", "german_grammar")
        ]

        self.anki_decks_choices = [(d, d) for d in self.anki_decks]
        self.anki_decks_choices.insert(0, ("[None / Unmapped]", Select.BLANK))

        self.kanji_presets_choices = [
            ("Jisho (English Kanji)", "jisho"),
            ("HvDic (Vietnamese Hán-Việt)", "hvdic")
        ]

        self.dict_presets_choices = [
            ("Jisho (Japanese preset)", "jisho"),
            ("Cambridge (English preset)", "cambridge"),
            ("MoeDict (Taiwanese preset)", "moedict"),
            ("Dict.cc (German preset)", "dict_cc"),
            ("Custom Scraper", "custom")
        ]

        self.active_model = config_manager.config["llm"]["model"] or Select.BLANK

    def get_deck_val(self, key: str) -> str:
        deck_cfg = self.config_manager.config["decks"].get(self.editing_deck_key, {})
        return deck_cfg.get(key, "") or ""

    def get_field_val(self, key: str) -> str:
        deck_cfg = self.config_manager.config["decks"].get(self.editing_deck_key, {})
        return deck_cfg.get("fields", {}).get(key, "") or ""

    def save_current_deck_inputs(self) -> None:
        if not self.editing_deck_key:
            return
        deck_cfg = self.config_manager.config["decks"].setdefault(self.editing_deck_key, {})

        target_deck_select = self.query_one("#select-target-deck", Select)
        deck_cfg["deck_name"] = None if target_deck_select.value == Select.BLANK else target_deck_select.value

        deck_cfg["note_type"] = self.query_one("#input-note-type", Input).value.strip()
        deck_cfg["ocr_langs"] = self.query_one("#input-ocr-langs", Input).value.strip()

        fields = deck_cfg.setdefault("fields", {})
        fields["expression"] = self.query_one("#input-field-expr", Input).value.strip()
        fields["meaning_image"] = self.query_one("#input-field-img", Input).value.strip()
        fields["meaning_text"] = self.query_one("#input-field-text", Input).value.strip()
        fields["kanji_construction"] = self.query_one("#input-field-kanji", Input).value.strip()
        fields["audio"] = self.query_one("#input-field-audio", Input).value.strip()

    def load_deck_inputs(self) -> None:
        if not self.editing_deck_key:
            return
        deck_cfg = self.config_manager.config["decks"].get(self.editing_deck_key, {})

        target_deck_select = self.query_one("#select-target-deck", Select)
        target_deck_select.value = deck_cfg.get("deck_name") or Select.BLANK

        self.query_one("#input-note-type", Input).value = deck_cfg.get("note_type", "")
        self.query_one("#input-ocr-langs", Input).value = deck_cfg.get("ocr_langs", "")

        fields = deck_cfg.get("fields", {})
        self.query_one("#input-field-expr", Input).value = fields.get("expression", "")
        self.query_one("#input-field-img", Input).value = fields.get("meaning_image", "")
        self.query_one("#input-field-text", Input).value = fields.get("meaning_text", "")
        self.query_one("#input-field-kanji", Input).value = fields.get("kanji_construction", "")
        self.query_one("#input-field-audio", Input).value = fields.get("audio", "")

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Vertical(id="config-container"):
            yield Label("[bold accent]● SETTINGS[/]  [dim]1-7 sections · Tab controls · s save · Esc cancel[/]", id="config-title")
            with Horizontal(id="settings-layout"):
                with Vertical(id="settings-nav-pane", classes="pane"):
                    yield Label("[bold accent]● SECTIONS[/]", classes="pane-title")
                    yield ListView(
                        ListItem(Label("1  Workflow & Safety"), id="settings-nav-general"),
                        ListItem(Label("2  Connections & Model"), id="settings-nav-connections"),
                        ListItem(Label("3  Decks & Fields"), id="settings-nav-decks"),
                        ListItem(Label("4  Generation & OCR"), id="settings-nav-generation"),
                        ListItem(Label("5  Kanji Construction"), id="settings-nav-kanji"),
                        ListItem(Label("6  Dictionaries"), id="settings-nav-dictionary"),
                        ListItem(Label("7  Filters & Media"), id="settings-nav-filters"),
                        id="settings-nav",
                    )

                with Vertical(id="settings-content-pane", classes="pane"):
                    with ContentSwitcher(initial="settings-general", id="settings-content"):
                        with ScrollableContainer(id="settings-general", classes="settings-section"):
                            yield Label("[bold accent]● WORKFLOW & SAFETY [1][/]", classes="pane-title")
                            yield Label("Dry Run Mode:")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config.get("dry_run", True), allow_blank=False, id="select-dry-run")
                            yield Label("Backup Directory:")
                            yield Input(value=self.config_manager.config["anki"].get("backup_dir", ""), id="input-backup-dir")
                            yield Label("[bold yellow]Batch Modernization[/]")
                            yield Label("Maximum Attempts per Card:")
                            yield Input(value=str(self.config_manager.config.get("batch", {}).get("max_attempts", 3)), id="input-batch-attempts")
                            yield Label("Initial Retry Backoff (seconds; doubles per attempt):")
                            yield Input(value=str(self.config_manager.config.get("batch", {}).get("retry_backoff_seconds", 5.0)), id="input-batch-backoff")
                            yield Label("Minimum Interval Between Anki Commits (seconds):")
                            yield Input(value=str(self.config_manager.config.get("batch", {}).get("commit_interval_seconds", 0.25)), id="input-batch-commit-interval")
                            yield Label("Per-service Start Intervals JSON (seconds):")
                            yield TextArea(json.dumps(self.config_manager.config.get("batch", {}).get("service_intervals", {}), indent=2), id="textarea-batch-rates")

                        with ScrollableContainer(id="settings-connections", classes="settings-section"):
                            yield Label("[bold accent]● CONNECTIONS & MODEL [2][/]", classes="pane-title")
                            yield Label("AnkiConnect API URL:")
                            yield Input(value=self.config_manager.config["anki"]["url"], id="input-anki-url")
                            yield Label("Ollama API URL:")
                            yield Input(value=self.config_manager.config["llm"]["ollama_url"], id="input-ollama-url")
                            yield Label("Ollama Active Model:")
                            yield Select(self.model_choices, value=self.active_model, id="select-ollama-model")
                            yield Label("Translation/Explanation Target Language:")
                            yield Input(value=self.config_manager.config["llm"].get("translation_language", "English"), id="input-trans-lang")

                        with ScrollableContainer(id="settings-decks", classes="settings-section"):
                            yield Label("[bold accent]● DECKS & FIELDS [3][/]", classes="pane-title")
                            yield Label("Select Deck Key to configure:")
                            yield Select(self.deck_keys_choices, value=self.editing_deck_key, id="select-deck-key")
                            yield Label("Target Anki Deck:")
                            yield Select(self.anki_decks_choices, value=self.get_deck_val("deck_name") or Select.BLANK, id="select-target-deck")
                            yield Label("Anki Note Type:")
                            yield Input(value=self.get_deck_val("note_type"), id="input-note-type")
                            yield Label("OCR Language Codes (e.g. jpn+eng):")
                            yield Input(value=self.get_deck_val("ocr_langs"), id="input-ocr-langs")
                            yield Label("[bold yellow]Field Mappings[/]")
                            yield Label("Expression (Word):")
                            yield Input(value=self.get_field_val("expression"), id="input-field-expr")
                            yield Label("Meaning Image (Picture):")
                            yield Input(value=self.get_field_val("meaning_image"), id="input-field-img")
                            yield Label("Meaning Text (HTML):")
                            yield Input(value=self.get_field_val("meaning_text"), id="input-field-text")
                            yield Label("Kanji Construction:")
                            yield Input(value=self.get_field_val("kanji_construction"), id="input-field-kanji")
                            yield Label("Audio (Pronunciation):")
                            yield Input(value=self.get_field_val("audio"), id="input-field-audio")

                        with ScrollableContainer(id="settings-generation", classes="settings-section"):
                            yield Label("[bold accent]● GENERATION & OCR [4][/]", classes="pane-title")
                            yield Label("Vocabulary System Prompt:")
                            yield TextArea(self.config_manager.config["llm"].get("system_prompt_vocab", ""), id="textarea-prompt-vocab")
                            yield Label("Grammar System Prompt:")
                            yield TextArea(self.config_manager.config["llm"].get("system_prompt_grammar", ""), id="textarea-prompt-grammar")
                            yield Label("OCR Method:")
                            yield Input(value=self.config_manager.config["ocr"].get("method", "tesseract"), id="input-ocr-method")
                            yield Label("OCR Vision Model:")
                            yield Input(value=self.config_manager.config["ocr"].get("ollama_model", ""), id="input-ocr-model")
                            yield Label("OCR Ollama URL:")
                            yield Input(value=self.config_manager.config["ocr"].get("ollama_url", ""), id="input-ocr-url")
                            yield Label("Preprocess Images Before OCR:")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config["ocr"].get("preprocess", True), allow_blank=False, id="select-ocr-preprocess")

                        with ScrollableContainer(id="settings-kanji", classes="settings-section"):
                            yield Label("[bold accent]● KANJI CONSTRUCTION [5][/]", classes="pane-title")
                            yield Label("Scrape Kanji Construction:")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config.get("kanji", {}).get("enabled", True), allow_blank=False, id="select-kanji-enabled")
                            yield Label("Source Language:")
                            yield Select([("English", "english"), ("Vietnamese", "vietnamese")], value=self.config_manager.config.get("kanji", {}).get("source_lang", "english"), allow_blank=False, id="select-kanji-source-lang")
                            yield Label("URL Template (use {char}):")
                            yield Input(value=self.config_manager.config.get("kanji", {}).get("url_template", ""), id="input-kanji-url")
                            yield Label("Summary Prompt (English):")
                            yield TextArea(self.config_manager.config.get("kanji", {}).get("prompt_en", ""), id="textarea-kanji-prompt-en")
                            yield Label("Summary Prompt (Vietnamese):")
                            yield TextArea(self.config_manager.config.get("kanji", {}).get("prompt_vi", ""), id="textarea-kanji-prompt-vi")
                            yield Label("Load Preset CSS Schema:")
                            yield Select(self.kanji_presets_choices, value="jisho", id="select-kanji-preset")
                            yield Label("CSS Extraction Schema JSON:")
                            yield TextArea(json.dumps(self.config_manager.config.get("kanji", {}).get("schema", {}), indent=2), id="textarea-kanji-schema")
                            yield Label("[bold yellow]Schema Generator[/]")
                            yield Label("Test URL (press Enter to generate):")
                            yield Input(placeholder="e.g. https://jisho.org/search/学%23kanji", id="input-generator-url")
                            yield Label("Target Fields (comma separated):")
                            yield Input(value="meanings, strokes, radical, parts", id="input-generator-fields")
                            yield Label("", id="lbl-generator-status")

                        with ScrollableContainer(id="settings-dictionary", classes="settings-section"):
                            yield Label("[bold accent]● DICTIONARIES [6][/]", classes="pane-title")
                            yield Label("Dictionary Preset Source:")
                            yield Select(self.dict_presets_choices, value=self.config_manager.config.get("dictionary", {}).get("preset", "jisho"), allow_blank=False, id="select-dict-preset")
                            yield Label("Custom URL Template (use {word}):")
                            yield Input(value=self.config_manager.config.get("dictionary", {}).get("url_template", ""), id="input-dict-url")
                            yield Label("Custom CSS Extraction Schema JSON:")
                            yield TextArea(json.dumps(self.config_manager.config.get("dictionary", {}).get("schema", {}), indent=2), id="textarea-dict-schema")
                            yield Label("[bold yellow]Schema Generator[/]")
                            yield Label("Test URL (press Enter to generate):")
                            yield Input(placeholder="e.g. https://jisho.org/search/食べる", id="input-dict-generator-url")
                            yield Label("Target Fields (comma separated):")
                            yield Input(value="word, reading, definition, audio_url", id="input-dict-generator-fields")
                            yield Label("", id="lbl-dict-generator-status")

                        with ScrollableContainer(id="settings-filters", classes="settings-section"):
                            yield Label("[bold accent]● FILTERS & MEDIA [7][/]", classes="pane-title")
                            yield Label("Remove Parentheses from Expression:")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config.get("filters", {}).get("remove_parentheses", True), allow_blank=False, id="select-filter-parentheses")
                            yield Label("Word Only Filter (Kanji/Kana/Alpha characters only):")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config.get("filters", {}).get("clean_word_only", False), allow_blank=False, id="select-filter-word-only")
                            yield Label("Find Illustrative Image for Empty Cards:")
                            yield Select([("Enabled", True), ("Disabled", False)], value=self.config_manager.config.get("image_search", {}).get("enabled_for_empty", True), allow_blank=False, id="select-image-search-empty")
                            yield Label("Image Search Query Suffix:")
                            yield Input(value=self.config_manager.config.get("image_search", {}).get("suffix", ""), id="input-image-search-suffix")

        yield Footer()

    async def on_mount(self) -> None:
        self.load_deck_inputs()
        self.is_ready = True
        self.query_one("#settings-nav", ListView).index = 0
        loop = asyncio.get_running_loop()
        try:
            self.anki_decks = await loop.run_in_executor(None, self.anki_client.get_decks)
            self.anki_online = True
            choices = [("[None / Unmapped]", Select.BLANK), *[(d, d) for d in self.anki_decks]]
            self.query_one("#select-target-deck", Select).set_options(choices)
            self.load_deck_inputs()
        except Exception as e:
            logging.warning(f"ConfigScreen could not refresh Anki decks: {e}")
        try:
            from linguist_anki_bridge.llm import OllamaClient
            ollama_cli = OllamaClient(url=self.config_manager.config["llm"]["ollama_url"])
            available = await loop.run_in_executor(None, ollama_cli.get_available_models)
            model_choices = [("None", Select.BLANK), *[(m, m) for m in available]]
            self.query_one("#select-ollama-model", Select).set_options(model_choices)
            active = self.config_manager.config["llm"].get("model")
            self.query_one("#select-ollama-model", Select).value = active if active in available else Select.BLANK
        except Exception as e:
            logging.warning(f"ConfigScreen could not refresh Ollama models: {e}")

    def show_settings_section(self, index: int, focus_nav: bool = False) -> None:
        if not 0 <= index < len(self.SETTINGS_SECTIONS):
            return
        section = self.SETTINGS_SECTIONS[index]
        self.query_one("#settings-content", ContentSwitcher).current = f"settings-{section}"
        nav = self.query_one("#settings-nav", ListView)
        nav.index = index
        if focus_nav:
            nav.focus()

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        if event.list_view.id != "settings-nav" or not event.item or not event.item.id:
            return
        section = event.item.id.removeprefix("settings-nav-")
        if section in self.SETTINGS_SECTIONS:
            self.query_one("#settings-content", ContentSwitcher).current = f"settings-{section}"

    def action_settings_section_1(self) -> None: self.show_settings_section(0, True)
    def action_settings_section_2(self) -> None: self.show_settings_section(1, True)
    def action_settings_section_3(self) -> None: self.show_settings_section(2, True)
    def action_settings_section_4(self) -> None: self.show_settings_section(3, True)
    def action_settings_section_5(self) -> None: self.show_settings_section(4, True)
    def action_settings_section_6(self) -> None: self.show_settings_section(5, True)
    def action_settings_section_7(self) -> None: self.show_settings_section(6, True)

    def on_select_changed(self, event: Select.Changed) -> None:
        if not getattr(self, "is_ready", False):
            return
        if event.select.id == "select-deck-key":
            self.save_current_deck_inputs()
            self.editing_deck_key = event.value
            self.load_deck_inputs()
        elif event.select.id == "select-kanji-preset":
            preset = event.value
            if preset == "jisho":
                self.query_one("#input-kanji-url", Input).value = "https://jisho.org/search/{char}%23kanji"
                schema = {
                    "name": "jisho_kanji",
                    "baseSelector": ".kanji",
                    "fields": [
                        {"name": "meanings", "selector": ".kanji-details__main-meanings", "type": "text"},
                        {"name": "strokes", "selector": ".kanji-details__stroke_count strong", "type": "text"},
                        {"name": "radical", "selector": ".radicals span", "type": "text"},
                        {"name": "parts", "selector": ".parts a", "type": "text"}
                    ]
                }
                self.query_one("#textarea-kanji-schema", TextArea).text = json.dumps(schema, indent=2)
            elif preset == "hvdic":
                self.query_one("#input-kanji-url", Input).value = "https://hvdic.thivien.net/whv/{char}"
                schema = {
                    "name": "hvdic_kanji",
                    "baseSelector": ".hvres",
                    "fields": [
                        {"name": "spell", "selector": ".hvres-spell", "type": "text"},
                        {"name": "chi_tiet", "selector": ".hvres-details", "type": "text"},
                        {"name": "nghia", "selector": ".hvres-meaning", "type": "text"}
                    ]
                }
                self.query_one("#textarea-kanji-schema", TextArea).text = json.dumps(schema, indent=2)
        elif event.select.id == "select-kanji-source-lang":
            lang = event.value
            if lang == "vietnamese":
                self.query_one("#select-kanji-preset", Select).value = "hvdic"
            else:
                self.query_one("#select-kanji-preset", Select).value = "jisho"

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id in ("input-generator-url", "input-dict-generator-url"):
            is_dict = (event.input.id == "input-dict-generator-url")
            self.run_worker(self.run_schema_generation(is_dict))

    async def run_schema_generation(self, is_dict: bool) -> None:
        if is_dict:
            url = self.query_one("#input-dict-generator-url", Input).value.strip()
            fields_desc = self.query_one("#input-dict-generator-fields", Input).value.strip()
            status_lbl = self.query_one("#lbl-dict-generator-status", Label)
            target_textarea = self.query_one("#textarea-dict-schema", TextArea)
        else:
            url = self.query_one("#input-generator-url", Input).value.strip()
            fields_desc = self.query_one("#input-generator-fields", Input).value.strip()
            status_lbl = self.query_one("#lbl-generator-status", Label)
            target_textarea = self.query_one("#textarea-kanji-schema", TextArea)

        if not url:
            self.notify("Please enter a URL to crawl first!", severity="error")
            return

        status_lbl.update("[yellow]Crawling webpage HTML...[/]")
        try:
            loop = asyncio.get_running_loop()
            req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
            html_content = await loop.run_in_executor(None, lambda: urllib.request.urlopen(req).read().decode("utf-8", errors="ignore"))
            status_lbl.update("[yellow]HTML fetched! Calling Ollama to generate CSS extraction schema...[/]")

            model_select = self.query_one("#select-ollama-model", Select)
            active_model = model_select.value
            if not active_model or active_model == Select.BLANK:
                active_model = self.config_manager.config["llm"]["model"] or "llama3"

            from crawl4ai.extraction_strategy import JsonCssExtractionStrategy
            from crawl4ai import LLMConfig

            llm_config = LLMConfig(
                provider=f"ollama/{active_model}",
                base_url=self.config_manager.config["llm"]["ollama_url"]
            )

            query = f"Extract repeating list of elements containing fields: {fields_desc}"
            schema = await JsonCssExtractionStrategy.agenerate_schema(
                html=html_content[:30000],
                query=query,
                llm_config=llm_config
            )

            if schema:
                target_textarea.text = json.dumps(schema, indent=2)
                status_lbl.update("[green]Schema generated successfully![/]")
                self.notify("Schema generated and loaded into text area!")
            else:
                status_lbl.update("[red]Failed to generate schema (empty result).[/]")
        except Exception as e:
            logging.error(f"Failed to generate schema: {e}")
            status_lbl.update(f"[red]Error: {e}[/]")

    def action_save_config(self) -> None:
        self.save_current_deck_inputs()

        self.config_manager.config["anki"]["url"] = self.query_one("#input-anki-url", Input).value.strip()
        self.config_manager.config["anki"]["backup_dir"] = self.query_one("#input-backup-dir", Input).value.strip()
        self.config_manager.config["llm"]["ollama_url"] = self.query_one("#input-ollama-url", Input).value.strip()

        model_select = self.query_one("#select-ollama-model", Select)
        self.config_manager.config["llm"]["model"] = None if model_select.value == Select.BLANK else model_select.value

        self.config_manager.config["llm"]["translation_language"] = self.query_one("#input-trans-lang", Input).value.strip()
        self.config_manager.config["llm"]["system_prompt_vocab"] = self.query_one("#textarea-prompt-vocab", TextArea).text.strip()
        self.config_manager.config["llm"]["system_prompt_grammar"] = self.query_one("#textarea-prompt-grammar", TextArea).text.strip()
        self.config_manager.config["dry_run"] = self.query_one("#select-dry-run", Select).value

        batch_cfg = self.config_manager.config.setdefault("batch", {})
        try:
            batch_cfg["max_attempts"] = max(1, int(self.query_one("#input-batch-attempts", Input).value.strip()))
            batch_cfg["retry_backoff_seconds"] = max(0.0, float(self.query_one("#input-batch-backoff", Input).value.strip()))
            batch_cfg["commit_interval_seconds"] = max(0.0, float(self.query_one("#input-batch-commit-interval", Input).value.strip()))
            rates = json.loads(self.query_one("#textarea-batch-rates", TextArea).text.strip() or "{}")
            if not isinstance(rates, dict):
                raise ValueError("service intervals must be a JSON object")
            batch_cfg["service_intervals"] = {str(key): max(0.0, float(value)) for key, value in rates.items()}
        except (TypeError, ValueError) as exc:
            self.notify(f"Invalid batch scheduling setting: {exc}", severity="error")
            return

        ocr_cfg = self.config_manager.config.setdefault("ocr", {})
        ocr_cfg["method"] = self.query_one("#input-ocr-method", Input).value.strip()
        ocr_cfg["ollama_model"] = self.query_one("#input-ocr-model", Input).value.strip()
        ocr_cfg["ollama_url"] = self.query_one("#input-ocr-url", Input).value.strip()
        ocr_cfg["preprocess"] = self.query_one("#select-ocr-preprocess", Select).value

        kanji_cfg = self.config_manager.config.setdefault("kanji", {})
        kanji_cfg["enabled"] = self.query_one("#select-kanji-enabled", Select).value
        kanji_cfg["source_lang"] = self.query_one("#select-kanji-source-lang", Select).value
        kanji_cfg["url_template"] = self.query_one("#input-kanji-url", Input).value.strip()
        kanji_cfg["prompt_en"] = self.query_one("#textarea-kanji-prompt-en", TextArea).text.strip()
        kanji_cfg["prompt_vi"] = self.query_one("#textarea-kanji-prompt-vi", TextArea).text.strip()

        kanji_schema_text = self.query_one("#textarea-kanji-schema", TextArea).text.strip()
        try:
            kanji_schema = json.loads(kanji_schema_text)
            kanji_cfg["schema"] = kanji_schema
        except Exception as e:
            self.notify(f"Invalid Kanji Schema JSON: {e}", severity="error")
            return

        dict_cfg = self.config_manager.config.setdefault("dictionary", {})
        dict_cfg["preset"] = self.query_one("#select-dict-preset", Select).value
        dict_cfg["url_template"] = self.query_one("#input-dict-url", Input).value.strip()

        dict_schema_text = self.query_one("#textarea-dict-schema", TextArea).text.strip()
        try:
            dict_schema = json.loads(dict_schema_text)
            dict_cfg["schema"] = dict_schema
        except Exception as e:
            self.notify(f"Invalid Dictionary Schema JSON: {e}", severity="error")
            return

        filter_cfg = self.config_manager.config.setdefault("filters", {})
        filter_cfg["remove_parentheses"] = self.query_one("#select-filter-parentheses", Select).value
        filter_cfg["clean_word_only"] = self.query_one("#select-filter-word-only", Select).value

        img_search_cfg = self.config_manager.config.setdefault("image_search", {})
        img_search_cfg["enabled_for_empty"] = self.query_one("#select-image-search-empty", Select).value
        img_search_cfg["suffix"] = self.query_one("#input-image-search-suffix", Input).value.strip()

        if not self.config_manager.save():
            self.notify("Failed to save configuration. Check the log and file permissions.", severity="error")
            return
        self.notify("Configuration saved successfully!", severity="information")
        self.on_complete_callback()

    def action_quit_config(self) -> None:
        self.config_manager.config = self.original_config
        self.on_complete_callback()


class NvimFormInput(Input):
    """Input that lets its owning form implement NORMAL/INSERT semantics."""

    async def _on_key(self, event: events.Key) -> None:
        screen = self.screen
        if isinstance(screen, InputManagementScreen) and screen.handle_nvim_key(event):
            event.stop()
            event.prevent_default()
            return
        await super()._on_key(event)


class InputManagementScreen(Screen):
    BINDINGS = []

    def __init__(self, config_manager, active_deck_key):
        super().__init__()
        self.config_manager = config_manager
        self.active_deck_key = active_deck_key
        self.mode = "normal"
        self.command_pending = False
        self.pending_words: list[str] = []

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal():
            with Vertical(id="left-column"):
                with Vertical(classes="pane", id="input-pane-left"):
                    yield Label("[bold accent]● INPUT MANAGEMENT[/]", classes="pane-title")
                    yield Label("[NORMAL]  arrows/Tab navigate · i/a edit · :w enqueue · :q cancel", id="input-mode-status")
                    with TabbedContent():
                        with TabPane("Manual Input", id="tab-single"):
                            with ScrollableContainer():
                                yield Label("Words/Phrases to Inject or Modernize:")
                                yield NvimFormInput(placeholder="Type one or comma-separated words; Enter adds them", id="input-single-word")
                                yield Static("No words staged.", id="input-staged-words")
                                yield Label("Optional Context Note:")
                                yield NvimFormInput(placeholder="e.g. Context sentence", id="input-single-note")
                                yield Label("Optional Word Type:")
                                yield NvimFormInput(placeholder="e.g. verb, noun", id="input-single-type")
                                yield Label("\n[dim]Enter stages words. Esc returns to NORMAL; :w resolves the batch.[/]")

                        with TabPane("CSV Load", id="tab-csv"):
                            with ScrollableContainer():
                                yield Label("Vocabulary CSV File Path:")
                                yield NvimFormInput(placeholder="e.g. /path/to/vocab.csv", id="input-csv-path")
                                yield Label("\n[dim]Esc returns to NORMAL, then :w loads and resolves.[/]")

                        with TabPane("Grammar Crawl", id="tab-grammar"):
                            with ScrollableContainer():
                                yield Label("Grammar Lesson URL:")
                                yield NvimFormInput(placeholder="e.g. https://example.com/grammar/point", id="input-grammar-url")
                                yield Label("\n[dim]Esc returns to NORMAL, then :w starts the crawl.[/]")

            with Vertical(id="right-column"):
                with Vertical(classes="pane", id="input-pane-right"):
                    yield Label("[bold accent]● DOCUMENTATION & INFO[/]", classes="pane-title")
                    yield Static("", id="input-doc-text")
        yield Footer()

    def on_mount(self) -> None:
        self.update_doc_text()
        self.query_one("#input-single-word", Input).focus()
        self.update_mode_status()

    def update_mode_status(self) -> None:
        if self.command_pending:
            text = "[COMMAND]  w enqueue · q cancel"
        elif self.mode == "insert":
            text = "[INSERT]  Esc normal · Enter stage/next · Tab next field"
        else:
            text = "[NORMAL]  arrows/Tab navigate · i/a edit · x remove last · :w enqueue · :q cancel"
        self.query_one("#input-mode-status", Label).update(text)

    def active_input_ids(self) -> list[str]:
        active = self.query_one(TabbedContent).active
        return {
            "tab-single": ["input-single-word", "input-single-note", "input-single-type"],
            "tab-csv": ["input-csv-path"],
            "tab-grammar": ["input-grammar-url"],
        }.get(active, ["input-single-word"])

    def move_input_focus(self, delta: int) -> None:
        ids = self.active_input_ids()
        current_id = getattr(self.focused, "id", None)
        index = ids.index(current_id) if current_id in ids else 0
        self.query_one(f"#{ids[(index + delta) % len(ids)]}", Input).focus()

    def move_input_tab(self, delta: int) -> None:
        tabs = ["tab-single", "tab-csv", "tab-grammar"]
        content = self.query_one(TabbedContent)
        index = tabs.index(content.active) if content.active in tabs else 0
        content.active = tabs[(index + delta) % len(tabs)]
        self.call_after_refresh(lambda: self.query_one(f"#{self.active_input_ids()[0]}", Input).focus())

    def submit_active_form(self) -> None:
        active = self.query_one(TabbedContent).active
        if active == "tab-single":
            self.stage_current_words()
            if not self.pending_words:
                self.notify("Word/Phrase is required.", severity="warning")
                return
            self.dismiss({
                "action": "manual",
                "words": list(self.pending_words),
                "word": self.pending_words[0],
                "note": self.query_one("#input-single-note", Input).value.strip(),
                "word_type": self.query_one("#input-single-type", Input).value.strip(),
            })
        elif active == "tab-csv":
            path = self.query_one("#input-csv-path", Input).value.strip()
            if path:
                self.dismiss({"action": "csv", "path": path})
            else:
                self.notify("CSV path is required.", severity="warning")
        else:
            url = self.query_one("#input-grammar-url", Input).value.strip()
            if url:
                self.dismiss({"action": "grammar", "url": url})
            else:
                self.notify("Grammar URL is required.", severity="warning")

    def stage_current_words(self) -> bool:
        import re

        input_widget = self.query_one("#input-single-word", Input)
        candidates = [value.strip() for value in re.split(r"[,;；、\n]+", input_widget.value) if value.strip()]
        changed = False
        for word in candidates:
            if word not in self.pending_words:
                self.pending_words.append(word)
                changed = True
        if candidates:
            input_widget.value = ""
        self.update_staged_words()
        self.update_doc_text()
        return changed

    def update_staged_words(self) -> None:
        widget = self.query_one("#input-staged-words", Static)
        if not self.pending_words:
            widget.update("[dim]No words staged.[/]")
            return
        widget.update("\n".join(f"{index}. {word}" for index, word in enumerate(self.pending_words, 1)))

    def handle_nvim_key(self, event: events.Key) -> bool:
        char = event.character or ""
        if self.command_pending:
            self.command_pending = False
            if char in {"w", "x"}:
                self.submit_active_form()
            elif char == "q":
                self.dismiss(None)
            self.update_mode_status()
            return True
        if event.key == "escape":
            if self.mode == "insert":
                self.mode = "normal"
                self.update_mode_status()
            else:
                self.dismiss(None)
            return True
        if self.mode == "insert":
            if event.key == "enter":
                if getattr(self.focused, "id", None) == "input-single-word":
                    self.stage_current_words()
                    self.query_one("#input-single-word", Input).focus()
                else:
                    self.move_input_focus(1)
                return True
            return False
        if char in {"i", "a"}:
            self.mode = "insert"
            if char == "a" and isinstance(self.focused, Input):
                self.focused.cursor_position = len(self.focused.value)
        elif event.key == "down":
            self.move_input_focus(1)
        elif event.key == "up":
            self.move_input_focus(-1)
        elif event.key == "left":
            self.move_input_tab(-1)
        elif event.key == "right":
            self.move_input_tab(1)
        elif char == "x" and self.query_one(TabbedContent).active == "tab-single":
            if self.pending_words:
                self.pending_words.pop()
                self.update_staged_words()
                self.update_doc_text()
        elif char == ":":
            self.command_pending = True
        elif event.key == "enter":
            self.mode = "insert"
        self.update_mode_status()
        return True

    def update_doc_text(self) -> None:
        deck_cfg = self.config_manager.config["decks"].get(self.active_deck_key, {})
        deck_name = deck_cfg.get("deck_name") or "[Unmapped]"
        note_type = deck_cfg.get("note_type") or "[Unmapped]"
        fields = deck_cfg.get("fields") or {}
        dry_run = self.config_manager.config.get("dry_run", True)

        doc = (
            f"[bold accent]Manual Batch Destination[/]\n"
            f"---------------------------\n"
            f"● [bold]Active Deck Key[/]: {self.active_deck_key}\n"
            f"● [bold]Target Anki Deck[/]: {deck_name}\n\n"
            f"● [bold]Note Type[/]: {note_type}\n"
            f"● [bold]Expression Field[/]: {fields.get('expression', 'Expression')}\n"
            f"● [bold]Meaning Field[/]: {fields.get('meaning_text', 'Meaning')}\n"
            f"● [bold]Write Mode[/]: {'Dry run' if dry_run else 'Live commit'}\n"
            f"● [bold]Staged Words[/]: {len(self.pending_words)}\n\n"
            f"[bold accent]Resolution Pipeline[/]\n"
            f"Each expression is checked exactly in this Anki deck. Existing notes enter Modernize; missing notes enter Inject. The resolved list returns to CARDS [3], where Mode is shown explicitly."
        )
        self.query_one("#input-doc-text", Static).update(doc)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        # ``on_key`` owns Enter so it cannot accidentally submit a partial
        # form; this hook only suppresses Input's legacy submit behavior.
        event.stop()

    def action_cancel(self) -> None:
        self.dismiss(None)
