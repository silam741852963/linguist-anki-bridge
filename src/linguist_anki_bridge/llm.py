import urllib.request
import json
import logging
import re
from urllib.error import URLError

class OllamaClient:
    def __init__(self, url="http://localhost:11434", model=None, timeout=30.0):
        self.url = url
        self.model = model
        self.timeout = timeout

    def _post(self, endpoint, payload):
        target_url = f"{self.url.rstrip('/')}{endpoint}"
        try:
            req = urllib.request.Request(
                target_url,
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"}
            )
            with urllib.request.urlopen(req, timeout=self.timeout) as res:
                return json.loads(res.read().decode("utf-8"))
        except URLError as e:
            logging.error(f"Ollama connection error at {target_url}: {e}")
            raise ConnectionError(f"Could not connect to Ollama at {self.url}.")
        except Exception as e:
            logging.error(f"Ollama error: {e}")
            raise

    def _get(self, endpoint):
        target_url = f"{self.url.rstrip('/')}{endpoint}"
        try:
            req = urllib.request.Request(target_url, method="GET")
            with urllib.request.urlopen(req, timeout=self.timeout) as res:
                return json.loads(res.read().decode("utf-8"))
        except Exception as e:
            logging.error(f"Ollama connection error at GET {target_url}: {e}")
            raise

    def is_online(self) -> bool:
        try:
            self._get("/api/tags")
            return True
        except Exception:
            return False

    def get_available_models(self) -> list:
        try:
            result = self._get("/api/tags")
            models = result.get("models", [])
            return [m["name"] for m in models]
        except Exception as e:
            logging.error(f"Failed to get Ollama models: {e}")
            return []

    def set_model(self, model_name: str):
        self.model = model_name

    @staticmethod
    def _decode_object(response: dict, required: tuple[str, ...]) -> dict:
        raw = response.get("response", "{}")
        data = json.loads(raw)
        if not isinstance(data, dict):
            raise ValueError("Ollama response must be a JSON object")
        missing = [key for key in required if key not in data]
        if missing:
            raise ValueError(f"Ollama response is missing fields: {', '.join(missing)}")
        if "examples" in data and not isinstance(data["examples"], list):
            raise ValueError("Ollama examples must be a list")
        return data

    @staticmethod
    def _extract_response_object(response: dict) -> dict:
        raw = response.get("response") or response.get("thinking") or "{}"
        if isinstance(raw, dict):
            return raw
        cleaned = str(raw).strip()
        cleaned = re.sub(r"^```(?:json)?\s*", "", cleaned, flags=re.IGNORECASE)
        cleaned = re.sub(r"\s*```$", "", cleaned)
        try:
            data = json.loads(cleaned)
        except json.JSONDecodeError:
            match = re.search(r"\{.*\}", cleaned, flags=re.DOTALL)
            if not match:
                raise
            data = json.loads(match.group(0))
        if not isinstance(data, dict):
            raise ValueError("Ollama response must be a JSON object")
        return data

    @staticmethod
    def _normalize_vocab_generation(data: dict, require_all: bool = True) -> dict:
        def normalized_key(value: str) -> str:
            return re.sub(r"[^\w]+", "_", str(value).strip().lower()).strip("_")

        # Models occasionally wrap the requested object in ``content``,
        # ``output`` or another envelope (especially larger reasoning models).
        # Walk nested mappings so valid nuance/example payloads are not
        # discarded merely because of that transport envelope.
        candidates = []
        pending = [data]
        seen: set[int] = set()
        while pending and len(candidates) < 32:
            candidate = pending.pop(0)
            if not isinstance(candidate, dict) or id(candidate) in seen:
                continue
            seen.add(id(candidate))
            candidates.append(candidate)
            for value in candidate.values():
                if isinstance(value, dict):
                    pending.append(value)
                elif isinstance(value, str) and len(value) < 20_000:
                    # A JSON string nested in an envelope is common with
                    # ``content`` fields; parse it opportunistically.
                    try:
                        parsed = json.loads(value)
                    except (TypeError, json.JSONDecodeError):
                        continue
                    if isinstance(parsed, dict):
                        pending.append(parsed)
        aliases = {
            "nuances": {
                "nuances", "nuance", "usage_nuance", "usage_nuances", "usage_note",
                "usage_notes", "notes", "note", "sắc_thái", "sac_thai",
            },
            "examples": {
                "examples", "example", "example_sentences", "sample_sentences",
                "sentences", "ví_dụ", "vi_du",
            },
        }

        found = {"nuances": None, "examples": None}
        for candidate in candidates:
            keyed = {normalized_key(key): value for key, value in candidate.items()}
            for field, field_aliases in aliases.items():
                if found[field] is None:
                    for alias in field_aliases:
                        if normalized_key(alias) in keyed:
                            found[field] = keyed[normalized_key(alias)]
                            break

        missing = [field for field, value in found.items() if value is None]
        if require_all and missing:
            raise ValueError(
                f"Ollama response is missing fields: {', '.join(missing)}; "
                f"returned keys: {', '.join(map(str, data.keys())) or '(none)'}"
            )
        if len(missing) == 2:
            raise ValueError(
                "Ollama response contained neither nuance nor examples; "
                f"returned keys: {', '.join(map(str, data.keys())) or '(none)'}"
            )

        nuances = "" if found["nuances"] is None else str(found["nuances"]).strip()
        raw_examples = [] if found["examples"] is None else found["examples"]
        if isinstance(raw_examples, dict):
            raw_examples = list(raw_examples.values())
        if not isinstance(raw_examples, list):
            raw_examples = [raw_examples]

        examples = []
        for example in raw_examples:
            if isinstance(example, str):
                examples.append({"sentence": example, "translation": ""})
                continue
            if not isinstance(example, dict):
                continue
            keyed = {normalized_key(key): value for key, value in example.items()}
            sentence = next(
                (keyed[key] for key in ("sentence", "example", "text", "japanese", "original") if keyed.get(key)),
                "",
            )
            translation = next(
                (keyed[key] for key in ("translation", "translated", "english", "meaning") if keyed.get(key)),
                "",
            )
            if sentence or translation:
                examples.append({"sentence": str(sentence), "translation": str(translation)})
        return {"nuances": nuances, "examples": examples}

    def lemmatize_word(self, word: str, lang: str) -> dict:
        if not self.model:
            raise ValueError("Ollama model not set/selected.")

        prompt = (
            f"You are a language dictionary assistant. Analyze the input word and determine if it is in standard dictionary (lemma) form for language '{lang}'.\n"
            f"If it is conjugated, inflected, or misspelled, provide the corrected dictionary form.\n"
            f"Input word: \"{word}\"\n\n"
            f"Return only a JSON object matching this schema:\n"
            f"{{\n"
            f"  \"input\": \"{word}\",\n"
            f"  \"is_dictionary_form\": true/false,\n"
            f"  \"suggestion\": \"dictionary form or null if it cannot be corrected or is non-existent\"\n"
            f"}}"
        )

        payload = {
            "model": self.model,
            "prompt": prompt,
            "stream": False,
            "format": "json"
        }

        try:
            res = self._post("/api/generate", payload)
            response_text = res.get("response", "")
            return json.loads(response_text)
        except Exception as e:
            logging.error(f"Lemmatize failed: {e}")
            return {"input": word, "is_dictionary_form": True, "suggestion": None}

    def generate_card_content(self, word: str, context: str, source_language: str,
                              translation_language: str, system_prompt: str) -> dict:
        if not self.model:
            raise ValueError("Ollama model not set/selected.")

        rendered_system_prompt = system_prompt.replace("{lang}", translation_language)
        rendered_system_prompt += (
            "\n\nMandatory vocabulary-generation boundary: the dictionary parsing is authoritative. "
            "Do not generate, summarize, translate, or rewrite definitions, readings, metadata, "
            "related entries, or senses. Generate only usage nuances for the exact target word "
            "and example sentences. The raw OCR may contain dictionary screenshots: recover every "
            "distinct usage nuance and every complete example for the exact target word from those "
            "screenshots. Preserve target-language example sentences, but translate non-"
            f"{translation_language} explanations and translations into {translation_language}. "
            "If OCR provides fewer than three usable examples, generate enough additional natural "
            "examples to reach three; never truncate a larger recovered set to three. Return exactly "
            "the JSON keys 'nuances' and 'examples'."
        )
        user_prompt = (
            f"Target word: \"{word}\"\n"
            f"Source language: \"{source_language}\"\n"
            f"Write the nuance and example translations in: \"{translation_language}\"\n"
        )
        if context:
            user_prompt += (
                "Inputs for nuance and example generation:\n"
                "<context>\n"
                f"{context}\n"
                "</context>\n"
                "Use these inputs only to understand usage of the exact target word. Do not output "
                "dictionary definitions or information for related entries. For OCR sections marked "
                "as dictionary images, include all recoverable examples and usage notes for the target "
                f"word, translating their explanation/translation into {translation_language}.\n"
                "Return only: {\"nuances\": \"...\", \"examples\": "
                "[{\"sentence\": \"...\", \"translation\": \"...\"}]}\n"
            )

        response_schema = {
            "type": "object",
            "properties": {
                "nuances": {"type": "string"},
                "examples": {
                    "type": "array",
                    "minItems": 3,
                    "items": {
                        "type": "object",
                        "properties": {
                            "sentence": {"type": "string"},
                            "translation": {"type": "string"},
                        },
                        "required": ["sentence", "translation"],
                    },
                },
            },
            "required": ["nuances", "examples"],
        }
        payload = {
            "model": self.model,
            "system": rendered_system_prompt,
            "prompt": user_prompt,
            "stream": False,
            "format": response_schema,
            "options": {"temperature": 0.2},
        }

        res = self._post("/api/generate", payload)
        try:
            generated = self._normalize_vocab_generation(
                self._extract_response_object(res), require_all=True
            )
        except (ValueError, json.JSONDecodeError) as exc:
            raw = str(res.get("response") or res.get("thinking") or "")
            logging.warning(
                "Vocabulary response did not match the requested schema (%s). "
                "Response preview: %r. Retrying once with a minimal prompt.",
                exc, raw[:400],
            )
            retry_payload = dict(payload)
            retry_payload["prompt"] = (
                f"For the exact word {word}, use the dictionary parsing and OCR below only as context. "
                "Recover every distinct usage nuance and every complete OCR example for this word. "
                f"Preserve source-language sentences and translate all translations into "
                f"{translation_language}. Add natural examples only if needed to reach at least three; "
                f"do not truncate a larger recovered set.\n{context}\n"
                'Required shape: {"nuances":"...","examples":'
                '[{"sentence":"...","translation":"..."}]}'
            )
            retry = self._post("/api/generate", retry_payload)
            generated = self._normalize_vocab_generation(
                self._extract_response_object(retry), require_all=False
            )

        # Enforce the ownership boundary even if a model emits extra dictionary-like
        # keys such as "definition".
        return generated

    def generate_grammar_content(self, raw_text: str, translation_language: str,
                                 system_prompt: str) -> dict:
        if not self.model:
            raise ValueError("Ollama model not set/selected.")

        payload = {
            "model": self.model,
            "system": system_prompt.replace("{lang}", translation_language),
            "prompt": f"Raw text explanation:\n\"\"\"\n{raw_text}\n\"\"\"",
            "stream": False,
            "format": "json"
        }

        res = self._post("/api/generate", payload)
        return self._decode_object(res, ("grammar_point",))

    def generate_ocr_from_image(self, base64_image_data: str, model_name: str = None) -> str:
        model = model_name or self.model or "llama3.2-vision"
        prompt = (
            "Transcribe the text in this image exactly as it is, without any commentary, conversational intro/outro, "
            "or formatting. Just output the extracted text."
        )

        payload = {
            "model": model,
            "prompt": prompt,
            "images": [base64_image_data],
            "stream": False
        }

        try:
            res = self._post("/api/generate", payload)
            return res.get("response", "").strip()
        except Exception as e:
            logging.error(f"Ollama vision query failed: {e}")
            raise

    def classify_image_ocr(self, ocr_text: str) -> str:
        """Legacy OCR-only classifier retained for API compatibility."""
        if not self.model or not ocr_text or not ocr_text.strip():
            return "visual_recall"
        prompt = (
            "Classify OCR text as dictionary or visual_recall. Return JSON only: "
            '{"classification":"dictionary|visual_recall"}.\nOCR:\n' + ocr_text
        )
        try:
            response = self._post("/api/generate", {
                "model": self.model, "prompt": prompt, "stream": False, "format": "json"
            }).get("response", "")
            value = str(json.loads(response).get("classification", "")).strip().lower()
            return value if value in ("dictionary", "visual_recall") else "visual_recall"
        except Exception as exc:
            logging.error("Failed to classify image OCR with LLM: %s", exc)
            return "visual_recall"

    def classify_image_visual(self, base64_image_data: str, model_name: str = None) -> dict:
        """Zero-shot classification using the actual image pixels.

        This is intentionally separate from OCR classification: it is only an
        optional adjudicator for the deterministic classifier's uncertain
        band.  Callers decide whether its confidence is safe to accept.
        """
        model = model_name or self.model
        if not model:
            raise ValueError("No vision model configured")
        prompt = (
            "Classify this language-learning card image. "
            "dictionary means a flat dictionary website/app screenshot with structured entries, readings, "
            "definitions, numbered senses, or dictionary controls. visual_recall means a photograph, drawing, "
            "mnemonic, scene, comic, sign, or other real-world visual. Return JSON only: "
            '{"classification":"dictionary|visual_recall","confidence":0.0,"reason":"short reason"}. '
            "Confidence must describe certainty from visible pixels, not invented OCR."
        )
        payload = {
            "model": model,
            "prompt": prompt,
            "images": [base64_image_data],
            "stream": False,
            "format": "json",
            "options": {"temperature": 0},
        }
        response = self._post("/api/generate", payload).get("response", "")
        data = json.loads(response)
        classification = str(data.get("classification", "")).strip().lower()
        if classification not in ("dictionary", "visual_recall"):
            raise ValueError("Vision model returned an invalid classification")
        confidence = max(0.0, min(1.0, float(data.get("confidence", 0))))
        return {
            "classification": classification,
            "confidence": confidence,
            "reason": str(data.get("reason", "")).strip(),
        }

    def summarize_kanji_details(self, word: str, raw_details: str, lang: str, prompt_template: str = None) -> str:
        """
        Uses the local Ollama model to clean up, summarize, and format raw scraped Kanji details.
        """
        if not self.model:
            raise ValueError("Ollama model not set/selected.")

        if prompt_template:
            prompt = prompt_template.replace("{word}", word).replace("{raw_details}", raw_details)
        else:
            if lang.lower() == "vietnamese":
                prompt = (
                    f"Bạn là một trợ lý học tiếng Nhật. Hãy tóm tắt cấu tạo chữ Kanji cho từ \"{word}\" dựa trên thông tin thô sau đây.\n"
                    f"Thông tin thô:\n{raw_details}\n\n"
                    f"Yêu cầu:\n"
                    f"1. Với mỗi chữ Kanji trong từ, hãy trình bày rõ nét: chữ Kanji, âm Hán Việt (spell), tổng số nét, bộ thủ (radical), các bộ phận cấu thành (parts), và nghĩa chính một cách ngắn gọn, súc tích.\n"
                    f"2. Loại bỏ các phần lặp lại hoặc thông tin rác từ từ điển.\n"
                    f"3. Định dạng kết quả sử dụng các thẻ HTML cơ bản như <b>, <br/>, <div> để hiển thị đẹp mắt trong thẻ Anki. Không dùng markdown.\n"
                    f"4. Trả về kết quả dưới dạng văn bản HTML trực tiếp, không có phần giải thích hay đoạn chat nào khác."
                )
            else:
                prompt = (
                    f"You are a Japanese language learning assistant. Summarize the Kanji construction details for the word \"{word}\" based on the following raw crawled details.\n"
                    f"Raw details:\n{raw_details}\n\n"
                    f"Requirements:\n"
                    f"1. For each Kanji character in the word, clearly present: the Kanji character, meanings, stroke count, radical, parts/components, and a clean explanation.\n"
                    f"2. Clean up any redundant database fields or dictionary junk.\n"
                    f"3. Format the output using basic HTML tags such as <b>, <br/>, <div> for clean rendering in Anki. Do not use markdown.\n"
                    f"4. Return only the raw HTML code directly, without any conversational intro, outro, or explanation."
                )

        payload = {
            "model": self.model,
            "prompt": prompt,
            "stream": False
        }

        is_json = False
        if prompt_template and "JSON" in prompt_template:
            payload["format"] = "json"
            is_json = True

        try:
            res = self._post("/api/generate", payload)
            response_text = res.get("response", "").strip()
            # Clean markdown code blocks if the LLM wrapped it in ```html ... ``` or ```json ... ```
            if response_text.startswith("```"):
                lines = response_text.splitlines()
                if lines[0].startswith("```"):
                    lines = lines[1:]
                if lines and lines[-1].startswith("```"):
                    lines = lines[:-1]
                response_text = "\n".join(lines).strip()

            if is_json:
                try:
                    data = json.loads(response_text)
                    return self.format_kanji_json_to_html(data)
                except Exception as ex:
                    logging.error(f"Failed to parse LLM JSON response: {ex}. Raw response: {response_text}")

            # Clean up LaTeX \text{...} wrappers
            import re
            response_text = re.sub(r'\$\s*\\text\s*\{\s*([^\}]+)\s*\}\s*\$', r'\1', response_text)
            response_text = re.sub(r'\\text\s*\{\s*([^\}]+)\s*\}', r'\1', response_text)
            response_text = re.sub(r'\$\s*([^\$]+)\s*\$', r'\1', response_text)
            return response_text
        except Exception as e:
            logging.error(f"Kanji summarization failed: {e}")
            return raw_details

    def format_kanji_json_to_html(self, data: dict) -> str:
        html_parts = []
        characters = data.get("kanji_characters", [])
        if not characters and isinstance(data, dict):
            for v in data.values():
                if isinstance(v, list):
                    characters = v
                    break

        for c in characters:
            if not isinstance(c, dict):
                continue
            char = c.get("character") or c.get("char") or c.get("kanji", "")
            if not char:
                continue
            parts = []
            parts.append(f"<div><b>Kanji:</b> <span style='font-size: 1.5em;'>{char}</span></div>")

            spell = c.get("spell") or c.get("han_viet") or c.get("hanviet", "")
            if spell:
                parts.append(f"<div><b>Hán Việt:</b> {spell}</div>")

            strokes = c.get("strokes") or c.get("stroke_count", "")
            if strokes:
                parts.append(f"<div><b>Stroke Count:</b> {strokes}</div>")

            radical = c.get("radical", "")
            if radical:
                parts.append(f"<div><b>Radical:</b> {radical}</div>")

            c_parts = c.get("parts") or c.get("components", [])
            if c_parts:
                if isinstance(c_parts, list):
                    parts.append(f"<div><b>Parts:</b> {', '.join(c_parts)}</div>")
                else:
                    parts.append(f"<div><b>Parts:</b> {c_parts}</div>")

            meanings = c.get("meanings") or c.get("meaning", [])
            if meanings:
                if isinstance(meanings, list):
                    parts.append(f"<div><b>Meanings:</b> {', '.join(meanings)}</div>")
                else:
                    parts.append(f"<div><b>Meanings:</b> {meanings}</div>")

            html_parts.append(f"<div style='margin-bottom: 10px; border-bottom: 1px dashed #ccc; padding-bottom: 5px;'>{''.join(parts)}</div>")

        return "".join(html_parts) if html_parts else "No Kanji details extracted"
