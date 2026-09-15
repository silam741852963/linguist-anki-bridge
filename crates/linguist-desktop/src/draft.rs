use linguist_application::NoteInfo;
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
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
    }
    pub fn empty(note_id: i64) -> Self {
        Self {
            note_id,
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
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
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
        self.pending = changes
            .into_iter()
            .filter(|change| {
                !self.locked(change.field) && !self.user_edited.contains(&change.field)
            })
            .collect();
    }

    pub fn accept(&mut self, index: usize) -> bool {
        if index >= self.pending.len() {
            return false;
        }
        let change = self.pending.remove(index);
        if self.locked(change.field) {
            return false;
        }
        self.edit(change.field, change.value);
        true
    }

    pub fn reject(&mut self, index: usize) -> bool {
        if index >= self.pending.len() {
            return false;
        }
        self.pending.remove(index);
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
            *entry = ReviewDraft::from_note(note);
        }
        self.active_note_id = Some(note.note_id);
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
}
