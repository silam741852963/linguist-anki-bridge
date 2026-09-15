#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, theme_background)]
        #[qproperty(QString, theme_surface)]
        #[qproperty(QString, theme_foreground)]
        #[qproperty(QString, theme_muted)]
        #[qproperty(QString, theme_accent)]
        #[qproperty(QString, anki_status)]
        #[qproperty(QString, ollama_status)]
        #[qproperty(QString, active_deck)]
        #[qproperty(QString, selection)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, queue_state)]
        #[qproperty(QString, queue_message)]
        #[qproperty(i32, deck_count)]
        #[qproperty(i32, selected_deck_index)]
        #[qproperty(i32, review_row_count)]
        #[qproperty(i32, selected_review_index)]
        #[qproperty(QString, draft_expression)]
        #[qproperty(QString, draft_meaning)]
        #[qproperty(QString, draft_kanji)]
        #[qproperty(QString, draft_images)]
        #[qproperty(QString, draft_audio)]
        #[qproperty(QString, draft_issues)]
        #[qproperty(QString, draft_provenance)]
        #[qproperty(bool, draft_dirty)]
        #[qproperty(i32, draft_pending_count)]
        #[qproperty(bool, draft_meaning_locked)]
        #[qproperty(bool, commit_dry_run)]
        #[qproperty(bool, commit_preview_ready)]
        #[qproperty(i32, commit_field_count)]
        #[qproperty(i32, commit_media_count)]
        #[qproperty(bool, commit_model_changed)]
        #[qproperty(i32, commit_snapshot_count)]
        #[qproperty(i32, batch_job_count)]
        #[qproperty(i32, batch_item_count)]
        #[qproperty(i32, batch_item_total)]
        #[qproperty(QString, batch_confirmation)]
        #[namespace = "linguist"]
        type AppBackend = super::AppBackendRust;

        #[qinvokable]
        #[cxx_name = "reloadTheme"]
        fn reload_theme(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshState"]
        fn refresh_state(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "selectItem"]
        fn select_item(self: Pin<&mut Self>, selection: &QString);
        #[qinvokable]
        #[cxx_name = "reportError"]
        fn report_error(self: Pin<&mut Self>, message: &QString);
        #[qinvokable]
        #[cxx_name = "deckName"]
        fn deck_name(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "selectDeckIndex"]
        fn select_deck_index(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "reviewExpression"]
        fn review_expression(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "reviewDetail"]
        fn review_detail(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "reviewState"]
        fn review_state(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "selectReviewIndex"]
        fn select_review_index(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "editDraftExpression"]
        fn edit_draft_expression(self: Pin<&mut Self>, value: &QString);
        #[qinvokable]
        #[cxx_name = "editDraftMeaning"]
        fn edit_draft_meaning(self: Pin<&mut Self>, value: &QString);
        #[qinvokable]
        #[cxx_name = "editDraftKanji"]
        fn edit_draft_kanji(self: Pin<&mut Self>, value: &QString);
        #[qinvokable]
        #[cxx_name = "undoDraft"]
        fn undo_draft(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "redoDraft"]
        fn redo_draft(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "regenerateDraft"]
        fn regenerate_draft(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "draftChangeValue"]
        fn draft_change_value(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "acceptDraftChange"]
        fn accept_draft_change(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "rejectDraftChange"]
        fn reject_draft_change(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "toggleDraftMeaningLock"]
        fn toggle_draft_meaning_lock(self: Pin<&mut Self>, locked: bool);
        #[qinvokable]
        #[cxx_name = "previewCommit"]
        fn preview_commit(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "applyCommit"]
        fn apply_commit(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "restoreSnapshot"]
        fn restore_snapshot(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "commitField"]
        fn commit_field(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "commitSnapshot"]
        fn commit_snapshot(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "cardPreview"]
        fn card_preview(self: &AppBackend, template_index: i32, back: bool) -> QString;
        #[qinvokable]
        #[cxx_name = "previewAudioUrl"]
        fn preview_audio_url(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "refreshBatches"]
        fn refresh_batches(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "selectBatch"]
        fn select_batch(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "batchJob"]
        fn batch_job(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "batchItem"]
        fn batch_item(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "pauseBatch"]
        fn pause_batch(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "resumeBatch"]
        fn resume_batch(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "retryBatch"]
        fn retry_batch(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "requestBatchAction"]
        fn request_batch_action(self: Pin<&mut Self>, action: i32);
        #[qinvokable]
        #[cxx_name = "confirmBatchAction"]
        fn confirm_batch_action(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "cancelBatchAction"]
        fn cancel_batch_action(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "createBatch"]
        fn create_batch(self: Pin<&mut Self>, deck_name: &QString, rows: &QString);
    }
}

use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use crate::controller::{ApplicationController, DesktopPort, DraftGenerationPort, DraftNotePort};
use crate::review_model::{ReviewQueueData, ReviewRow, ReviewState};
use crate::theme::{ThemePalette, ThemeWatch, omarchy_palette_path};

#[derive(Default)]
struct LocalBatchPort(Option<linguist_jobs::JobRepository>, String);

impl LocalBatchPort {
    fn open() -> Self {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|root| {
                root.join("linguist-anki-bridge")
                    .join("batch_jobs.native.sqlite3")
            });
        match root {
            Some(path) => match linguist_jobs::JobRepository::open(path) {
                Ok(repository) => {
                    let _ = repository.recover_interrupted();
                    Self(Some(repository), String::new())
                }
                Err(error) => Self(None, error.to_string()),
            },
            None => Self(None, "XDG config directory is unavailable".into()),
        }
    }
    fn repository(&self) -> Result<&linguist_jobs::JobRepository, String> {
        self.0.as_ref().ok_or_else(|| self.1.clone())
    }
}

struct NoBatchRestore;
impl linguist_jobs::BatchRollbackPort for NoBatchRestore {
    fn restore_snapshot(&self, _: &str) -> Result<(), String> {
        Err("Snapshot restore adapter is not connected yet".into())
    }
}
impl crate::batch_model::BatchManagementPort for LocalBatchPort {
    fn job_summaries(&self) -> Result<Vec<linguist_jobs::JobSummary>, String> {
        self.repository()?
            .job_summaries()
            .map_err(|error| error.to_string())
    }
    fn item_page(
        &self,
        id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<linguist_jobs::ItemPage, String> {
        self.repository()?
            .item_page(id, limit, offset)
            .map_err(|error| error.to_string())
    }
    fn create(&mut self, job: linguist_jobs::NewJob) -> Result<String, String> {
        self.repository()?
            .create_job(job)
            .map_err(|error| error.to_string())
    }
    fn pause(&mut self, id: &str) -> Result<(), String> {
        self.repository()?
            .set_job_state(id, linguist_core::BatchJobState::Paused, "")
            .map_err(|error| error.to_string())
    }
    fn resume(&mut self, id: &str) -> Result<(), String> {
        self.repository()?
            .set_job_state(id, linguist_core::BatchJobState::Running, "")
            .map_err(|error| error.to_string())
    }
    fn retry(&mut self, id: &str) -> Result<(), String> {
        self.resume(id)
    }
    fn cancel(&mut self, id: &str) -> Result<(), String> {
        self.repository()?
            .cancel(id)
            .map_err(|error| error.to_string())
    }
    fn rollback(&mut self, id: &str) -> Result<linguist_jobs::RollbackReport, String> {
        self.repository()?
            .rollback(id, &NoBatchRestore)
            .map_err(|error| error.to_string())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        self.repository()?
            .delete_job(id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub struct AppBackendRust {
    theme_background: QString,
    theme_surface: QString,
    theme_foreground: QString,
    theme_muted: QString,
    theme_accent: QString,
    anki_status: QString,
    ollama_status: QString,
    active_deck: QString,
    selection: QString,
    error_message: QString,
    busy: bool,
    queue_state: QString,
    queue_message: QString,
    deck_count: i32,
    selected_deck_index: i32,
    review_row_count: i32,
    selected_review_index: i32,
    draft_expression: QString,
    draft_meaning: QString,
    draft_kanji: QString,
    draft_images: QString,
    draft_audio: QString,
    draft_issues: QString,
    draft_provenance: QString,
    draft_dirty: bool,
    draft_pending_count: i32,
    draft_meaning_locked: bool,
    commit_dry_run: bool,
    commit_preview_ready: bool,
    commit_field_count: i32,
    commit_media_count: i32,
    commit_model_changed: bool,
    commit_snapshot_count: i32,
    batch_job_count: i32,
    batch_item_count: i32,
    batch_item_total: i32,
    batch_confirmation: QString,
    theme_watch: Option<ThemeWatch>,
    batch_port: LocalBatchPort,
    controller: ApplicationController,
}

impl Default for AppBackendRust {
    fn default() -> Self {
        Self::from_palette(ThemePalette::load())
    }
}

impl AppBackendRust {
    fn from_palette(palette: ThemePalette) -> Self {
        let controller = ApplicationController::default();
        let batch_port = LocalBatchPort::open();
        let state = controller.state();
        Self {
            theme_background: palette.background.into(),
            theme_surface: palette.surface.into(),
            theme_foreground: palette.foreground.into(),
            theme_muted: palette.muted.into(),
            theme_accent: palette.accent.into(),
            anki_status: state.anki.label().into(),
            ollama_status: state.ollama.label().into(),
            active_deck: state.active_deck.clone().into(),
            selection: state.selection.clone().into(),
            error_message: state.error.clone().into(),
            busy: state.busy,
            queue_state: controller.queue().state().label().into(),
            queue_message: controller.queue().state().message().into(),
            deck_count: queue_len(controller.queue().decks().len()),
            selected_deck_index: queue_index(controller.queue().selected_deck_index()),
            review_row_count: queue_len(controller.queue().rows().len()),
            selected_review_index: queue_index(controller.queue().selected_index()),
            draft_expression: QString::default(),
            draft_meaning: QString::default(),
            draft_kanji: QString::default(),
            draft_images: QString::default(),
            draft_audio: QString::default(),
            draft_issues: QString::default(),
            draft_provenance: QString::default(),
            draft_dirty: false,
            draft_pending_count: 0,
            draft_meaning_locked: false,
            commit_dry_run: true,
            commit_preview_ready: false,
            commit_field_count: 0,
            commit_media_count: 0,
            commit_model_changed: false,
            commit_snapshot_count: 0,
            batch_job_count: 0,
            batch_item_count: 0,
            batch_item_total: 0,
            batch_confirmation: QString::default(),
            theme_watch: omarchy_palette_path().map(ThemeWatch::new),
            batch_port,
            controller,
        }
    }
}

impl qobject::AppBackend {
    pub fn reload_theme(mut self: Pin<&mut Self>) {
        let palette = self
            .as_mut()
            .rust_mut()
            .theme_watch
            .as_mut()
            .and_then(ThemeWatch::poll)
            .unwrap_or_else(ThemePalette::load);
        self.as_mut()
            .set_theme_background(palette.background.into());
        self.as_mut().set_theme_surface(palette.surface.into());
        self.as_mut()
            .set_theme_foreground(palette.foreground.into());
        self.as_mut().set_theme_muted(palette.muted.into());
        self.set_theme_accent(palette.accent.into());
    }

    pub fn refresh_state(mut self: Pin<&mut Self>) {
        match LiveDesktopPort::from_environment() {
            Ok(port) => self.as_mut().rust_mut().controller.refresh(&port),
            Err(error) => self.as_mut().rust_mut().controller.report_error(error),
        }
        sync_controller_state(self);
    }

    pub fn clear_error(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().controller.clear_error();
        sync_controller_state(self);
    }

    pub fn select_item(mut self: Pin<&mut Self>, selection: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .select(selection.to_string());
        sync_controller_state(self);
    }

    pub fn report_error(mut self: Pin<&mut Self>, message: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .report_error(message.to_string());
        sync_controller_state(self);
    }

    pub fn deck_name(&self, index: i32) -> QString {
        self.rust()
            .controller
            .queue()
            .decks()
            .get(index as usize)
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn select_deck_index(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut().rust_mut().controller.select_deck_index(index);
            sync_controller_state(self);
        }
    }

    pub fn review_expression(&self, index: i32) -> QString {
        review_row_value(self, index, |row| &row.expression)
    }

    pub fn review_detail(&self, index: i32) -> QString {
        review_row_value(self, index, |row| &row.detail)
    }

    pub fn review_state(&self, index: i32) -> QString {
        review_row_value(self, index, |row| row.state.label())
    }

    pub fn select_review_index(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            let mut backend = self.as_mut();
            backend
                .as_mut()
                .rust_mut()
                .controller
                .select_queue_index(index);
            match LiveDesktopPort::from_environment() {
                Ok(port) => backend.rust_mut().controller.hydrate_selected(&port),
                Err(error) => backend.rust_mut().controller.report_error(error),
            }
            sync_controller_state(self);
        }
    }

    pub fn edit_draft_meaning(mut self: Pin<&mut Self>, value: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .edit_draft(crate::draft::DraftField::Meaning, value.to_string());
        sync_controller_state(self);
    }

    pub fn edit_draft_expression(mut self: Pin<&mut Self>, value: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .edit_draft(crate::draft::DraftField::Expression, value.to_string());
        sync_controller_state(self);
    }

    pub fn edit_draft_kanji(mut self: Pin<&mut Self>, value: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .edit_draft(crate::draft::DraftField::Kanji, value.to_string());
        sync_controller_state(self);
    }

    pub fn undo_draft(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().controller.undo_draft();
        sync_controller_state(self);
    }

    pub fn redo_draft(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().controller.redo_draft();
        sync_controller_state(self);
    }

    pub fn regenerate_draft(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .controller
            .regenerate_draft(&DisconnectedGenerator);
        sync_controller_state(self);
    }

    pub fn draft_change_value(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().controller.pending_change(index))
            .map(|change| change.value.clone())
            .unwrap_or_default()
            .into()
    }

    pub fn accept_draft_change(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut()
                .rust_mut()
                .controller
                .accept_draft_change(index);
        }
        sync_controller_state(self);
    }

    pub fn reject_draft_change(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut()
                .rust_mut()
                .controller
                .reject_draft_change(index);
        }
        sync_controller_state(self);
    }

    pub fn toggle_draft_meaning_lock(mut self: Pin<&mut Self>, locked: bool) {
        self.as_mut()
            .rust_mut()
            .controller
            .set_draft_locked(crate::draft::DraftField::Meaning, locked);
        sync_controller_state(self);
    }

    pub fn preview_commit(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .controller
            .preview_commit(&mut DisconnectedCommitAdapter);
        sync_controller_state(self);
    }

    pub fn apply_commit(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .controller
            .apply_commit(&mut DisconnectedCommitAdapter);
        sync_controller_state(self);
    }

    pub fn restore_snapshot(mut self: Pin<&mut Self>, index: i32) {
        let snapshot_id = {
            let binding = self.as_ref();
            usize::try_from(index)
                .ok()
                .and_then(|index| binding.rust().controller.commit().snapshots.get(index))
                .map(|snapshot| snapshot.snapshot_id.clone())
        };
        match snapshot_id {
            Some(snapshot_id) => self
                .as_mut()
                .rust_mut()
                .controller
                .restore_commit(&mut DisconnectedCommitAdapter, &snapshot_id),
            None => self
                .as_mut()
                .rust_mut()
                .controller
                .report_error("Snapshot is not available in this history"),
        }
        sync_controller_state(self);
    }

    pub fn commit_field(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().controller.commit().fields.get(index))
            .map(|change| format!("{}: {} → {}", change.field, change.before, change.after))
            .unwrap_or_default()
            .into()
    }

    pub fn commit_snapshot(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().controller.commit().snapshots.get(index))
            .map(|snapshot| format!("{} · note {}", snapshot.snapshot_id, snapshot.note_id))
            .unwrap_or_default()
            .into()
    }

    pub fn card_preview(&self, template_index: i32, back: bool) -> QString {
        let Some(draft) = self.rust().controller.active_draft() else {
            return QString::default();
        };
        let document = linguist_core::CardDocument {
            schema_version: linguist_core::CONTRACT_VERSION,
            expression: draft.expression.clone(),
            values: linguist_core::LogicalFields {
                meaning_image: draft
                    .images
                    .first()
                    .map(|filename| format!("<img src=\"{filename}\">")),
                meaning_text: Some(draft.meaning.clone()),
                kanji_construction: Some(draft.kanji.clone()),
                audio: Some(draft.audio.join("<br/>")),
            },
            media: vec![],
            obsolete_media: vec![],
            issues: vec![],
            tags: vec![],
            provenance: Default::default(),
        };
        usize::try_from(template_index)
            .ok()
            .and_then(|index| {
                crate::preview_model::render_managed_card(
                    &document,
                    index,
                    if back {
                        crate::preview_model::CardFace::Back
                    } else {
                        crate::preview_model::CardFace::Front
                    },
                )
            })
            .map(|rendered| rendered.html)
            .unwrap_or_default()
            .into()
    }

    pub fn preview_audio_url(&self, index: i32) -> QString {
        let Some(draft) = self.rust().controller.active_draft() else {
            return QString::default();
        };
        usize::try_from(index)
            .ok()
            .and_then(|index| {
                crate::preview_model::audio_media_url(&draft.audio.join("<br/>"), index)
            })
            .unwrap_or_default()
            .into()
    }

    pub fn refresh_batches(mut self: Pin<&mut Self>) {
        let port = std::mem::take(&mut self.as_mut().rust_mut().batch_port);
        self.as_mut().rust_mut().controller.refresh_batches(&port);
        self.as_mut().rust_mut().batch_port = port;
        sync_controller_state(self);
    }

    pub fn select_batch(mut self: Pin<&mut Self>, index: i32) {
        let id = {
            let binding = self.as_ref();
            usize::try_from(index)
                .ok()
                .and_then(|index| binding.rust().controller.batch().jobs.get(index))
                .map(|job| job.job.id.clone())
        };
        if let Some(id) = id {
            let port = std::mem::take(&mut self.as_mut().rust_mut().batch_port);
            self.as_mut()
                .rust_mut()
                .controller
                .select_batch(&port, &id, 0);
            self.as_mut().rust_mut().batch_port = port;
        }
        sync_controller_state(self);
    }

    pub fn batch_job(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().controller.batch().jobs.get(index))
            .map(|job| {
                format!(
                    "{} · {} · {} items",
                    job.job.deck_name, job.job.status, job.total
                )
            })
            .unwrap_or_default()
            .into()
    }

    pub fn batch_item(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| {
                self.rust()
                    .controller
                    .batch()
                    .page
                    .as_ref()?
                    .items
                    .get(index)
            })
            .map(|item| format!("{} · {} · {}", item.note_id, item.word, item.status))
            .unwrap_or_default()
            .into()
    }

    pub fn pause_batch(self: Pin<&mut Self>) {
        self.batch_command(ApplicationController::pause_batch);
    }
    pub fn resume_batch(self: Pin<&mut Self>) {
        self.batch_command(ApplicationController::resume_batch);
    }
    pub fn retry_batch(self: Pin<&mut Self>) {
        self.batch_command(ApplicationController::retry_batch);
    }
    pub fn request_batch_action(mut self: Pin<&mut Self>, action: i32) {
        let action = match action {
            0 => Some(crate::batch_model::BatchAction::Cancel),
            1 => Some(crate::batch_model::BatchAction::Rollback),
            2 => Some(crate::batch_model::BatchAction::Delete),
            _ => None,
        };
        if let Some(action) = action {
            self.as_mut()
                .rust_mut()
                .controller
                .request_batch_confirmation(action);
        }
        sync_controller_state(self);
    }
    pub fn confirm_batch_action(self: Pin<&mut Self>) {
        self.batch_command(ApplicationController::confirm_batch);
    }
    pub fn cancel_batch_action(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .controller
            .cancel_batch_confirmation();
        sync_controller_state(self);
    }
    pub fn create_batch(mut self: Pin<&mut Self>, deck_name: &QString, rows: &QString) {
        let parsed = parse_batch_rows(&rows.to_string());
        match parsed {
            Ok(items) => {
                let job = linguist_jobs::NewJob {
                    deck_key: deck_name.to_string(),
                    deck_name: deck_name.to_string(),
                    dry_run: true,
                    settings: Default::default(),
                    items,
                };
                self.batch_command(|controller, port| controller.create_batch(port, job));
            }
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
            }
        }
    }
}

impl qobject::AppBackend {
    fn batch_command(
        mut self: Pin<&mut Self>,
        command: impl FnOnce(&mut ApplicationController, &mut LocalBatchPort),
    ) {
        let mut port = std::mem::take(&mut self.as_mut().rust_mut().batch_port);
        command(&mut self.as_mut().rust_mut().controller, &mut port);
        self.as_mut().rust_mut().batch_port = port;
        sync_controller_state(self);
    }
}

fn parse_batch_rows(rows: &str) -> Result<Vec<linguist_jobs::BatchItemSeed>, String> {
    let items = rows
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (note_id, word) = line
                .split_once('\t')
                .ok_or_else(|| "Each batch row needs: note ID, tab, expression".to_owned())?;
            let note_id = note_id
                .trim()
                .parse()
                .map_err(|_| format!("Invalid note ID: {note_id}"))?;
            let word = word.trim();
            if word.is_empty() {
                return Err("Batch expression cannot be empty".into());
            }
            Ok(linguist_jobs::BatchItemSeed {
                note_id,
                word: word.into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if items.is_empty() {
        return Err("Enter at least one batch row".into());
    }
    Ok(items)
}

fn sync_controller_state(mut qobject: Pin<&mut qobject::AppBackend>) {
    let state = qobject.as_ref().rust().controller.state().clone();
    let (
        queue_state,
        queue_message,
        deck_count,
        selected_deck_index,
        review_row_count,
        selected_review_index,
    ) = {
        let binding = qobject.as_ref();
        let queue = binding.rust().controller.queue();
        (
            queue.state().label(),
            queue.state().message(),
            queue_len(queue.decks().len()),
            queue_index(queue.selected_deck_index()),
            queue_len(queue.rows().len()),
            queue_index(queue.selected_index()),
        )
    };
    let (batch_job_count, batch_item_count, batch_item_total, batch_confirmation) = {
        let binding = qobject.as_ref();
        let batch = binding.rust().controller.batch();
        (
            queue_len(batch.jobs.len()),
            queue_len(batch.page.as_ref().map_or(0, |page| page.items.len())),
            batch
                .page
                .as_ref()
                .map_or(0, |page| page.total.try_into().unwrap_or(i32::MAX)),
            batch
                .pending_confirmation
                .map(|action| match action {
                    crate::batch_model::BatchAction::Cancel => "Cancel this job?",
                    crate::batch_model::BatchAction::Rollback => "Restore completed cards?",
                    crate::batch_model::BatchAction::Delete => "Delete job artifacts?",
                })
                .unwrap_or_default()
                .to_owned(),
        )
    };
    let (
        commit_dry_run,
        commit_preview_ready,
        commit_field_count,
        commit_media_count,
        commit_model_changed,
        commit_snapshot_count,
    ) = {
        let binding = qobject.as_ref();
        let commit = binding.rust().controller.commit();
        (
            commit.dry_run,
            commit.preview_ready,
            queue_len(commit.fields.len()),
            queue_len(commit.media.len()),
            commit.model_changed,
            queue_len(commit.snapshots.len()),
        )
    };
    let (
        draft_expression,
        draft_meaning,
        draft_kanji,
        draft_images,
        draft_audio,
        draft_issues,
        draft_provenance,
        draft_dirty,
        draft_pending_count,
        draft_meaning_locked,
    ) = {
        let binding = qobject.as_ref();
        let draft = binding.rust().controller.active_draft();
        draft
            .map(|draft| {
                (
                    draft.expression.clone(),
                    draft.meaning.clone(),
                    draft.kanji.clone(),
                    draft.images.join("\n"),
                    draft.audio.join("\n"),
                    draft.issues.join("\n"),
                    draft.provenance.join("\n"),
                    draft.dirty(),
                    queue_len(draft.pending().len()),
                    draft.locked(crate::draft::DraftField::Meaning),
                )
            })
            .unwrap_or_default()
    };
    qobject.as_mut().set_anki_status(state.anki.label().into());
    qobject
        .as_mut()
        .set_ollama_status(state.ollama.label().into());
    qobject.as_mut().set_active_deck(state.active_deck.into());
    qobject.as_mut().set_selection(state.selection.into());
    qobject.as_mut().set_error_message(state.error.into());
    qobject.as_mut().set_busy(state.busy);
    qobject.as_mut().set_queue_state(queue_state.into());
    qobject.as_mut().set_queue_message(queue_message.into());
    qobject.as_mut().set_deck_count(deck_count);
    qobject
        .as_mut()
        .set_selected_deck_index(selected_deck_index);
    qobject.as_mut().set_review_row_count(review_row_count);
    qobject
        .as_mut()
        .set_selected_review_index(selected_review_index);
    qobject
        .as_mut()
        .set_draft_expression(draft_expression.into());
    qobject.as_mut().set_draft_meaning(draft_meaning.into());
    qobject.as_mut().set_draft_kanji(draft_kanji.into());
    qobject.as_mut().set_draft_images(draft_images.into());
    qobject.as_mut().set_draft_audio(draft_audio.into());
    qobject.as_mut().set_draft_issues(draft_issues.into());
    qobject
        .as_mut()
        .set_draft_provenance(draft_provenance.into());
    qobject.as_mut().set_draft_dirty(draft_dirty);
    qobject
        .as_mut()
        .set_draft_pending_count(draft_pending_count);
    qobject
        .as_mut()
        .set_draft_meaning_locked(draft_meaning_locked);
    qobject.as_mut().set_commit_dry_run(commit_dry_run);
    qobject
        .as_mut()
        .set_commit_preview_ready(commit_preview_ready);
    qobject.as_mut().set_commit_field_count(commit_field_count);
    qobject.as_mut().set_commit_media_count(commit_media_count);
    qobject
        .as_mut()
        .set_commit_model_changed(commit_model_changed);
    qobject
        .as_mut()
        .set_commit_snapshot_count(commit_snapshot_count);
    qobject.as_mut().set_batch_job_count(batch_job_count);
    qobject.as_mut().set_batch_item_count(batch_item_count);
    qobject.as_mut().set_batch_item_total(batch_item_total);
    qobject
        .as_mut()
        .set_batch_confirmation(batch_confirmation.into());
}

fn review_row_value(
    qobject: &qobject::AppBackend,
    index: i32,
    value: impl FnOnce(&crate::review_model::ReviewRow) -> &str,
) -> QString {
    usize::try_from(index)
        .ok()
        .and_then(|index| qobject.rust().controller.queue().rows().get(index))
        .map(value)
        .unwrap_or_default()
        .into()
}

fn queue_len(len: usize) -> i32 {
    len.try_into().unwrap_or(i32::MAX)
}

fn queue_index(index: Option<usize>) -> i32 {
    index.and_then(|index| index.try_into().ok()).unwrap_or(-1)
}

const MAX_LIVE_QUEUE_NOTES: usize = 1_000;
struct LiveDesktopPort {
    runtime: tokio::runtime::Runtime,
    anki: linguist_anki::AnkiConnectTransport,
    ollama: linguist_ollama::OllamaClient,
}
impl LiveDesktopPort {
    fn from_environment() -> Result<Self, String> {
        let anki_url =
            std::env::var("LINGUIST_ANKI_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
        let ollama_url = std::env::var("LINGUIST_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".into());
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|error| error.to_string())?,
            anki: linguist_anki::AnkiConnectTransport::new(&anki_url)
                .map_err(|error| error.to_string())?,
            ollama: linguist_ollama::OllamaClient::new(&ollama_url)
                .map_err(|error| error.to_string())?,
        })
    }
    fn decks(&self) -> Result<Vec<String>, String> {
        self.runtime
            .block_on(self.anki.deck_names())
            .map(|decks| decks.into_iter().map(|deck| deck.0).collect())
            .map_err(|error| error.to_string())
    }
}
impl DesktopPort for LiveDesktopPort {
    fn anki_available(&self) -> Result<(), String> {
        self.runtime
            .block_on(self.anki.version())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    fn ollama_available(&self) -> Result<(), String> {
        self.runtime
            .block_on(self.ollama.models())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    fn active_deck(&self) -> Result<String, String> {
        self.decks()?
            .into_iter()
            .next()
            .ok_or_else(|| "No Anki decks are available".into())
    }
    fn review_queue(&self) -> Result<ReviewQueueData, String> {
        let decks = self.decks()?;
        let Some(deck) = decks.first() else {
            return Ok(ReviewQueueData {
                decks,
                rows: Vec::new(),
            });
        };
        let query = format!("deck:\"{}\"", deck.replace('"', "\\\""));
        let mut ids = self
            .runtime
            .block_on(self.anki.find_notes(&query))
            .map_err(|error| error.to_string())?;
        ids.truncate(MAX_LIVE_QUEUE_NOTES);
        let notes = self
            .runtime
            .block_on(self.anki.notes_info(&ids))
            .map_err(|error| error.to_string())?;
        let rows = notes.into_iter().map(review_row).collect();
        Ok(ReviewQueueData { decks, rows })
    }
}
impl DraftNotePort for LiveDesktopPort {
    fn note(&self, note_id: i64) -> Result<linguist_application::NoteInfo, String> {
        self.runtime
            .block_on(self.anki.notes_info(&[note_id]))
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| format!("Anki note {note_id} was not found"))
    }
}
fn review_row(note: linguist_application::NoteInfo) -> ReviewRow {
    let expression = ["Expression", "Word", "Front", "Vocabulary"]
        .into_iter()
        .find_map(|name| note.fields.get(name))
        .cloned()
        .or_else(|| note.fields.values().next().cloned())
        .unwrap_or_else(|| format!("Note {}", note.note_id));
    ReviewRow {
        note_id: note.note_id,
        expression,
        detail: note.model_name.0,
        state: ReviewState::Ready,
    }
}

struct DisconnectedGenerator;

struct DisconnectedCommitAdapter;

impl DraftGenerationPort for DisconnectedGenerator {
    fn generate(
        &self,
        _draft: &crate::draft::ReviewDraft,
    ) -> Result<Vec<crate::draft::GeneratedChange>, String> {
        Err("Generation adapter is not connected yet".into())
    }
}

impl crate::commit_model::CommitExecutor<crate::draft::ReviewDraft> for DisconnectedCommitAdapter {
    fn preview(
        &mut self,
        _draft: &crate::draft::ReviewDraft,
    ) -> Result<crate::commit_model::CommitPreview, String> {
        Err("Commit adapter is not connected yet".into())
    }

    fn apply(
        &mut self,
        _draft: &crate::draft::ReviewDraft,
    ) -> Result<crate::commit_model::SnapshotHistoryItem, String> {
        Err("Commit adapter is not connected yet".into())
    }

    fn restore(&mut self, _snapshot_id: &str) -> Result<(), String> {
        Err("Commit adapter is not connected yet".into())
    }
}
