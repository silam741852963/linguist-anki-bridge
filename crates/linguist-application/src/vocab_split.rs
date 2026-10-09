//! Vocabulary split: one source note holding several words (私 / 僕 / 俺) or
//! readings (脅かす: おどかす / おびやかす) becomes one note per unit. The
//! anchor keeps the source note and its history; every other unit is a new
//! note. Apply reuses the split-group path (children first, then the anchor).
use crate::grammar::{SplitKind, SplitUnit, publish_split};
use linguist_core::{records::*, validation::vocabulary_units, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VocabularyUnit {
    pub expression: String,
    #[serde(default)]
    pub pronunciation: String,
    /// Source files (original names) this unit keeps for review; the others
    /// stay archived on it.
    #[serde(default)]
    pub media: Vec<String>,
    /// Tasks of a new sibling note; empty keeps the original's. The anchor
    /// keeps its note's cards, so it takes none.
    #[serde(default)]
    pub tasks: Vec<Task>,
}

fn kana_only(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}'))
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitRequest {
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub document_id: uuid::Uuid,
    pub input_digest: String,
    pub actor: String,
    pub anchor_index: usize,
    pub units: Vec<VocabularyUnit>,
}

fn source_media(document: &LearningDocument) -> Vec<(String, bool)> {
    document
        .media
        .iter()
        .filter(|m| m.owner == MediaOwner::Source)
        .map(|m| {
            (
                m.original_filename.clone().unwrap_or(m.filename.clone()),
                m.mime.starts_with("audio/"),
            )
        })
        .collect()
}

/// `[sound:NAME]` references of the captured source fields, in order.
fn sound_order(document: &LearningDocument) -> Vec<String> {
    let mut names = Vec::new();
    for source in document
        .sources
        .iter()
        .filter(|s| s.kind == "anki_read_capture_v2")
    {
        for value in source.fields.values() {
            let mut rest = value.as_str();
            while let Some(start) = rest.find("[sound:") {
                let tail = &rest[start + 7..];
                let Some(end) = tail.find(']') else { break };
                names.push(tail[..end].to_owned());
                rest = &tail[end + 1..];
            }
        }
    }
    names
}

/// A split request detected from the item: one unit per listed word (paired
/// with the listed readings when the counts agree), or one per reading of a
/// single word. Recordings are given to units in order when their count
/// matches; pictures stay with the anchor (the first unit). The reviewer
/// edits it and names the actor before `plans split-vocab --request`.
pub fn template(plan: &PlanRevision, document_id: uuid::Uuid) -> Result<SplitRequest, String> {
    let document = plan
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    let LearningContent::Vocabulary(v) = &document.content else {
        return Err("VOCAB_SPLIT_INPUT_CONFLICT".into());
    };
    let words = vocabulary_units(&v.expression);
    let readings = vocabulary_units(&v.pronunciation);
    let pairs: Vec<(String, String)> = if words.len() > 1 {
        let paired = readings.len() == words.len();
        words
            .into_iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    w,
                    if paired {
                        readings[i].clone()
                    } else {
                        String::new()
                    },
                )
            })
            .collect()
    } else if readings.len() > 1 {
        readings
            .into_iter()
            .map(|r| (v.expression.trim().to_owned(), r))
            .collect()
    } else {
        return Err("VOCAB_SPLIT_NOT_NEEDED: the item holds one word and one reading".into());
    };
    let sounds = sound_order(document);
    let media = source_media(document);
    let pictures: Vec<String> = media
        .iter()
        .filter(|(name, audio)| !audio && !sounds.contains(name))
        .map(|(name, _)| name.clone())
        .collect();
    let per_unit_sound = sounds.len() == pairs.len();
    let units = pairs
        .into_iter()
        .enumerate()
        .map(|(i, (expression, pronunciation))| {
            let mut media = Vec::new();
            if i == 0 {
                media.extend(pictures.iter().cloned());
            }
            if per_unit_sound {
                media.push(sounds[i].clone());
            }
            // A new sibling gets the tasks the purpose gives a new word; a
            // word written in kana leaves out Spelling (its reading is the answer).
            let tasks = if i > 0 {
                let on =
                    |key: &str| plan.settings.values.get(key) == Some(&serde_json::json!(true));
                let mut tasks = vec![Task::Comprehension];
                if on("learning.vocabulary.production") {
                    tasks.push(Task::Production);
                }
                if on("learning.vocabulary.spelling") && !kana_only(&expression) {
                    tasks.push(Task::Spelling);
                }
                tasks
            } else {
                vec![]
            };
            VocabularyUnit {
                expression,
                pronunciation,
                media,
                tasks,
            }
        })
        .collect();
    Ok(SplitRequest {
        schema_version: 2,
        base_revision: plan.revision,
        base_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        document_id,
        input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
        actor: String::new(),
        anchor_index: 0,
        units,
    })
}

/// Publish the reviewed split: every unit is a fresh vocabulary item with only
/// its word and reading; the dictionary, enrichment and generation stages run
/// again on the new revision.
pub fn split(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    request: &SplitRequest,
    raw: &[u8],
) -> Result<PlanRevision, String> {
    let parsed: SplitRequest = canonical::parse(raw).map_err(|e| e.to_string())?;
    if canonical::bytes(&parsed).map_err(|e| e.to_string())?
        != canonical::bytes(request).map_err(|e| e.to_string())?
        || request.schema_version != 2
        || request.base_revision != base.revision
        || request.base_digest != base.approval_digest().map_err(|e| e.to_string())?
        || store.latest_revision(base.id)? != base.revision
        || store.revision(base.id, base.revision)? != *base
    {
        return Err("VOCAB_SPLIT_BASE_CONFLICT".into());
    }
    let original = base
        .documents
        .iter()
        .find(|document| document.id == request.document_id)
        .ok_or("VOCAB_SPLIT_DOCUMENT_MISSING")?;
    if !matches!(original.content, LearningContent::Vocabulary(_))
        || request.input_digest != original.semantic_digest().map_err(|e| e.to_string())?
        || !(2..=100).contains(&request.units.len())
        || request.anchor_index >= request.units.len()
        || request.actor.trim().is_empty()
        || base
            .grammar_groups
            .iter()
            .any(|group| group.units.contains(&original.id))
    {
        return Err("VOCAB_SPLIT_INPUT_CONFLICT".into());
    }
    let available: BTreeSet<String> = source_media(original).into_iter().map(|(n, _)| n).collect();
    let mut seen = BTreeSet::new();
    let mut units = Vec::new();
    for (index, unit) in request.units.iter().enumerate() {
        let expression = unit.expression.trim();
        let pronunciation = unit.pronunciation.trim();
        if expression.is_empty()
            || vocabulary_units(expression).len() != 1
            || vocabulary_units(pronunciation).len() > 1
            || !seen.insert((expression, pronunciation))
            || unit.media.iter().any(|m| !available.contains(m))
            || index == request.anchor_index && !unit.tasks.is_empty()
            || !unit.tasks.is_empty() && !unit.tasks.contains(&Task::Comprehension)
            || unit
                .tasks
                .iter()
                .any(|t| !matches!(t, Task::Comprehension | Task::Production | Task::Spelling))
        {
            return Err("VOCAB_SPLIT_UNIT_INVALID".into());
        }
        units.push(SplitUnit {
            content: LearningContent::Vocabulary(Vocabulary {
                expression: expression.to_owned(),
                meaning: String::new(),
                sense_key: String::new(),
                reading: String::new(),
                pronunciation: pronunciation.to_owned(),
                usage: String::new(),
                examples: vec![],
                dictionary: vec![],
                kanji: String::new(),
                production_prompt: String::new(),
                spelling_prompt: String::new(),
                nuance: vec![],
                collocations: vec![],
                kanji_details: vec![],
            }),
            sibling_tasks: if unit.tasks.is_empty() {
                original.requested_tasks.clone()
            } else {
                unit.tasks.clone()
            },
            keep_media: Some(unit.media.iter().cloned().collect()),
        });
    }
    publish_split(
        store,
        base,
        original,
        raw,
        &request.actor,
        request.anchor_index,
        SplitKind::Vocabulary,
        units,
    )
}
