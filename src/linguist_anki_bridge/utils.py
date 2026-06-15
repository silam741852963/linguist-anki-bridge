import urllib.request
import logging
import os
import datetime
from pathlib import Path
from urllib.error import URLError

def check_url_status(url: str, timeout=2.0) -> bool:
    try:
        # Use HEAD request if possible, or simple GET
        req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return res.status in (200, 301, 302, 405) # 405 is fine for HEAD
    except URLError:
        return False
    except Exception:
        # Retry with GET in case HEAD is not allowed
        try:
            req = urllib.request.Request(url, method="GET", headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=timeout) as res:
                return res.status in (200, 301, 302)
        except Exception:
            return False

def check_health(anki_url="http://localhost:8765", ollama_url="http://localhost:11434") -> dict:
    # Quick health check endpoints
    status = {
        "anki": False,
        "ollama": False,
        "jisho": False,
        "cambridge": False,
        "moedict": False,
        "dict_cc": False
    }
    
    # Check local
    try:
        # Check AnkiConnect
        req = urllib.request.Request(anki_url, data=b'{"action": "version", "version": 6}', headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=2.0) as res:
            status["anki"] = (res.status == 200)
    except Exception:
        status["anki"] = False
        
    try:
        # Check Ollama
        req = urllib.request.Request(f"{ollama_url.rstrip('/')}/api/tags", method="GET")
        with urllib.request.urlopen(req, timeout=2.0) as res:
            status["ollama"] = (res.status == 200)
    except Exception:
        status["ollama"] = False

    # Check websites
    status["jisho"] = check_url_status("https://jisho.org")
    status["cambridge"] = check_url_status("https://dictionary.cambridge.org")
    status["moedict"] = check_url_status("https://www.moedict.tw")
    status["dict_cc"] = check_url_status("https://www.dict.cc")
    
    return status

def setup_logging(debug=False):
    log_dir = Path.home() / ".config" / "linguist-anki-bridge"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_file = log_dir / "app.log"
    
    level = logging.DEBUG if debug else logging.INFO
    
    logging.basicConfig(
        level=level,
        format="%(asctime)s [%(levelname)s] %(message)s",
        handlers=[
            logging.FileHandler(log_file, encoding="utf-8"),
            logging.StreamHandler()
        ]
    )
    logging.info("Logging initialized.")

def create_deck_backup(anki_client, deck_name: str, backup_dir_str: str) -> str:
    # Create directory if not exists
    backup_dir = Path(os.path.expanduser(backup_dir_str))
    backup_dir.mkdir(parents=True, exist_ok=True)
    
    # Generate timestamped filename
    sanitized_deck = deck_name.replace("::", "_").replace(" ", "_")
    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_file = backup_dir / f"backup_{sanitized_deck}_{timestamp}.apkg"
    
    # Run export
    try:
        success = anki_client.export_package(deck_name, str(backup_file))
        if success:
            logging.info(f"Deck '{deck_name}' backed up to {backup_file}")
            return str(backup_file)
        else:
            raise RuntimeError("Export package returned False.")
    except Exception as e:
        logging.error(f"Backup failed for deck '{deck_name}': {e}")
        raise RuntimeError(f"Backup failed: {e}")
