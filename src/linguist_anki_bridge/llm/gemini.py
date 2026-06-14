from google import genai
from google.genai import types

from linguist_anki_bridge.llm.base import LLMProvider
from linguist_anki_bridge.config import settings

class GeminiProvider(LLMProvider):
    def __init__(self):
        if not settings.gemini_api_key:
            raise ValueError("GEMINI_API_KEY environment variable is required when using the Gemini provider.")
        self.client = genai.Client(api_key=settings.gemini_api_key)
        self.model_name = "gemini-2.5-flash" # Use an appropriate multimodal model

    def generate_text(self, prompt: str) -> str:
        response = self.client.models.generate_content(
            model=self.model_name,
            contents=prompt,
            config=types.GenerateContentConfig(response_mime_type="application/json")
        )
        return response.text

    def generate_from_image(self, prompt: str, image_base64: str) -> str:
        import base64
        # We need to pass the raw bytes to Gemini through the SDK types
        image_bytes = base64.b64decode(image_base64)
        image_part = types.Part.from_bytes(data=image_bytes, mime_type="image/jpeg",) # Assumes JPEG or PNG usually works
        
        response = self.client.models.generate_content(
            model=self.model_name,
            contents=[prompt, image_part],
            config=types.GenerateContentConfig(response_mime_type="application/json")
        )
        return response.text
