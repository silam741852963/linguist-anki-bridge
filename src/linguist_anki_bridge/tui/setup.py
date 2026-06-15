import logging
from textual.app import ComposeResult
from textual.containers import Vertical, Horizontal
from textual.screen import Screen
from textual.widgets import Label, Select, Button, Header, Footer
from linguist_anki_bridge.anki import AnkiConnectClient

class SetupScreen(Screen):
    def __init__(self, config_manager, on_complete_callback):
        super().__init__()
        self.config_manager = config_manager
        self.on_complete_callback = on_complete_callback
        
        # Connect to Anki to fetch decks
        self.anki_client = AnkiConnectClient(url=config_manager.config["anki"]["url"])
        self.deck_choices = []
        self.anki_online = False
        try:
            decks = self.anki_client.get_decks()
            self.deck_choices = [(d, d) for d in decks]
            self.anki_online = True
        except Exception as e:
            logging.error(f"SetupScreen failed to query Anki: {e}")
            self.anki_online = False

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        
        with Vertical(id="main-content"):
            yield Label("[bold accent]Linguist Anki Bridge - First-Run Setup Wizard[/]", id="setup-title")
            yield Label("\nWelcome! Please map your local Anki decks to the languages you want to learn.")
            yield Label("[yellow]Note: You must map at least one language deck to continue.[/]\n")
            
            if not self.anki_online:
                yield Label("[bold red]ERROR: Local Anki / AnkiConnect is offline![/]")
                yield Label("Please make sure Anki Desktop is running and the AnkiConnect add-on is installed.")
                yield Label("Then restart this application.\n")
                yield Button("Quit Setup", id="btn-quit-setup")
            else:
                # Japanese Deck Select
                yield Label("[bold]Japanese Deck Mapping[/]")
                yield Select(self.deck_choices, id="select-deck-japanese", prompt="Select Japanese Deck (Optional)")
                
                # English Deck Select
                yield Label("\n[bold]English Deck Mapping[/]")
                yield Select(self.deck_choices, id="select-deck-english", prompt="Select English Deck (Optional)")
                
                # Taiwanese Deck Select
                yield Label("\n[bold]Taiwanese (Hokkien/Mandarin) Deck Mapping[/]")
                yield Select(self.deck_choices, id="select-deck-taiwanese", prompt="Select Taiwanese Deck (Optional)")
                
                # German Deck Select
                yield Label("\n[bold]German Deck Mapping[/]")
                yield Select(self.deck_choices, id="select-deck-german", prompt="Select German Deck (Optional)")
                
                # Action Buttons
                with Horizontal(classes="mt-2"):
                    yield Button("Save and Continue", variant="success", id="btn-save-setup")
                    yield Button("Quit", variant="error", id="btn-quit-setup")
                    
        yield Footer()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-quit-setup":
            self.app.exit()
            
        elif event.button.id == "btn-save-setup":
            # Extract chosen decks
            sel_ja = self.query_one("#select-deck-japanese", Select).value
            sel_en = self.query_one("#select-deck-english", Select).value
            sel_tw = self.query_one("#select-deck-taiwanese", Select).value
            sel_de = self.query_one("#select-deck-german", Select).value
            
            # Textual's Select.BLANK needs to be converted to None for yaml serialization
            sel_ja = None if sel_ja == Select.BLANK else sel_ja
            sel_en = None if sel_en == Select.BLANK else sel_en
            sel_tw = None if sel_tw == Select.BLANK else sel_tw
            sel_de = None if sel_de == Select.BLANK else sel_de
            
            # Check at least one selected
            if not any([sel_ja, sel_en, sel_tw, sel_de]):
                self.app.bell()
                # Display warning label or notify
                self.notify("You must select at least one deck!", severity="error")
                return
                
            # Update config
            self.config_manager.config["decks"]["japanese"]["deck_name"] = sel_ja
            self.config_manager.config["decks"]["english"]["deck_name"] = sel_en
            self.config_manager.config["decks"]["taiwanese"]["deck_name"] = sel_tw
            self.config_manager.config["decks"]["german"]["deck_name"] = sel_de
            
            # Save config
            self.config_manager.save()
            
            # Complete
            self.notify("Setup saved successfully!", severity="information")
            self.on_complete_callback()
