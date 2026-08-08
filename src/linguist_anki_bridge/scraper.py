import json
import re
import base64
import html
import urllib.parse
import urllib.request
import asyncio
import logging
from bs4 import BeautifulSoup
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

    @staticmethod
    def _validate_schema(schema: dict) -> None:
        if not isinstance(schema, dict) or not schema.get("baseSelector"):
            raise ValueError("Extraction schema requires a baseSelector")
        fields = schema.get("fields")
        if not isinstance(fields, list) or not fields:
            raise ValueError("Extraction schema requires at least one field")
        for field in fields:
            if not isinstance(field, dict) or not field.get("name") or not field.get("selector"):
                raise ValueError("Each extraction field requires name and selector")

    @staticmethod
    def _display_jisho_tag(tag: str) -> str:
        match = re.fullmatch(r"wanikani(\d+)", tag or "", re.IGNORECASE)
        if match:
            return f"Wanikani level {match.group(1)}"
        return str(tag).replace("_", " ").strip()

    @classmethod
    def parse_jisho_response(cls, word: str, data: dict) -> dict:
        """Preserve every Jisho result, written form, and sense for LLM context."""
        results = data.get("data", [])
        if not results:
            return {"word": word, "input_word": word, "found": False, "entries": []}

        entries = []
        readable_entries = []
        for result in results:
            forms = []
            for form in result.get("japanese", []):
                normalized = {
                    "word": form.get("word", ""),
                    "reading": form.get("reading", ""),
                }
                if normalized["word"] or normalized["reading"]:
                    forms.append(normalized)
            if not forms:
                continue

            primary = forms[0]
            entry_word = primary.get("word") or primary.get("reading") or word
            reading = primary.get("reading", "")
            tags = [cls._display_jisho_tag(tag) for tag in result.get("tags", []) if tag]
            jlpt = [str(level).upper().replace("JLPT-", "JLPT ") for level in result.get("jlpt", [])]
            senses = []
            sense_lines = []
            for number, sense in enumerate(result.get("senses", []), 1):
                definitions = [str(value) for value in sense.get("english_definitions", []) if value]
                # Keep Wikipedia-derived definitions and entries, but present them like
                # ordinary dictionary senses instead of exposing Jisho's source label.
                parts_of_speech = [
                    str(value)
                    for value in sense.get("parts_of_speech", [])
                    if value and "wikipedia definition" not in str(value).lower()
                ]
                if not definitions:
                    continue
                normalized_sense = {
                    "number": number,
                    "definitions": definitions,
                    "parts_of_speech": parts_of_speech,
                    "tags": [str(value) for value in sense.get("tags", []) if value],
                    "see_also": [str(value) for value in sense.get("see_also", []) if value],
                    "antonyms": [str(value) for value in sense.get("antonyms", []) if value],
                    "info": [str(value) for value in sense.get("info", []) if value],
                    "restrictions": [str(value) for value in sense.get("restrictions", []) if value],
                }
                senses.append(normalized_sense)
                labels = normalized_sense["parts_of_speech"] + normalized_sense["tags"]
                line = f"{number}. "
                if labels:
                    line += f"[{' · '.join(labels)}] "
                line += "; ".join(definitions)
                if normalized_sense["see_also"]:
                    line += f" — See also: {', '.join(normalized_sense['see_also'])}"
                if normalized_sense["info"]:
                    line += f" — {'; '.join(normalized_sense['info'])}"
                sense_lines.append(line)

            if not senses:
                continue

            attribution = result.get("attribution", {})
            entry = {
                "word": entry_word,
                "reading": reading,
                "forms": forms,
                "is_common": bool(result.get("is_common", False)),
                "jlpt": jlpt,
                "tags": tags,
                "senses": senses,
                "attribution": {
                    key: attribution.get(key)
                    for key in ("jmdict", "jmnedict")
                    if key in attribution
                },
            }
            entries.append(entry)

            heading = " / ".join(part for part in (reading, entry_word) if part)
            metadata = (["Common word"] if entry["is_common"] else []) + jlpt + tags
            readable = heading
            if metadata:
                readable += f"\n{' · '.join(metadata)}"
            if sense_lines:
                readable += "\n" + "\n".join(sense_lines)
            readable_entries.append(readable)

        if not entries:
            return {"word": word, "input_word": word, "found": False, "entries": []}

        first = entries[0]
        is_conjugated = word not in {
            value
            for form in first["forms"]
            for value in (form.get("word"), form.get("reading"))
            if value
        }
        full_definition = "\n\n".join(readable_entries)
        return {
            "word": first["word"],
            "input_word": word,
            "found": True,
            "reading": first["reading"],
            "definition": full_definition,
            "entries": entries,
            "llm_context": json.dumps(entries, ensure_ascii=False, indent=2),
            "is_common": first["is_common"],
            "is_conjugated": is_conjugated,
            "suggestion": first["word"] if is_conjugated else None,
            "audio_url": None,
        }

    @classmethod
    def parse_jisho_html(cls, word: str, html_text: str) -> dict:
        """Convert Jisho's rendered search results into the API-shaped model."""
        soup = BeautifulSoup(html_text or "", "html.parser")
        api_results = []
        for concept in soup.select(".concept_light"):
            representation = concept.select_one(".concept_light-representation")
            written_node = representation.select_one(".text") if representation else None
            reading_node = representation.select_one(".furigana") if representation else None
            written = "".join(written_node.stripped_strings) if written_node else ""
            reading = "".join(reading_node.stripped_strings) if reading_node else ""
            japanese = [{"word": written, "reading": reading}] if written or reading else []
            senses = []
            for meaning in concept.select(".meaning-wrapper"):
                definition_node = meaning.select_one(".meaning-meaning")
                definitions = [definition_node.get_text(" ", strip=True)] if definition_node else []
                if not definitions:
                    continue
                tag_node = meaning.select_one(".meaning-tags")
                parts = [tag_node.get_text(" ", strip=True)] if tag_node else []
                supplemental = meaning.select_one(".supplemental_info")
                info = [supplemental.get_text(" ", strip=True)] if supplemental else []
                senses.append({
                    "english_definitions": definitions,
                    "parts_of_speech": parts,
                    "info": info,
                    "tags": [], "see_also": [], "antonyms": [], "restrictions": [],
                })
            if not japanese or not senses:
                continue
            visible_tags = [node.get_text(" ", strip=True) for node in concept.select(".concept_light-tag")]
            api_results.append({
                "japanese": japanese,
                "senses": senses,
                "is_common": bool(concept.select_one(".concept_light-common")) or any("Common" in tag for tag in visible_tags),
                "jlpt": [tag.lower().replace(" ", "-") for tag in visible_tags if "JLPT" in tag.upper()],
                "tags": [tag.lower().replace(" ", "") for tag in visible_tags if "Wanikani" in tag],
                "attribution": {"jmdict": True},
            })
        return cls.parse_jisho_response(word, {"data": api_results})

    async def _scrape_jisho_browser(self, word: str) -> dict:
        from crawl4ai import CacheMode
        api_url = f"https://jisho.org/api/v1/search/words?keyword={urllib.parse.quote(word)}"
        config = CrawlerRunConfig(cache_mode=CacheMode.BYPASS, page_timeout=20000)
        async with AsyncWebCrawler() as crawler:
            # Try the JSON endpoint through Chromium first. Some edge failures
            # differ between urllib and a browser request.
            response = await crawler.arun(url=api_url, config=config)
            if response.success:
                candidates = [getattr(response, "markdown", ""), BeautifulSoup(getattr(response, "html", ""), "html.parser").get_text()]
                for candidate in candidates:
                    if not isinstance(candidate, str) or not candidate.strip():
                        continue
                    try:
                        data = self._extract_json(candidate)
                        parsed = self.parse_jisho_response(word, data)
                        if parsed.get("found"):
                            return parsed
                    except (ValueError, TypeError):
                        continue

            search_url = f"https://jisho.org/search/{urllib.parse.quote(word)}"
            response = await crawler.arun(url=search_url, config=config)
            if not response.success:
                raise RuntimeError(f"Crawl4AI could not render {search_url}")
            parsed = self.parse_jisho_html(word, getattr(response, "html", ""))
            if not parsed.get("found"):
                raise RuntimeError("Crawl4AI rendered Jisho but extracted no dictionary entries")
            return parsed

    async def _fetch_bytes(self, url: str, timeout: float = 15.0) -> bytes:
        req = urllib.request.Request(url, headers={"User-Agent": "LinguistAnkiBridge/0.1"})
        return await asyncio.get_running_loop().run_in_executor(
            None, lambda: urllib.request.urlopen(req, timeout=timeout).read()
        )

    async def scrape_jisho(self, word: str, retry_count: int = 3, backoff: float = 0.6, browser_fallback: bool = True) -> dict:
        url = f"https://jisho.org/api/v1/search/words?keyword={urllib.parse.quote(word)}"
        attempts = max(1, min(5, int(retry_count)))
        last_error = None
        for attempt in range(1, attempts + 1):
            req = urllib.request.Request(
                url,
                headers={
                    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) LinguistAnkiBridge/0.1",
                    "Accept": "application/json",
                },
            )
            try:
                raw = await asyncio.get_running_loop().run_in_executor(
                    None, lambda: urllib.request.urlopen(req, timeout=10).read()
                )
                return self.parse_jisho_response(word, json.loads(raw.decode("utf-8")))
            except Exception as exc:
                last_error = exc
                logging.warning("Jisho request %d/%d failed for %r: %s", attempt, attempts, word, exc)
                if attempt < attempts:
                    await asyncio.sleep(max(0.0, float(backoff)) * attempt)
        if browser_fallback:
            logging.info("Direct Jisho requests exhausted; trying Crawl4AI browser fallback for %r", word)
            try:
                return await self._scrape_jisho_browser(word)
            except Exception as browser_error:
                raise RuntimeError(
                    f"Failed to query Jisho after {attempts} direct attempts and Crawl4AI fallback: {browser_error}"
                ) from browser_error
        raise RuntimeError(f"Failed to query Jisho after {attempts} attempts: {last_error}") from last_error

    async def scrape_cambridge(self, word: str) -> dict:
        url = f"https://dictionary.cambridge.org/dictionary/english/{urllib.parse.quote(word)}"
        config = CrawlerRunConfig(page_timeout=15000)
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)

            # Search for definitions in Cambridge markdown
            markdown = res.markdown if res.success else ""

            # If Cambridge failed or blocked, use DictionaryAPI fallback
            blocked = any(marker in markdown.lower() for marker in ("access denied", "captcha", "temporarily blocked"))
            if not res.success or blocked or len(markdown.strip()) < 200:
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
            definition = ""
            for line in lines:
                clean_line = line.strip(" -*#\t")
                if 20 < len(clean_line) < 500 and not clean_line.startswith("http") and word.lower() not in clean_line.lower():
                    definition = clean_line
                    break

            if not definition:
                return {"word": word, "found": False}

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
        req = urllib.request.Request(url, headers={"User-Agent": "LinguistAnkiBridge/0.1"})
        try:
            raw = await asyncio.get_running_loop().run_in_executor(
                None, lambda: urllib.request.urlopen(req, timeout=10).read()
            )
            data = json.loads(raw.decode("utf-8"))
        except Exception as exc:
            logging.info(f"MoeDict lookup failed for '{word}': {exc}")
            return {"word": word, "found": False}

        try:
            heteronyms = data.get("h", [])
            if not heteronyms:
                return {"word": word, "found": False}

            first_het = heteronyms[0]
            reading = first_het.get("T", first_het.get("p", ""))
            definitions = [d.get("f", "") for d in first_het.get("d", []) if d.get("f")]
            audio_id = first_het.get("_")
            audio_url = None
            if audio_id:
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
                "audio_url": audio_url,
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

            if not translations:
                return {"word": word, "found": False}
            definition = "; ".join(translations[:5])

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

    @staticmethod
    def _stroke_order_gif_url(char: str) -> str:
        return (
            "https://raw.githubusercontent.com/mistval/kanji_images/master/gifs/"
            f"{ord(char):x}.gif"
        )

    async def _stroke_order_image(self, char: str) -> str:
        url = self._stroke_order_gif_url(char)
        try:
            image = await self._fetch_bytes(url)
            if image[:6] not in (b"GIF87a", b"GIF89a"):
                raise ValueError("stroke-order asset is not a GIF")
            src = f"data:image/gif;base64,{base64.b64encode(image).decode('ascii')}"
        except Exception as exc:
            logging.warning("Could not embed stroke-order GIF for %s: %s", char, exc)
            src = url
        return (
            "<div style='margin:6px 0;padding-bottom:8px;border-bottom:1px dashed #888'>"
            f"<b>Stroke order:</b><br/><img src='{html.escape(src, quote=True)}' "
            f"alt='Stroke order for {html.escape(char)}' style='max-width:220px'/></div>"
        )

    @staticmethod
    def _kanji_section(char: str, gif: str, rows: list[str]) -> str:
        return (
            f"<section data-kanji='{html.escape(char, quote=True)}' "
            "style='margin:10px 0;border:1px solid #888;border-radius:6px;overflow:hidden'>"
            "<div style='padding:7px 9px;border-bottom:2px solid #888;background:#f3f3f3'>"
            f"<b>Kanji {html.escape(char)}</b></div>"
            f"<div style='padding:2px 9px 9px'>{gif}"
            + "".join(rows)
            + "</div></section>"
        )

    @staticmethod
    def _clean_lines(node) -> list[str]:
        if node is None:
            return []
        fragment = BeautifulSoup(str(node), "html.parser")
        marker = "___LINGUIST_LINE_BREAK___"
        for br in fragment.find_all("br"):
            br.replace_with(marker)
        # A space separator keeps inline spans together (e.g. "Âm Hán Việt: bài,
        # bồi" and ideographic components), while explicit <br> markers retain the
        # source's intended definition lines.
        text = fragment.get_text(" ", strip=True)
        lines = []
        for line in text.split(marker):
            cleaned = re.sub(r"\s+", " ", line).strip()
            cleaned = re.sub(r"\s+([,.;:])", r"\1", cleaned)
            if cleaned:
                lines.append(cleaned)
        return lines

    async def _scrape_jisho_kanji(self, char: str, url: str) -> str:
        soup = BeautifulSoup((await self._fetch_bytes(url)).decode("utf-8", "replace"), "html.parser")
        details = soup.select_one(".kanji.details") or soup.select_one(".kanji")
        if details is None:
            return ""

        meanings = " ".join(self._clean_lines(details.select_one(".kanji-details__main-meanings")))
        strokes = " ".join(self._clean_lines(details.select_one(".kanji-details__stroke_count strong")))
        readings = []
        for group in details.select(".kanji-details__main-readings dl"):
            label = " ".join(self._clean_lines(group.find("dt"))).rstrip(":")
            value = " ".join(self._clean_lines(group.find("dd")))
            if value:
                readings.append(f"{label}: {value}" if label else value)

        radical = ""
        parts = ""
        for term in details.select(".radicals dt"):
            label = " ".join(self._clean_lines(term)).lower()
            value = " ".join(self._clean_lines(term.find_next_sibling("dd")))
            if "radical" in label:
                radical = value
            elif "parts" in label:
                parts = value

        rows = []
        for label, value in (
            ("Meanings", meanings), ("Readings", " · ".join(readings)),
            ("Stroke count", strokes), ("Radical", radical), ("Parts", parts),
        ):
            if value:
                rows.append(f"<div style='padding:3px 0;border-bottom:1px dotted #aaa'><b>{label}:</b> {html.escape(value)}</div>")
        gif = await self._stroke_order_image(char)
        return self._kanji_section(char, gif, rows)

    async def _scrape_kanjiapi_kanji(self, char: str, url: str) -> str:
        """Reliable KANJIDIC-backed fallback when Jisho HTML is unavailable."""
        payload = json.loads((await self._fetch_bytes(url)).decode("utf-8"))
        rows = []
        values = (
            ("Meanings", ", ".join(payload.get("meanings") or [])),
            ("On readings", " · ".join(payload.get("on_readings") or [])),
            ("Kun readings", " · ".join(payload.get("kun_readings") or [])),
            ("Name readings", " · ".join(payload.get("name_readings") or [])),
            ("Stroke count", payload.get("stroke_count")),
            ("JLPT", payload.get("jlpt")),
            ("Grade", payload.get("grade")),
        )
        for label, value in values:
            if value not in (None, "", []):
                rows.append(
                    "<div style='padding:3px 0;border-bottom:1px dotted #aaa'>"
                    f"<b>{label}:</b> {html.escape(str(value))}</div>"
                )
        gif = await self._stroke_order_image(char)
        return self._kanji_section(char, gif, rows)

    async def _scrape_hvdic_kanji(self, char: str, url: str) -> str:
        soup = BeautifulSoup((await self._fetch_bytes(url)).decode("utf-8", "replace"), "html.parser")
        rows = []

        # The first block contains construction metadata. Exclude the animation widget's
        # internal labels, then retain every meaningful line instead of truncating to four.
        main = soup.select_one("div.hvres.han-word .hvres-details .hvres-meaning")
        if main:
            animation = main.select_one(".hvres-animation")
            if animation:
                animation.decompose()
            for line in self._clean_lines(main):
                if not line.startswith(("Tự hình", "Dị thể")):
                    rows.append(f"<div style='padding:3px 0;border-bottom:1px dotted #aaa'>{html.escape(line)}</div>")

        # Only definition result blocks have data-hvres-idx. This avoids unrelated poems,
        # character-shape galleries, and navigation blocks that share the .hvres class.
        for result in soup.select("div.hvres[data-hvres-idx]"):
            spell = " ".join(self._clean_lines(result.select_one(".hvres-spell")))
            info = " ".join(self._clean_lines(result.select_one(".hvres-info")))
            section_rows = []
            details = result.select_one(".hvres-details")
            if details:
                for source in details.select("p.hvres-source"):
                    source_name = " ".join(self._clean_lines(source))
                    meaning = source.find_next_sibling(class_="hvres-meaning")
                    meaning_lines = self._clean_lines(meaning)
                    if not meaning_lines:
                        continue
                    section_rows.append(
                        f"<div style='margin-top:5px'><b>{html.escape(source_name)}:</b></div>"
                        + "".join(f"<div>{html.escape(line)}</div>" for line in meaning_lines)
                    )
            if section_rows:
                title = spell or info or char
                suffix = f" — {html.escape(info)}" if info and info != spell else ""
                rows.append(
                    "<div style='margin-top:8px'>"
                    f"<b>{html.escape(title)}</b>{suffix}{''.join(section_rows)}</div>"
                )

        if not rows:
            return ""
        gif = await self._stroke_order_image(char)
        return self._kanji_section(char, gif, rows)

    async def scrape_kanji_details(self, char: str, url_template: str, schema: dict) -> str:
        if "{char}" not in (url_template or ""):
            raise ValueError("Kanji URL template must contain {char}")
        # Construct the URL
        url = url_template.replace("{char}", urllib.parse.quote(char))

        # Built-in sources are static HTML and do not need Chromium/Playwright. Besides
        # being faster, direct parsing keeps line boundaries and works after Playwright
        # browser revisions without an extra browser download.
        hostname = urllib.parse.urlparse(url).hostname or ""
        if hostname.endswith("jisho.org"):
            return await self._scrape_jisho_kanji(char, url)
        if hostname.endswith("hvdic.thivien.net"):
            return await self._scrape_hvdic_kanji(char, url)
        if hostname.endswith("kanjiapi.dev"):
            return await self._scrape_kanjiapi_kanji(char, url)

        self._validate_schema(schema)

        # We run the Crawl4AI dynamic extractor using JsonCssExtractionStrategy
        from crawl4ai.extraction_strategy import JsonCssExtractionStrategy
        from crawl4ai import AsyncWebCrawler, CrawlerRunConfig, CacheMode

        extraction_strategy = JsonCssExtractionStrategy(schema)
        config = CrawlerRunConfig(
            extraction_strategy=extraction_strategy,
            cache_mode=CacheMode.BYPASS,
            page_timeout=15000
        )

        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            if not res.success:
                raise RuntimeError(f"Failed to crawl Kanji {char} from {url}")

            data = json.loads(res.extracted_content)
            # Format the extracted fields into a nice string
            if not data:
                return ""

            formatted_items = []
            for item in data:
                item_parts = []
                spell = item.get("spell") or item.get("reading") or ""

                # Check if it is HVDic schema
                is_hvdic = ("chi_tiet" in item or "nghia" in item)

                if is_hvdic:
                    # Clean up HVDic meanings
                    nghia_val = item.get("nghia") or item.get("chi_tiet") or ""
                    if nghia_val:
                        # Split by lines, strip, and take the first few lines
                        lines = [l.strip() for l in nghia_val.replace("<br/>", "\n").replace("<br>", "\n").split("\n") if l.strip()]
                        # Limit to first 4 entries to keep it concise and readable
                        short_nghia = "<br/>".join(lines[:4])
                        item_parts.append(f"<b>Nghĩa:</b><br/>{short_nghia}")
                else:
                    # Jisho schema
                    meanings = item.get("meanings", "")
                    strokes = item.get("strokes", "")
                    radical = item.get("radical", "")
                    parts = item.get("parts", "")

                    if meanings:
                        item_parts.append(f"<b>Meanings:</b> {meanings}")
                    if radical:
                        item_parts.append(f"<b>Radical:</b> {radical}")
                    if strokes:
                        item_parts.append(f"<b>Strokes:</b> {strokes}")
                    if parts:
                        item_parts.append(f"<b>Parts:</b> {parts}")

                content_str = "<br/>".join(item_parts)
                reading = f"<b>Reading:</b> {html.escape(spell)}<br/>" if spell else ""
                formatted_items.append(
                    "<div style='padding:5px 0;border-bottom:1px dotted #aaa'>"
                    f"{reading}{content_str}</div>"
                )
            gif = await self._stroke_order_image(char)
            return self._kanji_section(char, gif, formatted_items)

    async def scrape_custom_dict(self, word: str, url_template: str, schema: dict) -> dict:
        if "{word}" not in (url_template or ""):
            raise ValueError("Dictionary URL template must contain {word}")
        self._validate_schema(schema)
        url = url_template.replace("{word}", urllib.parse.quote(word))
        from crawl4ai.extraction_strategy import JsonCssExtractionStrategy
        from crawl4ai import AsyncWebCrawler, CrawlerRunConfig, CacheMode

        extraction_strategy = JsonCssExtractionStrategy(schema)
        config = CrawlerRunConfig(
            extraction_strategy=extraction_strategy,
            cache_mode=CacheMode.BYPASS,
            page_timeout=15000
        )
        async with AsyncWebCrawler() as crawler:
            res = await crawler.arun(url=url, config=config)
            if not res.success:
                return {"word": word, "found": False}

            try:
                data = json.loads(res.extracted_content)
                if not data:
                    return {"word": word, "found": False}

                item = data[0]
                defs = item.get("definition", "")
                if isinstance(defs, list):
                    defs_str = "; ".join([str(d) for d in defs])
                else:
                    defs_str = str(defs)

                return {
                    "word": item.get("word", word),
                    "found": True,
                    "reading": item.get("reading", ""),
                    "definition": defs_str,
                    "is_common": False,
                    "is_conjugated": False,
                    "suggestion": None,
                    "audio_url": item.get("audio_url", None)
                }
            except Exception as e:
                logging.error(f"Failed to parse custom dictionary data: {e}")
                return {"word": word, "found": False}
