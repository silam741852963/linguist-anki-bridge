//! Explicit source-field selection. Raw values are not interpreted as normalized learning facts.
use linguist_core::canonical;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Vocabulary,
    Grammar,
}

/// Consume one resolved purpose mapping without inferring the source model or aliases.
pub fn map_purpose_fields(
    settings: &linguist_config::Effective,
    purpose: &str,
    source_model: &str,
    fields: &BTreeMap<String, String>,
) -> Result<FieldMapping, String> {
    let kind = match purpose {
        "japanese_vocab" | "english_vocab" => SourceKind::Vocabulary,
        "japanese_grammar" | "english_grammar" => SourceKind::Grammar,
        _ => return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into()),
    };
    let registry = linguist_config::Registry::builtin();
    for key in [
        format!("purposes.{purpose}.fields"),
        format!("purposes.{purpose}.source_model"),
        "input.max_file_mb".into(),
        "input.max_record_chars".into(),
    ] {
        registry.validate_value(
            &key,
            settings
                .values
                .get(&key)
                .ok_or("SOURCE_MAPPING_SETTING_MISSING")?,
        )?;
    }
    if settings.values[&format!("purposes.{purpose}.source_model")]
        .as_str()
        .is_some_and(|expected| expected != source_model)
    {
        return Err("SOURCE_MAPPING_MODEL_CONFLICT".into());
    }
    let mapping: BTreeMap<String, String> =
        serde_json::from_value(settings.values[&format!("purposes.{purpose}.fields")].clone())
            .map_err(|_| "SOURCE_MAPPING_SETTING_INVALID")?;
    map_fields(
        kind,
        fields,
        &mapping,
        (settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024)
            .min(100 * 1024 * 1024),
        settings.values["input.max_record_chars"].as_u64().unwrap() as usize,
    )
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MappedField {
    pub source_field: String,
    pub raw_value: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct FieldMapping {
    pub kind: SourceKind,
    pub source_digest: String,
    pub mapping_digest: String,
    pub roles: BTreeMap<String, MappedField>,
    pub unmapped_fields: Vec<String>,
    /// Shared source values need role-specific parsing, never blind duplication into facts.
    pub shared_fields: BTreeMap<String, Vec<String>>,
    pub missing_required_roles: Vec<String>,
    pub normalized_facts_verified: bool,
    pub apply_authorized: bool,
}

/// A missing explicitly named field is a configuration conflict, not an empty value.
/// No alias guessing, HTML stripping, field omission or task/history inference occurs.
pub fn map_fields(
    kind: SourceKind,
    fields: &BTreeMap<String, String>,
    mapping: &BTreeMap<String, String>,
    max_bytes: u64,
    max_chars: usize,
) -> Result<FieldMapping, String> {
    if !(1..=100 * 1024 * 1024).contains(&max_bytes) || !(1..=1_000_000).contains(&max_chars) {
        return Err("SOURCE_MAPPING_INVALID_LIMITS".into());
    }
    if fields.len() > 1000 || mapping.len() > 25 {
        return Err("SOURCE_MAPPING_COUNT_LIMIT".into());
    }
    let mut total = 0u64;
    let mut total_chars = 0usize;
    for (name, value) in fields {
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("SOURCE_FIELD_NAME_INVALID".into());
        }
        total = total
            .checked_add(name.len() as u64)
            .and_then(|n| n.checked_add(value.len() as u64))
            .ok_or("SOURCE_MAPPING_INPUT_LIMIT")?;
        total_chars = total_chars
            .checked_add(value.chars().take(max_chars + 1).count())
            .ok_or("SOURCE_MAPPING_INPUT_LIMIT")?;
        if total > max_bytes || total_chars > max_chars {
            return Err("SOURCE_MAPPING_INPUT_LIMIT".into());
        }
    }
    let common = [
        "meaning",
        "usage",
        "examples",
        "picture",
        "audio",
        "personal_notes",
        "source",
        "language",
        "explanation_language",
    ];
    let specific: &[&str] = match kind {
        SourceKind::Vocabulary => &[
            "expression",
            "reading",
            "pronunciation",
            "kanji",
            "sense_key",
            "production_prompt",
            "spelling_prompt",
            "enable_production",
            "enable_spelling",
        ],
        SourceKind::Grammar => &[
            "pattern",
            "formation",
            "use_key",
            "recognition_prompt",
            "exercise_prompt",
            "exercise_answer",
            "enable_application",
        ],
    };
    let mut roles = BTreeMap::new();
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (role, name) in mapping {
        if !common.contains(&role.as_str()) && !specific.contains(&role.as_str()) {
            return Err("SOURCE_MAPPING_ROLE_INVALID".into());
        }
        let raw_value = fields.get(name).ok_or("SOURCE_MAPPING_FIELD_MISSING")?;
        roles.insert(
            role.clone(),
            MappedField {
                source_field: name.clone(),
                raw_value: raw_value.clone(),
            },
        );
        grouped.entry(name.clone()).or_default().push(role.clone());
    }
    let required: &[&str] = match kind {
        SourceKind::Vocabulary => &["expression", "meaning"],
        SourceKind::Grammar => &["pattern", "meaning", "formation"],
    };
    let missing_required_roles = required
        .iter()
        .filter(|role| {
            roles
                .get(**role)
                .is_none_or(|field| field.raw_value.trim().is_empty())
        })
        .map(|s| s.to_string())
        .collect();
    let used: BTreeSet<_> = grouped.keys().collect();
    let unmapped_fields = fields
        .keys()
        .filter(|name| !used.contains(name))
        .cloned()
        .collect();
    grouped.retain(|_, roles| roles.len() > 1);
    Ok(FieldMapping {
        kind,
        source_digest: canonical::digest("source-fields-v2", fields).map_err(|e| e.to_string())?,
        mapping_digest: canonical::digest("source-field-mapping-v2", &(kind, mapping))
            .map_err(|e| e.to_string())?,
        roles,
        unmapped_fields,
        shared_fields: grouped,
        missing_required_roles,
        normalized_facts_verified: false,
        apply_authorized: false,
    })
}
