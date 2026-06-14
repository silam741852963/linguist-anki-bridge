import os
import glob
from datetime import datetime
from linguist_anki_bridge.anki.client import invoke
from linguist_anki_bridge.config import settings
from rich.console import Console

def create_backup(console: Console) -> str:
    """Exports the entire Anki collection for backup purposes and rotates old backups."""
    # Standard user local share directory for safety
    backup_dir = os.path.expanduser("~/.local/share/linguist-anki-bridge/backups")
    try:
        os.makedirs(backup_dir, exist_ok=True)
    except Exception:
        # Fallback if home directory is not writable
        backup_dir = "backups"
        os.makedirs(backup_dir, exist_ok=True)
        
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_file = os.path.abspath(os.path.join(backup_dir, f"anki_backup_{timestamp}.colpkg"))
    
    console.print(f"[cyan]Creating Anki collection backup...[/cyan]")
    invoke("exportPackage", deck="*", path=backup_file, includeSched=True)
    console.print(f"[green]Backup created successfully at {backup_file}[/green]")
    
    # Rotation logic: Keep only the max_backups count
    try:
        pattern = os.path.join(backup_dir, "anki_backup_*.colpkg")
        backups = sorted(glob.glob(pattern))
        if len(backups) > settings.max_backups:
            excess = len(backups) - settings.max_backups
            for i in range(excess):
                os.remove(backups[i])
                console.print(f"[dim]Removed oldest backup: {os.path.basename(backups[i])}[/dim]")
    except Exception as e:
        console.print(f"[yellow]Failed to rotate backups: {e}[/yellow]")
        
    return backup_file
