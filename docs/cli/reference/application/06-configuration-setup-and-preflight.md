# Application specification reference

## 6. Configuration, setup, and preflight

`doctor` checks only the capabilities relevant to requested work: configuration syntax/version; readable/writable app storage; Anki endpoint/protocol and required actions; deck/model/mapping existence; Ollama installed model and required text/vision capability; Tesseract binary/language packs; selected audio provider; optional browser fallback; recovery-store accessibility. An empty mapped deck is valid. An image/dictionary failure must not be reported as Anki being offline.

**REQUIRED configuration domains:** Anki endpoint and backup policy; Ollama endpoint, selected model, translation language, prompts and generation settings; OCR method, language packs and preprocessing; classification policy and feedback; dictionary selection per purpose, custom URL/CSS schema and fallbacks; Kanji settings; image policy/search suffix; audio providers/voices; expression filters; retry/timeouts/concurrency/rate limits; deck mappings; storage and retention; dry-run default.

Separate `source_mapping` from `target_mapping`: legacy fields are ingestion sources; managed output schemas define destination fields. Mapping can combine multiple logical values into one physical field, but order must be deterministic and shown in the plan. Optional LLM mapping suggestions are suggestions only; explicit validated mappings control processing. Never guess a destination schema from a deck name or silently use a Japanese model for another language.

**CURRENT defaults worth preserving visibly:** Anki localhost:8765; Ollama localhost:11434; English translations; dry-run true; Japanese OCR `jpn+eng+vie`, English `eng`, Taiwanese `chi_tra+eng+vie`, German `deu+eng`; dictionary attempts 3/backoff 0.6 seconds; batch attempts 3/backoff 5 seconds/commit interval 0.25 seconds; per-service intervals dictionary 1, Ollama 0.25, Kanji 1, image 1, TTS 0.5 seconds. Python classification threshold 0.50, margin 0.12, accepted vision confidence 0.95. These are historical defaults, not interchangeable Rust defaults or universal accuracy guarantees.

Recommended precedence: explicit flags > documented environment variables > selected config file > application defaults. Persist only explicit config edits, never ordinary CLI overrides. Store a resolved settings snapshot in each plan/job. Model auto-selection must be recorded and deterministic; do not arbitrarily switch models on resume. **REVIEW R07 selected direction:** versioned TOML is canonical CLI configuration; import legacy YAML/native JSON completely and read-only, reporting unsupported settings. Choose the local model through evaluation and freeze its concrete identity/settings in each plan.

Missing OCR language packs should produce an actionable capability error with explicit installation instructions. Python currently copies/downloads tessdata automatically; recommend an explicit `doctor` remedy or separate installation command instead. **REVIEW R08**.
