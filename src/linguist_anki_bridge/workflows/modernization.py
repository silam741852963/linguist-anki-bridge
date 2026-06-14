import re
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.markup import escape
from rich.prompt import Confirm

from linguist_anki_bridge.anki.client import AnkiConnectError
from linguist_anki_bridge.anki.notes import find_notes, notes_info, retrieve_media_file, update_note_fields
from linguist_anki_bridge.anki.backup import create_backup
from linguist_anki_bridge.llm.prompts import MODERNIZATION_PROMPT
from linguist_anki_bridge.validation import execute_with_retry, ValidationError
from linguist_anki_bridge.config import settings

def get_llm():
    if settings.llm_provider == "gemini":
        from linguist_anki_bridge.llm.gemini import GeminiProvider
        return GeminiProvider()
    else:
        from linguist_anki_bridge.llm.ollama import OllamaProvider
        return OllamaProvider()

def run(deck: str, dry_run: bool, console: Console) -> None:
    """Modernization workflow: processes legacy image cards into text."""
    if not dry_run:
        try:
            create_backup(console)
        except AnkiConnectError as e:
            console.print(f"[red]Backup failed: {e}. Aborting for safety.[/red]")
            return

    try:
        # Find notes that contain images in Meaning field
        query = f'deck:"{deck}" Meaning:*<img*'
        note_ids = find_notes(query)
        console.print(f"[green]Found {len(note_ids)} notes to modernize in deck '{deck}'.[/green]")
        
        if not note_ids:
            return

        infos = notes_info(note_ids)
        llm = get_llm()

        for info in infos:
            fields = info.get("fields", {})
            meaning_val = fields.get("Meaning", {}).get("value", "")
            front_val = fields.get("Front", {}).get("value", "")
            
            # Simple regex to locate the image filename
            img_match = re.search(r'<img[^>]+src="([^">]+)"', meaning_val)
            if not img_match:
                continue
                
            img_src = img_match.group(1)
            console.print(f"\n[cyan]Modernizing Note {info['noteId']} (Image: {img_src})[/cyan]")
            
            try:
                img_base64 = retrieve_media_file(img_src)
                if not img_base64:
                    console.print(f"[yellow]Could not fetch media file '{img_src}' from Anki. Skipping.[/yellow]")
                    continue
                
                # Execute with strict JSON validation & retry loop
                result = execute_with_retry(
                    llm_call_fn=lambda p: llm.generate_from_image(p, img_base64),
                    initial_prompt=MODERNIZATION_PROMPT,
                    expected_keys=["extracted_text", "meaning", "reading", "context"]
                )
                
                # Generate new formatted HTML for meaning field
                new_meaning = (
                    f"<b>{escape(result['extracted_text'])}</b> ({escape(result['reading'])})<br/>"
                    f"{escape(result['meaning'])}<br/><br/>"
                    f"<i>{escape(result['context'])}</i>"
                )
                
                # QoL feature: Create a neat comparison table
                comparison = Table(title="Proposed Card Changes", show_header=True, header_style="bold magenta")
                comparison.add_column("Field", style="dim", width=12)
                comparison.add_column("Original Value", style="red")
                comparison.add_column("Proposed Value", style="green")
                
                comparison.add_row("Front", front_val, result['extracted_text'])
                comparison.add_row("Meaning", meaning_val, new_meaning)
                
                console.print(comparison)
                
                if dry_run:
                    console.print("[yellow](DRY RUN) skipping actual update.[/yellow]")
                    continue
                    
                if Confirm.ask("Apply these updates to the card?"):
                    update_note_fields(info["noteId"], {
                        "Meaning": new_meaning, 
                        "Front": result['extracted_text']
                    })
                    console.print("[green]Anki note updated successfully![/green]")
                else:
                    console.print("[yellow]Update skipped by user.[/yellow]")
                    
            except ValidationError as e:
                console.print(f"[red]LLM Validation error: {e}[/red]")
            except Exception as e:
                console.print(f"[red]Error processing note: {e}[/red]")
                
    except AnkiConnectError as e:
        console.print(f"[red]AnkiConnect request failed: {e}[/red]")
