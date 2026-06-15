import io
import base64
import logging
from gtts import gTTS

# Map our internal deck language keys to gTTS language codes
LANG_MAP = {
    "japanese": "ja",
    "english": "en",
    "taiwanese": "zh-TW", # Taiwanese Mandarin fallback
    "german": "de"
}

def generate_tts_base64(text: str, language_key: str) -> str:
    gtts_lang = LANG_MAP.get(language_key.lower(), "en")
    try:
        # Create gTTS in-memory
        tts = gTTS(text=text, lang=gtts_lang)
        fp = io.BytesIO()
        tts.write_to_fp(fp)
        fp.seek(0)
        audio_bytes = fp.read()
        
        # Base64 encode and decode to string
        return base64.b64encode(audio_bytes).decode("utf-8")
    except Exception as e:
        logging.error(f"gTTS audio generation failed for '{text}' ({language_key}): {e}")
        raise RuntimeError(f"Failed to generate TTS audio: {e}")
