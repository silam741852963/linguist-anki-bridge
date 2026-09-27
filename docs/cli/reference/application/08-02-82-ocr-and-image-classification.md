# Application specification reference

### 8.2 OCR and image classification

Decode and validate source images; run configured Tesseract or optional vision OCR with cancellation/timeouts. Python preprocessing uses grayscale, dark-background inversion, and 3× enlargement; Rust preprocessing differs. Record original filename/content digest, OCR text, language/preprocessing settings, classifier version, features, class, confidence, reason, and review requirement. Cache by image content and settings, not filename alone.

Use a domain state of `dictionary`, `visual_recall`, or `uncertain`, plus explicit operator confirmation. Aggregate multi-image states for display only; replacement decisions remain per-image. Preserve uncertain/missing/unreadable images. Only confirmed dictionary screenshots are eligible for replacement. A sole original image must remain when replacement fails. Mixed-image notes keep mnemonic images and remove screenshot references only under the approved policy.

Python has layout/OCR/visual features, a fitted classifier, and a local feedback layer after sufficient labeled samples; Rust desktop does not implement identical classification. **REVIEW R16:** choose baseline parity, local feedback persistence, and confidence policy. Recommended: retain conservative preservation while porting and validating a labeled corpus. Never claim classifier accuracy from a few passing unit tests.

OCR marked as dictionary evidence enters the generation context before vocabulary annotations are generated. It must not rewrite authoritative senses. Vision classification and OCR are separate model capabilities and can use separate selected models.
