import os
import yaml
import logging
from pathlib import Path

# Use tomllib if Python 3.11+, otherwise use tomli
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib
    except ImportError:
        tomllib = None

CONFIG_DIR = Path.home() / ".config" / "linguist-anki-bridge"
CONFIG_FILE = CONFIG_DIR / "config.yaml"
OMARCHY_THEME_FILE = Path.home() / ".config" / "omarchy" / "current" / "theme" / "colors.toml"

DEFAULT_SYSTEM_PROMPTS = {
    "vocabulary": (
        "You are a helpful language learning assistant. The user will provide a vocabulary word, "
        "and potentially a contextual note. Extract the meaning and generate three high-quality "
        "example sentences in the target language with English/Vietnamese translations.\n"
        "If a contextual note is provided, tailor the meaning and examples to that context.\n"
        "Return the response in JSON format matching this schema:\n"
        "{\n"
        "  \"definition\": \"Main translation and explanation\",\n"
        "  \"nuances\": \"Short notes on usage/nuances\",\n"
        "  \"examples\": [\n"
        "    {\"sentence\": \"Example sentence in target language\", \"translation\": \"English or Vietnamese translation\"}\n"
        "  ]\n"
        "}"
    ),
    "grammar": (
        "You are an expert language teacher. The user will provide raw text extracted from a grammar textbook page or article. "
        "Extract the grammar point, explain the meaning, detail the structure/rules, and provide three example sentences with translations.\n"
        "Return the response in JSON format matching this schema:\n"
        "{\n"
        "  \"grammar_point\": \"Name of grammar pattern\",\n"
        "  \"meaning\": \"General explanation and meaning\",\n"
        "  \"rules\": \"Connection and structure rules (how to form)\",\n"
        "  \"examples\": [\n"
        "    {\"sentence\": \"Example sentence in target language\", \"translation\": \"English or Vietnamese translation\"}\n"
        "  ]\n"
        "}"
    )
}

DEFAULT_FIELD_MAPPING = {
    "expression": "Word",
    "meaning_image": "Picture",
    "meaning_text": "Gender, Personal Connection, Extra Info (Back side)",
    "audio": "Pronunciation (Recording and/or IPA)",
}

DEFAULT_CONFIG = {
    "anki": {
        "url": "http://localhost:8765",
        "backup_dir": str(Path.home() / "AnkiBackups")
    },
    "llm": {
        "ollama_url": "http://localhost:11434",
        "model": None, # Will auto-detect from /api/tags
        "system_prompt_vocab": DEFAULT_SYSTEM_PROMPTS["vocabulary"],
        "system_prompt_grammar": DEFAULT_SYSTEM_PROMPTS["grammar"]
    },
    "ocr": {
        "method": "tesseract",
        "ollama_model": "llama3.2-vision",
        "ollama_url": "http://localhost:11434",
        "preprocess": True
    },
    "dry_run": True,
    "decks": {
        "japanese": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "jpn+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "english": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "taiwanese": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "chi_tra+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "german": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "deu+eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        }
    }
}

class ConfigManager:
    def __init__(self):
        self.config = DEFAULT_CONFIG.copy()
        self.loaded = False
        self.load()
        
    def load(self):
        if CONFIG_FILE.exists():
            try:
                with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                    user_config = yaml.safe_load(f)
                    if user_config:
                        # Deep merge user_config with default_config
                        for k, v in user_config.items():
                            if isinstance(v, dict) and k in self.config:
                                self.config[k].update(v)
                            else:
                                self.config[k] = v
                        self.loaded = True
            except Exception as e:
                logging.error(f"Failed to load config: {e}")
                
    def save(self):
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                yaml.safe_dump(self.config, f, default_flow_style=False, allow_unicode=True)
            self.loaded = True
            logging.info(f"Config saved to {CONFIG_FILE}")
        except Exception as e:
            logging.error(f"Failed to save config: {e}")

    def is_setup_completed(self) -> bool:
        # User must set up at least one language deck
        for lang, spec in self.config.get("decks", {}).items():
            if spec.get("deck_name"):
                return True
        return False

def load_omarchy_theme(debug=False) -> dict:
    fallback_theme = {
        "background": "#121212",
        "foreground": "#bebebe",
        "accent": "#e68e0d",
        "selection_background": "#333333",
        "active_border_color": "#595959",
        "active_tab_background": "#121212",
    }
    
    if not OMARCHY_THEME_FILE.exists():
        if debug:
            print("[DEBUG] Omarchy theme file not found. Using fallback theme.")
        return fallback_theme
        
    if tomllib is None:
        if debug:
            print("[DEBUG] tomllib/tomli not available. Cannot parse Omarchy theme. Using fallback.")
        return fallback_theme
        
    try:
        with open(OMARCHY_THEME_FILE, "rb") as f:
            theme_data = tomllib.load(f)
            # Extracted required colors with fallbacks
            theme = {
                "background": theme_data.get("background", fallback_theme["background"]),
                "foreground": theme_data.get("foreground", fallback_theme["foreground"]),
                "accent": theme_data.get("accent", fallback_theme["accent"]),
                "selection_background": theme_data.get("selection_background", fallback_theme["selection_background"]),
                "active_border_color": theme_data.get("active_border_color", fallback_theme["active_border_color"]),
                "active_tab_background": theme_data.get("active_tab_background", fallback_theme["active_tab_background"]),
            }
            if debug:
                print(f"[DEBUG] Successfully loaded Omarchy theme colors from {OMARCHY_THEME_FILE}")
            return theme
    except Exception as e:
        if debug:
            print(f"[DEBUG] Error reading Omarchy theme: {e}. Using fallback.")
        return fallback_theme
