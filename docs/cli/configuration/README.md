# Configuration contract

Status: target TOML v2, not the current YAML/native JSON loader. [settings-registry.json](settings-registry.json) indexes the exhaustive machine-readable setting groups in `settings/`; [config.example.toml](config.example.toml) lists every concrete default. Every entry specifies type, default, constraints, scope and consuming algorithm. Null means an intentionally unconfigured optional value, not a pending architecture choice or random discovery. Final decisions: [baseline](../decisions/README.md).

## ALG-CONFIG — resolve and freeze

1. Resolve config file from explicit `--config`, `LAB_CONFIG`, then `$XDG_CONFIG_HOME/linguist-anki-bridge/config.toml`; XDG fallbacks are `~/.config`, `~/.local/state`, `~/.cache`, `~/.local/share`. Missing file uses builtin defaults for reads/help; required mappings/model choices fail only the operation that needs them. Help/completions do not initialize state or contact services.
2. Parse strict TOML, validate version and reject unknown keys. Registry path patterns accept only validated record names. Reject YAML tags, command substitution and arbitrary interpolation. Path expansion supports only HOME/XDG tokens defined here, and leading `~`; never invokes a shell.
3. Merge global defaults → builtin purpose presets → base file → selected named profile → selected purpose overrides → `LAB_*` environment → explicit command flags. Exactly one profile and purpose per item; multi-purpose jobs resolve/freeze each separately. Profile/purpose overrides may contain only entries whose registry scope is purpose. Arrays replace; maps merge validated known keys. Missing values inherit; TOML has no null literal; `config unset` removes an override.
4. Environment spelling is `LAB_` + uppercase registry key with dots replaced by `__`, e.g. `LAB_LLM__MODEL`; scalar values parse according to type and array/map values use strict JSON. Wildcard mapping records are edited via config/decks commands, not arbitrary environment object expansion. `LAB_CONFIG`/`LAB_PROFILE` select sources; they are control options, not alternate durable settings.
5. Validate types/ranges/enums/formats and cross-field constraints: heartbeat < lease/3; initial backoff <= maximum; examples_min <= generated_examples_max; output token budget plus input fits context; relevant provider requires endpoint/model/voice/schema/resources; supported language/purpose matches provider and OCR packs. An unsupported provider parameter errors, never silently ignored.
6. Reject remote service endpoints without explicit allowed-host configuration; loopback Anki/Ollama allowed. Offline permits allowed loopback only and cached/local resources; blocks external dictionaries/images/custom remote services and all downloads. Redirects/proxies cannot bypass host/private-network policy. Public dictionary/image hosts are adapter allowlists pinned in resource manifests; adding custom hosts requires explicit config.
7. Resolve credential *names* to process environment only at adapter use. Store names, not secret values. Redact URL credentials, keys and authorization headers on output/log/export at every level. Reject credentials embedded in URLs. Private payload debug does not disable secret redaction.
8. Produce effective values with provenance, semantic/execution fingerprints and prompt/schema/model/resource hashes. Persist them with plans/jobs. Moving state paths with any existing nonempty state is blocked until explicit migration is implemented; use a future explicit storage migration, not config reset.
9. All local config writes use private permissions, temp file, fsync, atomic rename and directory sync; preserve comments where parser supports it. Existing files require a scoped replacement flag and backup copy. Failed validation never changes the live file.

## Scope and mutability

Global keys configure process/output/storage/network/jobs. Purpose keys configure learning/provider behavior and can be overridden in named profiles or purpose records. Mapping keys occur under `purposes.<purpose>`; the initial supported names are japanese_vocab, japanese_grammar, english_vocab and english_grammar. Additional names must map to a validated language/kind pair; unsupported languages can be archived/inspected but cannot be advertised as validated learning output.

`profiles.<name>.overrides` and `purposes.<purpose>.overrides` accept only the typed purpose-scoped keys, represented as nested TOML or explicitly quoted dotted keys with identical canonical interpretation; reject duplicate representations of one key. Mapping names must match `[A-Za-z][A-Za-z0-9_-]{0,63}`. Source field names and deck names preserve Unicode exactly.

Flags for provider/learning settings have identical validation to their registry key; `--set KEY=VALUE` provides a generic override so every eligible setting is accessible without a dedicated flag. Operational flags (selector, file path, IDs, --apply, --execute, --accept-warning CODE, revision, pagination cursor) are request inputs, not persistent settings. They are documented in operations and must never be inferred from config. There is deliberately no auto_apply, force, disable_journal, skip_backup or delete_source_media setting.

The [builtin purpose presets](purpose-defaults.json) include Japanese grammar vi explanations and narrow OCR packs. They resolve before explicit base/profile/purpose/env/flag overrides. `config init` emits only the version; the full example deliberately lists explicit global defaults.

Model field order, task names, safe HTML allowlist, journal states and canonical hash algorithm are versioned contracts/resources. They are not free-form settings. Custom rendering resources can be introduced only with a schema/version, sanitizer and compatibility tests; the initial release uses fixed managed v2 templates. The hard safety ceilings remain enforced even when a numeric setting is changed within its range.

Frozen semantic settings cannot change during resume. Logging/output/retry budgets can be changed for a new execution envelope and recorded in its journal; document/prompt/provider/media changes create a new plan revision and invalidate approval. No command mutates an already-approved revision in place.

Current frozen records carry the original full-value fingerprint plus separate semantic and execution fingerprints. Keys under `output.`, `logging.` and `retry.` are execution settings; every other key is semantic unless explicitly classified later. The fingerprints hash the two value maps with distinct domains. New plan approvals bind semantic values, their provenance, the semantic fingerprint and resource hashes; execution values and provenance do not change that approval digest. Older records without split fingerprints retain their original full-settings approval projection. Stored jobs still replay their complete frozen settings. Creating a new execution envelope to override those settings on resume remains unimplemented.

## Legacy migration accounting

Import reads the original file without writing it, produces candidate v2 TOML plus a per-key report: transferred, transformed, unresolved, retired or unsupported. Unknown keys are errors requiring review, not dropped warnings. Sensitive values move to environment-reference instructions; do not print the original secret.

| Legacy key/family | Target/accounting |
| --- | --- |
| Python config_version/native version | Explicit source parser; target config.version=2, not copied blindly |
| anki.url/native anki_url | anki.endpoint |
| anki.backup_dir | storage.backup_dir; validate protected destination |
| llm.ollama_url/native ollama_url, llm.model/native ollama_model | llm.endpoint, llm.model; no first-installed-model auto-choice |
| llm.translation_language | learning.explanation_language, normalize supported language label |
| llm.system_prompt_vocab/grammar | Pinned prompt files referenced by llm.prompts.*; preserve original text, revalidate output contract |
| ocr.method/preprocess/ollama_model/ollama_url | ocr.engine/preprocess, llm.vision_model/endpoint purpose overrides; conflicting OCR/LLM endpoint is reported |
| image_classification.* | classification.* and llm.vision_model; changed vision default needs explicit report |
| kanji.enabled/source_lang/url_template/schema | kanji.enabled/explanation_language/url_template/schema; extract schema into pinned resource |
| kanji.prompt_en/prompt_vi | Separate pinned prompt resources; selected language resolves llm.prompts.kanji; never combine silently |
| filters.* | Same-named filters keys; imported true remove_parentheses stays explicit, builtin default now false |
| image_search.enabled_for_empty/suffix | images.search_when_missing/query_suffix |
| dictionary.preset/native dictionary_preset | dictionary.provider per supported purpose; global jisho for English flagged |
| dictionary.url_template/schema | dictionary.url_template/schema_path with pinned resource export |
| dictionary.retry_count/retry_backoff_seconds | retry.read_attempts/initial_backoff_seconds; resolve conflicting batch retry defaults explicitly |
| dictionary.browser_fallback | dictionary.browser_fallback plus explicit browser dependency settings |
| batch.max_attempts/retry_backoff_seconds/commit_interval_seconds | jobs.max_item_attempts, retry policy candidate, anki.commit_interval_seconds |
| batch.service_intervals.* | services.*.min_interval_seconds |
| decks.*.deck_name/note_type/ocr_langs/fields | purposes.* source mapping; destination cannot be inferred silently; split `jpn+eng+vie` into array; legacy meaning_image→picture, meaning_text→meaning, kanji_construction→kanji |
| native decks.*.model_name/ocr_languages | Same source mapping conversion |
| dry_run | Retired: all preparation/read commands are non-writing; invocation --apply is mandatory, regardless of old false value |
| Taiwan/other unvalidated purpose entries | Preserve as unsupported candidate records; do not map to Japanese |
| theme/Omarchy/TUI appearance | Retired CLI setting with report; retain original source file |

Any legacy key absent from this table still receives an explicit per-key report (`unknown`, blocking). The importer is `linguist_config::legacy` (OP-09); activation is OP-10 `config migrate --from-import`.

Setting consumer coverage is generated into [setting-coverage.md](setting-coverage.md): each of the 156 registry entries is consumed at a named site, interpreted by the resolver, or gated behind a feature this build lacks. `config validate` reports values that need a missing feature as `unavailable_settings` (exit 3). A non-empty `purposes.<purpose>.ocr_languages` replaces `ocr.languages` for that purpose (provenance `purpose-mapping`); explicit purpose overrides, environment and flags still win.

## Configuration acceptance tests

Generate describe/default output from the registry. For each key, test nondefault consumption at its documented consumer or explicitly gate the unavailable optional feature. Reject unknown keys, duplicate merged keys, invalid formats, impossible cross-field combinations and unsupported optional adapters. Parse the example and compare all emitted concrete values with defaults. Test precedence, provenance, credential redaction, offline redirects, XDG paths, atomic write failures and frozen-job replay.

Adding a setting requires registry entry + consumer + describe/help + example + a nondefault behavior test + migration accounting if applicable. A registered but unused setting is a release failure.

## Format and schema validation details

Integers reject float/string coercion; numbers reject NaN/infinity; booleans accept actual bool or explicit env true/false. Unknown enum values fail. Paths reject NUL/traversal outside configured storage roots for generated artifacts; user input files may be read outside storage, never overwritten implicitly. Executable values are names or resolved paths, never commands with arguments; build a separate argument array. Resource references are known builtin IDs or pinned readable file paths with hashes. Duration uses validated provider syntax; invalid/unsupported syntax fails.

URL values require http/https with parsed host/port, no embedded credentials or fragment-as-command. URL templates permit only provider-declared placeholders, percent-encode substituted text, and validate resolved destinations/redirects. Host allow rules are exact normalized hosts, not arbitrary regexes/wildcards. Environment reference names match `[A-Za-z_][A-Za-z0-9_]*`. Language values parse as BCP-47 then validate against each selected provider; a syntactically valid but unsupported language fails that stage. Arrays enforce typed elements and finite size, with uniqueness only where the registry specifies it (editor argv may repeat arguments); model/provider parameter support is checked before requests.

The registry generates [a JSON Schema for normalized configuration values](../../../contracts/v2/config-file-normalized.schema.json) with exact concrete and profile/purpose keys, closed mapping objects, types, ranges and enum choices. Generate it with `cargo run --locked -q -p linguist-config --example schema`; verify it against actual CLI defaults and invalid samples with `/usr/bin/python3.14 scripts/verify-config-schema.py`. This schema applies after strict TOML flattening to `ConfigFile.values`, not to raw TOML text. The Rust resolver additionally checks cross-field numeric relationships, exact string formats, and selected provider resources. Scope/profile/map validation cannot be skipped because an example TOML parses or the structural schema accepts it.

Current resolver presence checks require URL and schema for a custom dictionary, endpoint for custom images/audio, executable and voice resource for Piper, vision model for Ollama OCR, resource path for PaddleOCR, executable for an enabled browser, and model for enabled LLM generation. Typed URL-template validation requires `{word}` for `dictionary.url_template` and `{char}` for `kanji.url_template`, rejects additional expressions and rejects placeholders in the URL authority. A selected custom dictionary template and a changed kanji template also require explicit remote-host permission; the builtin kanji template remains an adapter-owned host. Offline configuration rejects those remote custom hosts. Substitution, destination and redirect checks remain adapter work. `config validate` separately checks whether configured local prompt/schema/voice/OCR paths are readable, whether browser/Piper/Tesseract helpers resolve to executable regular files, and whether the nearest existing state-path ancestor currently has the configured free-space reserve; capability gaps exit 3. PATH lookup uses only absolute search directories and never runs the helper. The free-space observation does not guarantee later writes or future mount state. It does not yet verify OCR language packs, resource hashes, installed model/executable compatibility, provider parameters or service behavior.

Source role mappings now include pronunciation/kanji/sense/use/cues/language/task flags. Shared legacy recording+IPA fields are parsed into role-specific evidence while the original field remains archived; never discard non-audio text because the same field also contains a sound tag. All source role maps are validated against actual fields.
