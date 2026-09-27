# Final learning/provider baseline

Owner: WP-05–WP-09/WP-14/WP-15. Choices are settled; corpus performance remains a test requirement.

## Source and language

Initial validated targets are Japanese and English; archived other-language mappings stay readable but cannot be processed as another language. Ten Basic notes in a vocab deck are classified per note using explicit purpose/source evidence; ambiguous kind is a review issue. The 195 linebreak candidates are not automatic splits. Grammar segmentation is source-region-based, with one reviewed history anchor and fresh siblings.

Builtin purpose defaults: japanese_vocab uses ja target/en explanation/jpn+eng OCR/Jisho; japanese_grammar uses ja target/vi explanation/jpn+eng+vie OCR; english_vocab uses en target/en explanation/eng OCR/Wiktionary; english_grammar uses en target/en explanation/eng OCR. Explicit user config overrides these presets. Preserve source/personal text and language regardless of generated explanation language. Purpose names do not silently bind observed private deck names; source/target mappings remain explicit local settings.

Revamp with no target override preserves each retained card's **home** deck. Add requires an existing regular target deck. Filtered deck membership is a blocker for migration until cards return home through ordinary Anki; do not empty filtered decks or rewrite odid/odue automatically. A new retained task's deck needs explicit resolution if source deck choices are ambiguous. Source task maps are inferred only for verified known manifests, never arbitrary ordinals. [Anki's filtered-deck manual](https://docs.ankiweb.net/filtered-decks.html) explains home-deck return behavior.

## Content and task defaults

New vocabulary: Comprehension only. New grammar: Recognition only. Production/Spelling/Application are explicit opt-ins with unambiguous reviewed cues. Revamp retains existing tasks and current scheduler/history, including suspended/buried/learning/FSRS-related state; no scheduler or deck-option modification.

examples_min is a **generation target**, not an unconditional readiness minimum. Vocabulary can be meaningful with no example if its selected answer/cues are sufficient; grammar requires at least one validated use example. Preserve all relevant source examples. Generated_examples_max limits only new supplement, never source archival. Missing optional enrichment preserves old values and warns; missing core meaning/formation/task prerequisites blocks.

Novel generated grammar facts and ungrounded readings/definitions require an explicit content review. Confidence from OCR/vision/LLM is evidence, not authorization. Exact source-grounded extraction with validated task cues may be ready without a redundant approval question; ambiguous transcription/operators/segmentation needs review. Authored facts retain authored provenance. Language correctness is never inferred from schema validity alone.

## Providers and model selection

Baseline OCR is installed Tesseract with purpose-specific packs; no runtime pack download. Optional vision adjudication is off. Baseline local generation candidate is **gemma4:12b**, selected from the dated installed inventory as an initial reproducible candidate, not claimed optimal. Missing model/capability gives an actionable error for requested generation; authored/dictionary-only preparation still works. Pin actual installed model manifest/digest and prompt/schema hashes per plan. No random first-model choice, implicit smaller-model substitution or pull.

Set temperature=0.0; schema-constrained Ollama format plus local runtime validation; two bounded structural repair attempts maximum through the shared read-retry budget. Do not repair facts by inventing them. [Ollama's structured-output guidance](https://docs.ollama.com/capabilities/structured-outputs) supports constrained schemas and low-temperature output; correctness is still independently validated.

Japanese dictionary: rich Jisho adapter retaining forms/senses/readings/labels. English: versioned MediaWiki/Wiktionary adapter retaining language sections, sense IDs, examples and pronunciation evidence; missing/changed structure errors explicitly. The custom selector schema allows fixed text/attribute/list extraction only, no executable code. Site structure/resource schema versions are pinned; metadata/license attribution stays with evidence. Browser fallback is deferred/off in baseline; selecting it returns CAPABILITY_UNAVAILABLE until its feature is delivered.

Preserve original picture/audio first. Commons is the baseline image candidate provider with attribution/license metadata and reviewed relevance; no image is mandatory by default. Dictionary pronunciation is an explicit fallback; local Piper/custom TTS is optional, never auto-used. PaddleOCR/vision/browser/custom remote providers are optional feature gates, not baseline prerequisites. All their registered settings remain typed; unsupported selected features error before requests. No promise all optional technologies ship in first baseline.

## Quality gate

Create 120 annotated fixtures: 30 per workflow, Japanese/English balanced, including Vietnamese explanations, kana/kanji, homographs, mixed models, mixed images, multi-pattern source, no dictionary match, shared media, task leakage and adversarial source text. Separate deterministic correctness tests from a local-model benchmark. Require zero critical source-loss/unsupported-claim/task-leakage cases, 100% recoverable provenance and passing semantic fixtures; benchmark schema compliance at least 95% after bounded repair. Latency/memory are measured, not fabricated targets. A failing candidate remains blocked or needs_review; selecting another model requires a new recorded fingerprint and benchmark, not a silent live-job change.
