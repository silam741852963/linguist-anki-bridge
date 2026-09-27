# Research reference

## 4. Technology choices and current research

Choose current stable tools where they solve a specific requirement. Do not adopt distributed workflow systems, vector databases, agent frameworks, or GPU runtimes solely because they are newer. This workload is a local data-conversion app; a typed bounded pipeline plus SQLite is sufficient.

| Area | Selected implementation | Evidence and rationale |
| --- | --- | --- |
| CLI runtime | Rust 2024, stable toolchain pinned for release; new `linguist-cli` crate | Installed Rust is 1.98.1; official [release notes](https://doc.rust-lang.org/releases.html) list 1.98.1. Retain/test workspace MSRV separately; upgrading compiler does not prove every dependency remains compatible. |
| Command parsing | clap derive, clap_complete, generated man pages | [clap docs](https://docs.rs/clap/latest/clap/) observed 4.6.7; typed commands/help/completion match the proposed interface. |
| Async composition | Existing Tokio with semaphores, cancellation and bounded queues | [Tokio docs](https://docs.rs/tokio/latest/tokio/) observed 1.53.1, matching current workspace line. CPU/OCR subprocess work stays outside async reactor blocking paths. |
| HTTP | Existing reqwest with rustls and pooled clients | [reqwest docs](https://docs.rs/reqwest/latest/reqwest/) observed 0.13.5 versus current 0.13.4. Select compatible patch after tests; local endpoints bypass inappropriate proxy settings. |
| Persistence | SQLite WAL, rusqlite, single writer; filesystem content-addressed blobs | [rusqlite docs](https://docs.rs/rusqlite/latest/rusqlite/) observed 0.40.2 versus current 0.32.1. Keep known working line initially; evaluate newer line in its own compatibility migration. No SQLx replacement is needed. |
| Schema | serde typed vocabulary/grammar enums; schemars plus runtime validator | [schemars docs](https://docs.rs/schemars/latest/schemars/) observed 1.2.2; emit [JSON Schema 2020-12](https://json-schema.org/draft/2020-12) contracts and test fixture compatibility. Schema generation is not runtime validation. |
| HTML/Markdown | deterministic rendering, pulldown-cmark and configured Ammonia allowlist | [Ammonia docs](https://docs.rs/ammonia/latest/ammonia/) observed 4.2.0. Keep native Anki sound tags/local images; preserve trusted template CSS while sanitizing untrusted content. |
| Observability | tracing spans with job/item/stage IDs; JSONL progress | [tracing docs](https://docs.rs/tracing/latest/tracing/) observed 0.1.44; useful per-stage latency/error metrics without dumping personal note data. |
| CLI tests | assert_cmd/fixtures, property tests for parsing/mapping, failure-injected ports | [assert_cmd docs](https://docs.rs/assert_cmd/latest/assert_cmd/) support subprocess contracts. Focus property testing on meaningful invariants, not trivial mirror tests. |
| Python tools | uv-managed optional helper environment; pinned dependencies/protocol | [uv](https://github.com/astral-sh/uv) offers isolated reproducible Python tooling. Helpers cannot import Textual UI modules or write Anki directly. |

The version table is a research snapshot. Pin selected versions in lockfiles at implementation time and rerun compatibility/security checks; do not mass-upgrade the current workspace in this planning task.

### 4.1 OCR: reliable baseline plus modern optional fallback

**Selected baseline:** Tesseract 5 (installed 5.5.3), with appropriate installed language data and source-image preprocessing/cropping. Obtain token boxes/confidence as well as text. Supported baseline behavior is described in [Tesseract's manual](https://tesseract-ocr.github.io/tessdoc/). For these screenshots, Japanese morphology/operators and Vietnamese diacritics matter more than page-level prose fluency.

**Selected modern candidate:** optional PaddleOCR-VL-1.6 document parsing, available in the maintainer's current [PaddleOCR-VL documentation](https://www.paddleocr.ai/main/en/version3.x/pipeline_usage/PaddleOCR-VL.html). Benchmark it on this screenshot corpus before enabling it by default. Published general document benchmarks are not evidence of performance on these grammar cards. Run in an isolated Python subprocess behind the OCR port, with schema-versioned JSON and explicit time/memory limits. Its dependencies/models are installed explicitly and lazy-loaded.

**Optional local VLM fallback:** installed Ollama Gemma 4 vision for difficult regions/classification, using an extract-only prompt and comparison to Tesseract/source pixels. A VLM's confident prose is not proof it read a missing/blurred Japanese character correctly. Never silently “repair” negation or grammatical operators. PaddleOCR is a benchmark candidate, not mandatory GPU infrastructure or a reason to replace good existing OCR wholesale.

### 4.2 Generation: local schema-constrained models, selected by evaluation

Live Ollama is 0.34.4. Installed models are `gemma4:12b`, `gemma4:e4b`, and `llama3.2-vision:latest`. `/api/show` reports completion/vision for all three; Gemma variants also report audio/tools/thinking. Capability reporting does not prove structured output quality, Japanese/Vietnamese extraction accuracy, or audio synthesis suitability.

**Candidate primary:** `gemma4:12b` for vocabulary annotations and grammar synthesis; candidate lower-cost/vision comparison `gemma4:e4b`; existing Llama vision as a comparison baseline. Choose final defaults from a small evaluated corpus rather than a marketing ranking. Record concrete model digest/version, prompt/schema hashes, seed/settings, and actual latency. No model was loaded or generated from during this inspection.

Use local Ollama JSON Schema `format`, validate typed responses, and bounded repair retry; the [official structured-output documentation](https://docs.ollama.com/capabilities/structured-outputs) covers schema and vision use. Temperature 0 is the starting point; it reduces variation but does not make results factually correct or bitwise reproducible. Keep tool invocation disabled in content-generation tasks. Source text cannot authorize tools or writes.

Grammar schemas separate `extracted_patterns`, `source_spans`, `formation`, `supported_meanings`, `uncertain_claims`, `examples` and `exercise`. Validation ensures required text exists, expected Japanese pattern survives, example/translation alignment is maintained, and claims without evidence are flagged. Vocabulary schema only allows nuances/examples, with dictionary senses outside model ownership.

### 4.3 Audio, providers and assets

Preserve original recordings/IPA first; retrieve dictionary pronunciation second. Native eSpeak is a functional local fallback, not a claim of high naturalness. Optional [Piper neural TTS](https://github.com/OHF-Voice/piper1-gpl) is a candidate where a suitable licensed target-language voice exists. Current engine is GPL-3.0; review distribution obligations and each voice license before bundling. Do not assume a good Japanese voice is available. gTTS/Google remains an explicit online fallback; no training/fine-tuning infrastructure is required.

Use direct provider APIs or robust parsed HTML behind ports. Optional Playwright/Crawl4AI fallback runs in an isolated pinned Python helper; [Playwright documents separate browser installation](https://playwright.dev/python/docs/intro). Browser fallback is opt-in and does not bypass provider restrictions. Do not replace every existing parser before verifying actual failures.

Internet image lookup retains Wikipedia/Commons and source-attribution metadata; MediaWiki's [imageinfo API](https://www.mediawiki.org/wiki/API:Imageinfo) provides image metadata/extended properties. Validate payload/pixels and semantic relevance. Store assets as readable-prefix + SHA-256 names, with MIME/size/source/license in a manifest. Original media stays physically intact by default; app-written media can be garbage-collected only when no live plan/note/recovery reference needs it.
