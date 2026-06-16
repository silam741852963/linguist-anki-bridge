import logging
from textual.app import ComposeResult
from textual.containers import Vertical, Horizontal
from textual.screen import Screen
from textual.widgets import Label, ListView, ListItem, Header, Footer
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
        self.on_complete_callback = on_complete_callback
        
        # Connect to Anki to fetch decks
        self.anki_client = AnkiConnectClient(url=config_manager.config["anki"]["url"])
        self.deck_choices = []
        self.anki_online = False
        
        # Temporary configuration state
        self.temp_decks = {
            "japanese": config_manager.config["decks"]["japanese"]["deck_name"],
            "english": config_manager.config["decks"]["english"]["deck_name"],
            "taiwanese": config_manager.config["decks"]["taiwanese"]["deck_name"],
            "german": config_manager.config["decks"]["german"]["deck_name"],
        }
        
        try:
            self.deck_choices = self.anki_client.get_decks()
            # Allow deselecting / None option
            self.deck_choices.insert(0, "[None / Unmapped]")
            self.anki_online = True
        except Exception as e:
            logging.error(f"SetupScreen failed to query Anki: {e}")
            self.anki_online = False

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        
        with Vertical(id="main-content"):
            yield Label("[bold accent]Linguist Anki Bridge - Keyboard-Only Setup Wizard[/]", id="setup-title")
            yield Label("\nUse arrow keys to navigate. Press [bold]Enter[/] to select mapping for language.")
            yield Label("Press [bold]s[/] to Save configuration, or [bold]q[/]/[bold]Esc[/] to Quit.\n")
            
            if not self.anki_online:
                yield Label("[bold red]ERROR: Local Anki / AnkiConnect is offline![/]")
                yield Label("Please make sure Anki Desktop is running and AnkiConnect add-on is installed.")
                yield Label("Then press 'q' or 'Esc' to exit, start Anki, and relaunch this tool.\n")
            else:
                yield ListView(
                    ListItem(Label(""), id="setup-japanese"),
                    ListItem(Label(""), id="setup-english"),
                    ListItem(Label(""), id="setup-taiwanese"),
                    ListItem(Label(""), id="setup-german"),
                    id="setup-langs-list"
                )
                
        yield Footer()

    def on_mount(self) -> None:
        if self.anki_online:
            self.update_list_labels()
            self.query_one("#setup-langs-list", ListView).focus()

    def update_list_labels(self):
        # Update text labels inside ListItems
        def get_desc(lang: str, val: str) -> str:
            val_str = f"[green]{val}[/]" if val else "[yellow][Unmapped / Configured Later][/]"
            return f"{lang.capitalize()}: {val_str}"

        self.query_one("#setup-japanese Label").update(get_desc("japanese", self.temp_decks["japanese"]))
        self.query_one("#setup-english Label").update(get_desc("english", self.temp_decks["english"]))
        self.query_one("#setup-taiwanese Label").update(get_desc("taiwanese", self.temp_decks["taiwanese"]))
        self.query_one("#setup-german Label").update(get_desc("german", self.temp_decks["german"]))

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if not event.item or not event.item.id:
            return
            
        lang_key = event.item.id.replace("setup-", "")
        # Trigger deck selection modal
        self.app.push_screen(
            SelectionListModal(f"Map {lang_key.capitalize()} Target Deck", self.deck_choices),
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
        self.notify(f"Mapped {lang_key.capitalize()} to deck '{choice}'")

    def action_save_setup(self) -> None:
        # Check at least one selected
        if not any(self.temp_decks.values()):
            self.notify("Error: You must map at least one language deck to save!", severity="error")
            return
            
        # Write back to config_manager
        for lang_key, val in self.temp_decks.items():
            self.config_manager.config["decks"][lang_key]["deck_name"] = val
            
        self.config_manager.save()
        self.notify("Setup saved successfully!", severity="information")
        self.on_complete_callback()

    def action_quit_setup(self) -> None:
        if not self.config_manager.is_setup_completed():
            # Force quit app if cancelled and not configured
            self.app.exit()
        else:
            self.on_complete_callback()
