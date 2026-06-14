import sys
import click
from rich.console import Console
from rich.panel import Panel

from linguist_anki_bridge.workflows import modernization, ingestion

console = Console()

@click.group()
def cli():
    """Linguist Anki Bridge - Automate and enhance language learning with Anki and LLMs."""
    pass

@cli.command()
@click.option("--deck", required=True, help="Target Anki deck to modernize.")
@click.option("--dry-run", is_flag=True, help="Simulation mode, no changes will be made to Anki.")
def modernize(deck: str, dry_run: bool):
    """Modernize legacy flashcards by extracting text from images via OCR."""
    console.print(Panel(f"Starting modernization for deck: [bold green]{deck}[/bold green]" + (" [bold yellow](DRY RUN)[/bold yellow]" if dry_run else "")))
    modernization.run(deck=deck, dry_run=dry_run, console=console)

@cli.command()
@click.option("--words", help="Comma-separated list of words to ingest.")
@click.option("--file", type=click.File("r", encoding="utf-8"), help="File containing words (one per line). Use '-' to read from stdin.")
@click.option("--language", required=True, type=click.Choice(["en", "ja"]), help="Target language (en, ja).")
@click.option("--dry-run", is_flag=True, help="Simulation mode, no changes will be made to Anki.")
def ingest(words: str, file: click.File, language: str, dry_run: bool):
    """Ingest new vocabulary from dictionaries and LLMs."""
    word_list = []
    
    if words:
        word_list = [w.strip() for w in words.split(",") if w.strip()]
    elif file:
        word_list = [line.strip() for line in file if line.strip()]
    else:
        # Fallback to stdin if data is piped in
        if not sys.stdin.isatty():
            word_list = [line.strip() for line in sys.stdin if line.strip()]
        else:
            raise click.UsageError("You must specify either --words, --file, or pipe words via stdin.")

    if not word_list:
        console.print("[yellow]No words found to ingest.[/yellow]")
        return

    console.print(Panel(
        f"Starting ingestion for {len(word_list)} words in {language.upper()}" + 
        (" [bold yellow](DRY RUN)[/bold yellow]" if dry_run else ""),
        title="Ingestion Mode"
    ))
    
    ingestion.run(words=word_list, language=language, dry_run=dry_run, console=console)

if __name__ == "__main__":
    cli()
