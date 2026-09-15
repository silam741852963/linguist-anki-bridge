use crate::review_model::{ReviewQueueData, ReviewQueueModel, ReviewRow};
use crate::{
    batch_model::{BatchAction, BatchManagementPort, BatchViewState},
    commit_model::{CommitExecutor, CommitViewState},
    draft::{DraftField, DraftPersistence, DraftStore, GeneratedChange, ReviewDraft},
};
use linguist_application::NoteInfo;

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
pub trait DraftNotePort {
    fn note(&self, note_id: i64) -> Result<NoteInfo, String>;
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationController {
    state: ViewState,
    queue: ReviewQueueModel,
    drafts: DraftStore,
    draft_persistence: MemoryDraftPersistence,
    commit: CommitViewState,
    batch: BatchViewState,
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

    pub fn commit(&self) -> &CommitViewState {
        &self.commit
    }
    pub fn batch(&self) -> &BatchViewState {
        &self.batch
    }
    pub fn create_batch<P: BatchManagementPort>(
        &mut self,
        port: &mut P,
        job: linguist_jobs::NewJob,
    ) {
        self.batch.create(port, job);
        self.report_batch_error();
    }
    pub fn refresh_batches<P: BatchManagementPort>(&mut self, port: &P) {
        self.batch.refresh(port);
        self.report_batch_error();
    }
    pub fn select_batch<P: BatchManagementPort>(&mut self, port: &P, id: &str, offset: usize) {
        self.batch.select(port, id, offset);
        self.report_batch_error();
    }
    pub fn pause_batch<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.batch.pause(port);
        self.report_batch_error();
    }
    pub fn resume_batch<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.batch.resume(port);
        self.report_batch_error();
    }
    pub fn retry_batch<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.batch.retry(port);
        self.report_batch_error();
    }
    pub fn request_batch_confirmation(&mut self, action: BatchAction) {
        self.batch.request_confirmation(action);
    }
    pub fn confirm_batch<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.batch.confirm(port);
        self.report_batch_error();
    }
    pub fn cancel_batch_confirmation(&mut self) {
        self.batch.pending_confirmation = None;
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
        self.commit.invalidate_preview();
        let _ = self.queue.select_index(index);
        self.state.selection = selection;
    }
    pub fn hydrate_selected<P: DraftNotePort>(&mut self, port: &P) {
        let Some(note_id) = self
            .queue
            .selected_index()
            .and_then(|index| self.queue.rows().get(index))
            .map(|row| row.note_id)
        else {
            return;
        };
        match port.note(note_id) {
            Ok(note) => self.drafts.hydrate(&note, &mut self.draft_persistence),
            Err(error) => self.report_error(error),
        }
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
            self.commit.invalidate_preview();
        }
    }

    pub fn undo_draft(&mut self) -> bool {
        let changed = self.drafts.active_mut().is_some_and(ReviewDraft::undo);
        if changed {
            self.commit.invalidate_preview();
        }
        changed
    }
    pub fn redo_draft(&mut self) -> bool {
        let changed = self.drafts.active_mut().is_some_and(ReviewDraft::redo);
        if changed {
            self.commit.invalidate_preview();
        }
        changed
    }

    #[allow(dead_code)]
    pub fn set_draft_locked(&mut self, field: DraftField, locked: bool) {
        if let Some(draft) = self.drafts.active_mut() {
            draft.set_locked(field, locked);
        }
    }

    #[allow(dead_code)]
    pub fn accept_draft_change(&mut self, index: usize) -> bool {
        let accepted = self
            .drafts
            .active_mut()
            .is_some_and(|draft| draft.accept(index));
        if accepted {
            self.commit.invalidate_preview();
        }
        accepted
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

    pub fn preview_commit<E: CommitExecutor<ReviewDraft>>(&mut self, executor: &mut E) {
        let Some(draft) = self.active_draft().cloned() else {
            self.report_error("Select a card before previewing changes");
            return;
        };
        if let Err(error) = self.commit.preview_with(executor, &draft) {
            self.report_error(error);
        }
    }

    pub fn apply_commit<E: CommitExecutor<ReviewDraft>>(&mut self, executor: &mut E) {
        let Some(draft) = self.active_draft().cloned() else {
            self.report_error("Select a card before applying changes");
            return;
        };
        if let Err(error) = self.commit.apply_with(executor, &draft) {
            self.report_error(error);
        }
    }

    pub fn restore_commit<E: CommitExecutor<ReviewDraft>>(
        &mut self,
        executor: &mut E,
        snapshot_id: &str,
    ) {
        if let Err(error) = self.commit.restore_with(executor, snapshot_id) {
            self.report_error(error);
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
    fn report_batch_error(&mut self) {
        if !self.batch.error.is_empty() {
            self.state.error = self.batch.error.clone();
        }
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::commit_model::{CommitPreview, SnapshotHistoryItem};

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

    struct MockCommitAdapter {
        kind: &'static str,
        calls: Vec<String>,
    }

    impl CommitExecutor<ReviewDraft> for MockCommitAdapter {
        fn preview(&mut self, draft: &ReviewDraft) -> Result<CommitPreview, String> {
            self.calls
                .push(format!("{}:preview:{}", self.kind, draft.note_id));
            Ok(CommitPreview {
                before: BTreeMap::from([("Meaning".into(), "old".into())]),
                after: BTreeMap::from([("Meaning".into(), draft.meaning.clone())]),
                media: vec!["word.mp3".into()],
                model_changed: self.kind == "modernize",
            })
        }

        fn apply(&mut self, draft: &ReviewDraft) -> Result<SnapshotHistoryItem, String> {
            self.calls
                .push(format!("{}:apply:{}", self.kind, draft.note_id));
            Ok(SnapshotHistoryItem {
                snapshot_id: format!("{}-{}", self.kind, draft.note_id),
                note_id: draft.note_id,
            })
        }

        fn restore(&mut self, snapshot_id: &str) -> Result<(), String> {
            self.calls
                .push(format!("{}:restore:{snapshot_id}", self.kind));
            Ok(())
        }
    }

    #[test]
    fn mock_adapter_completes_modernize_and_inject_with_restoration() {
        for (kind, note_id) in [("modernize", 42), ("inject", 100)] {
            let mut controller = ApplicationController::default();
            controller.replace_queue(ReviewQueueData {
                decks: vec!["Japanese".into()],
                rows: vec![ReviewRow {
                    note_id,
                    expression: "食べる".into(),
                    detail: String::new(),
                    state: crate::review_model::ReviewState::NeedsReview,
                }],
            });
            controller.select_queue_index(0);
            controller.edit_draft(DraftField::Meaning, "to eat");
            let mut adapter = MockCommitAdapter {
                kind,
                calls: vec![],
            };

            controller.preview_commit(&mut adapter);
            assert!(controller.commit().dry_run);
            assert!(controller.commit().preview_ready);
            assert_eq!(controller.commit().fields[0].after, "to eat");
            controller.apply_commit(&mut adapter);
            assert!(!controller.commit().dry_run);
            let snapshot_id = controller.commit().snapshots[0].snapshot_id.clone();
            controller.restore_commit(&mut adapter, &snapshot_id);

            assert_eq!(
                adapter.calls,
                [
                    format!("{kind}:preview:{note_id}"),
                    format!("{kind}:apply:{note_id}"),
                    format!("{kind}:restore:{snapshot_id}"),
                ]
            );
        }
    }

    #[test]
    fn changing_the_draft_cancels_a_prior_preview() {
        let mut controller = ApplicationController::default();
        controller.replace_queue(ReviewQueueData {
            decks: vec![],
            rows: vec![ReviewRow {
                note_id: 42,
                expression: "食べる".into(),
                detail: String::new(),
                state: crate::review_model::ReviewState::NeedsReview,
            }],
        });
        controller.select_queue_index(0);
        let mut adapter = MockCommitAdapter {
            kind: "modernize",
            calls: vec![],
        };
        controller.preview_commit(&mut adapter);
        controller.edit_draft(DraftField::Meaning, "changed after preview");
        controller.apply_commit(&mut adapter);
        assert!(!controller.commit().preview_ready);
        assert_eq!(adapter.calls, ["modernize:preview:42"]);
        assert_eq!(
            controller.state().error,
            "Preview changes before applying them"
        );
    }
}
