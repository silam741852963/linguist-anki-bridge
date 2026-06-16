import urllib.request
import json
import logging
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

    def generate_card_content(self, word: str, context: str, lang: str, system_prompt: str) -> dict:
        if not self.model:
            raise ValueError("Ollama model not set/selected.")
            
        user_prompt = f"Target word: \"{word}\"\nTarget language: \"{lang}\"\n"
        if context:
            user_prompt += f"Context: \"{context}\"\n"
            
        payload = {
            "model": self.model,
            "system": system_prompt,
            "prompt": user_prompt,
            "stream": False,
            "format": "json"
        }
        
        res = self._post("/api/generate", payload)
        return json.loads(res.get("response", "{}"))

    def generate_grammar_content(self, raw_text: str, system_prompt: str) -> dict:
        if not self.model:
            raise ValueError("Ollama model not set/selected.")
            
        payload = {
            "model": self.model,
            "system": system_prompt,
            "prompt": f"Raw text explanation:\n\"\"\"\n{raw_text}\n\"\"\"",
            "stream": False,
            "format": "json"
        }
        
        res = self._post("/api/generate", payload)
        return json.loads(res.get("response", "{}"))

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
