from abc import ABC, abstractmethod
from typing import Optional, Dict, Any

class LLMProvider(ABC):
    @abstractmethod
    def generate_text(self, prompt: str) -> str:
        """Generates text from a prompt."""
        pass
        
    @abstractmethod
    def generate_from_image(self, prompt: str, image_base64: str) -> str:
        """Generates text from a prompt and an image."""
        pass
