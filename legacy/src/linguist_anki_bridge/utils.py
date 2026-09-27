import urllib.request
import logging
import os
import datetime
import uuid
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
    import time
    status = {
        "anki": False,
        "anki_latency": -1,
        "ollama": False,
        "ollama_latency": -1,
        "jisho": False,
        "cambridge": False,
        "moedict": False,
        "dict_cc": False
    }

    # Check local AnkiConnect
    t0 = time.time()
    try:
        req = urllib.request.Request(anki_url, data=b'{"action": "version", "version": 6}', headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=2.0) as res:
            status["anki"] = (res.status == 200)
            status["anki_latency"] = int((time.time() - t0) * 1000)
    except Exception:
        status["anki"] = False
        status["anki_latency"] = -1

    # Check local Ollama
    t0 = time.time()
    try:
        req = urllib.request.Request(f"{ollama_url.rstrip('/')}/api/tags", method="GET")
        with urllib.request.urlopen(req, timeout=2.0) as res:
            status["ollama"] = (res.status == 200)
            status["ollama_latency"] = int((time.time() - t0) * 1000)
    except Exception:
        status["ollama"] = False
        status["ollama_latency"] = -1

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

import shutil

APP_NAME = "linguist-anki-bridge"

def get_app_cache_dir() -> Path:
    root = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    path = root / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path

def get_media_cache_dir() -> Path:
    path = get_app_cache_dir() / "media"
    path.mkdir(parents=True, exist_ok=True)
    return path

def get_anki_profile_dir() -> Path:
    paths = [
        Path.home() / ".local/share/Anki2/User 1",
        Path.home() / ".var/app/net.ankiweb.Anki/data/Anki2/User 1",
        Path.home() / "Anki/User 1",
    ]
    for p in paths:
        if p.exists():
            return p
    fallback = Path.home() / ".local/share/Anki2/User 1"
    fallback.mkdir(parents=True, exist_ok=True)
    return fallback

def create_deck_backup(anki_client, deck_name: str, backup_dir_str: str) -> str:
    # Create directory if not exists
    backup_dir = Path(os.path.expanduser(backup_dir_str))
    backup_dir.mkdir(parents=True, exist_ok=True)

    # Generate timestamped filename
    sanitized_deck = deck_name.replace("::", "_").replace(" ", "_")
    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    backup_file = backup_dir / f"backup_{sanitized_deck}_{timestamp}.apkg"

    # Determine safe profile directory to avoid sandbox crashes
    profile_dir = get_anki_profile_dir()
    temp_export = profile_dir / f".linguist_anki_bridge_{uuid.uuid4().hex}.apkg"

    # Run export
    try:
        logging.info(f"Exporting package to sandboxed path: {temp_export}")
        success = anki_client.export_package(deck_name, str(temp_export))
        if success and temp_export.exists():
            # Copy to user-defined backup path
            logging.info(f"Copying backup to final destination: {backup_file}")
            shutil.copy2(temp_export, backup_file)
            temp_export.unlink(missing_ok=True)
            logging.info(f"Deck '{deck_name}' backed up successfully to {backup_file}")
            return str(backup_file)
        else:
            raise RuntimeError("Export package returned False or file was not created.")
    except Exception as e:
        if temp_export.exists():
            temp_export.unlink(missing_ok=True)
        logging.error(f"Backup failed for deck '{deck_name}': {e}")
        raise RuntimeError(f"Backup failed: {e}")

def copy_to_clipboard(text: str) -> bool:
    import shutil
    import subprocess

    # Try xclip
    if shutil.which("xclip"):
        try:
            p = subprocess.Popen(["xclip", "-selection", "clipboard"], stdin=subprocess.PIPE, text=True)
            p.communicate(input=text)
            return True
        except Exception:
            pass

    # Try xsel
    if shutil.which("xsel"):
        try:
            p = subprocess.Popen(["xsel", "--clipboard", "--input"], stdin=subprocess.PIPE, text=True)
            p.communicate(input=text)
            return True
        except Exception:
            pass

    # Try wl-copy (Wayland)
    if shutil.which("wl-copy"):
        try:
            p = subprocess.Popen(["wl-copy"], stdin=subprocess.PIPE, text=True)
            p.communicate(input=text)
            return True
        except Exception:
            pass

    # Try pyperclip
    try:
        import pyperclip
        pyperclip.copy(text)
        return True
    except Exception:
        pass

    return False
