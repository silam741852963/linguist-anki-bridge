import base64
import io
import re
import logging
import hashlib
import json
import math
from dataclasses import asdict, dataclass
from pathlib import Path
from bs4 import BeautifulSoup
from PIL import Image, ImageFilter, ImageOps, ImageStat
import pytesseract


@dataclass
class ImageClassification:
    classification: str
    probability: float
    source: str
    reason: str
    features: dict
    needs_confirmation: bool = False

    def as_dict(self) -> dict:
        return asdict(self)

class OcrEngine:
    # Baseline logistic model calibrated from the repository's labelled
    # dictionary_* and visual_recall_* corpus.  Inputs are standardized in the
    # order returned by ``_baseline_feature_vector``.  Keeping this small model
    # over the deterministic OCR/layout features makes inference cheap and
    # permits user feedback to remain a second, local calibration layer.
    _BASELINE_MEANS = (
        1.54545455, 0.15633636, 0.68181818, 0.64623182, 0.18001727,
        0.14048182, 0.40213455, 0.31430727, 0.55998636,
    )
    _BASELINE_SCALES = (
        1.49931114, 0.08732968, 0.40931496, 0.27720170, 0.15194505,
        0.05387307, 0.16433440, 0.14910790, 0.22126837,
    )
    _BASELINE_WEIGHTS = (
        -0.03386932, 2.72293941, 2.47928581, -0.68369121,
        -0.91972688, -4.04364271, 0.50892469, -0.64331472,
        -0.16827450, -0.34053463,
    )

    def __init__(self):
        self.feedback_path = Path.home() / ".config" / "linguist-anki-bridge" / "image-classifier-feedback.json"

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

            # We return the upscaled grayscale image. Doing a fixed-threshold binarization (e.g. at 140)
            # destroys details of complex CJK character strokes, whereas Tesseract performs better
            # using its internal adaptive Otsu binarization on grayscale upscaled inputs.
            return img_resized
        except Exception as e:
            logging.warning(f"Image preprocessing failed: {e}. Using original image.")
            return img

    def ensure_tessdata_for_langs(self, langs: list, log_cb: callable = None) -> str:
        import urllib.request
        import shutil
        from pathlib import Path

        def log(msg):
            logging.info(msg)
            if log_cb:
                log_cb(msg)

        tessdata_dir = Path.home() / ".config" / "linguist-anki-bridge" / "tessdata"
        tessdata_dir.mkdir(parents=True, exist_ok=True)

        # Always ensure eng and osd are copied/downloaded as core dependencies
        core_langs = ["eng", "osd"]
        for lang in core_langs + langs:
            target_path = tessdata_dir / f"{lang}.traineddata"
            if target_path.exists():
                continue

            # Try copying from system tessdata first
            system_path = Path("/usr/share/tessdata") / f"{lang}.traineddata"
            if system_path.exists():
                try:
                    log(f"Copying {lang}.traineddata from system tessdata...")
                    shutil.copy(system_path, target_path)
                    continue
                except Exception as e:
                    logging.warning(f"Failed to copy system {lang}.traineddata: {e}")

            # If not in system, download from tessdata_fast repo
            url = f"https://github.com/tesseract-ocr/tessdata_fast/raw/main/{lang}.traineddata"
            log(f"Downloading missing Tesseract language pack '{lang}'...")
            try:
                # Use urllib to be free of external request deps
                with urllib.request.urlopen(url, timeout=30) as response:
                    with open(target_path, "wb") as f:
                        shutil.copyfileobj(response, f)
                log(f"Successfully downloaded '{lang}' language pack.")
            except Exception as e:
                log(f"Error downloading '{lang}' language pack: {e}")

        return str(tessdata_dir)

    def perform_ocr(self, base64_image_data: str, lang_string: str, ocr_config: dict = None, log_cb: callable = None) -> str:
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

            # Ensure the required languages exist in our custom tessdata directory
            langs = [l.strip() for l in lang_string.split("+") if l.strip()]
            tessdata_dir = self.ensure_tessdata_for_langs(langs, log_cb=log_cb)

            # Execute OCR using the custom tessdata path
            config_str = f'--tessdata-dir "{tessdata_dir}"'
            text = pytesseract.image_to_string(img, lang=lang_string, config=config_str)
            return text.strip()
        except Exception as e:
            logging.error(f"Tesseract OCR execution failed: {e}")
            raise RuntimeError(f"OCR failed: {e}")

    def dictionary_evidence_score(self, ocr_text: str) -> int:
        """Score dictionary-like structure without depending on an LLM or its JSON output."""
        if not ocr_text or not ocr_text.strip():
            return 0

        text = ocr_text.lower()
        score = 0
        source_markers = (
            "jisho", "cambridge", "dict.cc", "moedict", "萌典", "duden",
            "wiktionary", "dictionary", "wörterbuch", "từ điển",
        )
        strong_ui_markers = (
            "common word", "jlpt", "wanikani", "play audio", "see also",
            "wikipedia definition", "organization name", "links", "details ▸",
            "pronunciation", "definition", "定義", "辞書", "釋義", "âm hán việt",
        )
        part_of_speech_markers = (
            "noun", "verb", "adjective", "adverb", "pronoun", "conjunction",
            "名詞", "動詞", "形容詞", "副詞", "substantiv", "adjektiv",
            "danh từ", "động từ", "tính từ", "phó từ",
        )

        if any(marker in text for marker in source_markers):
            score += 4
        score += min(4, sum(marker in text for marker in strong_ui_markers) * 2)
        score += min(3, sum(marker in text for marker in part_of_speech_markers))

        lines = [line.strip() for line in ocr_text.splitlines() if line.strip()]
        numbered_definitions = sum(
            bool(re.match(r"^(?:\d+[.)]|[①-⑳])\s*", line)) for line in lines
        )
        if numbered_definitions:
            score += 2
        if len(lines) >= 5 and len(ocr_text) >= 80:
            score += 1
        if re.search(r"\b(?:synonym|antonym|translation|meaning|example)s?\b", text):
            score += 1

        # Compact bilingual dictionary cards often contain only one Japanese
        # example followed by a colon and its Vietnamese/English gloss.  They
        # have too few rows for the generic layout model, but the cross-script
        # ``source : translation`` structure is strong semantic evidence.  A
        # plain Japanese sentence or a photograph caption does not satisfy all
        # three requirements.
        vietnamese = r"A-Za-zÀ-ỹĐđ"
        bilingual_gloss = re.search(
            rf"[\u3040-\u30ff\u3400-\u9fff][^\n:]{{1,100}}\s*[:：]\s*[{vietnamese}][{vietnamese}\s,.;'’\-]{{2,}}",
            ocr_text,
        )
        if bilingual_gloss:
            score += 2
        return score

    @staticmethod
    def _sigmoid(value: float) -> float:
        return 1.0 / (1.0 + math.exp(-max(-30.0, min(30.0, value))))

    def extract_visual_features(self, base64_image_data: str, lang_string: str = "jpn+eng+vie", ocr_config: dict = None) -> dict:
        """Extract cheap, deterministic screenshot/photograph features.

        Tesseract's token boxes provide text coverage, row count, and alignment.
        Pillow provides edges, entropy, colour variation, and flat-background
        estimates.  The image is capped at 768 px to keep this stage bounded.
        """
        ocr_config = ocr_config or {}
        raw = base64.b64decode(base64_image_data)
        image = Image.open(io.BytesIO(raw)).convert("RGB")
        image.thumbnail((768, 768))
        width, height = image.size
        area = max(1, width * height)

        langs = [item.strip() for item in lang_string.split("+") if item.strip()]
        tessdata_dir = self.ensure_tessdata_for_langs(langs)
        config = f'--tessdata-dir "{tessdata_dir}"'
        data = pytesseract.image_to_data(
            self.preprocess_image(image) if ocr_config.get("preprocess", True) else image,
            lang=lang_string,
            config=config,
            output_type=pytesseract.Output.DICT,
        )
        scale = 3.0 if ocr_config.get("preprocess", True) else 1.0
        boxes = []
        line_keys = set()
        x_bins = {}
        for index, token in enumerate(data.get("text", [])):
            try:
                confidence = float(data["conf"][index])
            except (ValueError, TypeError, KeyError):
                confidence = -1
            if not str(token).strip() or confidence < 20:
                continue
            left = float(data["left"][index]) / scale
            top = float(data["top"][index]) / scale
            box_w = float(data["width"][index]) / scale
            box_h = float(data["height"][index]) / scale
            boxes.append((left, top, box_w, box_h))
            line_keys.add((data["block_num"][index], data["par_num"][index], data["line_num"][index]))
            bucket = round(left / max(8.0, width * 0.04))
            x_bins[bucket] = x_bins.get(bucket, 0) + 1

        text_coverage = min(1.0, sum(w * h for _, _, w, h in boxes) / area)
        alignment = max(x_bins.values(), default=0) / max(1, len(boxes))
        row_density = min(1.0, len(line_keys) / max(3.0, height / 32.0))

        sample = image.copy()
        sample.thumbnail((256, 256))
        gray = sample.convert("L")
        edges = gray.filter(ImageFilter.FIND_EDGES)
        edge_density = sum(1 for value in edges.getdata() if value > 32) / max(1, sample.width * sample.height)
        entropy = gray.entropy() / 8.0
        stats = ImageStat.Stat(sample)
        colour_std = sum(stats.stddev[:3]) / (3.0 * 128.0)
        quantized = sample.quantize(colors=32)
        histogram = quantized.histogram()
        dominant_colour = max(histogram, default=0) / max(1, sample.width * sample.height)
        return {
            "text_coverage": round(text_coverage, 5),
            "token_count": len(boxes),
            "row_count": len(line_keys),
            "row_density": round(row_density, 5),
            "alignment": round(alignment, 5),
            "edge_density": round(edge_density, 5),
            "entropy": round(entropy, 5),
            "colour_std": round(colour_std, 5),
            "dominant_colour": round(dominant_colour, 5),
        }

    @staticmethod
    def _feature_vector(features: dict) -> list:
        return [
            float(features.get("ocr_score", 0)) / 8.0,
            float(features.get("text_coverage", 0)),
            min(1.0, float(features.get("token_count", 0)) / 80.0),
            float(features.get("row_density", 0)),
            float(features.get("alignment", 0)),
            float(features.get("edge_density", 0)),
            float(features.get("entropy", 0)),
            float(features.get("colour_std", 0)),
            float(features.get("dominant_colour", 0)),
        ]

    @staticmethod
    def _baseline_feature_vector(features: dict) -> list:
        return [
            float(features.get("ocr_score", 0)),
            float(features.get("text_coverage", 0)),
            min(1.0, float(features.get("token_count", 0)) / 80.0),
            float(features.get("row_density", 0)),
            float(features.get("alignment", 0)),
            float(features.get("edge_density", 0)),
            float(features.get("entropy", 0)),
            float(features.get("colour_std", 0)),
            float(features.get("dominant_colour", 0)),
        ]

    def _baseline_probability(self, features: dict) -> float:
        vector = self._baseline_feature_vector(features)
        standardized = [
            (value - mean) / scale
            for value, mean, scale in zip(
                vector, self._BASELINE_MEANS, self._BASELINE_SCALES
            )
        ]
        score = self._BASELINE_WEIGHTS[0] + sum(
            weight * value
            for weight, value in zip(self._BASELINE_WEIGHTS[1:], standardized)
        )
        return self._sigmoid(score)

    def _feedback_rows(self) -> list:
        try:
            data = json.loads(self.feedback_path.read_text(encoding="utf-8"))
            return data if isinstance(data, list) else []
        except (OSError, ValueError):
            return []

    def _learned_probability(self, features: dict):
        rows = [row for row in self._feedback_rows() if row.get("source") == "user"]
        labels = {row.get("label") for row in rows}
        if len(rows) < 6 or labels != {"dictionary", "visual_recall"}:
            return None
        samples = [(self._feature_vector(row.get("features", {})), 1.0 if row["label"] == "dictionary" else 0.0) for row in rows]
        weights = [0.0] * (len(samples[0][0]) + 1)
        for _ in range(240):
            gradients = [0.0] * len(weights)
            for vector, label in samples:
                prediction = self._sigmoid(weights[0] + sum(w * x for w, x in zip(weights[1:], vector)))
                error = prediction - label
                gradients[0] += error
                for index, value in enumerate(vector, 1):
                    gradients[index] += error * value
            rate = 0.7 / len(samples)
            weights = [weight - rate * gradient for weight, gradient in zip(weights, gradients)]
        vector = self._feature_vector(features)
        return self._sigmoid(weights[0] + sum(w * x for w, x in zip(weights[1:], vector)))

    def record_classification_feedback(self, base64_image_data: str, result: dict, corrected_class: str) -> None:
        if corrected_class not in ("dictionary", "visual_recall"):
            raise ValueError("Corrected class must be dictionary or visual_recall")
        rows = self._feedback_rows()
        digest = hashlib.sha256(base64.b64decode(base64_image_data)).hexdigest()
        row = {
            "image_hash": digest,
            "predicted_probability": float(result.get("probability", 0.5)),
            "label": corrected_class,
            "source": "user",
            "features": result.get("features", {}),
        }
        rows = [existing for existing in rows if existing.get("image_hash") != digest]
        rows.append(row)
        self.feedback_path.parent.mkdir(parents=True, exist_ok=True)
        self.feedback_path.write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf-8")

    def classify_image(self, base64_image_data: str, ocr_text: str, lang_string: str = "jpn+eng+vie", config: dict = None, llm_client=None) -> dict:
        """Return an automatic binary decision plus a non-blocking review hint."""
        config = config or {}
        decision_threshold = float(config.get("decision_threshold", 0.50))
        confirmation_margin = max(0.0, float(config.get("confirmation_margin", 0.12)))
        features = self.extract_visual_features(base64_image_data, lang_string, config.get("ocr", {}))
        features["ocr_score"] = self.dictionary_evidence_score(ocr_text)

        probability = self._baseline_probability(features)
        source = "ocr-layout-calibrated-v1"
        # The fitted feature space contains text-bearing images. If Tesseract
        # finds no usable token and no dictionary marker, do not extrapolate a
        # confident dictionary result from standardized layout values alone.
        if features["token_count"] == 0 and features["ocr_score"] == 0:
            probability = min(probability, 0.25)
            source += "+empty-ocr-guard"
        learned = self._learned_probability(features)
        if learned is not None:
            probability = 0.45 * probability + 0.55 * learned
            source += "+local-feedback"

        needs_confirmation = abs(probability - decision_threshold) <= confirmation_margin

        # A vision model is an adjudicator, not a user label. Only consult it
        # near the decision boundary and only accept very confident answers.
        if needs_confirmation and config.get("llm_adjudication", False) and llm_client:
            try:
                verdict = llm_client.classify_image_visual(
                    base64_image_data,
                    model_name=config.get("vision_model") or None,
                )
                confidence = float(verdict.get("confidence", 0))
                if confidence >= float(config.get("llm_accept_confidence", 0.95)):
                    probability = confidence if verdict.get("classification") == "dictionary" else 1.0 - confidence
                    source += "+vision-llm"
                    needs_confirmation = abs(probability - decision_threshold) <= confirmation_margin
            except Exception as exc:
                logging.warning("Vision adjudication unavailable: %s", exc)

        classification = (
            "dictionary" if probability >= decision_threshold else "visual_recall"
        )
        reason = (
            f"dictionary probability {probability:.2f}; OCR markers {features['ocr_score']}; "
            f"text coverage {features['text_coverage']:.2f}; rows {features['row_count']}; "
            f"decision threshold {decision_threshold:.2f}"
        )
        return ImageClassification(
            classification, probability, source, reason, features, needs_confirmation
        ).as_dict()

    def classify_image_ocr(self, ocr_text: str) -> str:
        score = self.dictionary_evidence_score(ocr_text)
        if score >= 3:
            return "dictionary"
        # Legacy callers expect a binary answer. The modern pipeline uses
        # ``classify_image`` and receives the safe ``uncertain`` state.
        return "visual_recall"

    def is_dictionary_screenshot(self, ocr_text: str) -> bool:
        return self.classify_image_ocr(ocr_text) == "dictionary"
