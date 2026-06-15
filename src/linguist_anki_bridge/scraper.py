import json
import re
import urllib.parse
import logging
from crawl4ai import AsyncWebCrawler, CrawlerRunConfig

class Crawl4AiScraper:
    def __init__(self):
        pass

    def _extract_json(self, markdown_text: str) -> dict:
        # Strip code block markdown if present
        cleaned = markdown_text.strip()
        if cleaned.startswith("```"):
            # Remove start block
            cleaned = re.sub(r"^```(?:json)?\n", "", cleaned)
            # Remove end block
            cleaned = re.sub(r"\n```$", "", cleaned)
        cleaned = cleaned.strip()
        try:
            return json.loads(cleaned)
        except json.JSONDecodeError as e:
            logging.error(f"Failed to decode JSON from crawled markdown: {e}\nRaw content:\n{markdown_text}")
            raise ValueError(f"Invalid JSON returned: {e}")

    async def scrape_jisho(self, word: str) -> dict:
        url = f"https://jisho.org/api/v1/search/words?keyword={urllib.parse.quote(word)}"
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            if not res.success:
                raise RuntimeError("Failed to scrape Jisho via Crawl4AI.")
            
            data = self._extract_json(res.markdown)
            results = data.get("data", [])
            if not results:
                return {"word": word, "found": False}
                
            # Parse first item
            item = results[0]
            japanese = item["japanese"][0]
            dict_word = japanese.get("word", japanese.get("reading", ""))
            reading = japanese.get("reading", "")
            senses = item["senses"][0].get("english_definitions", [])
            is_common = item.get("is_common", False)
            
            # Check if input word is conjugated
            is_conjugated = (word != dict_word) and (word != reading)
            
            return {
                "word": dict_word,
                "input_word": word,
                "found": True,
                "reading": reading,
                "definition": ", ".join(senses),
                "is_common": is_common,
                "is_conjugated": is_conjugated,
                "suggestion": dict_word if is_conjugated else None,
                "audio_url": f"https://resources.allpro.exercise/audio/{dict_word}.mp3" # Placeholder audio
            }

    async def scrape_cambridge(self, word: str) -> dict:
        url = f"https://dictionary.cambridge.org/dictionary/english/{urllib.parse.quote(word)}"
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            
            # Search for definitions in Cambridge markdown
            markdown = res.markdown if res.success else ""
            
            # Simple regex search for definition pattern
            def_match = re.search(r"to want something very much|definition of [^\n]+ in English", markdown, re.IGNORECASE)
            
            # If Cambridge failed or blocked, use DictionaryAPI fallback
            if not res.success or "blocked" in markdown.lower() or not def_match:
                logging.info("Cambridge blocked or definition not found. Falling back to DictionaryAPI...")
                fallback_url = f"https://api.dictionaryapi.dev/api/v2/entries/en/{urllib.parse.quote(word)}"
                fallback_res = await crawler.arun(url=fallback_url, config=config)
                if not fallback_res.success:
                    return {"word": word, "found": False}
                    
                try:
                    data = self._extract_json(fallback_res.markdown)
                    if not data or not isinstance(data, list):
                        return {"word": word, "found": False}
                    
                    entry = data[0]
                    dict_word = entry.get("word", word)
                    meanings = entry.get("meanings", [])
                    definition = meanings[0]["definitions"][0]["definition"] if meanings else ""
                    phonetic = entry.get("phonetic", "")
                    
                    # Extract audio URL
                    audio_url = None
                    for phon in entry.get("phonetics", []):
                        if phon.get("audio"):
                            audio_url = phon["audio"]
                            break
                            
                    return {
                        "word": dict_word,
                        "input_word": word,
                        "found": True,
                        "reading": phonetic,
                        "definition": definition,
                        "audio_url": audio_url
                    }
                except Exception as e:
                    logging.error(f"Fallback parse failed: {e}")
                    return {"word": word, "found": False}

            # Parse Cambridge markdown
            # Locate IPA /.../
            ipa_match = re.search(r"/([^/]+)/", markdown)
            ipa = ipa_match.group(0) if ipa_match else ""
            
            # Locate audio links
            audio_match = re.search(r"https://dictionary\.cambridge\.org/external/images/[^\s)]+\.mp3", markdown)
            audio_url = audio_match.group(0) if audio_match else None
            
            # Try to extract a clean definition
            # Usually definitions start after the POS (e.g. noun, verb)
            lines = markdown.split("\n")
            definition = "Definition found on Cambridge Dictionary."
            for line in lines:
                if ":" in line and len(line) > 20 and not line.startswith("http"):
                    definition = line.strip()
                    break

            return {
                "word": word,
                "input_word": word,
                "found": True,
                "reading": ipa,
                "definition": definition,
                "audio_url": audio_url
            }

    async def scrape_moedict(self, word: str, dialect="t") -> dict:
        # dialect "t" is Hokkien, "a" is Mandarin
        url = f"https://www.moedict.tw/{dialect}/{urllib.parse.quote(word)}.json"
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            if not res.success or "not found" in res.markdown.lower():
                return {"word": word, "found": False}
                
            try:
                data = self._extract_json(res.markdown)
                heteronyms = data.get("h", [])
                if not heteronyms:
                    return {"word": word, "found": False}
                    
                first_het = heteronyms[0]
                reading = first_het.get("T", first_het.get("p", "")) # Pinyin or Hokkien reading
                definitions = []
                for d in first_het.get("d", []):
                    def_text = d.get("f", "")
                    if def_text:
                        definitions.append(def_text)
                
                # Audio ID
                audio_id = first_het.get("_")
                audio_url = None
                if audio_id:
                    # Construct rackcdn link or direct moedict mp3 link
                    if dialect == "t":
                        audio_url = f"https://t.moedict.tw/mp3/{audio_id}.mp3"
                    else:
                        audio_url = f"https://203146b5091e8f0aafda-15d8553a928a30eef40a64ebd36ed408.ssl.cf2.rackcdn.com/{audio_id}.mp3"

                return {
                    "word": data.get("t", word),
                    "input_word": word,
                    "found": True,
                    "reading": reading,
                    "definition": "; ".join(definitions),
                    "audio_url": audio_url
                }
            except Exception as e:
                logging.error(f"MoeDict parse failed: {e}")
                return {"word": word, "found": False}

    async def scrape_dict_cc(self, word: str) -> dict:
        url = f"https://www.dict.cc/?s={urllib.parse.quote(word)}"
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            markdown = res.markdown if res.success else ""
            
            if not res.success or "no translations found" in markdown.lower() or "not found" in markdown.lower():
                return {"word": word, "found": False}
                
            # Extracted translations
            # Try to grab translation lines
            translations = []
            lines = markdown.split("\n")
            for line in lines:
                if "|" in line and word.lower() in line.lower() and not line.startswith("["):
                    translations.append(line.strip())
            
            definition = "; ".join(translations[:5]) if translations else "Translation found on dict.cc."
            
            return {
                "word": word,
                "input_word": word,
                "found": True,
                "reading": "",
                "definition": definition,
                "audio_url": None
            }

    async def scrape_custom_url(self, url: str) -> str:
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            if not res.success:
                raise RuntimeError(f"Failed to crawl custom URL: {url}")
            return res.markdown
