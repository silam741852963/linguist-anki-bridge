import os
import yaml
import logging
from pathlib import Path
import copy

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
        "You are a helpful language learning assistant. The user provides an exact vocabulary word, "
        "authoritative parsed dictionary data, and possibly raw OCR text from dictionary screenshots. "
        "Generate usage nuances for the exact word and examples with {lang} translations. Extract every "
        "distinct usage nuance and every complete, usable example visible in dictionary-screenshot OCR. "
        "Preserve each original target-language example sentence. Translate any non-{lang} explanation "
        "or example translation into {lang}; do not copy a Vietnamese or Japanese translation into the "
        "translation field when {lang} is English. If fewer than three examples are recoverable, generate "
        "additional natural examples until there are at least three. Never discard screenshot examples "
        "merely to enforce a fixed example count.\n"
        "Never generate, summarize, translate, or rewrite dictionary definitions, readings, metadata, "
        "related entries, or senses.\n"
        "Return the response in JSON format matching this schema:\n"
        "{\n"
        "  \"nuances\": \"Short notes on usage/nuances\",\n"
        "  \"examples\": [\n"
        "    {\"sentence\": \"Example sentence in target language\", \"translation\": \"{lang} translation\"}\n"
        "  ]\n"
        "}"
    ),
    "grammar": (
        "You are an expert language teacher. The user will provide raw text extracted from a grammar textbook page or article. "
        "Extract the grammar point, explain the meaning, detail the structure/rules, and provide three example sentences with {lang} translations.\n"
        "Return the response in JSON format matching this schema:\n"
        "{\n"
        "  \"grammar_point\": \"Name of grammar pattern\",\n"
        "  \"meaning\": \"General explanation and meaning\",\n"
        "  \"rules\": \"Connection and structure rules (how to form)\",\n"
        "  \"examples\": [\n"
        "    {\"sentence\": \"Example sentence in target language\", \"translation\": \"{lang} translation\"}\n"
        "  ]\n"
        "}"
    )
}

DEFAULT_FIELD_MAPPING = {
    "expression": "Word",
    "meaning_image": "Picture",
    "meaning_text": "Gender, Personal Connection, Extra Info (Back side)",
    "kanji_construction": "Gender, Personal Connection, Extra Info (Back side)",
    "audio": "Pronunciation (Recording and/or IPA)",
}

DEFAULT_CONFIG = {
    "config_version": 2,
    "anki": {
        "url": "http://localhost:8765",
        "backup_dir": str(Path.home() / "AnkiBackups"),
    },
    "llm": {
        "ollama_url": "http://localhost:11434",
        "model": None, # Will auto-detect from /api/tags
        "translation_language": "English",
        "system_prompt_vocab": DEFAULT_SYSTEM_PROMPTS["vocabulary"],
        "system_prompt_grammar": DEFAULT_SYSTEM_PROMPTS["grammar"]
    },
    "ocr": {
        "method": "tesseract",
        "ollama_model": "llama3.2-vision",
        "ollama_url": "http://localhost:11434",
        "preprocess": True
    },
    "image_classification": {
        "decision_threshold": 0.50,
        "confirmation_margin": 0.12,
        "llm_adjudication": True,
        "llm_accept_confidence": 0.95,
        "vision_model": "llama3.2-vision",
    },
    "kanji": {
        "enabled": True,
        "source_lang": "english", # or "vietnamese"
        "url_template": "https://jisho.org/search/{char}%23kanji",
        "prompt_en": (
            "You are a Japanese language learning assistant. Summarize the Kanji construction details for the word \"{word}\" based on the following raw crawled details.\n"
            "Raw details:\n{raw_details}\n\n"
            "Requirements:\n"
            "Return ONLY a JSON object matching this schema:\n"
            "{\n"
            "  \"kanji_characters\": [\n"
            "    {\n"
            "      \"character\": \"character string\",\n"
            "      \"meanings\": [\"meaning1\", \"meaning2\"],\n"
            "      \"strokes\": 10,\n"
            "      \"radical\": \"radical string\",\n"
            "      \"parts\": [\"part1\", \"part2\"]\n"
            "    }\n"
            "  ]\n"
            "}"
        ),
        "prompt_vi": (
            "Bạn là một trợ lý học tiếng Nhật. Hãy tóm tắt cấu tạo chữ Kanji cho từ \"{word}\" dựa trên thông tin thô sau đây.\n"
            "Thông tin thô:\n{raw_details}\n\n"
            "Yêu cầu:\n"
            "Trả về DUY NHẤT một đối tượng JSON khớp với cấu trúc sau:\n"
            "{\n"
            "  \"kanji_characters\": [\n"
            "    {\n"
            "      \"character\": \"chữ Kanji\",\n"
            "      \"spell\": \"âm Hán Việt\",\n"
            "      \"strokes\": 10,\n"
            "      \"radical\": \"bộ thủ\",\n"
            "      \"parts\": [\"bộ phận cấu thành 1\", \"bộ phận cấu thành 2\"],\n"
            "      \"meanings\": [\"nghĩa chính 1\", \"nghĩa chính 2\"]\n"
            "    }\n"
            "  ]\n"
            "}"
        ),
        "schema": {
            "name": "jisho_kanji",
            "baseSelector": ".kanji",
            "fields": [
                {"name": "meanings", "selector": ".kanji-details__main-meanings", "type": "text"},
                {"name": "strokes", "selector": ".kanji-details__stroke_count strong", "type": "text"},
                {"name": "radical", "selector": ".radicals span", "type": "text"},
                {"name": "parts", "selector": ".parts a", "type": "text"}
            ]
        }
    },
    "filters": {
        "remove_parentheses": True,
        "clean_word_only": False,
    },
    "image_search": {
        "enabled_for_empty": True,
        "suffix": "",
    },
    "dictionary": {
        "preset": "jisho",
        "url_template": "",
        "schema": {},
        "retry_count": 3,
        "retry_backoff_seconds": 0.6,
        "browser_fallback": True,
    },
    "batch": {
        "max_attempts": 3,
        "retry_backoff_seconds": 5.0,
        "commit_interval_seconds": 0.25,
        "service_intervals": {
            "dictionary": 1.0,
            "ollama": 0.25,
            "kanji": 1.0,
            "image": 1.0,
            "tts": 0.5,
        },
    },
    "dry_run": True,
    "decks": {
        "japanese_vocab": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "jpn+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "japanese_grammar": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "jpn+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "english_vocab": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "english_grammar": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "taiwanese_vocab": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "chi_tra+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "taiwanese_grammar": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "chi_tra+eng+vie",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "german_vocab": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "deu+eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        },
        "german_grammar": {
            "deck_name": None,
            "note_type": "2. Picture Words",
            "ocr_langs": "deu+eng",
            "fields": DEFAULT_FIELD_MAPPING.copy()
        }
    }
}

def deep_merge(dict1, dict2):
    for k, v in dict2.items():
        if k in dict1 and isinstance(dict1[k], dict) and isinstance(v, dict):
            deep_merge(dict1[k], v)
        else:
            dict1[k] = copy.deepcopy(v)

class ConfigManager:
    def __init__(self):
        self.config = copy.deepcopy(DEFAULT_CONFIG)
        self.loaded = False
        self.load()
        
    def load(self):
        # Always rebuild from defaults. This makes reload useful for cancelling
        # edits and prevents deleted settings from lingering in memory.
        self.config = copy.deepcopy(DEFAULT_CONFIG)
        self.loaded = False
        if CONFIG_FILE.exists():
            try:
                with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                    user_config = yaml.safe_load(f)
                    if user_config:
                        user_config = self._migrate(user_config)
                        # Recursive deep merge user_config with default_config
                        deep_merge(self.config, user_config)
                        self.loaded = True
            except Exception as e:
                logging.error(f"Failed to load config: {e}")

    @staticmethod
    def _migrate(user_config: dict) -> dict:
        """Return a migrated copy of configuration from older releases."""
        migrated = copy.deepcopy(user_config)
        anki = migrated.get("anki")
        if isinstance(anki, dict):
            # Whole-deck exports are intentionally manual.  Per-card snapshots
            # now protect every write without blocking the commit path.
            anki.pop("auto_backup_before_write", None)
        decks = migrated.get("decks")
        if isinstance(decks, dict):
            for old_key in ("japanese", "english", "taiwanese", "german"):
                new_key = f"{old_key}_vocab"
                if old_key in decks and new_key not in decks:
                    decks[new_key] = decks[old_key]
                decks.pop(old_key, None)
        image_classification = migrated.get("image_classification")
        if isinstance(image_classification, dict):
            # Superseded by the calibrated binary decision threshold. Keeping
            # these keys would expose obsolete controls in the settings view.
            image_classification.pop("dictionary_threshold", None)
            image_classification.pop("visual_threshold", None)
        migrated["config_version"] = 2
        return migrated
                
    def save(self):
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                yaml.safe_dump(self.config, f, default_flow_style=False, allow_unicode=True)
            self.loaded = True
            logging.info(f"Config saved to {CONFIG_FILE}")
            return True
        except Exception as e:
            logging.error(f"Failed to save config: {e}")
            return False

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
            logging.debug("Omarchy theme file not found. Using fallback theme.")
        return fallback_theme
        
    if tomllib is None:
        if debug:
            logging.debug("tomllib/tomli not available. Cannot parse Omarchy theme. Using fallback.")
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
                logging.debug(f"Successfully loaded Omarchy theme colors from {OMARCHY_THEME_FILE}")
            return theme
    except Exception as e:
        if debug:
            logging.debug(f"Error reading Omarchy theme: {e}. Using fallback.")
        return fallback_theme
