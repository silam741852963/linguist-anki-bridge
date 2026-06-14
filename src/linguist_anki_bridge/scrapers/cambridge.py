import requests
from bs4 import BeautifulSoup
from linguist_anki_bridge.scrapers.base import Scraper
from typing import Dict, Any

class CambridgeScraper(Scraper):
    def fetch(self, word: str) -> Dict[str, Any]:
        """Fetches English vocabulary details from Cambridge Dictionary."""
        url = f"https://dictionary.cambridge.org/dictionary/english/{requests.utils.quote(word)}"
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
        }
        
        response = requests.get(url, headers=headers)
        if response.status_code != 200:
            return {"word": word, "definition": "No definition found", "reading": ""}
            
        soup = BeautifulSoup(response.content, "html.parser")
        
        # Cambridge typical structure:
        # definition class: ddef_d 
        # pronunciation class: ipa
        
        def_block = soup.find(class_="ddef_d")
        definition = def_block.text.strip() if def_block else "No definition found"
        
        ipa_block = soup.find(class_="ipa")
        reading = ipa_block.text.strip() if ipa_block else ""
        
        return {
            "word": word,
            "reading": reading,
            "definition": definition
        }
