import base64
import io
import re
import logging
from bs4 import BeautifulSoup
from PIL import Image, ImageOps, ImageStat
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

    def preprocess_image(self, img: Image.Image) -> Image.Image:
        try:
            # 1. Convert to grayscale
            img_gray = img.convert('L')
            
            # 2. Check background intensity to dynamically invert dark images
            stat = ImageStat.Stat(img_gray)
            mean_val = stat.mean[0]
            # If the mean pixel value is less than 120, the background is dark (white text on dark background)
            # We invert it so it becomes dark text on light background (which Tesseract prefers)
            if mean_val < 120:
                img_gray = ImageOps.invert(img_gray)
                
            # 3. Resize/upscale the image to make text bigger and clearer
            w, h = img_gray.size
            if hasattr(Image, "Resampling"):
                resample_filter = Image.Resampling.LANCZOS
            else:
                resample_filter = getattr(Image, "LANCZOS", getattr(Image, "ANTIALIAS", 1))
            img_resized = img_gray.resize((w * 3, h * 3), resample_filter)
            
            # 4. Adaptive binarization / thresholding
            img_binarized = img_resized.point(lambda x: 0 if x < 140 else 255, '1')
            
            return img_binarized
        except Exception as e:
            logging.warning(f"Image preprocessing failed: {e}. Using original image.")
            return img

    def perform_ocr(self, base64_image_data: str, lang_string: str, ocr_config: dict = None) -> str:
        if not ocr_config:
            ocr_config = {}
            
        method = ocr_config.get("method", "tesseract").lower()
        preprocess_enabled = ocr_config.get("preprocess", True)
        
        if method == "ollama":
            url = ocr_config.get("ollama_url") or "http://localhost:11434"
            model = ocr_config.get("ollama_model") or "llama3.2-vision"
            
            try:
                from linguist_anki_bridge.llm import OllamaClient
                client = OllamaClient(url=url, model=model)
                
                # Check if Ollama is online and model is available
                available = client.get_available_models()
                if any(m == model or m.split(":")[0] == model.split(":")[0] for m in available):
                    logging.info(f"Performing Ollama Vision OCR using model '{model}'...")
                    text = client.generate_ocr_from_image(base64_image_data, model_name=model)
                    if text:
                        return text
                else:
                    logging.warning(
                        f"Ollama vision model '{model}' not found in available models: {available}. "
                        f"Please run 'ollama pull {model}' in terminal. Falling back to tesseract."
                    )
            except Exception as e:
                logging.error(f"Ollama vision OCR failed: {e}. Falling back to tesseract.")
                
        # Fallback to Tesseract
        try:
            # Decode base64
            img_bytes = base64.b64decode(base64_image_data)
            img = Image.open(io.BytesIO(img_bytes))
            
            if preprocess_enabled:
                img = self.preprocess_image(img)
                
            # Execute OCR
            text = pytesseract.image_to_string(img, lang=lang_string)
            return text.strip()
        except Exception as e:
            logging.error(f"Tesseract OCR execution failed: {e}")
            raise RuntimeError(f"OCR failed: {e}")

    def is_dictionary_screenshot(self, ocr_text: str) -> bool:
        if not ocr_text:
            return False
            
        text_len = len(ocr_text.strip())
        if text_len < 30:
            return False
            
        markers = [
            "noun", "verb", "adjective", "adverb", "pronunciation", "definition", "synonym", "antonym",
            "cambridge", "jisho", "dictionary", "dict.cc", "moedict", "api",
            "名詞", "動詞", "形容詞", "副詞", "定義", "辞書", "意味", "語彙", "例文", "類義語", "對義語",
            "注音", "拼音", "釋義", "萌典",
            "substantiv", "adjektiv", "übersetzung", "aussprache", "duden"
        ]
        
        ocr_lower = ocr_text.lower()
        has_marker = any(m in ocr_lower for m in markers)
        
        if text_len > 120:
            return True
            
        return has_marker
