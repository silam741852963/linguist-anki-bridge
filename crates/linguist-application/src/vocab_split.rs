//! Vocabulary split: one source note holding several words (私 / 僕 / 俺) or
//! readings (脅かす: おどかす / おびやかす) becomes one note per unit. The
//! anchor keeps the source note and its history; every other unit is a new
//! note. Apply reuses the split-group path (children first, then the anchor).
//! A unit whose word already has a note in the target deck (味をつける,
//! 迷惑がかかる) makes no new note: it merges into that note, which is revamped
//! on its own and keeps its history.
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
    /// The existing note (Anki note ID) that already holds this word. The
    /// unit merges into it: no new note is made, and that note is revamped
    /// on its own. Never the anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_note: Option<String>,
}

fn kana_only(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}'))
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
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
                existing_note: None,
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

/// The word of a note field: markup, entities and spaces removed.
fn field_word(value: &str) -> String {
    let mut text = String::new();
    let mut tag = false;
    for c in value.replace("&nbsp;", " ").chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag && !c.is_whitespace() && c != '\u{3000}' => text.push(c),
            _ => {}
        }
    }
    text
}

/// The kana of a reading field: markup, `[sound:]` tags and everything that
/// is not kana removed.
fn kana_of(value: &str) -> String {
    let mut text = value.to_owned();
    while let Some(start) = text.find("[sound:") {
        let end = text[start..]
            .find(']')
            .map_or(text.len(), |e| start + e + 1);
        text.replace_range(start..end, "");
    }
    field_word(&text)
        .chars()
        .filter(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | 'ー'))
        .collect()
}

/// Mark every unit whose word already has a note in the purpose's target
/// deck (the source model's word field, or a revamped note's Expression),
/// and move the anchor to the first unit that has none. Read-only.
pub fn find_existing(
    plan: &PlanRevision,
    request: &mut SplitRequest,
    reader: &dyn crate::duplicate_candidates::CandidateReader,
) -> Result<(), String> {
    let purpose = &plan
        .selection
        .as_ref()
        .ok_or("VOCAB_SPLIT_PURPOSE_UNRESOLVED")?
        .purpose;
    let setting = |key: &str| {
        plan.settings
            .values
            .get(&format!("purposes.{purpose}.{key}"))
            .cloned()
            .unwrap_or_default()
    };
    let deck = setting("target_deck");
    let deck = deck.as_str().ok_or("VOCAB_SPLIT_PURPOSE_UNRESOLVED")?;
    let source_model = setting("source_model");
    let fields = setting("fields");
    let source_field = fields["expression"]
        .as_str()
        .unwrap_or("Expression")
        .to_owned();
    let source_reading = fields["pronunciation"].as_str().map(str::to_owned);
    let managed = linguist_core::model::vocabulary().name;
    let original = plan
        .documents
        .iter()
        .find(|document| document.id == request.document_id)
        .ok_or("VOCAB_SPLIT_DOCUMENT_MISSING")?;
    let own: BTreeSet<String> = original
        .sources
        .iter()
        .filter_map(|source| source.location.strip_prefix("anki_note:"))
        .map(str::to_owned)
        .collect();
    for unit in &mut request.units {
        let word = field_word(&unit.expression);
        let query = format!(
            "{} \"{}\"",
            linguist_anki::deck_query(deck)?,
            crate::duplicate_candidates::search_value(&word)?
        );
        let ids: Vec<String> = reader
            .find_notes(&query)?
            .into_iter()
            .filter(|id| !own.contains(id))
            .collect();
        if ids.is_empty() {
            continue;
        }
        let notes = reader.notes_info(&ids)?;
        let reading = kana_of(&unit.pronunciation);
        unit.existing_note = notes.iter().find_map(|note| {
            let model = note["modelName"].as_str()?;
            let (field, reading_field) = if model == managed {
                ("Expression", Some("Pronunciation"))
            } else if source_model.as_str() == Some(model) {
                (source_field.as_str(), source_reading.as_deref())
            } else {
                return None;
            };
            // Same word, another reading (節 ふし / せつ): a different note.
            let theirs = reading_field
                .and_then(|f| note["fields"][f]["value"].as_str())
                .map(kana_of)
                .unwrap_or_default();
            (field_word(note["fields"][field]["value"].as_str()?) == word
                && (reading.is_empty() || theirs.is_empty() || reading == theirs))
                .then(|| note["noteId"].to_string().trim_matches('"').to_owned())
        });
    }
    if request.units[request.anchor_index].existing_note.is_some()
        && let Some(index) = request.units.iter().position(|u| u.existing_note.is_none())
    {
        // The new anchor keeps the source note's cards and pictures.
        let media = std::mem::take(&mut request.units[request.anchor_index].media);
        let anchor = &mut request.units[index];
        anchor.tasks.clear();
        for name in media {
            if !anchor.media.contains(&name) {
                anchor.media.insert(0, name);
            }
        }
        request.anchor_index = index;
    }
    Ok(())
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
    let mut anchor = request.anchor_index;
    for (index, unit) in request.units.iter().enumerate() {
        let expression = unit.expression.trim();
        let pronunciation = unit.pronunciation.trim();
        if expression.is_empty()
            || vocabulary_units(expression).len() != 1
            || vocabulary_units(pronunciation).len() > 1
            || !seen.insert((expression, pronunciation))
            || unit.media.iter().any(|m| !available.contains(m))
            || index == request.anchor_index && !unit.tasks.is_empty()
            || index == request.anchor_index && unit.existing_note.is_some()
            || unit.existing_note.as_ref().is_some_and(|id| {
                id.parse::<u64>().map_or(true, |n| n == 0)
                    || original
                        .sources
                        .iter()
                        .any(|s| s.location == format!("anki_note:{id}"))
            })
            || !unit.tasks.is_empty() && !unit.tasks.contains(&Task::Comprehension)
            || unit
                .tasks
                .iter()
                .any(|t| !matches!(t, Task::Comprehension | Task::Production | Task::Spelling))
        {
            return Err("VOCAB_SPLIT_UNIT_INVALID".into());
        }
        if unit.existing_note.is_some() {
            if index < request.anchor_index {
                anchor -= 1;
            }
            continue;
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
    // With every other unit merged into an existing note, the anchor is
    // narrowed in place (no group); a split always splits something.
    if units.len() == 1 && request.units.iter().all(|u| u.existing_note.is_none()) {
        return Err("VOCAB_SPLIT_UNIT_INVALID".into());
    }
    publish_split(
        store,
        base,
        original,
        raw,
        &request.actor,
        anchor,
        SplitKind::Vocabulary,
        units,
    )
}
