//! Full read-only Anki capture archives. Cross-request consistency and native history stay unverified.
use linguist_core::{
    AnkiId, canonical,
    records::{SourceArchive, SourceRecord},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub mod media;
pub struct CapturedSource {
    pub source: SourceRecord,
    pub archive: SourceArchive,
    pub assets: BTreeMap<String, Vec<u8>>,
}

pub struct RevampCapture {
    pub captured: CapturedSource,
    pub mapping: crate::mapping::FieldMapping,
}

/// Read and archive before normalization. The caller publishes assets before any plan revision.
/// No store, learning document, approval or collection mutation is created here.
pub fn capture_for_revamp(
    client: &linguist_anki::Client,
    settings: &linguist_config::Effective,
    purpose: &str,
    note_id: &str,
) -> Result<RevampCapture, String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "input.max_file_mb",
        "input.max_record_chars",
        "media.max_asset_mb",
    ] {
        registry.validate_value(
            key,
            settings
                .values
                .get(key)
                .ok_or("SOURCE_CAPTURE_SETTING_MISSING")?,
        )?;
    }
    // Fail unsupported purposes before service traffic, even if no role map has been configured.
    if ![
        "japanese_vocab",
        "english_vocab",
        "japanese_grammar",
        "english_grammar",
    ]
    .contains(&purpose)
    {
        return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into());
    }
    let read = client.capture_note(note_id)?;
    let note = canonical::bytes(&read.note).map_err(|e| e.to_string())?;
    let model = canonical::bytes(&read.model).map_err(|e| e.to_string())?;
    let cards = canonical::bytes(&read.cards).map_err(|e| e.to_string())?;
    let mut captured = archive_read_capture(
        &note,
        &model,
        &cards,
        settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024,
    )?;
    let mapping = crate::mapping::map_purpose_fields(
        settings,
        purpose,
        &read.model.model.name,
        &captured.source.fields,
    )?;
    let task_key = format!("purposes.{purpose}.card_tasks");
    let task_mapping = settings
        .values
        .get(&task_key)
        .ok_or("SOURCE_CAPTURE_SETTING_MISSING")?;
    registry.validate_value(&task_key, task_mapping)?;
    let tasks = task_mapping
        .as_object()
        .ok_or("SOURCE_CAPTURE_TASK_MAP_INVALID")?;
    if tasks.keys().any(|ordinal| {
        ordinal
            .parse::<usize>()
            .ok()
            .is_none_or(|index| index >= read.model.templates.len())
    }) {
        return Err("SOURCE_CAPTURE_TASK_MAP_CONFLICT".into());
    }
    let old_digest = captured.source.digest.clone();
    let mut manifest: Value =
        canonical::parse(&captured.assets[&old_digest]).map_err(|e| e.to_string())?;
    manifest["repeated_reads_matched"] = json!(read.repeated_reads_matched);
    manifest["mapping_digest"] = json!(mapping.mapping_digest);
    manifest["source_task_mapping"] = task_mapping.clone();
    manifest["source_task_mapping_verified"] = json!(read.model.template_order_verified);
    let manifest = canonical::bytes(&manifest).map_err(|e| e.to_string())?;
    let digest = canonical::asset_digest(&manifest);
    captured.assets.remove(&old_digest);
    captured.assets.insert(digest.clone(), manifest);
    captured.source.digest = digest.clone();
    captured.archive.digest = digest;
    captured.archive.asset_digests = captured.assets.keys().cloned().collect();
    if !captured.source.media_refs.is_empty() {
        let mut observations = BTreeMap::new();
        let limit = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
        let mut total = captured
            .assets
            .values()
            .map(|bytes| bytes.len() as u64)
            .sum::<u64>();
        for filename in &captured.source.media_refs {
            let file = client.retrieve_media_file(filename)?;
            if let Some(file) = &file {
                total = total
                    .checked_add(file.bytes.len() as u64)
                    .ok_or("SOURCE_MEDIA_LIMIT")?;
                if total > limit {
                    return Err("SOURCE_MEDIA_LIMIT".into());
                }
            }
            observations.insert(filename.clone(), file.map(|file| file.bytes));
        }
        media::attach_original_media(
            &mut captured,
            observations,
            settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024,
            limit,
        )?;
    }
    Ok(RevampCapture { captured, mapping })
}
fn id(value: &Value) -> Result<String, String> {
    let text = value.as_str().ok_or("SOURCE_CAPTURE_ID_INVALID")?;
    AnkiId::try_from(text.to_owned()).map_err(|_| "SOURCE_CAPTURE_ID_INVALID")?;
    Ok(text.into())
}
/// Inputs are complete normalized read-port payloads, not native snapshot evidence.
/// Unknown properties survive in their exact raw assets. Nothing is published by this helper.
pub fn archive_read_capture(
    note: &[u8],
    model: &[u8],
    cards: &[u8],
    max_bytes: u64,
) -> Result<CapturedSource, String> {
    if !(1..=100 * 1024 * 1024).contains(&max_bytes) {
        return Err("SOURCE_CAPTURE_INVALID_LIMIT".into());
    }
    let total = note
        .len()
        .checked_add(model.len())
        .and_then(|n| n.checked_add(cards.len()))
        .ok_or("SOURCE_CAPTURE_LIMIT")?;
    if total as u64 > max_bytes {
        return Err("SOURCE_CAPTURE_LIMIT".into());
    }
    let parse = |bytes: &[u8]| {
        canonical::parse::<Value>(bytes).map_err(|_| "SOURCE_CAPTURE_SCHEMA_INVALID")
    };
    let n = parse(note)?;
    let m = parse(model)?;
    let c = parse(cards)?;
    let note_id = id(&n["noteId"])?;
    let model_name = n["modelName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("SOURCE_CAPTURE_MODEL_INVALID")?;
    if m["model"]["name"].as_str() != Some(model_name) {
        return Err("SOURCE_CAPTURE_MODEL_CONFLICT".into());
    }
    id(&m["model"]["id"])?;
    let model_fields = m["fields"]
        .as_array()
        .ok_or("SOURCE_CAPTURE_MODEL_INVALID")?;
    let raw_fields = n["fields"]
        .as_object()
        .ok_or("SOURCE_CAPTURE_FIELDS_INVALID")?;
    if raw_fields.len() > 1000 || raw_fields.len() != model_fields.len() {
        return Err("SOURCE_CAPTURE_FIELDS_CONFLICT".into());
    }
    let mut fields = BTreeMap::new();
    let mut ordinals = BTreeSet::new();
    for (name, field) in raw_fields {
        let ordinal = field["order"]
            .as_u64()
            .ok_or("SOURCE_CAPTURE_FIELDS_INVALID")?;
        if !ordinals.insert(ordinal)
            || model_fields.get(ordinal as usize).and_then(Value::as_str) != Some(name.as_str())
        {
            return Err("SOURCE_CAPTURE_FIELDS_CONFLICT".into());
        }
        fields.insert(
            name.clone(),
            field["value"]
                .as_str()
                .ok_or("SOURCE_CAPTURE_FIELDS_INVALID")?
                .into(),
        );
    }
    let templates = m["templates"]
        .as_object()
        .ok_or("SOURCE_CAPTURE_MODEL_INVALID")?;
    if templates.len() > 1000
        || !m["css"].is_string()
        || templates
            .values()
            .any(|t| !t["Front"].is_string() || !t["Back"].is_string())
    {
        return Err("SOURCE_CAPTURE_MODEL_INVALID".into());
    }
    let note_cards = n["cards"]
        .as_array()
        .ok_or("SOURCE_CAPTURE_CARDS_INVALID")?;
    let card_rows = c.as_array().ok_or("SOURCE_CAPTURE_CARDS_INVALID")?;
    if note_cards.len() > 10000 || note_cards.len() != card_rows.len() {
        return Err("SOURCE_CAPTURE_CARDS_CONFLICT".into());
    }
    let mut expected = BTreeSet::new();
    for value in note_cards {
        if !expected.insert(id(value)?) {
            return Err("SOURCE_CAPTURE_CARDS_CONFLICT".into());
        }
    }
    for card in card_rows {
        if id(&card["note"])? != note_id || !expected.remove(&id(&card["cardId"])?) {
            return Err("SOURCE_CAPTURE_CARDS_CONFLICT".into());
        }
    }
    let tags = n["tags"]
        .as_array()
        .filter(|tags| tags.len() <= 1000)
        .ok_or("SOURCE_CAPTURE_TAGS_INVALID")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("SOURCE_CAPTURE_TAGS_INVALID")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let discovery = crate::capture::discover_media(&fields, max_bytes, 10000)?;
    let media_refs = discovery
        .references
        .iter()
        .map(|r| r.filename.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut assets = BTreeMap::new();
    let template_bytes = canonical::bytes(&m["templates"]).map_err(|e| e.to_string())?;
    let template_digest = canonical::asset_digest(&template_bytes);
    assets.insert(template_digest.clone(), template_bytes);
    let digests: BTreeMap<_, _> = [("note", note), ("model", model), ("cards", cards)]
        .into_iter()
        .map(|(name, bytes)| {
            let digest = canonical::asset_digest(bytes);
            assets.insert(digest.clone(), bytes.to_vec());
            (name, digest)
        })
        .collect();
    let manifest=canonical::bytes(&json!({"schema_version":2,"kind":"anki_read_capture_v2","note_id":note_id,"payloads":digests,
        "template_manifest": template_digest,
        "media_discovery": discovery,
        "native_history_verified":false,"atomic_snapshot_verified":false,"media_bytes_archived":false})).map_err(|e|e.to_string())?;
    let digest = canonical::asset_digest(&manifest);
    assets.insert(digest.clone(), manifest);
    let source_id = uuid::Uuid::new_v4();
    let captured_at_unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "SOURCE_CAPTURE_CLOCK_INVALID")?
        .as_secs();
    Ok(CapturedSource {
        source: SourceRecord {
            id: source_id,
            kind: "anki_read_capture_v2".into(),
            location: format!("anki_note:{note_id}"),
            digest: digest.clone(),
            text: None,
            fields: fields.clone(),
            model_manifest: digests["model"].clone(),
            template_manifest: Some(template_digest),
            captured_at_unix_seconds: Some(captured_at_unix_seconds),
            tags,
            cards: vec![],
            media_refs,
        },
        archive: SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id,
            digest,
            original_text: None,
            original_fields: fields,
            asset_digests: assets.keys().cloned().collect(),
        },
        assets,
    })
}
