# Application specification reference

## 13. Storage, privacy, and network policy

**SELECTED new layout:** configuration in `$XDG_CONFIG_HOME/linguist-anki-bridge`; durable jobs/plans/snapshots/receipts/logs in `$XDG_STATE_HOME/linguist-anki-bridge`; installed reusable resources in `$XDG_DATA_HOME/linguist-anki-bridge`; disposable downloads/OCR/provider cache in `$XDG_CACHE_HOME/linguist-anki-bridge`. Existing Python paths often hard-code `~/.config`, while Rust respects XDG for some stores. **REVIEW R34** confirms paths and compatibility migration.

Plans and snapshots are not disposable cache. Do not prune media referenced by recovery history or accepted plans. Import legacy YAML, native config, Python snapshots, and batch jobs explicitly, with source hashes/migration reports and no writes to original history. Preserve unsupported/unknown data rather than inventing successful conversion. Reject incompatible versions before mutation.

Keep app files private where practical; logs avoid full personal notes, OCR text, prompts, credentials and base64 by default. Verbose evidence export is explicit. No telemetry by default. A local Ollama model does not make the whole workflow offline: dictionaries, images, remote TTS, and language-pack downloads can use the internet. Show active providers and offer separate skip/offline policies.

Provider adapters must use bounded timeouts/payloads, a descriptive User-Agent, encoded queries, controlled redirects and validated destinations. Do not shell-interpolate expression/editor/voice values. Reject path traversal in filenames, snapshot IDs, artifacts, and archive imports. Keep local Anki/Ollama URLs configurable without silently accepting untrusted remote services as local.
