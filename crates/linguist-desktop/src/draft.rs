use linguist_application::NoteInfo;
use linguist_core::{CardDocument, CardMode};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[allow(dead_code)] // QML exposes meaning now; generation adapters use all fields.
pub enum DraftField {
    Expression,
    Meaning,
    Kanji,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedChange {
    pub field: DraftField,
    pub value: String,
    pub provenance: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Edit {
    field: DraftField,
    before: String,
    after: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDraft {
    pub note_id: i64,
    pub mode: CardMode,
    pub deck_name: String,
    pub language_key: Option<String>,
    pub target_model: String,
    pub expression: String,
    pub meaning: String,
    pub kanji: String,
    pub images: Vec<String>,
    pub audio: Vec<String>,
    pub issues: Vec<String>,
    pub provenance: Vec<String>,
    locked: BTreeSet<DraftField>,
    user_edited: BTreeSet<DraftField>,
    pending: Vec<GeneratedChange>,
    pending_document: Option<CardDocument>,
    accepted_document: Option<CardDocument>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    dirty: bool,
}

#[allow(dead_code)] // N18 command surface is completed as enrichment ports arrive.
impl ReviewDraft {
    pub fn from_note(note: &NoteInfo) -> Self {
        let expression = ["Expression", "Word", "Front", "Vocabulary"]
            .into_iter()
            .find_map(|name| note.fields.get(name))
            .cloned()
            .or_else(|| note.fields.values().next().cloned())
            .unwrap_or_default();
        let meaning = ["Meaning", "Definition", "Back"]
            .into_iter()
            .find_map(|name| note.fields.get(name))
            .cloned()
            .unwrap_or_default();
        let kanji = ["Kanji", "Kanji Construction"]
            .into_iter()
            .find_map(|name| note.fields.get(name))
            .cloned()
            .unwrap_or_default();
        let images = note
            .fields
            .values()
            .flat_map(|value| media_names(value, "img", "src"))
            .collect();
        let audio = note
            .fields
            .values()
            .flat_map(|value| media_names(value, "sound", ""))
            .collect();
        Self {
            note_id: note.note_id,
            mode: CardMode::Modernize,
            deck_name: note
                .deck_names
                .first()
                .map(|deck| deck.0.clone())
                .unwrap_or_default(),
            target_model: note.model_name.0.clone(),
            language_key: None,
            expression,
            meaning,
            kanji,
            images,
            audio,
            issues: Vec::new(),
            provenance: vec![format!("{} · {}", note.model_name.0, note.tags.join(", "))],
            locked: BTreeSet::new(),
            user_edited: BTreeSet::new(),
            pending: Vec::new(),
            pending_document: None,
            accepted_document: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
    }
    pub fn empty(note_id: i64) -> Self {
        Self {
            note_id,
            mode: CardMode::Modernize,
            deck_name: String::new(),
            language_key: None,
            target_model: String::new(),
            expression: String::new(),
            meaning: String::new(),
            kanji: String::new(),
            images: Vec::new(),
            audio: Vec::new(),
            issues: Vec::new(),
            provenance: Vec::new(),
            locked: BTreeSet::new(),
            user_edited: BTreeSet::new(),
            pending: Vec::new(),
            pending_document: None,
            accepted_document: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
    }

    pub fn injection(
        note_id: i64,
        expression: impl Into<String>,
        context: impl Into<String>,
        deck_name: impl Into<String>,
        target_model: impl Into<String>,
    ) -> Self {
        let mut draft = Self::empty(note_id);
        draft.mode = CardMode::Inject;
        draft.expression = expression.into();
        draft.meaning = context.into();
        draft.deck_name = deck_name.into();
        draft.target_model = target_model.into();
        draft.provenance = vec!["Manual import".into()];
        draft
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }
    pub fn pending(&self) -> &[GeneratedChange] {
        &self.pending
    }
    pub fn locked(&self, field: DraftField) -> bool {
        self.locked.contains(&field)
    }

    pub fn edit(&mut self, field: DraftField, value: impl Into<String>) {
        let value = value.into();
        let before = self.value(field).to_owned();
        if before == value {
            return;
        }
        self.set_value(field, value.clone());
        if field == DraftField::Expression {
            self.pending_document = None;
            self.accepted_document = None;
            self.pending.clear();
        }
        self.undo.push(Edit {
            field,
            before,
            after: value,
        });
        self.redo.clear();
        self.user_edited.insert(field);
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.set_value(edit.field, edit.before.clone());
        self.redo.push(edit);
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.set_value(edit.field, edit.after.clone());
        self.undo.push(edit);
        self.dirty = true;
        true
    }

    pub fn set_locked(&mut self, field: DraftField, locked: bool) {
        if locked {
            self.locked.insert(field);
        } else {
            self.locked.remove(&field);
        }
    }

    pub fn regenerate(&mut self, changes: impl IntoIterator<Item = GeneratedChange>) {
        self.pending_document = None;
        self.pending = changes
            .into_iter()
            .filter(|change| {
                !self.locked(change.field) && !self.user_edited.contains(&change.field)
            })
            .collect();
    }

    pub fn regenerate_document(
        &mut self,
        document: CardDocument,
        changes: impl IntoIterator<Item = GeneratedChange>,
    ) {
        self.regenerate(changes);
        self.pending_document = (!self.pending.is_empty()).then_some(document);
    }

    pub fn accepted_document(&self) -> Option<CardDocument> {
        let mut document = self.accepted_document.clone()?;
        if document.expression != self.expression {
            return None;
        }
        document.values.meaning_text = Some(self.meaning.clone());
        document.values.kanji_construction = Some(self.kanji.clone());
        Some(document)
    }

    pub fn accept(&mut self, index: usize) -> bool {
        if index >= self.pending.len() {
            return false;
        }
        let change = self.pending.remove(index);
        if self.locked(change.field) {
            return false;
        }
        let accepts_document = change.field == DraftField::Meaning;
        self.edit(change.field, change.value);
        if accepts_document {
            if let Some(document) = self.pending_document.take() {
                self.images = media_names(
                    document.values.meaning_image.as_deref().unwrap_or_default(),
                    "img",
                    "src",
                );
                self.audio = media_names(
                    document.values.audio.as_deref().unwrap_or_default(),
                    "sound",
                    "",
                );
                self.issues = document.issues.clone();
                self.accepted_document = Some(document);
            }
        }
        if self.pending.is_empty() {
            self.pending_document = None;
        }
        true
    }

    pub fn reject(&mut self, index: usize) -> bool {
        if index >= self.pending.len() {
            return false;
        }
        self.pending.remove(index);
        if self.pending.is_empty() {
            self.pending_document = None;
        }
        true
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    fn value(&self, field: DraftField) -> &str {
        match field {
            DraftField::Expression => &self.expression,
            DraftField::Meaning => &self.meaning,
            DraftField::Kanji => &self.kanji,
        }
    }

    fn set_value(&mut self, field: DraftField, value: String) {
        match field {
            DraftField::Expression => self.expression = value,
            DraftField::Meaning => self.meaning = value,
            DraftField::Kanji => self.kanji = value,
        }
    }
}

fn media_names(value: &str, marker: &str, attribute: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find(marker) {
        rest = &rest[start + marker.len()..];
        let tail = if attribute.is_empty() {
            rest
        } else {
            let Some(attribute_start) = rest.find(&format!("{attribute}=")) else {
                continue;
            };
            &rest[attribute_start + attribute.len() + 1..]
        };
        let tail = tail.trim_start_matches(['"', '\'']);
        let end = tail
            .find(|ch: char| ch == '"' || ch == '\'' || ch == ']' || ch.is_whitespace())
            .unwrap_or(tail.len());
        let candidate = tail[..end].trim_matches(|ch| ch == ':' || ch == '=' || ch == '[');
        if !candidate.is_empty() && !candidate.contains("http") {
            names.push(candidate.to_owned());
        }
        rest = tail.get(end..).unwrap_or("");
    }
    names
}

pub trait DraftPersistence {
    fn save(&mut self, draft: &ReviewDraft) -> Result<(), String>;
}

#[derive(Clone, Debug, Default)]
pub struct DraftStore {
    drafts: BTreeMap<i64, ReviewDraft>,
    active_note_id: Option<i64>,
}

impl DraftStore {
    pub fn active(&self) -> Option<&ReviewDraft> {
        self.active_note_id.and_then(|id| self.drafts.get(&id))
    }
    pub fn active_mut(&mut self) -> Option<&mut ReviewDraft> {
        self.active_note_id.and_then(|id| self.drafts.get_mut(&id))
    }

    pub fn switch_to<P: DraftPersistence>(
        &mut self,
        note_id: i64,
        persistence: &mut P,
    ) -> Result<(), String> {
        if self.active_note_id == Some(note_id) {
            return Ok(());
        }
        if let Some(active) = self.active_mut().filter(|draft| draft.dirty()) {
            persistence.save(active)?;
            active.mark_saved();
        }
        self.drafts
            .entry(note_id)
            .or_insert_with(|| ReviewDraft::empty(note_id));
        self.active_note_id = Some(note_id);
        Ok(())
    }
    pub fn hydrate<P: DraftPersistence>(&mut self, note: &NoteInfo, _persistence: &mut P) {
        let entry = self
            .drafts
            .entry(note.note_id)
            .or_insert_with(|| ReviewDraft::from_note(note));
        if !entry.dirty() && entry.undo.is_empty() && entry.pending.is_empty() {
            let language_key = entry.language_key.clone();
            *entry = ReviewDraft::from_note(note);
            entry.language_key = language_key;
        }
        self.active_note_id = Some(note.note_id);
    }

    pub fn insert(&mut self, draft: ReviewDraft) {
        self.active_note_id = Some(draft.note_id);
        self.drafts.insert(draft.note_id, draft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakePersistence {
        saved: Vec<i64>,
        fail: bool,
    }
    impl DraftPersistence for FakePersistence {
        fn save(&mut self, draft: &ReviewDraft) -> Result<(), String> {
            if self.fail {
                return Err("disk full".into());
            }
            self.saved.push(draft.note_id);
            Ok(())
        }
    }

    #[test]
    fn edits_undo_redo_and_generated_changes_preserve_user_fields() {
        let mut draft = ReviewDraft::empty(42);
        draft.edit(DraftField::Meaning, "user meaning");
        assert!(draft.undo());
        assert!(draft.meaning.is_empty());
        assert!(draft.redo());
        draft.set_locked(DraftField::Kanji, true);
        draft.regenerate([
            GeneratedChange {
                field: DraftField::Meaning,
                value: "generated".into(),
                provenance: "Ollama".into(),
            },
            GeneratedChange {
                field: DraftField::Kanji,
                value: "kanji".into(),
                provenance: "KanjiAPI".into(),
            },
            GeneratedChange {
                field: DraftField::Expression,
                value: "食べる".into(),
                provenance: "OCR".into(),
            },
        ]);
        assert_eq!(draft.pending().len(), 1);
        assert!(draft.accept(0));
        assert_eq!(draft.expression, "食べる");
    }

    #[test]
    fn switching_saves_dirty_draft_and_only_prompts_after_autosave_failure() {
        let mut store = DraftStore::default();
        let mut persistence = FakePersistence::default();
        store.switch_to(1, &mut persistence).unwrap();
        store
            .active_mut()
            .unwrap()
            .edit(DraftField::Meaning, "saved first");
        store.switch_to(2, &mut persistence).unwrap();
        assert_eq!(persistence.saved, vec![1]);
        assert_eq!(store.active().unwrap().note_id, 2);
        store
            .active_mut()
            .unwrap()
            .edit(DraftField::Meaning, "cannot save");
        persistence.fail = true;
        assert_eq!(
            store.switch_to(3, &mut persistence),
            Err("disk full".into())
        );
        assert_eq!(store.active().unwrap().note_id, 2);
    }

    #[test]
    fn hydrates_note_fields_and_media_without_marking_dirty() {
        let note = NoteInfo {
            note_id: 8,
            model_name: linguist_application::ModelName("Japanese".into()),
            deck_names: vec![],
            fields: BTreeMap::from([
                ("Word".into(), "食べる".into()),
                ("Meaning".into(), "to eat".into()),
                ("Picture".into(), r#"<img src="food.jpg">"#.into()),
                ("Audio".into(), "[sound:taberu.mp3]".into()),
            ]),
            tags: vec!["source".into()],
        };
        let draft = ReviewDraft::from_note(&note);
        assert_eq!(draft.expression, "食べる");
        assert_eq!(draft.meaning, "to eat");
        assert_eq!(draft.images, ["food.jpg"]);
        assert_eq!(draft.audio, ["taberu.mp3"]);
        assert!(!draft.dirty());
    }

    #[test]
    fn accepted_generation_keeps_media_and_user_edits() {
        let mut draft = ReviewDraft::injection(-1, "猫", "context", "Japanese", "Model");
        let document = CardDocument {
            schema_version: linguist_core::CONTRACT_VERSION,
            expression: "猫".into(),
            values: linguist_core::LogicalFields {
                meaning_image: Some("<img src=\"cat.jpg\">".into()),
                meaning_text: Some("generated meaning".into()),
                kanji_construction: Some("猫: cat".into()),
                audio: Some("[sound:cat.mp3]".into()),
            },
            media: vec![linguist_core::MediaAsset {
                filename: "cat.jpg".into(),
                data_base64: "Y2F0".into(),
            }],
            obsolete_media: Vec::new(),
            issues: Vec::new(),
            tags: Vec::new(),
            provenance: Default::default(),
        };
        draft.regenerate_document(
            document,
            [GeneratedChange {
                field: DraftField::Meaning,
                value: "generated meaning".into(),
                provenance: "test".into(),
            }],
        );
        assert!(draft.accepted_document().is_none());
        assert!(draft.accept(0));
        draft.edit(DraftField::Meaning, "user correction");
        let accepted = draft.accepted_document().unwrap();
        assert_eq!(accepted.media[0].filename, "cat.jpg");
        assert_eq!(
            accepted.values.meaning_text.as_deref(),
            Some("user correction")
        );
        assert_eq!(draft.images, ["cat.jpg"]);
        assert_eq!(draft.audio, ["cat.mp3"]);
        draft.edit(DraftField::Expression, "犬");
        assert!(draft.accepted_document().is_none());
    }

    #[test]
    fn hydration_keeps_import_language_key() {
        let note = NoteInfo {
            note_id: 42,
            model_name: linguist_application::ModelName("Model".into()),
            deck_names: vec![],
            fields: BTreeMap::from([("Expression".into(), "猫".into())]),
            tags: Vec::new(),
        };
        let mut draft = ReviewDraft::from_note(&note);
        draft.language_key = Some("japanese_vocab".into());
        let mut store = DraftStore::default();
        store.insert(draft);
        store.hydrate(&note, &mut FakePersistence::default());
        assert_eq!(
            store.active().unwrap().language_key.as_deref(),
            Some("japanese_vocab")
        );
    }
}
