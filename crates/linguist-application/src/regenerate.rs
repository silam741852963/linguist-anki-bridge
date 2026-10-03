//! OP-30 `plans regenerate`: rerun one pipeline stage for selected items.
//!
//! Only content the stage itself produced is replaced by default. Values a user
//! authored, edited or decided (and source-captured values) are protected and
//! reported; they are cleared only when named in the explicit overwrite list.
//! The parent revision is never modified; a child revision is published.
use crate::vocab::Providers;
use linguist_config::Effective;
use linguist_core::{
    LearningContent, LearningDocument, Provenance, canonical,
    records::{EvidenceTarget, MediaRole, PlanRevision},
    validation,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Dictionary,
    Enrichment,
    Generation,
}
impl std::str::FromStr for Stage {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "dictionary" => Ok(Self::Dictionary),
            "enrichment" => Ok(Self::Enrichment),
            "generation" => Ok(Self::Generation),
            _ => Err("REGENERATE_STAGE_UNKNOWN: use dictionary, enrichment or generation".into()),
        }
    }
}

/// Fields a stage may produce, by content kind.
fn stage_fields(stage: Stage, document: &LearningDocument) -> &'static [&'static str] {
    match (stage, &document.content) {
        (Stage::Generation, LearningContent::Vocabulary(_)) => {
            &["usage", "production_prompt", "spelling_prompt", "examples"]
        }
        (Stage::Generation, LearningContent::Grammar(_)) => &[
            "meaning",
            "formation",
            "usage",
            "recognition_prompt",
            "exercise_prompt",
            "exercise_answer",
            "examples",
        ],
        (Stage::Enrichment, LearningContent::Vocabulary(_)) => &["kanji", "picture", "audio"],
        (Stage::Dictionary, LearningContent::Vocabulary(_)) => &["meaning", "sense_key", "reading"],
        _ => &[],
    }
}

/// Managed field name used by typed edit intents.
fn managed(field: &str) -> Option<&'static str> {
    Some(match field {
        "meaning" => "Meaning",
        "reading" => "Reading",
        "usage" => "Usage",
        "production_prompt" => "ProductionPrompt",
        "spelling_prompt" => "SpellingPrompt",
        "kanji" => "Kanji",
        "formation" => "Formation",
        "recognition_prompt" => "RecognitionPrompt",
        "exercise_prompt" => "ExercisePrompt",
        "exercise_answer" => "ExerciseAnswer",
        _ => return None,
    })
}

fn text_field<'a>(document: &'a mut LearningDocument, field: &str) -> Option<&'a mut String> {
    match &mut document.content {
        LearningContent::Vocabulary(v) => match field {
            "meaning" => Some(&mut v.meaning),
            "sense_key" => Some(&mut v.sense_key),
            "reading" => Some(&mut v.reading),
            "usage" => Some(&mut v.usage),
            "production_prompt" => Some(&mut v.production_prompt),
            "spelling_prompt" => Some(&mut v.spelling_prompt),
            "kanji" => Some(&mut v.kanji),
            _ => None,
        },
        LearningContent::Grammar(g) => match field {
            "meaning" => Some(&mut g.meaning),
            "formation" => Some(&mut g.formation),
            "usage" => Some(&mut g.usage),
            "recognition_prompt" => Some(&mut g.recognition_prompt),
            "exercise_prompt" => Some(&mut g.exercise_prompt),
            "exercise_answer" => Some(&mut g.exercise_answer),
            _ => None,
        },
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ItemPreview {
    pub document_id: uuid::Uuid,
    /// Stage output that will be discarded and produced again.
    pub cleared: Vec<String>,
    /// Non-empty values the stage would touch but which are user/source owned.
    pub protected: Vec<String>,
    /// Owned values cleared because they were explicitly listed for overwrite.
    pub overwritten: Vec<String>,
    /// Decisions invalidated by the content change.
    pub invalidated_decisions: Vec<String>,
    pub removed_candidate_media: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub schema_version: u16,
    pub plan_id: uuid::Uuid,
    pub base_revision: u32,
    pub stage: Stage,
    pub items: Vec<ItemPreview>,
    pub writes_enabled: bool,
}

/// Was this field produced by `stage` (as opposed to authored, edited or captured)?
fn produced_by(stage: Stage, document: &LearningDocument, field: &str) -> bool {
    match stage {
        Stage::Generation => document
            .evidence
            .iter()
            .any(|e| e.field == field && e.provenance == Provenance::Generated),
        Stage::Enrichment => match field {
            "kanji" => document
                .evidence
                .iter()
                .any(|e| e.field == "kanji" && e.provenance == Provenance::Dictionary),
            _ => false,
        },
        Stage::Dictionary => false,
    }
}

fn is_empty(document: &mut LearningDocument, field: &str) -> bool {
    text_field(document, field).is_none_or(|value| value.trim().is_empty())
}

/// Compute and apply the clearing for one item; returns the prepared copy.
pub fn prepare_item(
    document: &LearningDocument,
    stage: Stage,
    overwrite: &BTreeSet<String>,
) -> Result<(LearningDocument, ItemPreview), String> {
    let fields = stage_fields(stage, document);
    if fields.is_empty() {
        return Err(format!(
            "REGENERATE_STAGE_NOT_APPLICABLE: {stage:?} does not apply to item {}",
            document.id
        ));
    }
    if let Some(unknown) = overwrite.iter().find(|f| !fields.contains(&f.as_str())) {
        return Err(format!("REGENERATE_OVERWRITE_FIELD_INVALID:{unknown}"));
    }
    let mut prepared = document.clone();
    let mut preview = ItemPreview {
        document_id: document.id,
        invalidated_decisions: document
            .reviews
            .iter()
            .map(|r| r.issue_id.clone())
            .collect(),
        ..Default::default()
    };
    for &field in fields {
        let named = overwrite.contains(field);
        match field {
            "examples" => {
                let examples = match &mut prepared.content {
                    LearningContent::Vocabulary(v) => &mut v.examples,
                    LearningContent::Grammar(g) => &mut g.examples,
                };
                let generated = examples
                    .iter()
                    .any(|e| e.provenance == Provenance::Generated);
                let owned = examples
                    .iter()
                    .any(|e| e.provenance != Provenance::Generated);
                // Generated examples are always replaced; owned ones only on request.
                let keep: Vec<bool> = examples
                    .iter()
                    .map(|e| e.provenance != Provenance::Generated && !named)
                    .collect();
                if generated {
                    preview.cleared.push("examples".into());
                }
                if owned {
                    if named {
                        preview.overwritten.push("examples".into());
                    } else {
                        preview.protected.push("examples".into());
                    }
                }
                let mut mapping = BTreeMap::new();
                let mut next = 0;
                for (old, kept) in keep.iter().enumerate() {
                    if *kept {
                        mapping.insert(old, next);
                        next += 1;
                    }
                }
                let mut index = 0;
                examples.retain(|_| {
                    index += 1;
                    keep[index - 1]
                });
                prepared.evidence.retain_mut(|e| match &mut e.target {
                    Some(EvidenceTarget::Example { index }) => match mapping.get(index) {
                        Some(new) => {
                            *index = *new;
                            true
                        }
                        None => false,
                    },
                    _ => true,
                });
            }
            "picture" | "audio" => {
                let role = if field == "picture" {
                    MediaRole::Picture
                } else {
                    MediaRole::Audio
                };
                let selected = prepared
                    .media
                    .iter()
                    .any(|m| m.source_id.is_none() && m.role == role);
                if selected {
                    if named {
                        preview.overwritten.push(field.into());
                    } else {
                        preview.protected.push(field.into());
                    }
                }
                let prefix = if field == "picture" {
                    "image/"
                } else {
                    "audio/"
                };
                let removed: BTreeSet<String> = prepared
                    .media
                    .iter()
                    .filter(|m| {
                        m.source_id.is_none()
                            && m.mime.starts_with(prefix)
                            && (m.role == MediaRole::Archive || named)
                    })
                    .map(|m| m.digest.clone())
                    .collect();
                if !removed.is_empty() {
                    preview.cleared.push(field.into());
                }
                if !(selected && !named) {
                    prepared.media.retain(|m| !removed.contains(&m.digest));
                    prepared.evidence.retain(|e| {
                        !matches!(&e.target, Some(EvidenceTarget::MediaAsset { digest }) if removed.contains(digest))
                            || e.provenance == Provenance::Ocr
                    });
                    preview.removed_candidate_media.extend(removed);
                }
            }
            _ => {
                let edited = managed(field).is_some_and(|m| prepared.edits.contains_key(m));
                let produced = produced_by(stage, &prepared, field);
                if is_empty(&mut prepared, field) && !edited {
                    continue;
                }
                let owned = edited || !produced || stage == Stage::Dictionary;
                if owned && !named {
                    preview.protected.push(field.into());
                    continue;
                }
                if owned {
                    preview.overwritten.push(field.into());
                } else {
                    preview.cleared.push(field.into());
                }
                if let Some(value) = text_field(&mut prepared, field) {
                    value.clear();
                }
                if let Some(managed) = managed(field) {
                    prepared.edits.remove(managed);
                }
                prepared
                    .evidence
                    .retain(|e| !(e.field == field && e.provenance == Provenance::Generated));
            }
        }
    }
    match stage {
        Stage::Enrichment => {
            // Kanji pages and enrichment observations are produced again.
            if !preview.protected.iter().any(|f| f == "kanji") {
                let kanji_sources: BTreeSet<_> = prepared
                    .sources
                    .iter()
                    .filter(|s| s.kind == "jisho_kanji_pages_v2")
                    .map(|s| s.id)
                    .collect();
                prepared.sources.retain(|s| !kanji_sources.contains(&s.id));
                prepared
                    .archives
                    .retain(|a| !kanji_sources.contains(&a.source_id));
                prepared
                    .evidence
                    .retain(|e| !e.source_id.is_some_and(|id| kanji_sources.contains(&id)));
            }
            prepared.issues.retain(|i| i.stage != "enrichment");
        }
        Stage::Dictionary => {
            let dictionary_sources: BTreeSet<_> = prepared
                .sources
                .iter()
                .filter(|s| {
                    matches!(
                        s.kind.as_str(),
                        "jisho_api_v1" | "wiktionary_definition_v0.8"
                    )
                })
                .map(|s| s.id)
                .collect();
            prepared
                .sources
                .retain(|s| !dictionary_sources.contains(&s.id));
            prepared
                .archives
                .retain(|a| !dictionary_sources.contains(&a.source_id));
            prepared.evidence.retain(|e| {
                !e.source_id
                    .is_some_and(|id| dictionary_sources.contains(&id))
            });
            prepared.issues.retain(|i| i.stage != "dictionary");
            if let LearningContent::Vocabulary(v) = &mut prepared.content {
                v.dictionary.clear();
            }
            preview.cleared.push("dictionary".into());
        }
        Stage::Generation => {}
    }
    prepared.reviews.clear();
    prepared.issues = validation::validate(&prepared);
    Ok((prepared, preview))
}

/// Preview a regeneration without any provider call or write.
pub fn preview(
    base: &PlanRevision,
    items: &[uuid::Uuid],
    stage: Stage,
    overwrite: &BTreeSet<String>,
) -> Result<Preview, String> {
    let mut previews = Vec::new();
    for id in selected(base, items)? {
        let document = base.documents.iter().find(|d| d.id == id).unwrap();
        previews.push(prepare_item(document, stage, overwrite)?.1);
    }
    Ok(Preview {
        schema_version: 2,
        plan_id: base.id,
        base_revision: base.revision,
        stage,
        items: previews,
        writes_enabled: false,
    })
}

fn selected(base: &PlanRevision, items: &[uuid::Uuid]) -> Result<Vec<uuid::Uuid>, String> {
    if items.is_empty() {
        return Ok(base.documents.iter().map(|d| d.id).collect());
    }
    for id in items {
        if !base.documents.iter().any(|d| d.id == *id) {
            return Err("PLAN_ITEM_NOT_FOUND".into());
        }
    }
    Ok(items.to_vec())
}

/// Execute a dictionary or enrichment regeneration with the current settings
/// frozen into the child. Generation uses [`regenerate_generation`].
#[allow(clippy::too_many_arguments)]
pub fn regenerate(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    digest: &str,
    items: &[uuid::Uuid],
    stage: Stage,
    overwrite: &BTreeSet<String>,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    providers: Providers<'_>,
) -> Result<(PlanRevision, Preview), String> {
    if stage == Stage::Generation {
        return Err("REGENERATE_GENERATION_REQUIRES_ENGINE".into());
    }
    check_base(store, base, digest)?;
    let frozen = crate::freeze_settings(settings, environment)?;
    if frozen.values["storage.state_dir"] != base.settings.values["storage.state_dir"] {
        return Err("REGENERATE_STORAGE_CONFLICT".into());
    }
    let mut child = base.clone();
    let mut previews = Vec::new();
    let mut assets = Vec::new();
    for id in selected(base, items)? {
        let index = child.documents.iter().position(|d| d.id == id).unwrap();
        let (prepared, preview) = prepare_item(&child.documents[index], stage, overwrite)?;
        let (enriched, bytes) = match stage {
            Stage::Dictionary => {
                if settings.values["dictionary.provider"] == "authored" {
                    return Err("REGENERATE_DICTIONARY_PROVIDER_REQUIRED".into());
                }
                crate::dictionary::enrich_document(&prepared, settings, providers.dictionary)?
            }
            Stage::Enrichment => {
                crate::vocab::enrich_document(&prepared, settings, environment, providers)?
            }
            Stage::Generation => unreachable!(),
        };
        assets.extend(bytes);
        let removed: BTreeSet<_> = child.documents[index]
            .reviews
            .iter()
            .map(|r| r.id)
            .collect();
        child.review_decisions.retain(|d| !removed.contains(&d.id));
        child.documents[index] = enriched;
        child.rendered.retain(|r| r.document_id != id);
        previews.push(preview);
    }
    child.settings = frozen;
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("REGENERATE_REVISION_LIMIT")?;
    child.parent_digest = Some(digest.into());
    child.binding = None;
    let sources: Vec<_> = child.documents.iter().flat_map(|d| &d.sources).collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    let cap = settings.values["network.max_response_mb"]
        .as_u64()
        .unwrap_or(20)
        .max(settings.values["media.max_asset_mb"].as_u64().unwrap_or(10))
        * 1024
        * 1024;
    for bytes in assets {
        store.publish_asset(&bytes, cap)?;
    }
    store.publish_revision(&child)?;
    Ok((
        child,
        Preview {
            schema_version: 2,
            plan_id: base.id,
            base_revision: base.revision,
            stage,
            items: previews,
            writes_enabled: false,
        },
    ))
}

/// Regenerate generated content for one item through the inference engine.
#[allow(clippy::too_many_arguments)]
pub fn regenerate_generation(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    digest: &str,
    item: uuid::Uuid,
    overwrite: &BTreeSet<String>,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    client: &crate::ollama::transport::Client,
) -> Result<(serde_json::Value, ItemPreview), String> {
    check_base(store, base, digest)?;
    let document = base
        .documents
        .iter()
        .find(|d| d.id == item)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    let (prepared, preview) = prepare_item(document, Stage::Generation, overwrite)?;
    let result = crate::generation::publish_candidate_from(
        store,
        base,
        &prepared,
        digest,
        settings,
        environment,
        client,
    )?;
    Ok((result, preview))
}

fn check_base(
    store: &linguist_store::Store,
    base: &PlanRevision,
    digest: &str,
) -> Result<(), String> {
    if store.latest_revision(base.id)? != base.revision
        || base.approval_digest().map_err(|e| e.to_string())? != digest
    {
        return Err("REGENERATE_BASE_CONFLICT".into());
    }
    Ok(())
}
