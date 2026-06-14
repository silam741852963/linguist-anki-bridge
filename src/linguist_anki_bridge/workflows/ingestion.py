from typing import List
from rich.console import Console
from rich.table import Table
from rich.markup import escape
from rich.prompt import Confirm

from linguist_anki_bridge.anki.client import AnkiConnectError
from linguist_anki_bridge.anki.notes import add_note
from linguist_anki_bridge.anki.backup import create_backup
from linguist_anki_bridge.llm.prompts import INGESTION_PROMPT
from linguist_anki_bridge.validation import execute_with_retry, ValidationError
from linguist_anki_bridge.cache import DictionaryCache
from linguist_anki_bridge.config import settings

def get_llm():
    if settings.llm_provider == "gemini":
        from linguist_anki_bridge.llm.gemini import GeminiProvider
        return GeminiProvider()
    else:
        from linguist_anki_bridge.llm.ollama import OllamaProvider
        return OllamaProvider()
        
def get_scraper(language: str):
    if language == "ja":
        from linguist_anki_bridge.scrapers.jisho import JishoScraper
        return JishoScraper()
    else:
        from linguist_anki_bridge.scrapers.cambridge import CambridgeScraper
        return CambridgeScraper()

def run(words: List[str], language: str, dry_run: bool, console: Console) -> None:
    """Ingestion workflow: fetches word information, generates examples via LLM, and creates cards."""
    if not dry_run:
        try:
            create_backup(console)
        except AnkiConnectError as e:
            console.print(f"[red]Backup failed: {e}. Aborting for safety.[/red]")
            return

    scraper = get_scraper(language)
    llm = get_llm()
    cache = DictionaryCache()

    for word in words:
        word = word.strip()
        if not word:
            continue
            
        console.print(f"\n[cyan]Ingesting vocabulary word: '{word}'[/cyan]")
        try:
            # Check Cache
            cached_data = cache.get(language, word)
            if cached_data:
                console.print(f"[dim]Cache hit for '{word}'[/dim]")
                scraped_data = cached_data
            else:
                console.print(f"[dim]Scraping dictionary for '{word}'...[/dim]")
                scraped_data = scraper.fetch(word)
                cache.set(language, word, scraped_data)

            definition = scraped_data.get("definition", "No definition found")
            
            # Execute LLM with validation & retry loop
            prompt = INGESTION_PROMPT.format(word=word, definition=definition)
            result = execute_with_retry(
                llm_call_fn=lambda p: llm.generate_text(p),
                initial_prompt=prompt,
                expected_keys=["example_sentence", "translation"]
            )
            
            front_val = scraped_data.get("word", word)
            back_val = (
                f"<b>Reading:</b> {escape(scraped_data.get('reading', ''))}<br/>"
                f"<b>Meaning:</b> {escape(definition)}<br/><br/>"
                f"<i>{escape(result['example_sentence'])}</i><br/>"
                f"({escape(result['translation'])})"
            )
            
            # Show preview table
            preview = Table(title=f"Proposed Flashcard for '{word}'", show_header=True, header_style="bold magenta")
            preview.add_column("Field", style="dim", width=12)
            preview.add_column("Content", style="green")
            
            preview.add_row("Front", front_val)
            preview.add_row("Back", back_val)
            
            console.print(preview)
            
            if dry_run:
                console.print("[yellow](DRY RUN) skipping actual insertion.[/yellow]")
                continue
                
            if Confirm.ask("Add this new note to Anki?"):
                fields = {
                    "Front": front_val,
                    "Back": back_val
                }
                add_note(
                    deck_name=settings.default_deck, 
                    model_name=settings.default_note_type, 
                    fields=fields, 
                    tags=["linguist-ingested", f"lang-{language}"]
                )
                console.print("[green]Note inserted successfully![/green]")
            else:
                console.print("[yellow]Ingestion skipped by user.[/yellow]")

        except ValidationError as e:
            console.print(f"[red]LLM Validation error: {e}[/red]")
        except Exception as e:
            console.print(f"[red]Error ingesting '{word}': {e}[/red]")
