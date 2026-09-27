# Application specification reference

### 8.3 Dictionary lookup and structure

Resolve default provider by purpose: Japanese → Jisho; English → Cambridge; Taiwanese Mandarin → Moedict; German → dict.cc. Allow explicit overrides/custom provider schemas. Store source URLs/provider identity and lookup time.

Preserve exact and related entries; written/reading forms; numbered senses; parts of speech; common/JLPT/tags; cross-references; restrictions; antonyms; information; available pronunciations. Exact entry appears first and receives LLM annotations; related entries retain their own structure. Do not silently substitute the first non-exact result. **REVIEW R17** determines handling when only related entries exist.

Jisho Python behavior: direct JSON API with bounded retry/backoff, then optional Crawl4AI JSON/browser and rendered HTML fallback. Rust exposes a browser port, but the desktop live call passes `None`. **REVIEW R18:** choose a headless non-Qt fallback implementation, optional Python helper, or explicitly degraded direct-only support. A browser must not be a hidden mandatory CLI dependency.

Custom dictionary extraction validates a URL template and CSS schema; encode the query before substitution. Treat custom network destinations as explicit configuration. Provider not-found, invalid HTML/JSON, timeout, blocked access, and rate limit are different results. Do not convert all of them to an authoritative “not found.”
