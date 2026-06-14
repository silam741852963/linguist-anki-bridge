from typing import Optional
from pydantic_settings import BaseSettings, SettingsConfigDict

class Settings(BaseSettings):
    anki_connect_url: str = "http://localhost:8765"
    
    # LLM Settings
    llm_provider: str = "ollama"  # "ollama" or "gemini"
    gemini_api_key: Optional[str] = None
    
    # Ollama Settings
    ollama_url: str = "http://localhost:11434"
    ollama_model: str = "gemma4:e4b"
    ollama_vision_model: str = "llava"  # Default vision model for OCR
    
    # Anki specific settings
    default_deck: str = "Default"
    default_note_type: str = "Basic"
    
    # Backup Settings
    max_backups: int = 5
    
    # Cache Settings
    cache_file: str = "dict_cache.json"
    
    model_config = SettingsConfigDict(env_file='.env', env_file_encoding='utf-8', extra='ignore')

settings = Settings()
