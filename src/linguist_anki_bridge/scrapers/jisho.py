import requests
from bs4 import BeautifulSoup
from linguist_anki_bridge.scrapers.base import Scraper
from typing import Dict, Any

class JishoScraper(Scraper):
    def fetch(self, word: str) -> Dict[str, Any]:
        """Fetches vocabulary details from Jisho.org via their API and HTML."""
        # Fast extraction using Jisho's unofficial API for exact terms
        api_url = f"https://jisho.org/api/v1/search/words?keyword={requests.utils.quote(word)}"
        response = requests.get(api_url)
        response.raise_for_status()
        
        data = response.json()
        if not data.get("data"):
             return {"word": word, "definition": "No definition found", "reading": ""}
             
        first_result = data["data"][0]
        japanese_data = first_result.get("japanese", [{}])[0]
        
        extracted_word = japanese_data.get("word", word)
        reading = japanese_data.get("reading", "")
        
        senses = first_result.get("senses", [])
        definitions = []
        for sense in senses:
            english_definitions = sense.get("english_definitions", [])
            definitions.append(", ".join(english_definitions))
            
        full_definition = " | ".join(definitions)
        
        return {
            "word": extracted_word,
            "reading": reading,
            "definition": full_definition
        }
