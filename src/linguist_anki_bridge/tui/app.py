import asyncio
import logging
from pathlib import Path
from textual.app import App, ComposeResult
from textual.containers import Vertical, Horizontal
from textual.widgets import Label, Header, Footer, TabbedContent, TabPane, Button, Select
from textual.reactive import reactive

from linguist_anki_bridge.config import ConfigManager, load_omarchy_theme
from linguist_anki_bridge.tui.setup import SetupScreen
from linguist_anki_bridge.tui.screens import ModernizeTab, IngestTab, GrammarTab, BackupLogsTab
from linguist_anki_bridge.utils import check_health
from linguist_anki_bridge.llm import OllamaClient

def generate_runtime_css(theme: dict):
    base_css_file = Path(__file__).parent / "styles.css"
    base_css = ""
    if base_css_file.exists():
        with open(base_css_file, "r", encoding="utf-8") as f:
            base_css = f.read()
            
    # Build CSS variables block
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

class AnkiBridgeApp(App):
    CSS_PATH = str(Path.home() / ".config" / "linguist-anki-bridge" / "custom_styles.css")
    
    def __init__(self, theme=None, debug=False):
        super().__init__()
        self.debug_mode = debug
        self.custom_theme = theme
        self.config_manager = ConfigManager()
        self.ollama = OllamaClient(url=self.config_manager.config["llm"]["ollama_url"])
        
        # Reactive status values
        self.anki_status = reactive("Checking...")
        self.ollama_status = reactive("Checking...")
        self.jisho_status = reactive("Checking...")
        self.cambridge_status = reactive("Checking...")
        self.moedict_status = reactive("Checking...")
        self.dict_cc_status = reactive("Checking...")
        self.models_list = []

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        
        with Horizontal():
            # Left Sidebar
            with Vertical(id="sidebar"):
                yield Label("[bold accent]Linguist Anki Bridge[/]\n", id="lbl-app-title")
                
                yield Label("[bold]API Status Indicators:[/]")
                yield Label("● AnkiConnect: Checking...", id="lbl-status-anki")
                yield Label("● Ollama: Checking...", id="lbl-status-ollama")
                yield Label("● Jisho: Checking...", id="lbl-status-jisho")
                yield Label("● Cambridge: Checking...", id="lbl-status-cambridge")
                yield Label("● MoeDict: Checking...", id="lbl-status-moedict")
                yield Label("● dict.cc: Checking...", id="lbl-status-dictcc")
                
                yield Label("\n[bold]Ollama Active Model:[/]")
                yield Select([], id="select-active-model", prompt="Select Model")
                
                # Dry Run Toggle
                dry_run_txt = "Dry Run: ON" if self.config_manager.config.get("dry_run", True) else "Dry Run: OFF"
                yield Button(dry_run_txt, variant="warning", id="btn-toggle-dry-run")
                
                # Setup Wizard trigger
                yield Button("Configure Decks", variant="primary", id="btn-configure-decks")
                yield Button("Quit", variant="error", id="btn-quit")

            # Right Main Panel
            with Vertical(id="main-content"):
                with TabbedContent():
                    with TabPane("Modernize Card"):
                        yield ModernizeTab(self.config_manager.config)
                    with TabPane("Ingest Vocab"):
                        yield IngestTab(self.config_manager.config)
                    with TabPane("Grammar Support"):
                        yield GrammarTab(self.config_manager.config)
                    with TabPane("Backups & Logs"):
                        yield BackupLogsTab(self.config_manager.config)

        yield Footer()

    async def on_mount(self) -> None:
        # Check first run setup completion
        if not self.config_manager.is_setup_completed():
            self.notify("First run detected. Please configure deck mappings.", severity="warning")
            self.push_screen(SetupScreen(self.config_manager, self.on_setup_completed))
            
        # Start background health pinger
        self.run_worker(self.health_loop())
        
        # Load Ollama models
        self.run_worker(self.load_ollama_models(), thread=True)

    def on_setup_completed(self) -> None:
        self.pop_screen()
        # Refresh tabs/configurations
        self.notify("Setup completed! Mappings loaded.", severity="information")
        # Re-initialize deck settings
        self.config_manager.load()

    async def health_loop(self):
        while True:
            # Query health checks
            loop = asyncio.get_event_loop()
            status = await loop.run_in_executor(None, check_health, 
                                                self.config_manager.config["anki"]["url"],
                                                self.config_manager.config["llm"]["ollama_url"])
            
            # Update labels
            self.update_status_label("#lbl-status-anki", "AnkiConnect", status["anki"])
            self.update_status_label("#lbl-status-ollama", "Ollama", status["ollama"])
            self.update_status_label("#lbl-status-jisho", "Jisho", status["jisho"])
            self.update_status_label("#lbl-status-cambridge", "Cambridge", status["cambridge"])
            self.update_status_label("#lbl-status-moedict", "MoeDict", status["moedict"])
            self.update_status_label("#lbl-status-dictcc", "dict.cc", status["dict_cc"])
            
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
            models = self.ollama.get_available_models()
            self.models_list = [(m, m) for m in models]
            select_box = self.query_one("#select-active-model", Select)
            select_box.set_options(self.models_list)
            
            # Select first available or active config
            config_model = self.config_manager.config["llm"]["model"]
            if config_model in models:
                select_box.value = config_model
                self.ollama.set_model(config_model)
            elif models:
                select_box.value = models[0]
                self.config_manager.config["llm"]["model"] = models[0]
                self.config_manager.save()
                self.ollama.set_model(models[0])
        except Exception as e:
            logging.error(f"Failed to query Ollama models: {e}")

    def on_select_changed(self, event: Select.Changed) -> None:
        if event.select.id == "select-active-model" and event.value and event.value != Select.BLANK:
            self.config_manager.config["llm"]["model"] = event.value
            self.config_manager.save()
            self.ollama.set_model(event.value)
            self.notify(f"Active model set to '{event.value}'")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-quit":
            self.exit()
        elif event.button.id == "btn-configure-decks":
            self.push_screen(SetupScreen(self.config_manager, self.on_setup_completed))
        elif event.button.id == "btn-toggle-dry-run":
            # Toggle dry run
            current = self.config_manager.config.get("dry_run", True)
            new_val = not current
            self.config_manager.config["dry_run"] = new_val
            self.config_manager.save()
            
            btn = self.query_one("#btn-toggle-dry-run", Button)
            btn.label = "Dry Run: ON" if new_val else "Dry Run: OFF"
            self.notify(f"Dry Run set to {new_val}")
            
            # Update Modernize tab warning label if present
            try:
                self.query_one(ModernizeTab).update_dry_run_label()
            except Exception:
                pass
