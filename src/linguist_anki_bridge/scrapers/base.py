from abc import ABC, abstractmethod
from typing import Dict, Any

class Scraper(ABC):
    @abstractmethod
    def fetch(self, word: str) -> Dict[str, Any]:
        """Fetches definition and context for a word."""
        pass
