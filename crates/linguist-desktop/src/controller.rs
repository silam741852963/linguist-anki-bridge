use crate::draft::{DraftField, DraftPersistence, DraftStore, GeneratedChange, ReviewDraft};
use crate::review_model::{ReviewQueueData, ReviewQueueModel, ReviewRow};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Checking,
    Ready,
    Unavailable(String),
}

impl ServiceState {
    pub fn label(&self) -> String {
        match self {
            Self::Checking => "Checking".into(),
            Self::Ready => "Ready".into(),
            Self::Unavailable(message) if message.is_empty() => "Unavailable".into(),
            Self::Unavailable(message) => format!("Unavailable: {message}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewState {
    pub anki: ServiceState,
    pub ollama: ServiceState,
    pub active_deck: String,
    pub selection: String,
    pub busy: bool,
    pub error: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            anki: ServiceState::Checking,
            ollama: ServiceState::Checking,
            active_deck: String::new(),
            selection: String::new(),
            busy: false,
            error: String::new(),
        }
    }
}

pub trait DesktopPort {
    fn anki_available(&self) -> Result<(), String>;
    fn ollama_available(&self) -> Result<(), String>;
    fn active_deck(&self) -> Result<String, String>;
    fn review_queue(&self) -> Result<ReviewQueueData, String>;
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationController {
    state: ViewState,
    queue: ReviewQueueModel,
    drafts: DraftStore,
    draft_persistence: MemoryDraftPersistence,
}

impl ApplicationController {
    pub fn state(&self) -> &ViewState {
        &self.state
    }

    pub fn queue(&self) -> &ReviewQueueModel {
        &self.queue
    }

    pub fn active_draft(&self) -> Option<&ReviewDraft> {
        self.drafts.active()
    }

    #[allow(dead_code)]
    pub fn pending_change(&self, index: usize) -> Option<&GeneratedChange> {
        self.active_draft()?.pending().get(index)
    }

    pub fn refresh<P: DesktopPort>(&mut self, port: &P) {
        self.state.busy = true;
        self.state.error.clear();
        self.state.anki = service_state(port.anki_available());
        self.state.ollama = service_state(port.ollama_available());
        match port.active_deck() {
            Ok(deck) => self.state.active_deck = deck,
            Err(error) => {
                self.state.active_deck.clear();
                self.state.error = error;
            }
        }
        self.begin_queue_loading();
        match port.review_queue() {
            Ok(data) => self.replace_queue(data),
            Err(error) => self.fail_queue(error),
        }
        self.state.busy = false;
    }

    pub fn select(&mut self, selection: impl Into<String>) {
        self.state.selection = selection.into();
    }

    pub fn begin_queue_loading(&mut self) {
        self.queue.begin_loading();
    }

    pub fn replace_queue(&mut self, data: ReviewQueueData) {
        self.queue.replace(data);
        self.sync_queue_selection();
    }

    pub fn fail_queue(&mut self, message: impl Into<String>) {
        self.queue.fail(message);
        self.state.selection.clear();
    }

    pub fn select_queue_index(&mut self, index: usize) {
        let Some(row) = self.queue.rows().get(index) else {
            return;
        };
        let note_id = row.note_id;
        let selection = selection_label(row);
        if let Err(error) = self.drafts.switch_to(note_id, &mut self.draft_persistence) {
            self.report_error(format!("Draft autosave failed: {error}"));
            return;
        }
        let _ = self.queue.select_index(index);
        self.state.selection = selection;
    }

    pub fn select_deck_index(&mut self, index: usize) {
        let _ = self.queue.select_deck_index(index);
    }

    pub fn report_error(&mut self, error: impl Into<String>) {
        self.state.error = error.into();
    }

    pub fn clear_error(&mut self) {
        self.state.error.clear();
    }

    pub fn edit_draft(&mut self, field: DraftField, value: impl Into<String>) {
        if let Some(draft) = self.drafts.active_mut() {
            draft.edit(field, value);
        }
    }

    pub fn undo_draft(&mut self) -> bool {
        self.drafts.active_mut().is_some_and(ReviewDraft::undo)
    }
    pub fn redo_draft(&mut self) -> bool {
        self.drafts.active_mut().is_some_and(ReviewDraft::redo)
    }

    #[allow(dead_code)]
    pub fn set_draft_locked(&mut self, field: DraftField, locked: bool) {
        if let Some(draft) = self.drafts.active_mut() {
            draft.set_locked(field, locked);
        }
    }

    #[allow(dead_code)]
    pub fn accept_draft_change(&mut self, index: usize) -> bool {
        self.drafts
            .active_mut()
            .is_some_and(|draft| draft.accept(index))
    }

    #[allow(dead_code)]
    pub fn reject_draft_change(&mut self, index: usize) -> bool {
        self.drafts
            .active_mut()
            .is_some_and(|draft| draft.reject(index))
    }

    pub fn regenerate_draft<P: DraftGenerationPort>(&mut self, port: &P) {
        let Some(draft) = self.active_draft().cloned() else {
            return;
        };
        match port.generate(&draft) {
            Ok(changes) => self
                .drafts
                .active_mut()
                .expect("active draft unchanged")
                .regenerate(changes),
            Err(error) => self.report_error(error),
        }
    }

    fn sync_queue_selection(&mut self) {
        self.state.selection = self
            .queue
            .selected_index()
            .and_then(|index| self.queue.rows().get(index))
            .map(selection_label)
            .unwrap_or_default();
    }
}

pub trait DraftGenerationPort {
    fn generate(&self, draft: &ReviewDraft) -> Result<Vec<GeneratedChange>, String>;
}

#[derive(Clone, Debug, Default)]
struct MemoryDraftPersistence;
impl DraftPersistence for MemoryDraftPersistence {
    fn save(&mut self, _draft: &ReviewDraft) -> Result<(), String> {
        Ok(())
    }
}

fn selection_label(row: &ReviewRow) -> String {
    format!("{} · {}", row.note_id, row.expression)
}

fn service_state(result: Result<(), String>) -> ServiceState {
    match result {
        Ok(()) => ServiceState::Ready,
        Err(error) => ServiceState::Unavailable(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePort {
        anki: Result<(), String>,
        ollama: Result<(), String>,
        deck: Result<String, String>,
        queue: Result<ReviewQueueData, String>,
    }

    impl DesktopPort for FakePort {
        fn anki_available(&self) -> Result<(), String> {
            self.anki.clone()
        }
        fn ollama_available(&self) -> Result<(), String> {
            self.ollama.clone()
        }
        fn active_deck(&self) -> Result<String, String> {
            self.deck.clone()
        }
        fn review_queue(&self) -> Result<ReviewQueueData, String> {
            self.queue.clone()
        }
    }

    #[test]
    fn refresh_exposes_service_deck_and_error_state_through_one_snapshot() {
        let mut controller = ApplicationController::default();
        controller.refresh(&FakePort {
            anki: Ok(()),
            ollama: Err("offline".into()),
            deck: Err("Anki unavailable".into()),
            queue: Err("Anki unavailable".into()),
        });
        assert_eq!(controller.state().anki, ServiceState::Ready);
        assert_eq!(controller.state().ollama.label(), "Unavailable: offline");
        assert_eq!(controller.state().error, "Anki unavailable");
        assert!(!controller.state().busy);
    }

    #[test]
    fn selection_and_error_commands_keep_view_state_immutable_to_callers() {
        let mut controller = ApplicationController::default();
        controller.select("42 · 食べる");
        controller.report_error("invalid model");
        assert_eq!(controller.state().selection, "42 · 食べる");
        controller.clear_error();
        assert!(controller.state().error.is_empty());
    }

    #[test]
    fn queue_refresh_updates_selection_through_controller_command() {
        let mut controller = ApplicationController::default();
        controller.replace_queue(ReviewQueueData {
            decks: vec!["Japanese".into()],
            rows: vec![ReviewRow {
                note_id: 42,
                expression: "食べる".into(),
                detail: "vocabulary".into(),
                state: crate::review_model::ReviewState::NeedsReview,
            }],
        });
        controller.select_queue_index(0);
        controller.replace_queue(ReviewQueueData {
            decks: vec!["Japanese".into()],
            rows: vec![ReviewRow {
                note_id: 42,
                expression: "食べる".into(),
                detail: "vocabulary".into(),
                state: crate::review_model::ReviewState::Ready,
            }],
        });
        assert_eq!(controller.state().selection, "42 · 食べる");
        assert_eq!(controller.queue().selected_index(), Some(0));
    }

    struct FakeGeneration;
    impl DraftGenerationPort for FakeGeneration {
        fn generate(&self, _draft: &ReviewDraft) -> Result<Vec<GeneratedChange>, String> {
            Ok(vec![GeneratedChange {
                field: DraftField::Meaning,
                value: "generated".into(),
                provenance: "fake".into(),
            }])
        }
    }

    #[test]
    fn generated_changes_can_be_reviewed_without_overwriting_user_edits() {
        let mut controller = ApplicationController::default();
        controller.replace_queue(ReviewQueueData {
            decks: vec![],
            rows: vec![ReviewRow {
                note_id: 7,
                expression: "読む".into(),
                detail: String::new(),
                state: crate::review_model::ReviewState::NeedsReview,
            }],
        });
        controller.select_queue_index(0);
        controller.edit_draft(DraftField::Meaning, "user");
        controller.regenerate_draft(&FakeGeneration);
        assert!(controller.pending_change(0).is_none());
        controller.set_draft_locked(DraftField::Kanji, true);
        assert!(controller.active_draft().unwrap().locked(DraftField::Kanji));
    }
}
