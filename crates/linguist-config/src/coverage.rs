//! Registry-to-consumer coverage. Every registered setting is either read by a
//! named consumer, interpreted by the resolver, or configures a feature this
//! build lacks; for the last group, any value that would need the missing
//! feature is reported by `config validate` and refused by the consumer when an
//! operation needs it, so no setting is silently ignored.
use crate::{Effective, Registry};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Read by the named consumer at its use site.
    Consumed,
    /// Interpreted by ALG-CONFIG resolution itself.
    Resolver,
    /// Only meaningful for a feature that is unavailable in this build.
    Gated,
}

/// Which values need an unavailable feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// Every supported value works.
    None,
    /// Any value other than the registry default.
    NonDefault,
    /// These exact JSON values.
    Values(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Row {
    pub key: &'static str,
    pub status: Status,
    /// Repository-relative source file of the consumer (or of the refusal).
    pub consumer: &'static str,
    /// Text that must appear in `consumer` (checked by tests).
    pub needle: &'static str,
    pub unavailable: Unavailable,
    pub feature: &'static str,
}

const fn c(key: &'static str, consumer: &'static str) -> Row {
    Row {
        key,
        status: Status::Consumed,
        consumer,
        needle: key,
        unavailable: Unavailable::None,
        feature: "",
    }
}
const fn n(key: &'static str, consumer: &'static str, needle: &'static str) -> Row {
    Row {
        key,
        status: Status::Consumed,
        consumer,
        needle,
        unavailable: Unavailable::None,
        feature: "",
    }
}
const fn v(
    key: &'static str,
    consumer: &'static str,
    values: &'static [&'static str],
    feature: &'static str,
) -> Row {
    Row {
        key,
        status: Status::Consumed,
        consumer,
        needle: key,
        unavailable: Unavailable::Values(values),
        feature,
    }
}
/// Consumed, but every non-default value is refused.
const fn d(key: &'static str, consumer: &'static str, feature: &'static str) -> Row {
    Row {
        key,
        status: Status::Consumed,
        consumer,
        needle: key,
        unavailable: Unavailable::NonDefault,
        feature,
    }
}
const fn g(
    key: &'static str,
    consumer: &'static str,
    needle: &'static str,
    feature: &'static str,
) -> Row {
    Row {
        key,
        status: Status::Gated,
        consumer,
        needle,
        unavailable: Unavailable::NonDefault,
        feature,
    }
}
const fn r(key: &'static str, needle: &'static str) -> Row {
    Row {
        key,
        status: Status::Resolver,
        consumer: "crates/linguist-config/src/lib.rs",
        needle,
        unavailable: Unavailable::None,
        feature: "",
    }
}

const ANKI: &str = "crates/linguist-anki/src/lib.rs";
const APPLY: &str = "crates/linguist-application/src/apply.rs";
const JOBS: &str = "crates/linguist-application/src/job_executor.rs";
const PREP: &str = "crates/linguist-application/src/jobs.rs";
const SPEECH: &str = "crates/linguist-application/src/speech.rs";
const VOCAB: &str = "crates/linguist-application/src/vocab.rs";
const CLI: &str = "crates/linguist-cli/src/main.rs";
const DIAG: &str = "crates/linguist-cli/src/diagnostics.rs";
const CACHE: &str = "crates/linguist-application/src/cache.rs";
const PCACHE: &str = "crates/linguist-provider/src/cache.rs";
const PROVIDER: &str = "crates/linguist-provider/src/lib.rs";
const INSPECT: &str = "crates/linguist-application/src/ocr_inspection.rs";
const OCR: &str = "crates/linguist-application/src/ocr.rs";
const DICT: &str = "crates/linguist-application/src/dictionary.rs";
const DTRANSPORT: &str = "crates/linguist-dictionary/src/transport.rs";
const KANJI: &str = "crates/linguist-dictionary/src/kanji.rs";
const IMAGES: &str = "crates/linguist-application/src/images.rs";
const GEN: &str = "crates/linguist-application/src/generation.rs";
const OLLAMA: &str = "crates/linguist-application/src/ollama/transport.rs";
const APP: &str = "crates/linguist-application/src/lib.rs";
const RES: &str = "crates/linguist-application/src/resources.rs";
const CRES: &str = "crates/linguist-config/src/resources.rs";
const STORE: &str = "crates/linguist-store/src/lib.rs";
const MEDIA: &str = "crates/linguist-application/src/media.rs";
const AUDIO: &str = "crates/linguist-application/src/audio.rs";
const DUP: &str = "crates/linguist-application/src/duplicate_candidates.rs";
const MAPPING: &str = "crates/linguist-application/src/mapping.rs";
const ARCHIVE: &str = "crates/linguist-application/src/source_archive.rs";
const RECORDS: &str = "crates/linguist-core/src/records.rs";
const REPAIR: &str = "crates/linguist-application/src/generation/repair.rs";

pub const ROWS: &[Row] = &[
    c("anki.endpoint", ANKI),
    c("anki.api_key_env", ANKI),
    c("anki.expected_profile", ANKI),
    c("anki.request_timeout_seconds", ANKI),
    c("anki.read_batch_size", ANKI),
    c("anki.commit_interval_seconds", JOBS),
    d(
        "anki.native_adapter",
        APPLY,
        "managed writes through a companion protocol other than lab-native-v1",
    ),
    v(
        "audio.provider",
        SPEECH,
        &["\"custom\""],
        "custom TTS audio adapter",
    ),
    c("audio.voice", SPEECH),
    g(
        "audio.endpoint",
        VOCAB,
        "audio.provider=custom",
        "custom TTS service (audio.provider=custom)",
    ),
    g(
        "audio.api_key_env",
        VOCAB,
        "audio.provider=custom",
        "custom TTS service credentials",
    ),
    c("audio.executable", SPEECH),
    c("audio.voice_resource", SPEECH),
    c("audio.speed", SPEECH),
    c("audio.timeout_seconds", SPEECH),
    c("backup.scope", CLI),
    c("backup.reuse_max_age_seconds", JOBS),
    c("backup.verify_timeout_seconds", CLI),
    c("backup.max_package_gb", CLI),
    c("backup.max_collection_gb", CLI),
    c("backup.max_media_gb", CLI),
    c("backup.max_media_map_mb", CLI),
    c("backup.max_entries", CLI),
    c("backup.verify_scratch_dir", CLI),
    v(
        "browser.enabled",
        CLI,
        &["true"],
        "controlled browser helper",
    ),
    g(
        "browser.executable",
        CRES,
        "browser.executable",
        "controlled browser helper",
    ),
    g(
        "browser.timeout_seconds",
        CLI,
        "BROWSER_HELPER_UNAVAILABLE",
        "controlled browser helper",
    ),
    g(
        "browser.max_pages",
        CLI,
        "BROWSER_HELPER_UNAVAILABLE",
        "controlled browser helper",
    ),
    c("cache.policy", PCACHE),
    c("cache.ttl_hours", PCACHE),
    c("cache.max_size_mb", CACHE),
    c("cache.unreferenced_retention_days", CACHE),
    c("classification.decision_threshold", INSPECT),
    c("classification.confirmation_margin", INSPECT),
    v(
        "classification.vision_adjudication",
        INSPECT,
        &["true"],
        "vision adjudication adapter",
    ),
    g(
        "classification.vision_accept_confidence",
        INSPECT,
        "classification.vision_adjudication",
        "vision adjudication adapter",
    ),
    r("config.version", "config.version"),
    r("config.default_profile", "config.default_profile"),
    v(
        "dictionary.provider",
        DTRANSPORT,
        &["\"custom\""],
        "custom dictionary adapter",
    ),
    g(
        "dictionary.url_template",
        DTRANSPORT,
        "\"custom\"",
        "custom dictionary adapter",
    ),
    g(
        "dictionary.schema_path",
        DTRANSPORT,
        "\"custom\"",
        "custom dictionary adapter",
    ),
    v(
        "dictionary.browser_fallback",
        DTRANSPORT,
        &["true"],
        "controlled browser helper",
    ),
    c("dictionary.user_agent", PROVIDER),
    c("editing.editor_argv", CLI),
    c("filters.remove_parentheses", DICT),
    c("filters.clean_word_only", DICT),
    c("filters.normalize_unicode", DICT),
    c("helpers.memory_limit_mb", OCR),
    c("helpers.max_output_mb", OCR),
    c("images.existing_policy", INSPECT),
    c("images.search_when_missing", IMAGES),
    v(
        "images.provider",
        VOCAB,
        &["\"custom\""],
        "custom image search adapter",
    ),
    g(
        "images.custom_endpoint",
        VOCAB,
        "images.provider=custom",
        "custom image search adapter",
    ),
    c("images.query_suffix", IMAGES),
    c("images.candidate_limit", IMAGES),
    c("input.max_file_mb", CLI),
    c("input.max_record_chars", CLI),
    c("jobs.prepare_workers", PREP),
    c("jobs.max_item_attempts", JOBS),
    c("jobs.on_item_error", JOBS),
    c("jobs.lease_seconds", JOBS),
    c("jobs.heartbeat_seconds", JOBS),
    c("jobs.pause_poll_ms", PREP),
    c("kanji.enabled", VOCAB),
    c("kanji.explanation_language", KANJI),
    c("kanji.url_template", KANJI),
    c("kanji.stroke_order", KANJI),
    d(
        "kanji.schema",
        VOCAB,
        "kanji schemas other than builtin:jisho-kanji-v2",
    ),
    c("learning.examples_min", GEN),
    c("learning.generated_examples_max", GEN),
    c("learning.explanation_language", APP),
    c("learning.vocabulary.production", APP),
    c("learning.vocabulary.spelling", APP),
    c("learning.grammar.application", APP),
    c("llm.endpoint", OLLAMA),
    c("llm.model", GEN),
    g(
        "llm.vision_model",
        "crates/linguist-config/src/lib.rs",
        "llm.vision_model",
        "vision OCR / adjudication adapter",
    ),
    c("llm.api_key_env", OLLAMA),
    c("llm.timeout_seconds", OLLAMA),
    c("llm.temperature", OLLAMA),
    c("llm.context_tokens", OLLAMA),
    c("llm.max_output_tokens", OLLAMA),
    c("llm.seed", OLLAMA),
    c("llm.keep_alive", OLLAMA),
    d(
        "llm.prompts.vocabulary",
        GEN,
        "pinned custom prompt loading",
    ),
    d("llm.prompts.grammar", GEN, "pinned custom prompt loading"),
    g(
        "llm.prompts.kanji",
        CRES,
        "llm.prompts.kanji",
        "LLM kanji summaries",
    ),
    c("llm.repair_attempts", REPAIR),
    c("llm.enabled", GEN),
    c("logging.level", DIAG),
    c("logging.file_enabled", DIAG),
    c("logging.max_file_mb", DIAG),
    c("logging.retained_files", DIAG),
    c("logging.include_private_payloads", DIAG),
    c("media.max_asset_mb", MEDIA),
    c("media.allowed_image_types", MEDIA),
    c("media.allowed_audio_types", AUDIO),
    c("network.offline", PROVIDER),
    c("network.allowed_remote_service_hosts", PROVIDER),
    d(
        "network.proxy_env",
        PROVIDER,
        "policy-checked proxy transport",
    ),
    c("network.connect_timeout_seconds", PROVIDER),
    c("network.request_timeout_seconds", PROVIDER),
    c("network.max_response_mb", ANKI),
    c("network.max_redirects", PROVIDER),
    v(
        "ocr.engine",
        OCR,
        &["\"ollama\"", "\"paddleocr\""],
        "Ollama vision and PaddleOCR engines",
    ),
    c("ocr.languages", OCR),
    c("ocr.preprocess", OCR),
    c("ocr.timeout_seconds", OCR),
    c("ocr.page_segmentation_mode", OCR),
    c("ocr.engine_mode", OCR),
    c("ocr.minimum_confidence", OCR),
    c("ocr.max_pixels", OCR),
    c("ocr.max_regions", OCR),
    c("ocr.executable", OCR),
    c("ocr.resource_path", OCR),
    c("output.format", CLI),
    c("output.color", DIAG),
    c("output.progress", DIAG),
    c("output.quiet", DIAG),
    c("output.page_size", CLI),
    c("output.language", DIAG),
    r("profiles.<name>.overrides", "profiles.{p}.overrides"),
    n("purposes.<purpose>.source_deck", CLI, ".source_deck"),
    n("purposes.<purpose>.target_deck", APPLY, "target_deck"),
    n("purposes.<purpose>.source_model", MAPPING, "source_model"),
    r("purposes.<purpose>.target_language", ".target_language"),
    r("purposes.<purpose>.ocr_languages", ".ocr_languages"),
    n("purposes.<purpose>.fields", MAPPING, ".fields"),
    n("purposes.<purpose>.card_tasks", ARCHIVE, "card_tasks"),
    r("purposes.<purpose>.overrides", "purposes.{p}.overrides"),
    c("resources.max_download_mb", RES),
    c("resources.max_unpacked_mb", RES),
    c("retry.read_attempts", PROVIDER),
    c("retry.initial_backoff_seconds", PROVIDER),
    c("retry.max_backoff_seconds", PROVIDER),
    c("retry.jitter_fraction", PROVIDER),
    c("selection.order", RECORDS),
    c("selection.max_notes", RECORDS),
    c("selection.duplicate_scope", DUP),
    v(
        "selection.duplicate_policy",
        DUP,
        &["\"skip_exact\""],
        "proven exact duplicate skipping",
    ),
    n(
        "services.dictionary.min_interval_seconds",
        PROVIDER,
        "services.{}.min_interval_seconds",
    ),
    n(
        "services.dictionary.concurrency",
        PROVIDER,
        "services.{}.concurrency",
    ),
    c("services.ollama.min_interval_seconds", OLLAMA),
    Row {
        unavailable: Unavailable::NonDefault,
        feature: "concurrent Ollama requests",
        ..c("services.ollama.concurrency", OLLAMA)
    },
    n(
        "services.kanji.min_interval_seconds",
        PROVIDER,
        "services.{}.min_interval_seconds",
    ),
    n(
        "services.kanji.concurrency",
        PROVIDER,
        "services.{}.concurrency",
    ),
    n(
        "services.image.min_interval_seconds",
        PROVIDER,
        "services.{}.min_interval_seconds",
    ),
    n(
        "services.image.concurrency",
        PROVIDER,
        "services.{}.concurrency",
    ),
    n(
        "services.tts.min_interval_seconds",
        PROVIDER,
        "services.{}.min_interval_seconds",
    ),
    c("services.tts.concurrency", SPEECH),
    c("storage.state_dir", CLI),
    c("storage.cache_dir", PCACHE),
    c("storage.backup_dir", CLI),
    c("storage.temp_dir", OCR),
    c("storage.resource_dir", RES),
    c("storage.free_space_reserve_mb", RES),
    n(
        "storage.sqlite_busy_timeout_ms",
        DIAG,
        "storage.sqlite_busy_timeout_ms",
    ),
];

/// Store busy timeouts are applied through `linguist_store::set_busy_timeout_ms`.
pub const BUSY_TIMEOUT_CONSUMER: &str = STORE;

pub fn row(key: &str) -> Option<&'static Row> {
    let registry = Registry::builtin();
    let entry = registry.lookup(key).ok()?;
    ROWS.iter().find(|r| r.key == entry.key)
}

/// Values in `effective` that need an unavailable feature. Defaults never
/// appear here; a selected unavailable feature is also refused at its use site.
pub fn unavailable_settings(effective: &Effective) -> Vec<Value> {
    let registry = Registry::builtin();
    let mut out = Vec::new();
    for row in ROWS {
        if row.key.contains('<') {
            continue;
        }
        let Some(value) = effective.values.get(row.key) else {
            continue;
        };
        let default = registry
            .lookup(row.key)
            .map(|e| e.default.clone())
            .unwrap_or(Value::Null);
        let hit = match row.unavailable {
            Unavailable::None => false,
            Unavailable::NonDefault => *value != default,
            Unavailable::Values(values) => values
                .iter()
                .any(|v| serde_json::from_str::<Value>(v).ok().as_ref() == Some(value)),
        };
        if hit {
            let sensitive = registry.lookup(row.key).is_ok_and(|e| e.sensitive);
            out.push(json!({
                "key": row.key,
                "value": if sensitive { json!("<redacted>") } else { value.clone() },
                "provenance": effective.provenance.get(row.key),
                "status": if row.status == Status::Gated { "dormant_unavailable_feature" } else { "unavailable_value" },
                "feature": row.feature,
                "code": "SETTING_FEATURE_UNAVAILABLE",
            }));
        }
    }
    out
}

/// Markdown table for `docs/cli/configuration/setting-coverage.md`.
pub fn markdown() -> String {
    let mut out = String::from(
        "# Setting consumer coverage\n\nGenerated by `cargo run --locked -q -p linguist-config --example coverage`; a test keeps this file in sync with `crates/linguist-config/src/coverage.rs`.\n\nEvery registry entry is consumed at a named site, interpreted by ALG-CONFIG resolution, or configures a feature this build lacks. Values that need a missing feature are reported by `config validate` (`unavailable_settings`, exit 3) and refused by the consumer when an operation needs them.\n\n",
    );
    let consumed = ROWS.iter().filter(|r| r.status == Status::Consumed).count();
    let resolver = ROWS.iter().filter(|r| r.status == Status::Resolver).count();
    let gated = ROWS.iter().filter(|r| r.status == Status::Gated).count();
    let patterns = ROWS.iter().filter(|r| r.key.contains('<')).count();
    out.push_str(&format!(
        "Totals: {} registry entries ({} concrete keys, {} mapping patterns): {consumed} consumed, {resolver} resolver, {gated} gated.\n\n",
        ROWS.len(),
        ROWS.len() - patterns,
        patterns
    ));
    out.push_str("| Key | Status | Consumer | Unavailable values | Missing feature |\n| --- | --- | --- | --- | --- |\n");
    for row in ROWS {
        let unavailable = match row.unavailable {
            Unavailable::None => "—".to_owned(),
            Unavailable::NonDefault => "any non-default".to_owned(),
            Unavailable::Values(values) => values.join(", "),
        };
        out.push_str(&format!(
            "| `{}` | {} | `{}` | {} | {} |\n",
            row.key,
            serde_json::to_value(row.status).unwrap().as_str().unwrap(),
            row.consumer,
            unavailable,
            if row.feature.is_empty() {
                "—"
            } else {
                row.feature
            }
        ));
    }
    out
}
