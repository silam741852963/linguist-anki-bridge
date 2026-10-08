//! Pure model-inventory verification. HTTP, generation and resource installation are separate.
use crate::generation::ModelIdentity;
use linguist_config::Effective;
use linguist_core::canonical;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Checked response envelope, not approval of its content or proof of input fit.
#[derive(Debug, Serialize)]
pub struct Completion {
    pub content: String,
    pub prompt_tokens: u64,
    pub cached_prompt_tokens: Option<u64>,
    pub output_tokens: u64,
    pub raw_digest: String,
    pub input_fit_verified: bool,
    #[serde(skip)]
    pub raw: Vec<u8>,
}

/// Accept only a complete, text-only, nonstreaming assistant response.
/// Provider extensions and thinking are archived but never used as card facts.
pub fn parse_completion(bytes: &[u8], settings: &Effective) -> Result<Completion, String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "llm.model",
        "llm.context_tokens",
        "llm.max_output_tokens",
        "network.max_response_mb",
    ] {
        registry.validate_value(
            key,
            settings.values.get(key).ok_or("OLLAMA_SETTING_MISSING")?,
        )?;
    }
    let name = settings.values["llm.model"]
        .as_str()
        .ok_or("OLLAMA_MODEL_MISSING")?;
    let value = response(
        bytes,
        settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
    )?;
    if value["model"].as_str() != Some(name) {
        return Err("OLLAMA_COMPLETION_MODEL_CONFLICT".into());
    }
    if value["done"] != true || value["done_reason"].as_str() != Some("stop") {
        return Err("OLLAMA_COMPLETION_INCOMPLETE".into());
    }
    let message = &value["message"];
    if message["role"].as_str() != Some("assistant") {
        return Err("OLLAMA_COMPLETION_SCHEMA_INVALID".into());
    }
    for key in ["tool_calls", "images"] {
        if let Some(extra) = message.get(key)
            && extra.as_array().is_none_or(|items| !items.is_empty())
        {
            return Err("OLLAMA_COMPLETION_NON_TEXT".into());
        }
    }
    if message.get("thinking").is_some_and(|v| !v.is_string()) {
        return Err("OLLAMA_COMPLETION_SCHEMA_INVALID".into());
    }
    let content = message["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("OLLAMA_COMPLETION_SCHEMA_INVALID")?;
    let positive = |key: &str| {
        value[key]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("OLLAMA_COMPLETION_COUNTER_INVALID")
    };
    let prompt_tokens = positive("prompt_eval_count")?;
    let output_tokens = positive("eval_count")?;
    let cached_prompt_tokens = value
        .get("prompt_eval_cached_count")
        .map(|v| v.as_u64().ok_or("OLLAMA_COMPLETION_COUNTER_INVALID"))
        .transpose()?;
    if cached_prompt_tokens.is_some_and(|cached| cached > prompt_tokens)
        || output_tokens > settings.values["llm.max_output_tokens"].as_u64().unwrap()
        || prompt_tokens
            .checked_add(output_tokens)
            .is_none_or(|total| total > settings.values["llm.context_tokens"].as_u64().unwrap())
    {
        return Err("OLLAMA_COMPLETION_TOKEN_LIMIT".into());
    }
    Ok(Completion {
        content: content.to_owned(),
        prompt_tokens,
        cached_prompt_tokens,
        output_tokens,
        raw_digest: canonical::asset_digest(bytes),
        input_fit_verified: false,
        raw: bytes.to_vec(),
    })
}

#[derive(Debug, Serialize)]
pub struct ModelEvidence {
    pub identity: ModelIdentity,
    pub architecture: String,
    pub context_limit: u64,
    pub capabilities: Vec<String>,
    pub tags_before_digest: String,
    pub show_digest: String,
    pub tags_after_digest: String,
    pub input_tokens_verified: bool,
    pub parameter_support_verified: bool,
    pub generation_ready: bool,
    #[serde(skip)]
    pub assets: BTreeMap<String, Vec<u8>>,
}
fn response(bytes: &[u8], limit: u64) -> Result<Value, String> {
    if bytes.len() as u64 > limit {
        return Err("OLLAMA_RESPONSE_LIMIT".into());
    }
    let value: Value = canonical::parse(bytes).map_err(|_| "OLLAMA_RESPONSE_SCHEMA_INVALID")?;
    if !value.is_object() {
        return Err("OLLAMA_RESPONSE_SCHEMA_INVALID".into());
    }
    if value.get("error").is_some() {
        return Err("OLLAMA_PROVIDER_FAILED".into());
    }
    Ok(value)
}
fn selected<'a>(tags: &'a Value, name: &str) -> Result<&'a Value, String> {
    let models = tags["models"]
        .as_array()
        .ok_or("OLLAMA_TAGS_SCHEMA_INVALID")?;
    if models.len() > 1000 {
        return Err("OLLAMA_MODEL_COUNT_LIMIT".into());
    }
    let mut names = BTreeSet::new();
    let mut selected = None;
    for model in models {
        let candidate = model["name"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
            .ok_or("OLLAMA_TAGS_SCHEMA_INVALID")?;
        if !names.insert(candidate) {
            return Err("OLLAMA_DUPLICATE_MODEL_NAME".into());
        }
        if model
            .get("model")
            .is_some_and(|v| v.as_str() != Some(candidate))
        {
            return Err("OLLAMA_MODEL_NAME_CONFLICT".into());
        }
        if candidate == name {
            selected = Some(model);
        }
    }
    selected.ok_or_else(|| "CAPABILITY_UNAVAILABLE: selected Ollama model is not installed; no pull or substitution was performed".into())
}
fn identity(model: &Value) -> Result<String, String> {
    for key in ["remote_host", "remote_model"] {
        if let Some(value) = model.get(key) {
            let value = value.as_str().ok_or("OLLAMA_TAGS_SCHEMA_INVALID")?;
            if !value.is_empty() {
                return Err(
                    "CAPABILITY_UNAVAILABLE: remote Ollama model forwarding is not supported"
                        .into(),
                );
            }
        }
    }
    if model["details"]["format"] != "gguf" || model["size"].as_u64().is_none_or(|size| size == 0) {
        return Err("CAPABILITY_UNAVAILABLE: local GGUF model metadata is required".into());
    }
    let digest = model["digest"]
        .as_str()
        .ok_or("OLLAMA_MODEL_DIGEST_INVALID")?;
    let raw = digest.strip_prefix("sha256:").unwrap_or(digest);
    if raw.len() != 64
        || !raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("OLLAMA_MODEL_DIGEST_INVALID".into());
    }
    Ok(format!("sha256:{raw}"))
}

/// Bind `/api/show` metadata between two inventories with the same exact selected name/digest.
/// This is engine-reported metadata evidence, not proof of prompt fit or option support.
pub fn verify_local_model(
    before: &[u8],
    show: &[u8],
    after: &[u8],
    settings: &Effective,
) -> Result<ModelEvidence, String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "llm.model",
        "llm.context_tokens",
        "llm.max_output_tokens",
        "network.max_response_mb",
    ] {
        registry.validate_value(
            key,
            settings.values.get(key).ok_or("OLLAMA_SETTING_MISSING")?,
        )?;
    }
    let name = settings.values["llm.model"]
        .as_str()
        .ok_or("CAPABILITY_UNAVAILABLE: select an installed generation model")?;
    let limit = settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024;
    let before_value = response(before, limit)?;
    let after_value = response(after, limit)?;
    let first = selected(&before_value, name)?;
    let second = selected(&after_value, name)?;
    let digest = identity(first)?;
    if identity(second)? != digest || first != second {
        return Err("OLLAMA_MODEL_MANIFEST_CONFLICT".into());
    }
    let details = response(show, limit)?;
    for key in ["remote_host", "remote_model"] {
        if details
            .get(key)
            .is_some_and(|value| value.as_str() != Some(""))
        {
            return Err("CAPABILITY_UNAVAILABLE: remote model metadata is not supported".into());
        }
    }
    if details["details"]["format"] != "gguf" {
        return Err("OLLAMA_SHOW_SCHEMA_INVALID".into());
    }
    let family = first["details"]["family"]
        .as_str()
        .filter(|family| !family.trim().is_empty())
        .ok_or("OLLAMA_TAGS_SCHEMA_INVALID")?;
    if details["details"]["family"].as_str() != Some(family) {
        return Err("OLLAMA_MODEL_FAMILY_CONFLICT".into());
    }
    let architecture = details["model_info"]["general.architecture"]
        .as_str()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .ok_or("CAPABILITY_UNAVAILABLE: model architecture metadata is missing")?;
    let context_limit = details["model_info"][format!("{architecture}.context_length")]
        .as_u64()
        .filter(|length| *length > 0)
        .ok_or("CAPABILITY_UNAVAILABLE: model context metadata is missing")?;
    let capabilities: Vec<String> = serde_json::from_value(details["capabilities"].clone())
        .map_err(|_| "OLLAMA_SHOW_SCHEMA_INVALID")?;
    let unique: BTreeSet<_> = capabilities.iter().collect();
    if capabilities.len() > 64
        || unique.len() != capabilities.len()
        || capabilities
            .iter()
            .any(|c| c.trim().is_empty() || c.chars().any(char::is_control))
    {
        return Err("OLLAMA_SHOW_SCHEMA_INVALID".into());
    }
    if !capabilities.iter().any(|c| c == "completion") {
        return Err("CAPABILITY_UNAVAILABLE: model lacks text completion capability".into());
    }
    let context = settings.values["llm.context_tokens"].as_u64().unwrap();
    if context > context_limit
        || settings.values["llm.max_output_tokens"].as_u64().unwrap() >= context
    {
        return Err("OLLAMA_CONTEXT_LIMIT_CONFLICT".into());
    }
    let tags_before_digest = canonical::asset_digest(before);
    let show_digest = canonical::asset_digest(show);
    let tags_after_digest = canonical::asset_digest(after);
    Ok(ModelEvidence {
        identity: ModelIdentity {
            name: name.into(),
            digest,
        },
        architecture: architecture.into(),
        context_limit,
        capabilities,
        tags_before_digest: tags_before_digest.clone(),
        show_digest: show_digest.clone(),
        tags_after_digest: tags_after_digest.clone(),
        input_tokens_verified: false,
        parameter_support_verified: false,
        generation_ready: false,
        assets: BTreeMap::from([
            (tags_before_digest, before.to_vec()),
            (show_digest, show.to_vec()),
            (tags_after_digest, after.to_vec()),
        ]),
    })
}
pub mod certify;
pub mod transport;
