import base64
import io
import re
import logging
from bs4 import BeautifulSoup
from PIL import Image
import pytesseract

class OcrEngine:
    def __init__(self):
        pass

    def get_installed_languages(self) -> list:
        try:
            return pytesseract.get_languages()
        except Exception as e:
            logging.error(f"Failed to get tesseract languages: {e}")
            return []

    def check_missing_languages(self, lang_string: str) -> list:
        installed = self.get_installed_languages()
        if not installed:
            return [lang_string] # Assume all missing if we can't query tesseract
            
        missing = []
        for l in lang_string.split("+"):
            l = l.strip()
            if l not in installed:
                missing.append(l)
        return missing

    def extract_image_filename(self, html_content: str) -> str:
        # Search for first img tag
        if not html_content:
            return None
        soup = BeautifulSoup(html_content, "html.parser")
        img = soup.find("img")
        if img and img.get("src"):
            return img["src"]
        # Fallback regex
        match = re.search(r'<img\s+[^>]*src=["\']([^"\']+)["\']', html_content)
        if match:
            return match.group(1)
        return None

    def perform_ocr(self, base64_image_data: str, lang_string: str) -> str:
        try:
            # Decode base64
            img_bytes = base64.b64decode(base64_image_data)
            img = Image.open(io.BytesIO(img_bytes))
            
            # Execute OCR
            text = pytesseract.image_to_string(img, lang=lang_string)
            return text.strip()
        except Exception as e:
            logging.error(f"OCR execution failed: {e}")
            raise RuntimeError(f"OCR failed: {e}")
