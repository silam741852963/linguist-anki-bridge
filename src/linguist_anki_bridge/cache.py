import os
import json
from linguist_anki_bridge.config import settings

class DictionaryCache:
    def __init__(self):
        # Safe standard UNIX user local share path
        data_dir = os.path.expanduser("~/.local/share/linguist-anki-bridge")
        try:
            os.makedirs(data_dir, exist_ok=True)
            self.cache_path = os.path.join(data_dir, settings.cache_file)
        except Exception:
            # Fallback to current directory if home path is read-only or error
            self.cache_path = settings.cache_file
            
        self.cache = self._load()

    def _load(self) -> dict:
        if os.path.exists(self.cache_path):
            try:
                with open(self.cache_path, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                return {}
        return {}

    def get(self, language: str, word: str) -> dict | None:
        key = f"{language}:{word.lower().strip()}"
        return self.cache.get(key)

    def set(self, language: str, word: str, data: dict) -> None:
        key = f"{language}:{word.lower().strip()}"
        self.cache[key] = data
        self._save()

    def _save(self) -> None:
        try:
            with open(self.cache_path, "w", encoding="utf-8") as f:
                json.dump(self.cache, f, ensure_ascii=False, indent=2)
        except Exception:
            pass
