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
        #[qproperty(bool, draft_available)]
        #[qproperty(bool, draft_can_undo)]
        #[qproperty(bool, draft_can_redo)]
        #[qproperty(i32, draft_pending_count)]
        #[qproperty(bool, draft_meaning_locked)]
        #[qproperty(bool, commit_dry_run)]
        #[qproperty(bool, commit_preview_ready)]
        #[qproperty(i32, commit_field_count)]
        #[qproperty(i32, commit_media_count)]
        #[qproperty(bool, commit_model_changed)]
        #[qproperty(i32, commit_snapshot_count)]
        #[qproperty(i32, batch_job_count)]
        #[qproperty(i32, batch_selected_index)]
        #[qproperty(QString, batch_status)]
        #[qproperty(i32, batch_item_count)]
        #[qproperty(i32, batch_item_total)]
        #[qproperty(QString, batch_confirmation)]
        #[qproperty(i32, manual_preview_count)]
        #[qproperty(i32, manual_enqueue_count)]
        #[qproperty(i32, manual_issue_count)]
        #[qproperty(QString, csv_headers)]
        #[qproperty(QString, csv_mapping)]
        #[qproperty(QString, selector_total)]
        #[qproperty(i32, selector_preview_count)]
        #[qproperty(bool, selector_limited)]
        #[qproperty(QString, settings_anki_url)]
        #[qproperty(QString, settings_ollama_url)]
        #[qproperty(QString, settings_ollama_model)]
        #[qproperty(QString, settings_dictionary_preset)]
        #[qproperty(bool, settings_dry_run)]
        #[qproperty(QString, settings_message)]
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
        #[cxx_name = "searchReview"]
        fn search_review(self: Pin<&mut Self>, query: &QString);
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
        #[cxx_name = "runBatchTick"]
        fn run_batch_tick(self: Pin<&mut Self>);
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
        fn create_batch(self: Pin<&mut Self>, deck_name: &QString, rows: &QString, dry_run: bool);
        #[qinvokable]
        #[cxx_name = "previewManualInput"]
        fn preview_manual_input(
            self: Pin<&mut Self>,
            raw: &QString,
            deck_key: &QString,
            language_key: &QString,
            type_tag: &QString,
        );
        #[qinvokable]
        #[cxx_name = "manualPreviewRow"]
        fn manual_preview_row(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "manualPreviewIssue"]
        fn manual_preview_issue(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "manualPreviewOptions"]
        fn manual_preview_options(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "selectManualDecision"]
        fn select_manual_decision(self: Pin<&mut Self>, index: i32, choice: i32);
        #[qinvokable]
        #[cxx_name = "enqueueManualInput"]
        fn enqueue_manual_input(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "previewCsvInput"]
        fn preview_csv_input(
            self: Pin<&mut Self>,
            content: &QString,
            deck_key: &QString,
            language_key: &QString,
            type_tag: &QString,
        );
        #[qinvokable]
        #[cxx_name = "previewCsvFile"]
        fn preview_csv_file(
            self: Pin<&mut Self>,
            file_url: &QString,
            deck_key: &QString,
            language_key: &QString,
            type_tag: &QString,
        );
        #[qinvokable]
        #[cxx_name = "previewBatchSelector"]
        fn preview_batch_selector(self: Pin<&mut Self>, selector_json: &QString);
        #[qinvokable]
        #[cxx_name = "selectorPreviewRow"]
        fn selector_preview_row(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "createBatchFromSelector"]
        fn create_batch_from_selector(self: Pin<&mut Self>, dry_run: bool);
        #[qinvokable]
        #[cxx_name = "saveSettings"]
        fn save_settings(
            self: Pin<&mut Self>,
            anki_url: &QString,
            ollama_url: &QString,
            ollama_model: &QString,
            dictionary_preset: &QString,
            dry_run: bool,
        );
        #[qinvokable]
        #[cxx_name = "mappedDeckName"]
        fn mapped_deck_name(self: &AppBackend, purpose: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "mappedModelName"]
        fn mapped_model_name(self: &AppBackend, purpose: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "saveDeckMapping"]
        fn save_deck_mapping(
            self: Pin<&mut Self>,
            purpose: &QString,
            deck_name: &QString,
            model_name: &QString,
        );
        #[qinvokable]
        #[cxx_name = "importLegacyConfig"]
        fn import_legacy_config(self: Pin<&mut Self>, file_url: &QString);
    }
}

use std::pin::Pin;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use linguist_application::{
    BatchSelector, CommitRequest, CommitSource, CsvIngestRequest, DuplicateDecision,
    IngestionPreview, ManualIngestRequest, commit_card, duplicate_decision_options,
    prepare_csv_input, prepare_manual_input, resolve_ingestion_preview, restore_snapshot,
    selector_preview,
};
use linguist_core::{
    CONTRACT_VERSION, CardBuildInput, CardDocument, CardMode, FieldMapping, LogicalFields,
    ProcessedCardData, Provenance, SourceKind,
};
use linguist_snapshots::SnapshotRepository;

use crate::controller::{
    ApplicationController, DesktopPort, DraftGenerationPort, DraftNotePort, GeneratedDraft,
};
use crate::review_model::{ReviewQueueData, ReviewRow, ReviewState};
use crate::theme::{ThemePalette, ThemeWatch, omarchy_palette_path};

#[derive(Clone, Default)]
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

    fn run_tick(&self) -> Result<(), String> {
        let repository = self.repository()?;
        let Some(job_id) = repository
            .job_summaries()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|summary| summary.job.status == "running")
            .map(|summary| summary.job.id)
        else {
            return Ok(());
        };
        let Some(_lease) = repository
            .acquire_runner_lease()
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        let mut worker = LiveBatchWorker::from_environment()?;
        repository
            .run_next(&job_id, &mut worker, 3, 2)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

struct NoBatchRestore;
impl linguist_jobs::BatchRollbackPort for NoBatchRestore {
    fn restore_snapshot(&self, snapshot_id: &str) -> Result<(), String> {
        let mut adapter = LiveCommitAdapter::from_environment()?;
        adapter.restore_snapshot_id(snapshot_id)
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
    draft_available: bool,
    draft_can_undo: bool,
    draft_can_redo: bool,
    draft_pending_count: i32,
    draft_meaning_locked: bool,
    commit_dry_run: bool,
    commit_preview_ready: bool,
    commit_field_count: i32,
    commit_media_count: i32,
    commit_model_changed: bool,
    commit_snapshot_count: i32,
    batch_job_count: i32,
    batch_selected_index: i32,
    batch_status: QString,
    batch_item_count: i32,
    batch_item_total: i32,
    batch_confirmation: QString,
    manual_preview_count: i32,
    manual_enqueue_count: i32,
    manual_issue_count: i32,
    manual_preview_rows: Vec<String>,
    manual_preview_issues: Vec<String>,
    manual_pending: Vec<(linguist_application::InputRow, DuplicateDecision)>,
    manual_options: Vec<Vec<DuplicateDecision>>,
    csv_headers: QString,
    csv_mapping: QString,
    selector_total: QString,
    selector_preview_count: i32,
    selector_limited: bool,
    settings_anki_url: QString,
    settings_ollama_url: QString,
    settings_ollama_model: QString,
    settings_dictionary_preset: QString,
    settings_dry_run: bool,
    settings_message: QString,
    selector_preview_rows: Vec<String>,
    selector_pending: Option<BatchSelector>,
    theme_watch: Option<ThemeWatch>,
    batch_port: LocalBatchPort,
    batch_worker_active: Arc<AtomicBool>,
    batch_worker_error: Arc<Mutex<Option<String>>>,
    generation_adapter: Option<LiveGenerationAdapter>,
    controller: ApplicationController,
}

impl Default for AppBackendRust {
    fn default() -> Self {
        Self::from_palette(ThemePalette::load())
    }
}

impl AppBackendRust {
    fn from_palette(palette: ThemePalette) -> Self {
        let config = runtime_config().unwrap_or_default();
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
            draft_available: false,
            draft_can_undo: false,
            draft_can_redo: false,
            draft_pending_count: 0,
            draft_meaning_locked: false,
            commit_dry_run: true,
            commit_preview_ready: false,
            commit_field_count: 0,
            commit_media_count: 0,
            commit_model_changed: false,
            commit_snapshot_count: 0,
            batch_job_count: 0,
            batch_selected_index: -1,
            batch_status: QString::default(),
            batch_item_count: 0,
            batch_item_total: 0,
            batch_confirmation: QString::default(),
            manual_preview_count: 0,
            manual_enqueue_count: 0,
            manual_issue_count: 0,
            manual_preview_rows: Vec::new(),
            manual_preview_issues: Vec::new(),
            manual_pending: Vec::new(),
            manual_options: Vec::new(),
            csv_headers: QString::default(),
            csv_mapping: QString::default(),
            selector_total: QString::from("0"),
            selector_preview_count: 0,
            selector_limited: false,
            settings_anki_url: config.anki_url.into(),
            settings_ollama_url: config.ollama_url.into(),
            settings_ollama_model: config.ollama_model.unwrap_or_default().into(),
            settings_dictionary_preset: config.dictionary_preset.into(),
            settings_dry_run: config.dry_run,
            settings_message: QString::default(),
            selector_preview_rows: Vec::new(),
            selector_pending: None,
            theme_watch: omarchy_palette_path().map(ThemeWatch::new),
            batch_port,
            batch_worker_active: Arc::new(AtomicBool::new(false)),
            batch_worker_error: Arc::new(Mutex::new(None)),
            generation_adapter: None,
            controller,
        }
    }
}

impl qobject::AppBackend {
    pub fn run_batch_tick(mut self: Pin<&mut Self>) {
        let error = self
            .as_ref()
            .rust()
            .batch_worker_error
            .lock()
            .ok()
            .and_then(|mut error| error.take());
        if let Some(error) = error {
            self.as_mut().rust_mut().controller.report_error(error);
            sync_controller_state(self.as_mut());
        }
        let (port, active, error) = {
            let pinned = self.as_ref();
            let state = pinned.rust();
            if state
                .batch_worker_active
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }
            (
                state.batch_port.clone(),
                state.batch_worker_active.clone(),
                state.batch_worker_error.clone(),
            )
        };
        std::thread::spawn(move || {
            if let Err(message) = port.run_tick()
                && let Ok(mut slot) = error.lock()
            {
                *slot = Some(message);
            }
            active.store(false, Ordering::Release);
        });
    }

    pub fn save_settings(
        mut self: Pin<&mut Self>,
        anki_url: &QString,
        ollama_url: &QString,
        ollama_model: &QString,
        dictionary_preset: &QString,
        dry_run: bool,
    ) {
        let mut config = runtime_config().unwrap_or_default();
        config.version = linguist_config::NATIVE_CONFIG_VERSION;
        config.anki_url = anki_url.to_string().trim().to_owned();
        config.ollama_url = ollama_url.to_string().trim().to_owned();
        config.ollama_model = nonempty_setting(&ollama_model.to_string());
        config.dictionary_preset = dictionary_preset.to_string().trim().to_owned();
        config.dry_run = dry_run;
        match native_config_path().and_then(|path| {
            linguist_config::save_native_replace(&path, &config).map_err(|error| error.to_string())
        }) {
            Ok(()) => apply_settings(self.as_mut(), config, "Settings saved"),
            Err(error) => self.set_settings_message(error.into()),
        }
    }

    pub fn mapped_deck_name(&self, purpose: &QString) -> QString {
        runtime_config()
            .ok()
            .and_then(|config| config.decks.get(&purpose.to_string()).cloned())
            .and_then(|deck| deck.deck_name)
            .unwrap_or_default()
            .into()
    }

    pub fn mapped_model_name(&self, purpose: &QString) -> QString {
        runtime_config()
            .ok()
            .and_then(|config| config.decks.get(&purpose.to_string()).cloned())
            .and_then(|deck| deck.model_name)
            .unwrap_or_default()
            .into()
    }

    pub fn save_deck_mapping(
        mut self: Pin<&mut Self>,
        purpose: &QString,
        deck_name: &QString,
        model_name: &QString,
    ) {
        let result = (|| {
            let mut config = runtime_config()?;
            set_deck_mapping(
                &mut config,
                &purpose.to_string(),
                &deck_name.to_string(),
                &model_name.to_string(),
            )?;
            let path = native_config_path()?;
            linguist_config::save_native_replace(&path, &config)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(config)
        })();
        match result {
            Ok(config) => apply_settings(self.as_mut(), config, "Deck purpose mapping saved"),
            Err(error) => self.set_settings_message(error.into()),
        }
    }

    pub fn import_legacy_config(mut self: Pin<&mut Self>, file_url: &QString) {
        let result: Result<(linguist_config::NativeConfig, String), String> = (|| {
            let source = local_file_path(&file_url.to_string())?;
            let metadata = std::fs::metadata(&source).map_err(|error| error.to_string())?;
            if metadata.len() > 1024 * 1024 {
                return Err("Legacy config exceeds 1 MiB limit".into());
            }
            let contents = std::fs::read_to_string(source).map_err(|error| error.to_string())?;
            let report = linguist_config::import_legacy_yaml(&contents)
                .map_err(|error| error.to_string())?;
            let path = native_config_path()?;
            linguist_config::save_native_new(&path, &report.config)
                .map_err(|error| error.to_string())?;
            let message = if report.warnings.is_empty() {
                "Legacy config imported".into()
            } else {
                format!("Imported with warnings: {}", report.warnings.join("; "))
            };
            Ok((report.config, message))
        })();
        match result {
            Ok((config, message)) => apply_settings(self.as_mut(), config, &message),
            Err(error) => self.set_settings_message(error.into()),
        }
    }

    pub fn reload_theme(mut self: Pin<&mut Self>) {
        let Some(palette) = self
            .as_mut()
            .rust_mut()
            .theme_watch
            .as_mut()
            .and_then(ThemeWatch::poll)
        else {
            return;
        };
        self.as_mut()
            .set_theme_background(palette.background.into());
        self.as_mut().set_theme_surface(palette.surface.into());
        self.as_mut()
            .set_theme_foreground(palette.foreground.into());
        self.as_mut().set_theme_muted(palette.muted.into());
        self.set_theme_accent(palette.accent.into());
    }

    pub fn refresh_state(mut self: Pin<&mut Self>) {
        let selected_deck = {
            let binding = self.as_ref();
            let queue = binding.rust().controller.queue();
            queue
                .selected_deck_index()
                .and_then(|index| queue.decks().get(index))
                .cloned()
        };
        match LiveDesktopPort::from_environment().map(|port| port.for_deck(selected_deck)) {
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

    pub fn search_review(self: Pin<&mut Self>, query: &QString) {
        let query = query.to_string().trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let (row, deck) = {
            let binding = self.as_ref();
            let queue = binding.rust().controller.queue();
            let row = queue.rows().iter().position(|row| {
                row.expression.to_lowercase().contains(&query)
                    || row.detail.to_lowercase().contains(&query)
            });
            let deck = queue
                .decks()
                .iter()
                .position(|deck| deck.to_lowercase().contains(&query));
            (row, deck)
        };
        if let Some(index) = row {
            self.select_review_index(index as i32);
        } else if let Some(index) = deck {
            self.select_deck_index(index as i32);
        } else {
            self.report_error(&format!("No card or deck matches '{query}'").into());
        }
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
            let selected_deck = self
                .as_ref()
                .rust()
                .controller
                .queue()
                .decks()
                .get(index)
                .cloned();
            if selected_deck.is_none() {
                return;
            }
            self.as_mut().rust_mut().controller.select_deck_index(index);
            match LiveDesktopPort::from_environment().map(|port| port.for_deck(selected_deck)) {
                Ok(port) => self.as_mut().rust_mut().controller.refresh(&port),
                Err(error) => self.as_mut().rust_mut().controller.report_error(error),
            }
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
            let needs_hydration = backend
                .as_ref()
                .rust()
                .controller
                .queue()
                .rows()
                .get(index)
                .is_some_and(|row| row.note_id >= 0);
            backend
                .as_mut()
                .rust_mut()
                .controller
                .select_queue_index(index);
            if needs_hydration {
                match LiveDesktopPort::from_environment() {
                    Ok(port) => backend.rust_mut().controller.hydrate_selected(&port),
                    Err(error) => backend.rust_mut().controller.report_error(error),
                }
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
        let mut state = self.as_mut().rust_mut();
        if state.generation_adapter.is_none() {
            match LiveGenerationAdapter::from_environment() {
                Ok(adapter) => state.generation_adapter = Some(adapter),
                Err(error) => {
                    state.controller.report_error(error);
                    sync_controller_state(self);
                    return;
                }
            }
        }
        let adapter = state
            .generation_adapter
            .take()
            .expect("generation adapter initialized");
        state.controller.regenerate_draft(&adapter);
        state.generation_adapter = Some(adapter);
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
        match LiveCommitAdapter::from_environment() {
            Ok(mut adapter) => self
                .as_mut()
                .rust_mut()
                .controller
                .preview_commit(&mut adapter),
            Err(error) => self.as_mut().rust_mut().controller.report_error(error),
        }
        sync_controller_state(self);
    }

    pub fn apply_commit(mut self: Pin<&mut Self>) {
        match LiveCommitAdapter::from_environment() {
            Ok(mut adapter) => self
                .as_mut()
                .rust_mut()
                .controller
                .apply_commit(&mut adapter),
            Err(error) => self.as_mut().rust_mut().controller.report_error(error),
        }
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
            Some(snapshot_id) => match LiveCommitAdapter::from_environment() {
                Ok(mut adapter) => self
                    .as_mut()
                    .rust_mut()
                    .controller
                    .restore_commit(&mut adapter, &snapshot_id),
                Err(error) => self.as_mut().rust_mut().controller.report_error(error),
            },
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
    pub fn create_batch(
        mut self: Pin<&mut Self>,
        deck_name: &QString,
        rows: &QString,
        dry_run: bool,
    ) {
        let parsed = parse_batch_rows(&rows.to_string());
        match parsed {
            Ok(items) => {
                let job = linguist_jobs::NewJob {
                    deck_key: deck_name.to_string(),
                    deck_name: deck_name.to_string(),
                    dry_run,
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

    pub fn preview_manual_input(
        mut self: Pin<&mut Self>,
        raw: &QString,
        deck_key: &QString,
        language_key: &QString,
        type_tag: &QString,
    ) {
        clear_ingestion_preview(self.as_mut());
        let prepared = prepare_manual_input(&ManualIngestRequest {
            raw: raw.to_string(),
            deck_key: deck_key.to_string(),
            language_key: language_key.to_string(),
            type_tag: type_tag.to_string(),
        });
        let preview = match runtime_config()
            .map(|config| route_ingestion_preview(prepared, &config, &language_key.to_string()))
        {
            Ok(preview) => preview,
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let display = build_ingestion_display(&preview);
        let row_count = queue_len(display.rows.len());
        let issue_count = queue_len(display.issues.len());
        let enqueue_count = pending_enqueue_count(&display.pending);
        self.as_mut().rust_mut().manual_preview_rows = display.rows;
        self.as_mut().rust_mut().manual_preview_issues = display.issues;
        self.as_mut().rust_mut().manual_pending = display.pending;
        self.as_mut().rust_mut().manual_options = display.options;
        self.as_mut().set_manual_preview_count(row_count);
        self.as_mut().set_manual_enqueue_count(enqueue_count);
        self.as_mut().set_manual_issue_count(issue_count);
    }

    pub fn manual_preview_row(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().manual_preview_rows.get(index))
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn manual_preview_issue(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().manual_preview_issues.get(index))
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn manual_preview_options(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().manual_options.get(index))
            .map(|options| {
                options
                    .iter()
                    .map(ingestion_decision_label)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
            .into()
    }

    pub fn select_manual_decision(mut self: Pin<&mut Self>, index: i32, choice: i32) {
        let selected = usize::try_from(index)
            .ok()
            .zip(usize::try_from(choice).ok())
            .and_then(|(index, choice)| {
                self.as_ref()
                    .rust()
                    .manual_options
                    .get(index)
                    .and_then(|options| options.get(choice))
                    .cloned()
                    .map(|decision| (index, decision))
            });
        if let Some((index, decision)) = selected {
            if let Some((_, pending)) = self.as_mut().rust_mut().manual_pending.get_mut(index) {
                *pending = decision;
            }
            let enqueue_count = pending_enqueue_count(&self.as_ref().rust().manual_pending);
            self.as_mut().set_manual_enqueue_count(enqueue_count);
        }
    }

    pub fn enqueue_manual_input(mut self: Pin<&mut Self>) {
        let pending = std::mem::take(&mut self.as_mut().rust_mut().manual_pending);
        let mut synthetic_id = -1_i64;
        let mut blocked = 0_usize;
        for (row, decision) in pending {
            match decision {
                DuplicateDecision::Inject => {
                    while self
                        .as_ref()
                        .rust()
                        .controller
                        .queue()
                        .rows()
                        .iter()
                        .any(|existing| existing.note_id == synthetic_id)
                    {
                        synthetic_id -= 1;
                    }
                    let config = runtime_config().unwrap_or_default();
                    let model = config
                        .decks
                        .get(&row.language_key)
                        .and_then(|deck| deck.model_name.clone())
                        .unwrap_or_else(|| match row.language_key.as_str() {
                            "japanese_vocab" | "japanese" | "ja" => {
                                "Linguist Japanese Vocabulary".into()
                            }
                            _ => "Linguist Vocabulary".into(),
                        });
                    let mut draft = crate::draft::ReviewDraft::injection(
                        synthetic_id,
                        row.expression,
                        row.context,
                        row.deck_key,
                        model,
                    );
                    draft.language_key = Some(row.language_key.clone());
                    draft.type_tag = row.type_tag.clone();
                    self.as_mut().rust_mut().controller.enqueue_draft(
                        draft,
                        format!("{} · {} · injection", row.language_key, row.type_tag),
                    );
                    synthetic_id -= 1;
                }
                DuplicateDecision::Modernize { note } => {
                    let detail = format!("{} · {} · modernization", row.language_key, row.type_tag);
                    let mut draft = crate::draft::ReviewDraft::from_note(&note);
                    draft.language_key = Some(row.language_key);
                    draft.type_tag = row.type_tag;
                    if !row.context.is_empty() {
                        draft.source_context = row.context;
                    }
                    self.as_mut()
                        .rust_mut()
                        .controller
                        .enqueue_draft(draft, detail);
                }
                DuplicateDecision::Ambiguous { .. } | DuplicateDecision::Skip => blocked += 1,
            }
        }
        self.as_mut().rust_mut().manual_preview_rows.clear();
        self.as_mut().rust_mut().manual_preview_issues.clear();
        self.as_mut().rust_mut().manual_options.clear();
        self.as_mut().set_manual_preview_count(0);
        self.as_mut().set_manual_enqueue_count(0);
        self.as_mut().set_manual_issue_count(0);
        if blocked > 0 {
            self.as_mut().rust_mut().controller.report_error(format!(
                "{blocked} ambiguous or unchecked rows were not enqueued"
            ));
        }
        sync_controller_state(self);
    }

    pub fn preview_csv_input(
        mut self: Pin<&mut Self>,
        content: &QString,
        deck_key: &QString,
        language_key: &QString,
        type_tag: &QString,
    ) {
        clear_ingestion_preview(self.as_mut());
        let csv = match prepare_csv_input(&CsvIngestRequest {
            content: content.to_string(),
            deck_key: deck_key.to_string(),
            language_key: language_key.to_string(),
            type_tag: type_tag.to_string(),
            mapping: None,
        }) {
            Ok(csv) => csv,
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let header = |column: Option<usize>| {
            column
                .and_then(|column| csv.headers.get(column))
                .cloned()
                .unwrap_or_else(|| "—".into())
        };
        let mapping = format!(
            "Expression: {} · Language: {} · Type: {} · Context: {}",
            header(Some(csv.mapping.expression)),
            header(csv.mapping.language),
            header(csv.mapping.type_tag),
            header(csv.mapping.context)
        );
        let prepared = IngestionPreview {
            rows: csv.rows,
            issues: csv.issues,
            duplicates: csv.duplicates,
        };
        let preview = match runtime_config()
            .map(|config| route_ingestion_preview(prepared, &config, &language_key.to_string()))
        {
            Ok(preview) => preview,
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let display = build_ingestion_display(&preview);
        let row_count = queue_len(display.rows.len());
        let issue_count = queue_len(display.issues.len());
        let enqueue_count = pending_enqueue_count(&display.pending);
        self.as_mut().rust_mut().manual_preview_rows = display.rows;
        self.as_mut().rust_mut().manual_preview_issues = display.issues;
        self.as_mut().rust_mut().manual_pending = display.pending;
        self.as_mut().rust_mut().manual_options = display.options;
        self.as_mut().set_manual_preview_count(row_count);
        self.as_mut().set_manual_enqueue_count(enqueue_count);
        self.as_mut().set_manual_issue_count(issue_count);
        self.as_mut()
            .set_csv_headers(csv.headers.join(" · ").into());
        self.as_mut().set_csv_mapping(mapping.into());
        sync_controller_state(self);
    }

    pub fn preview_csv_file(
        mut self: Pin<&mut Self>,
        file_url: &QString,
        deck_key: &QString,
        language_key: &QString,
        type_tag: &QString,
    ) {
        clear_ingestion_preview(self.as_mut());
        let path = match local_file_path(&file_url.to_string()) {
            Ok(path) => path,
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) if metadata.len() <= 10 * 1024 * 1024 => metadata,
            Ok(_) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .report_error("CSV file exceeds the 10 MiB preview limit");
                sync_controller_state(self);
                return;
            }
            Err(error) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .report_error(error.to_string());
                sync_controller_state(self);
                return;
            }
        };
        let _ = metadata;
        match std::fs::read_to_string(path) {
            Ok(content) => {
                self.preview_csv_input(&content.into(), deck_key, language_key, type_tag)
            }
            Err(error) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .report_error(error.to_string());
                sync_controller_state(self);
            }
        }
    }

    pub fn preview_batch_selector(mut self: Pin<&mut Self>, selector_json: &QString) {
        let selector: BatchSelector = match serde_json::from_str(&selector_json.to_string()) {
            Ok(selector) => selector,
            Err(error) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .report_error(format!("Invalid batch selector: {error}"));
                sync_controller_state(self);
                return;
            }
        };
        match LiveDesktopPort::from_environment().and_then(|port| port.preview_selector(&selector))
        {
            Ok(preview) => {
                let rows = preview
                    .notes
                    .into_iter()
                    .map(|note| {
                        format!(
                            "{} · {} · {} · {}",
                            note.note_id, note.expression, note.deck_key, note.model_name
                        )
                    })
                    .collect::<Vec<_>>();
                let count = queue_len(rows.len());
                self.as_mut().rust_mut().selector_preview_rows = rows;
                self.as_mut().rust_mut().selector_pending = Some(selector);
                self.as_mut()
                    .set_selector_total(preview.total.to_string().into());
                self.as_mut().set_selector_preview_count(count);
                self.as_mut().set_selector_limited(preview.limited);
            }
            Err(error) => self.as_mut().rust_mut().controller.report_error(error),
        }
        sync_controller_state(self);
    }

    pub fn selector_preview_row(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().selector_preview_rows.get(index))
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn create_batch_from_selector(mut self: Pin<&mut Self>, dry_run: bool) {
        let Some(selector) = self.as_ref().rust().selector_pending.clone() else {
            self.as_mut()
                .rust_mut()
                .controller
                .report_error("Preview a selector before creating its batch");
            sync_controller_state(self);
            return;
        };
        let items = match LiveDesktopPort::from_environment()
            .and_then(|port| port.selector_items(&selector))
        {
            Ok(items) if !items.is_empty() => items,
            Ok(_) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .report_error("Selector returned no notes");
                sync_controller_state(self);
                return;
            }
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let deck_name = selector.deck.clone().unwrap_or_default();
        let settings = BTreeMap::from([(
            "selector".into(),
            serde_json::to_value(&selector).expect("batch selector is serializable"),
        )]);
        let job = linguist_jobs::NewJob {
            deck_key: deck_name.clone(),
            deck_name,
            dry_run,
            settings,
            items,
        };
        self.as_mut().rust_mut().selector_pending = None;
        self.batch_command(|controller, port| controller.create_batch(port, job));
    }
}

fn ingestion_decision_label(decision: &DuplicateDecision) -> String {
    match decision {
        DuplicateDecision::Inject => "Inject new card".into(),
        DuplicateDecision::Modernize { note } => format!("Modernize note {}", note.note_id),
        DuplicateDecision::Ambiguous { matches } => {
            format!("Ambiguous · {} exact notes", matches.len())
        }
        DuplicateDecision::Skip => "Skip".into(),
    }
}

fn pending_enqueue_count(pending: &[(linguist_application::InputRow, DuplicateDecision)]) -> i32 {
    queue_len(
        pending
            .iter()
            .filter(|(_, decision)| {
                matches!(
                    decision,
                    DuplicateDecision::Inject | DuplicateDecision::Modernize { .. }
                )
            })
            .count(),
    )
}

fn clear_ingestion_preview(mut backend: Pin<&mut qobject::AppBackend>) {
    backend.as_mut().rust_mut().manual_preview_rows.clear();
    backend.as_mut().rust_mut().manual_preview_issues.clear();
    backend.as_mut().rust_mut().manual_pending.clear();
    backend.as_mut().rust_mut().manual_options.clear();
    backend.as_mut().set_manual_preview_count(0);
    backend.as_mut().set_manual_enqueue_count(0);
    backend.as_mut().set_manual_issue_count(0);
}

fn route_ingestion_preview(
    mut preview: IngestionPreview,
    config: &linguist_config::NativeConfig,
    default_language: &str,
) -> IngestionPreview {
    let default_key = linguist_application::canonical_language_key(default_language);
    preview.rows.retain_mut(|row| {
        let Some(key) = linguist_application::canonical_language_key(&row.language_key) else {
            preview.issues.push(linguist_application::RowIssue {
                line: row.source_line,
                message: format!("Unknown language key: {}", row.language_key),
            });
            return false;
        };
        if Some(key.as_str()) != default_key.as_deref() {
            let Some(deck_name) = config
                .decks
                .get(&key)
                .and_then(|deck| deck.deck_name.as_deref())
                .filter(|name| !name.trim().is_empty())
            else {
                preview.issues.push(linguist_application::RowIssue {
                    line: row.source_line,
                    message: format!("No target deck mapped for {key}"),
                });
                return false;
            };
            row.deck_key = deck_name.to_owned();
        }
        row.language_key = key;
        true
    });
    preview
}

struct IngestionDisplay {
    rows: Vec<String>,
    issues: Vec<String>,
    pending: Vec<(linguist_application::InputRow, DuplicateDecision)>,
    options: Vec<Vec<DuplicateDecision>>,
}

fn build_ingestion_display(preview: &IngestionPreview) -> IngestionDisplay {
    let mut issues = preview
        .issues
        .iter()
        .map(|issue| format!("Line {} · {}", issue.line, issue.message))
        .collect::<Vec<_>>();
    issues.extend(
        preview
            .duplicates
            .iter()
            .map(|line| format!("Line {line} · duplicate in imported input")),
    );
    let decisions = match LiveDesktopPort::from_environment()
        .and_then(|port| port.ingestion_candidates(preview))
    {
        Ok(notes) => resolve_ingestion_preview(preview, notes),
        Err(error) => {
            issues.push(format!("Duplicate check unavailable · {error}"));
            vec![DuplicateDecision::Skip; preview.rows.len()]
        }
    };
    let options = preview
        .rows
        .iter()
        .zip(decisions)
        .map(|(row, decision)| {
            duplicate_decision_options(&decision, preview.duplicates.contains(&row.source_line))
        })
        .collect::<Vec<_>>();
    let rows = preview
        .rows
        .iter()
        .map(|row| {
            let context = if row.context.is_empty() {
                String::new()
            } else {
                format!(" · {}", row.context)
            };
            format!(
                "{} · {}{} · {} · {}",
                row.ordinal, row.expression, context, row.language_key, row.type_tag
            )
        })
        .collect();
    let pending = preview
        .rows
        .iter()
        .cloned()
        .zip(options.iter().map(|choices| choices[0].clone()))
        .collect();
    IngestionDisplay {
        rows,
        issues,
        pending,
        options,
    }
}

fn local_file_path(value: &str) -> Result<PathBuf, String> {
    let encoded = value
        .strip_prefix("file://")
        .ok_or_else(|| "Only local file URLs are accepted".to_owned())?;
    let encoded = encoded.strip_prefix("localhost").unwrap_or(encoded);
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| "Invalid percent escape in file URL".to_owned())?;
            let text = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            decoded.push(u8::from_str_radix(text, 16).map_err(|_| "Invalid file URL escape")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(decoded).map_err(|error| error.to_string())?;
    if !path.starts_with('/') {
        return Err("CSV file URL must be absolute".into());
    }
    Ok(PathBuf::from(path))
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
    let (
        batch_job_count,
        batch_selected_index,
        batch_status,
        batch_item_count,
        batch_item_total,
        batch_confirmation,
    ) = {
        let binding = qobject.as_ref();
        let batch = binding.rust().controller.batch();
        let selected = batch.selected_job_id.as_ref().and_then(|id| {
            batch
                .jobs
                .iter()
                .enumerate()
                .find(|(_, job)| &job.job.id == id)
        });
        (
            queue_len(batch.jobs.len()),
            selected.map(|(index, _)| queue_len(index)).unwrap_or(-1),
            selected
                .map(|(_, job)| job.job.status.clone())
                .unwrap_or_default(),
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
    let (draft_available, draft_can_undo, draft_can_redo) = {
        let binding = qobject.as_ref();
        binding
            .rust()
            .controller
            .active_draft()
            .map(|draft| (true, draft.can_undo(), draft.can_redo()))
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
    qobject.as_mut().set_draft_available(draft_available);
    qobject.as_mut().set_draft_can_undo(draft_can_undo);
    qobject.as_mut().set_draft_can_redo(draft_can_redo);
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
    qobject
        .as_mut()
        .set_batch_selected_index(batch_selected_index);
    qobject.as_mut().set_batch_status(batch_status.into());
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

fn native_config_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "XDG config directory is unavailable".to_owned())?;
    Ok(linguist_config::native_config_path(&root))
}

fn runtime_config() -> Result<linguist_config::NativeConfig, String> {
    let path = native_config_path()?;
    if !path.exists() {
        return Ok(linguist_config::NativeConfig::default());
    }
    linguist_config::load_native(&path).map_err(|error| error.to_string())
}

fn nonempty_setting(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn set_deck_mapping(
    config: &mut linguist_config::NativeConfig,
    purpose: &str,
    deck_name: &str,
    model_name: &str,
) -> Result<(), String> {
    let canonical = linguist_application::canonical_language_key(purpose)
        .ok_or_else(|| format!("Unknown deck purpose: {purpose}"))?;
    let deck = nonempty_setting(deck_name);
    if let Some(name) = deck.as_deref() {
        if config
            .decks
            .iter()
            .any(|(key, mapped)| key != &canonical && mapped.deck_name.as_deref() == Some(name))
        {
            return Err(format!("Anki deck '{name}' already has another purpose"));
        }
    }
    config.version = linguist_config::NATIVE_CONFIG_VERSION;
    let mapping = config.decks.entry(canonical).or_default();
    mapping.deck_name = deck.clone();
    mapping.model_name = deck.and_then(|_| nonempty_setting(model_name));
    Ok(())
}

fn apply_settings(
    mut backend: Pin<&mut qobject::AppBackend>,
    config: linguist_config::NativeConfig,
    message: &str,
) {
    backend
        .as_mut()
        .set_settings_anki_url(config.anki_url.into());
    backend
        .as_mut()
        .set_settings_ollama_url(config.ollama_url.into());
    backend
        .as_mut()
        .set_settings_ollama_model(config.ollama_model.unwrap_or_default().into());
    backend
        .as_mut()
        .set_settings_dictionary_preset(config.dictionary_preset.into());
    backend.as_mut().set_settings_dry_run(config.dry_run);
    backend.as_mut().set_settings_message(message.into());
    backend.as_mut().rust_mut().generation_adapter = None;
}

fn configured_value(variable: &str, configured: &str, fallback: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| {
        if configured.trim().is_empty() {
            fallback.into()
        } else {
            configured.into()
        }
    })
}

const MAX_LIVE_QUEUE_NOTES: usize = 1_000;
struct LiveDesktopPort {
    runtime: tokio::runtime::Runtime,
    anki: linguist_anki::AnkiConnectTransport,
    ollama: linguist_ollama::OllamaClient,
    preferred_deck: Option<String>,
}
impl LiveDesktopPort {
    fn for_deck(mut self, deck: Option<String>) -> Self {
        self.preferred_deck = deck;
        self
    }
    fn from_environment() -> Result<Self, String> {
        let config = runtime_config()?;
        let anki_url = configured_value(
            "LINGUIST_ANKI_URL",
            &config.anki_url,
            "http://127.0.0.1:8765",
        );
        let ollama_url = configured_value(
            "LINGUIST_OLLAMA_URL",
            &config.ollama_url,
            "http://127.0.0.1:11434",
        );
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|error| error.to_string())?,
            anki: linguist_anki::AnkiConnectTransport::new(&anki_url)
                .map_err(|error| error.to_string())?,
            ollama: linguist_ollama::OllamaClient::new(&ollama_url)
                .map_err(|error| error.to_string())?,
            preferred_deck: None,
        })
    }
    fn decks(&self) -> Result<Vec<String>, String> {
        self.runtime
            .block_on(self.anki.deck_names())
            .map(|decks| decks.into_iter().map(|deck| deck.0).collect())
            .map_err(|error| error.to_string())
    }

    fn ingestion_candidates(
        &self,
        preview: &IngestionPreview,
    ) -> Result<Vec<linguist_application::NoteInfo>, String> {
        let mut ids = BTreeSet::new();
        for row in &preview.rows {
            let query = format!(
                "deck:\"{}\" \"{}\"",
                row.deck_key.replace('"', "\\\""),
                row.expression.replace('"', "\\\"")
            );
            ids.extend(
                self.runtime
                    .block_on(self.anki.find_notes(&query))
                    .map_err(|error| error.to_string())?,
            );
        }
        self.runtime
            .block_on(self.anki.notes_info(&ids.into_iter().collect::<Vec<_>>()))
            .map_err(|error| error.to_string())
    }

    fn preview_selector(
        &self,
        selector: &BatchSelector,
    ) -> Result<linguist_application::SelectorPreview, String> {
        self.runtime
            .block_on(selector_preview(&self.anki, selector))
            .map_err(|error| error.to_string())
    }

    fn selector_items(
        &self,
        selector: &BatchSelector,
    ) -> Result<Vec<linguist_jobs::BatchItemSeed>, String> {
        let ids = self
            .runtime
            .block_on(self.anki.find_notes(&selector.query()))
            .map_err(|error| error.to_string())?;
        let mut items = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let notes = self
                .runtime
                .block_on(self.anki.notes_info(chunk))
                .map_err(|error| error.to_string())?;
            items.extend(notes.into_iter().map(|note| {
                linguist_jobs::BatchItemSeed {
                    note_id: note.note_id,
                    word: ["Expression", "Word", "Front", "Vocabulary"]
                        .into_iter()
                        .find_map(|field| note.fields.get(field))
                        .cloned()
                        .or_else(|| note.fields.values().next().cloned())
                        .unwrap_or_default(),
                }
            }));
        }
        Ok(items)
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
        choose_deck(&self.decks()?, self.preferred_deck.as_deref())
            .ok_or_else(|| "No Anki decks are available".into())
    }
    fn review_queue(&self) -> Result<ReviewQueueData, String> {
        let decks = self.decks()?;
        let Some(deck) = choose_deck(&decks, self.preferred_deck.as_deref()) else {
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
fn choose_deck(decks: &[String], preferred: Option<&str>) -> Option<String> {
    preferred
        .and_then(|name| decks.iter().find(|deck| deck.as_str() == name))
        .or_else(|| decks.first())
        .cloned()
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
    let raw_expression = ["Expression", "Word", "Front", "Vocabulary"]
        .into_iter()
        .find_map(|name| note.fields.get(name))
        .cloned()
        .or_else(|| note.fields.values().next().cloned())
        .unwrap_or_else(|| format!("Note {}", note.note_id));
    let expression = linguist_core::normalize_expression(&raw_expression);
    let expression = if expression.is_empty() {
        format!("Note {}", note.note_id)
    } else {
        expression
    };
    ReviewRow {
        note_id: note.note_id,
        expression,
        detail: note.model_name.0,
        state: ReviewState::Ready,
    }
}

struct LiveGenerationAdapter {
    runtime: tokio::runtime::Runtime,
    pipeline: linguist_pipeline::NativePipeline<LiveEnrichmentServices>,
    anki: linguist_anki::AnkiConnectTransport,
    ollama: linguist_ollama::OllamaClient,
    model: String,
    ocr: linguist_ocr::Tesseract,
    decks: BTreeMap<String, linguist_config::DeckConfig>,
}

fn generation_deck_key<'a>(
    draft: &'a crate::draft::ReviewDraft,
    decks: &'a BTreeMap<String, linguist_config::DeckConfig>,
) -> Result<&'a str, String> {
    if let Some(key) = draft.language_key.as_deref() {
        return Ok(key);
    }
    let matches = decks
        .iter()
        .filter(|(key, deck)| {
            key.as_str() == draft.deck_name
                || deck.deck_name.as_deref() == Some(draft.deck_name.as_str())
        })
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [key] => Ok(key),
        [] => Err(format!(
            "No language mapping for Anki deck '{}'; configure it before generation",
            draft.deck_name
        )),
        _ => Err(format!(
            "Multiple language mappings for Anki deck '{}'; choose one before generation",
            draft.deck_name
        )),
    }
}

fn generation_input(
    draft: &crate::draft::ReviewDraft,
    deck_key: &str,
    expression: &str,
) -> CardBuildInput {
    CardBuildInput {
        mode: draft.mode,
        language_key: deck_key.into(),
        processed_data: ProcessedCardData {
            word: expression.into(),
            source_note: draft.source_context.clone(),
            type_tag: draft.type_tag.clone(),
            ..Default::default()
        },
        ..Default::default()
    }
}

struct LiveEnrichmentServices {
    jisho: linguist_dictionary::JishoClient,
    cambridge: linguist_dictionary::cambridge::CambridgeClient,
    moedict: linguist_dictionary::moedict::MoedictClient,
    dictcc: linguist_dictionary::dictcc::DictCcClient,
    custom_dictionary: Option<linguist_dictionary::custom::CustomDictionary>,
    custom_extraction: linguist_dictionary::custom::HttpCssExtraction,
    dictionary_preset: String,
    ollama: linguist_ollama::OllamaClient,
    model: String,
    tts: linguist_audio::EspeakTts,
    voices: Vec<linguist_audio::Voice>,
    dictionary_audio: linguist_audio::HttpAudioFetcher,
    remote_tts: linguist_audio::GoogleTts,
    kanji: linguist_dictionary::kanji::KanjiApiClient,
    jisho_kanji: linguist_dictionary::kanji::JishoKanjiClient,
    hvdic_kanji: linguist_dictionary::kanji::HvdicKanjiClient,
    kanji_media: linguist_dictionary::kanji::KanjiMediaFetcher,
    kanji_source_lang: String,
    image_search: linguist_media::WikipediaAndCommons,
    image_fetch: linguist_media::WikimediaCommons,
}

struct OllamaImageClassifier<'a> {
    client: &'a linguist_ollama::OllamaClient,
    model: &'a str,
}

fn dictionary_provider<'a>(preset: &'a str, deck_key: &'a str) -> &'a str {
    match preset.trim().to_ascii_lowercase().as_str() {
        "jisho" | "japanese" => "jisho",
        "cambridge" | "english" => "cambridge",
        "moedict" | "taiwanese" => "moedict",
        "dict_cc" | "dict.cc" | "german" => "dict_cc",
        "custom" => "custom",
        _ if deck_key.starts_with("english") => "cambridge",
        _ if deck_key.starts_with("taiwanese") => "moedict",
        _ if deck_key.starts_with("german") => "dict_cc",
        _ => "jisho",
    }
}

fn dictionary_pipeline_error(
    error: linguist_dictionary::DictionaryError,
) -> linguist_pipeline::PipelineError {
    linguist_pipeline::PipelineError::Provider {
        service: "dictionary",
        retryable: error.retry_class() == linguist_dictionary::RetryClass::Retryable,
        message: error.to_string(),
    }
}

fn deck_locale(deck_key: &str) -> &'static str {
    if deck_key.starts_with("japanese") {
        "ja-JP"
    } else if deck_key.starts_with("taiwanese") {
        "zh-TW"
    } else if deck_key.starts_with("german") {
        "de-DE"
    } else {
        "en-US"
    }
}

fn tts_voices(mut local: Vec<linguist_audio::Voice>) -> Vec<linguist_audio::Voice> {
    local.extend(
        [
            ("ja", "ja-JP"),
            ("en", "en-US"),
            ("zh-TW", "zh-TW"),
            ("de", "de-DE"),
        ]
        .into_iter()
        .map(|(id, locale)| linguist_audio::Voice {
            id: id.into(),
            locale: locale.into(),
            local: false,
        }),
    );
    local
}

impl linguist_media::ImageClassifierPort for OllamaImageClassifier<'_> {
    fn classify<'a>(
        &'a self,
        _: &'a str,
        _: &'a linguist_media::ImageCandidate,
        normalized_jpeg: &'a [u8],
    ) -> linguist_media::ClassifyFuture<'a> {
        Box::pin(async move {
            let result = self
                .client
                .classify_image(self.model, normalized_jpeg)
                .await
                .map_err(|error| error.to_string())?;
            if result.confidence < 0.95 {
                return Ok(linguist_media::ImageClass::Uncertain);
            }
            Ok(match result.classification {
                linguist_ollama::VisionClass::Dictionary => linguist_media::ImageClass::Dictionary,
                linguist_ollama::VisionClass::VisualRecall => {
                    linguist_media::ImageClass::VisualRecall
                }
            })
        })
    }
}

impl linguist_pipeline::EnrichmentServices for LiveEnrichmentServices {
    fn dictionary<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
    ) -> linguist_pipeline::PipelineFuture<'a> {
        Box::pin(async move {
            if deck_key.ends_with("grammar") {
                return Ok(linguist_pipeline::ProviderOutput::Dictionary(
                    linguist_core::DictionaryData {
                        found: true,
                        word: expression.into(),
                        ..Default::default()
                    },
                ));
            }
            let provider = dictionary_provider(&self.dictionary_preset, deck_key);
            let data = match provider {
                "custom" => {
                    let custom = self.custom_dictionary.as_ref().ok_or_else(|| {
                        linguist_pipeline::PipelineError::Provider {
                            service: "dictionary",
                            retryable: false,
                            message: "Custom dictionary needs URL template and CSS schema in native settings".into(),
                        }
                    })?;
                    let entry = custom
                        .search(expression, &self.custom_extraction)
                        .await
                        .map_err(dictionary_pipeline_error)?;
                    match entry {
                        Some(entry) => linguist_core::DictionaryData {
                            found: true,
                            word: entry.word.clone(),
                            reading: entry.reading.clone(),
                            definition: entry.definition,
                            pronunciations: entry
                                .audio_url
                                .map(|audio_url| linguist_core::DictionaryPronunciation {
                                    text: if entry.reading.is_empty() {
                                        entry.word
                                    } else {
                                        entry.reading
                                    },
                                    locale: deck_locale(deck_key).into(),
                                    audio_url: Some(audio_url),
                                    source: "Custom dictionary".into(),
                                })
                                .into_iter()
                                .collect(),
                        },
                        None => linguist_core::DictionaryData::default(),
                    }
                }
                "cambridge" => {
                    let entry = self
                        .cambridge
                        .search(expression)
                        .await
                        .map_err(dictionary_pipeline_error)?;
                    let pronunciation = linguist_core::DictionaryPronunciation {
                        text: entry.headword.clone(),
                        locale: "en-US".into(),
                        audio_url: entry.audio_url.clone(),
                        source: "Cambridge".into(),
                    };
                    linguist_core::DictionaryData {
                        found: true,
                        word: entry.headword,
                        reading: String::new(),
                        definition: entry.definitions.join("; "),
                        pronunciations: vec![pronunciation],
                    }
                }
                "moedict" => {
                    let entry = self
                        .moedict
                        .search(expression)
                        .await
                        .map_err(dictionary_pipeline_error)?;
                    let pronunciations = entry
                        .readings
                        .iter()
                        .enumerate()
                        .map(|(index, reading)| linguist_core::DictionaryPronunciation {
                            text: reading.clone(),
                            locale: "zh-TW".into(),
                            audio_url: entry
                                .audio_urls
                                .get(index)
                                .or_else(|| entry.audio_urls.first())
                                .cloned(),
                            source: "MoeDict".into(),
                        })
                        .collect();
                    linguist_core::DictionaryData {
                        found: true,
                        word: entry.title,
                        reading: entry.readings.join(" / "),
                        definition: entry.definitions.join("; "),
                        pronunciations,
                    }
                }
                "dict_cc" => {
                    let entries = self
                        .dictcc
                        .search(expression)
                        .await
                        .map_err(dictionary_pipeline_error)?;
                    linguist_core::DictionaryData {
                        found: !entries.is_empty(),
                        word: expression.into(),
                        reading: String::new(),
                        definition: entries
                            .into_iter()
                            .map(|entry| format!("{} — {}", entry.source, entry.target))
                            .collect::<Vec<_>>()
                            .join("; "),
                        pronunciations: vec![linguist_core::DictionaryPronunciation {
                            text: expression.into(),
                            locale: "de-DE".into(),
                            audio_url: None,
                            source: "dict.cc".into(),
                        }],
                    }
                }
                _ => {
                    let entries = self
                        .jisho
                        .search(expression, None)
                        .await
                        .map_err(dictionary_pipeline_error)?;
                    let Some(entry) = entries.first() else {
                        return Ok(linguist_pipeline::ProviderOutput::Dictionary(
                            Default::default(),
                        ));
                    };
                    linguist_core::DictionaryData {
                        found: true,
                        word: entry.word.clone(),
                        reading: entry.reading.clone(),
                        definition: entry
                            .senses
                            .iter()
                            .flat_map(|sense| sense.definitions.iter())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; "),
                        pronunciations: entry
                            .forms
                            .iter()
                            .filter(|form| !form.reading.trim().is_empty())
                            .map(|form| linguist_core::DictionaryPronunciation {
                                text: form.reading.clone(),
                                locale: "ja-JP".into(),
                                audio_url: None,
                                source: "Jisho".into(),
                            })
                            .collect(),
                    }
                }
            };
            Ok(linguist_pipeline::ProviderOutput::Dictionary(data))
        })
    }

    fn generation<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
        context: &'a str,
        dictionary: &'a linguist_core::DictionaryData,
    ) -> linguist_pipeline::PipelineFuture<'a> {
        Box::pin(async move {
            if deck_key.ends_with("grammar") {
                let prompt = format!(
                    "Explain grammar point `{expression}` for a language learner. Context: {context}"
                );
                let generated = self
                    .ollama
                    .generate_grammar(&self.model, &prompt)
                    .await
                    .map_err(|error| linguist_pipeline::PipelineError::Provider {
                        service: "generation",
                        message: error.to_string(),
                        retryable: error.retryable(),
                    })?;
                return Ok(linguist_pipeline::ProviderOutput::Generation(
                    linguist_core::LlmResponse {
                        grammar_point: Some(generated.grammar_point),
                        meaning: generated.meaning,
                        rules: generated.rules,
                        examples: generated
                            .examples
                            .into_iter()
                            .map(|example| linguist_core::ExamplePair {
                                sentence: example.sentence,
                                translation: example.translation,
                            })
                            .collect(),
                        ..Default::default()
                    },
                ));
            }
            let prompt = format!(
                "Explain `{expression}`. Reading: {}. Dictionary: {}. Return concise nuance and examples.",
                dictionary.reading, dictionary.definition
            );
            let generated = self
                .ollama
                .generate_vocabulary(&self.model, &prompt)
                .await
                .map_err(|error| linguist_pipeline::PipelineError::Provider {
                    service: "generation",
                    message: error.to_string(),
                    retryable: error.retryable(),
                })?;
            Ok(linguist_pipeline::ProviderOutput::Generation(
                linguist_core::LlmResponse {
                    nuances: generated.nuances,
                    examples: generated
                        .examples
                        .into_iter()
                        .map(|example| linguist_core::ExamplePair {
                            sentence: example.sentence,
                            translation: example.translation,
                        })
                        .collect(),
                    ..Default::default()
                },
            ))
        })
    }

    fn kanji<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
    ) -> linguist_pipeline::PipelineFuture<'a> {
        Box::pin(async move {
            if !deck_key.starts_with("japanese") {
                return Ok(linguist_pipeline::ProviderOutput::Unavailable);
            }
            let providers: Vec<&dyn linguist_dictionary::kanji::KanjiLookupPort> =
                if self.kanji_source_lang.eq_ignore_ascii_case("vietnamese") {
                    vec![&self.hvdic_kanji, &self.jisho_kanji, &self.kanji]
                } else {
                    vec![&self.jisho_kanji, &self.kanji, &self.hvdic_kanji]
                };
            let mut result = linguist_dictionary::kanji::lookup_word_with_sources(
                &providers,
                expression,
                "",
                Some("https://raw.githubusercontent.com/KanjiVG/kanjivg/master/kanji"),
            )
            .await;
            for summary in &mut result.summaries {
                match self.kanji_media.gif_data_uri(summary.character).await {
                    Ok(uri) => summary.stroke_order_url = Some(uri),
                    Err(error) => result.warnings.push(format!(
                        "Stroke-order GIF for {}: {error}; using remote image",
                        summary.character
                    )),
                }
            }
            if result.summaries.is_empty() && !result.warnings.is_empty() {
                return Err(linguist_pipeline::PipelineError::Provider {
                    service: "kanji",
                    message: result.warnings.join("; "),
                    retryable: true,
                });
            }
            let summary = linguist_dictionary::kanji::render_kanji_summaries(&result.summaries);
            Ok(linguist_pipeline::ProviderOutput::Kanji(summary))
        })
    }

    fn image<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
    ) -> linguist_pipeline::PipelineFuture<'a> {
        Box::pin(async move {
            if deck_key.ends_with("grammar") {
                return Ok(linguist_pipeline::ProviderOutput::Unavailable);
            }
            use base64::Engine;
            use std::sync::{Arc, atomic::AtomicBool};
            let result = linguist_media::discover_image(
                &self.image_search,
                &self.image_fetch,
                &OllamaImageClassifier {
                    client: &self.ollama,
                    model: &self.model,
                },
                expression,
                None,
                Arc::new(AtomicBool::new(false)),
                22,
            )
            .await;
            let Some(selected) = result.selected else {
                if result.issues.is_empty() {
                    return Ok(linguist_pipeline::ProviderOutput::Unavailable);
                }
                return Err(linguist_pipeline::PipelineError::Provider {
                    service: "image",
                    message: result.issues.join("; "),
                    retryable: true,
                });
            };
            Ok(linguist_pipeline::ProviderOutput::Image {
                filename: selected.filename,
                b64: base64::engine::general_purpose::STANDARD.encode(selected.jpeg),
                classification: match selected.classification {
                    linguist_media::ImageClass::Dictionary => "dictionary",
                    linguist_media::ImageClass::VisualRecall => "visual_recall",
                    linguist_media::ImageClass::Mixed => "mixed",
                    linguist_media::ImageClass::Uncertain => "uncertain",
                    linguist_media::ImageClass::NoImage => "No Image",
                }
                .into(),
            })
        })
    }

    fn audio<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
        dictionary: &'a linguist_core::DictionaryData,
    ) -> linguist_pipeline::PipelineFuture<'a> {
        Box::pin(async move {
            if deck_key.ends_with("grammar") {
                return Ok(linguist_pipeline::ProviderOutput::Unavailable);
            }
            use base64::Engine;
            let locale = deck_locale(deck_key);
            let pronunciations = if dictionary.pronunciations.is_empty() {
                vec![linguist_audio::Pronunciation {
                    text: if dictionary.reading.trim().is_empty() {
                        expression.into()
                    } else {
                        dictionary.reading.clone()
                    },
                    locale: locale.into(),
                    audio_url: None,
                    source: "TTS fallback".into(),
                }]
            } else {
                dictionary
                    .pronunciations
                    .iter()
                    .map(|row| linguist_audio::Pronunciation {
                        text: row.text.clone(),
                        locale: if row.locale.is_empty() {
                            locale.into()
                        } else {
                            row.locale.clone()
                        },
                        audio_url: row.audio_url.clone(),
                        source: row.source.clone(),
                    })
                    .collect()
            };
            let result = linguist_audio::discover_audio(
                &self.dictionary_audio,
                &self.tts,
                &self.remote_tts,
                pronunciations,
                &self.voices,
                Arc::new(AtomicBool::new(false)),
            )
            .await;
            if result.clips.is_empty() {
                return Err(linguist_pipeline::PipelineError::Provider {
                    service: "audio",
                    message: if result.issues.is_empty() {
                        "No audio provider returned a clip".into()
                    } else {
                        result.issues.join("; ")
                    },
                    retryable: true,
                });
            }
            Ok(linguist_pipeline::ProviderOutput::Audio(
                result
                    .clips
                    .into_iter()
                    .map(|clip| linguist_pipeline::ProviderAudio {
                        filename: clip.filename,
                        b64: base64::engine::general_purpose::STANDARD.encode(clip.clip.data),
                        reading: clip.pronunciation.text,
                    })
                    .collect(),
            ))
        })
    }
}

impl LiveGenerationAdapter {
    fn from_environment() -> Result<Self, String> {
        let config = runtime_config()?;
        let ollama_url = configured_value(
            "LINGUIST_OLLAMA_URL",
            &config.ollama_url,
            "http://127.0.0.1:11434",
        );
        let anki_url = configured_value(
            "LINGUIST_ANKI_URL",
            &config.anki_url,
            "http://127.0.0.1:8765",
        );
        let model = configured_value(
            "LINGUIST_OLLAMA_MODEL",
            config.ollama_model.as_deref().unwrap_or_default(),
            "llama3.2",
        );
        let ollama =
            linguist_ollama::OllamaClient::new(&ollama_url).map_err(|error| error.to_string())?;
        let custom_dictionary = if config.dictionary_preset.eq_ignore_ascii_case("custom") {
            let schema = config
                .dictionary_schema
                .clone()
                .ok_or("Custom dictionary needs CSS schema in native settings")?;
            Some(
                linguist_dictionary::custom::CustomDictionary::new(
                    &config.dictionary_url_template,
                    schema,
                )
                .map_err(|error| format!("Custom dictionary settings: {error}"))?,
            )
        } else {
            None
        };
        let tts = linguist_audio::EspeakTts::default();
        let voices = tts_voices(tts.available_voices());
        let pipeline = linguist_pipeline::NativePipeline::new(
            LiveEnrichmentServices {
                jisho: linguist_dictionary::JishoClient::new()
                    .map_err(|error| error.to_string())?,
                cambridge: linguist_dictionary::cambridge::CambridgeClient::new()
                    .map_err(|error| error.to_string())?,
                moedict: linguist_dictionary::moedict::MoedictClient::new()
                    .map_err(|error| error.to_string())?,
                dictcc: linguist_dictionary::dictcc::DictCcClient::new("https://deen.dict.cc/")
                    .map_err(|error| error.to_string())?,
                custom_dictionary,
                custom_extraction: linguist_dictionary::custom::HttpCssExtraction::new()
                    .map_err(|error| error.to_string())?,
                dictionary_preset: config.dictionary_preset.clone(),
                ollama: ollama.clone(),
                model: model.clone(),
                tts,
                voices,
                dictionary_audio: linguist_audio::HttpAudioFetcher::dictionary_defaults()
                    .map_err(|error| error.to_string())?,
                remote_tts: linguist_audio::GoogleTts::new().map_err(|error| error.to_string())?,
                kanji: linguist_dictionary::kanji::KanjiApiClient::new()?,
                jisho_kanji: linguist_dictionary::kanji::JishoKanjiClient::new()?,
                hvdic_kanji: linguist_dictionary::kanji::HvdicKanjiClient::new()?,
                kanji_media: linguist_dictionary::kanji::KanjiMediaFetcher::new()?,
                kanji_source_lang: config.kanji_source_lang.clone(),
                image_search: linguist_media::WikipediaAndCommons::new()?,
                image_fetch: linguist_media::WikimediaCommons::new()?,
            },
            linguist_pipeline::PipelineConfig::default(),
        );
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|error| error.to_string())?,
            pipeline,
            anki: linguist_anki::AnkiConnectTransport::new(&anki_url)
                .map_err(|error| error.to_string())?,
            ollama,
            model,
            ocr: linguist_ocr::Tesseract::new("tesseract"),
            decks: config.decks,
        })
    }

    fn document(&self, draft: &crate::draft::ReviewDraft) -> Result<CardDocument, String> {
        let deck_key = generation_deck_key(draft, &self.decks)?;
        let expression = linguist_core::normalize_expression(&draft.expression);
        if expression.is_empty() {
            return Err("Expression contains no searchable text".into());
        }
        self.runtime.block_on(async {
            let mut document = self
                .pipeline
                .enrich_with_input(generation_input(draft, deck_key, &expression))
                .await
                .map_err(|error| error.to_string())?;
            if draft.mode == CardMode::Inject || draft.images.is_empty() {
                return Ok(document);
            }
            self.apply_existing_image_policy(draft, &mut document).await;
            Ok(document)
        })
    }

    async fn apply_existing_image_policy(
        &self,
        draft: &crate::draft::ReviewDraft,
        document: &mut CardDocument,
    ) {
        use base64::Engine;
        let mut retained = Vec::new();
        let mut dictionary = Vec::new();
        for filename in &draft.images {
            let media = match self.anki.retrieve_media_file(filename).await {
                Ok(Some(media)) => media,
                Ok(None) => {
                    document
                        .issues
                        .push(format!("OCR media missing: {filename}"));
                    retained.push(filename.clone());
                    continue;
                }
                Err(error) => {
                    document
                        .issues
                        .push(format!("OCR media {filename}: {error}"));
                    retained.push(filename.clone());
                    continue;
                }
            };
            let bytes = match base64::engine::general_purpose::STANDARD.decode(media.data_base64) {
                Ok(bytes) => bytes,
                Err(error) => {
                    document
                        .issues
                        .push(format!("OCR media {filename}: invalid base64: {error}"));
                    retained.push(filename.clone());
                    continue;
                }
            };
            let evidence =
                self.ocr
                    .recognize_bytes(&bytes, "jpn+eng", Arc::new(AtomicBool::new(false)));
            let score = match evidence {
                Ok(evidence) => linguist_ocr::dictionary_evidence_score(&evidence.text),
                Err(error) => {
                    document.issues.push(format!("OCR {filename}: {error}"));
                    0
                }
            };
            if score >= 3 {
                dictionary.push(filename.clone());
                continue;
            }
            match self.ollama.classify_image(&self.model, &bytes).await {
                Ok(result)
                    if result.confidence >= 0.95
                        && result.classification == linguist_ollama::VisionClass::Dictionary =>
                {
                    dictionary.push(filename.clone());
                }
                Ok(_) => retained.push(filename.clone()),
                Err(error) => {
                    document
                        .issues
                        .push(format!("Image classification {filename}: {error}"));
                    retained.push(filename.clone());
                }
            }
        }

        apply_existing_image_result(document, &draft.images, &mut retained, dictionary);
        if !retained.is_empty() {
            document.values.meaning_image = Some(
                retained
                    .iter()
                    .map(|name| format!("<img src=\"{name}\">"))
                    .collect::<Vec<_>>()
                    .join("<br/>"),
            );
            document
                .provenance
                .entry("meaning_image".into())
                .or_default()
                .push(Provenance {
                    source: SourceKind::Ocr,
                    label: "Existing image retained after OCR/vision review".into(),
                    confidence_percent: None,
                });
        }
        document.obsolete_media.sort();
        document.obsolete_media.dedup();
    }
}

fn is_image_filename(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp", ".gif"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn apply_existing_image_result(
    document: &mut CardDocument,
    originals: &[String],
    retained: &mut Vec<String>,
    dictionary: Vec<String>,
) {
    let replacement_available = document
        .values
        .meaning_image
        .as_deref()
        .is_some_and(|html| !html.trim().is_empty());
    if retained.is_empty() {
        if replacement_available {
            document.obsolete_media.extend(originals.iter().cloned());
        } else {
            retained.extend(originals.iter().cloned());
        }
    } else {
        document
            .media
            .retain(|media| !is_image_filename(&media.filename));
        document.obsolete_media.extend(dictionary);
    }
}

impl DraftGenerationPort for LiveGenerationAdapter {
    fn generate(&self, draft: &crate::draft::ReviewDraft) -> Result<GeneratedDraft, String> {
        let document = self.document(draft)?;
        let value = document.values.meaning_text.clone().unwrap_or_default();
        if value.trim().is_empty() {
            return Err("Ollama returned no usable meaning".into());
        }
        let mut changes = vec![crate::draft::GeneratedChange {
            field: crate::draft::DraftField::Meaning,
            value,
            provenance: "Native enrichment pipeline · Jisho + Ollama".into(),
        }];
        if let Some(kanji) = document
            .values
            .kanji_construction
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            changes.push(crate::draft::GeneratedChange {
                field: crate::draft::DraftField::Kanji,
                value: kanji.clone(),
                provenance: "Kanji lookup".into(),
            });
        }
        Ok(GeneratedDraft { document, changes })
    }
}

struct LiveBatchWorker {
    generation: LiveGenerationAdapter,
    commit: LiveCommitAdapter,
}

impl LiveBatchWorker {
    fn from_environment() -> Result<Self, String> {
        Ok(Self {
            generation: LiveGenerationAdapter::from_environment()?,
            commit: LiveCommitAdapter::from_environment()?,
        })
    }

    fn draft(&self, note_id: i64) -> Result<crate::draft::ReviewDraft, String> {
        let note = self
            .commit
            .runtime
            .block_on(self.commit.commit.note_info(note_id))
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Anki note {note_id} was not found"))?;
        Ok(crate::draft::ReviewDraft::from_note(&note))
    }
}

impl linguist_jobs::BatchWorkerPort for LiveBatchWorker {
    fn process(
        &mut self,
        _: &linguist_core::BatchJobContract,
        item: &linguist_core::BatchItemContract,
    ) -> Result<serde_json::Value, String> {
        let draft = self.draft(item.note_id)?;
        let document = self.generation.document(&draft)?;
        serde_json::to_value(document).map_err(|error| error.to_string())
    }

    fn commit(
        &mut self,
        job: &linguist_core::BatchJobContract,
        item: &linguist_core::BatchItemContract,
        artifact: &serde_json::Value,
    ) -> Result<linguist_jobs::BatchCommitResult, String> {
        let draft = self.draft(item.note_id)?;
        let mut request = self.commit.request(&draft, job.dry_run)?;
        request.document = serde_json::from_value(artifact.clone())
            .map_err(|error| format!("Invalid processed artifact: {error}"))?;
        match self
            .commit
            .runtime
            .block_on(commit_card(&self.commit.commit, request))
            .map_err(|error| error.to_string())?
        {
            linguist_application::CommitOutcome::Committed(receipt) => {
                Ok(linguist_jobs::BatchCommitResult {
                    snapshot_id: receipt.snapshot_id,
                    result_note_id: receipt.note_id,
                })
            }
            linguist_application::CommitOutcome::DryRun { .. } => {
                Ok(linguist_jobs::BatchCommitResult {
                    snapshot_id: "dry-run".into(),
                    result_note_id: item.note_id,
                })
            }
        }
    }
}

struct LiveCommitAdapter {
    runtime: tokio::runtime::Runtime,
    commit: linguist_anki::AnkiCommitPort,
    snapshots: SnapshotRepository,
}

impl LiveCommitAdapter {
    fn from_environment() -> Result<Self, String> {
        let config = runtime_config()?;
        let url = configured_value(
            "LINGUIST_ANKI_URL",
            &config.anki_url,
            "http://127.0.0.1:8765",
        );
        let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
        let transport =
            linguist_anki::AnkiConnectTransport::new(&url).map_err(|error| error.to_string())?;
        let snapshots =
            SnapshotRepository::default_location().map_err(|error| error.to_string())?;
        let commit = linguist_anki::AnkiCommitPort::new(transport, snapshots.clone());
        Ok(Self {
            runtime,
            commit,
            snapshots,
        })
    }

    fn request(
        &self,
        draft: &crate::draft::ReviewDraft,
        dry_run: bool,
    ) -> Result<CommitRequest, String> {
        let expression = draft.expression.trim().to_owned();
        if expression.is_empty() {
            return Err("Expression cannot be empty".into());
        }
        let values = LogicalFields {
            meaning_image: draft
                .images
                .first()
                .map(|name| format!("<img src=\"{name}\">")),
            meaning_text: Some(draft.meaning.clone()),
            kanji_construction: Some(draft.kanji.clone()),
            audio: Some(draft.audio.join("<br/>")),
        };
        let managed_spec = linguist_core::japanese_vocab_spec();
        let managed_mapping = FieldMapping {
            expression: Some("Expression".into()),
            meaning_image: Some("Picture".into()),
            meaning_text: Some("Meaning".into()),
            kanji_construction: Some("Kanji".into()),
            audio: Some("Audio".into()),
        };
        let config = runtime_config()?;
        let deck_config = config.decks.iter().find_map(|(key, configured)| {
            (key == &draft.deck_name
                || configured.deck_name.as_deref() == Some(draft.deck_name.as_str()))
            .then_some(configured)
        });
        let (deck_name, target_model, source, mapping, tags) = match draft.mode {
            CardMode::Modernize => {
                let note = self.runtime.block_on(self.commit_note(draft))?;
                let deck_name = note
                    .deck_names
                    .first()
                    .map(|deck| deck.0.clone())
                    .unwrap_or_else(|| draft.deck_name.clone());
                let target_model = deck_config
                    .and_then(|configured| configured.model_name.clone())
                    .unwrap_or_else(|| note.model_name.0.clone());
                let mapping = deck_config
                    .map(|configured| configured_mapping(&configured.fields))
                    .filter(mapping_has_expression)
                    .unwrap_or_else(|| {
                        if target_model == managed_spec.model_name {
                            managed_mapping.clone()
                        } else {
                            mapping_from_fields(&note.fields)
                        }
                    });
                let tags = note.tags.clone();
                let source = Some(CommitSource {
                    note_id: note.note_id,
                    model_name: note.model_name.0,
                    fields: note.fields,
                    tags: note.tags,
                });
                (deck_name, target_model, source, mapping, tags)
            }
            CardMode::Inject => (
                draft.deck_name.clone(),
                deck_config
                    .and_then(|configured| configured.model_name.clone())
                    .filter(|model| !model.trim().is_empty())
                    .unwrap_or_else(|| draft.target_model.clone()),
                None,
                deck_config
                    .map(|configured| configured_mapping(&configured.fields))
                    .filter(mapping_has_expression)
                    .unwrap_or(managed_mapping),
                Vec::new(),
            ),
        };
        if deck_name.trim().is_empty() || target_model.trim().is_empty() {
            return Err("Deck and target model are required".into());
        }
        let mut document = draft.accepted_document().unwrap_or_else(|| CardDocument {
            schema_version: CONTRACT_VERSION,
            expression,
            values,
            media: Vec::new(),
            obsolete_media: Vec::new(),
            issues: draft.issues.clone(),
            tags: Vec::new(),
            provenance: BTreeMap::new(),
        });
        document.tags = tags;
        let template_plan = if target_model == managed_spec.model_name {
            self.runtime
                .block_on(self.commit.japanese_template_plan())
                .map_err(|error| error.to_string())?
        } else {
            linguist_core::ManagedTemplatePlan::NoChange
        };
        Ok(CommitRequest {
            mode: draft.mode,
            dry_run,
            deck_key: deck_name.clone(),
            deck_name,
            target_model,
            source,
            document,
            field_mapping: mapping,
            template_plan,
        })
    }

    async fn commit_note(
        &self,
        draft: &crate::draft::ReviewDraft,
    ) -> Result<linguist_application::NoteInfo, String> {
        self.commit
            .note_info(draft.note_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Anki note {} was not found", draft.note_id))
    }

    fn restore_snapshot_id(&mut self, snapshot_id: &str) -> Result<(), String> {
        let document = self
            .snapshots
            .load()
            .map_err(|error| error.to_string())?
            .snapshots
            .into_iter()
            .find(|document| document.snapshot.id == snapshot_id)
            .ok_or_else(|| format!("Snapshot {snapshot_id} was not found"))?;
        self.runtime
            .block_on(restore_snapshot(&self.commit, &document))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn configured_mapping(fields: &BTreeMap<String, String>) -> FieldMapping {
    let field = |name: &str| {
        fields
            .get(name)
            .filter(|value| !value.trim().is_empty())
            .cloned()
    };
    FieldMapping {
        expression: field("expression"),
        meaning_image: field("meaning_image"),
        meaning_text: field("meaning_text"),
        kanji_construction: field("kanji_construction"),
        audio: field("audio"),
    }
}

fn mapping_has_expression(mapping: &FieldMapping) -> bool {
    mapping.expression.is_some()
}

fn mapping_from_fields(fields: &BTreeMap<String, String>) -> FieldMapping {
    let find = |aliases: &[&str]| {
        aliases
            .iter()
            .find(|alias| fields.contains_key(**alias))
            .map(|alias| (*alias).to_owned())
    };
    FieldMapping {
        expression: find(&["Expression", "Word", "Front", "Vocabulary"]),
        meaning_image: find(&["Meaning Image", "Image", "Picture"]),
        meaning_text: find(&["Meaning", "Definition", "Back"]),
        kanji_construction: find(&["Kanji", "Kanji Construction"]),
        audio: find(&["Audio", "Pronunciation"]),
    }
}

impl crate::commit_model::CommitExecutor<crate::draft::ReviewDraft> for LiveCommitAdapter {
    fn preview(
        &mut self,
        draft: &crate::draft::ReviewDraft,
    ) -> Result<crate::commit_model::CommitPreview, String> {
        let request = self.request(draft, true)?;
        let before = request
            .source
            .as_ref()
            .map(|source| source.fields.clone())
            .unwrap_or_default();
        let model_changed = request
            .source
            .as_ref()
            .is_none_or(|source| source.model_name != request.target_model);
        let media = request
            .document
            .media
            .iter()
            .map(|asset| format!("Add {}", asset.filename))
            .chain(
                request
                    .document
                    .obsolete_media
                    .iter()
                    .map(|filename| format!("Remove {filename}")),
            )
            .collect();
        let outcome = self
            .runtime
            .block_on(commit_card(&self.commit, request))
            .map_err(|error| error.to_string())?;
        let after = match outcome {
            linguist_application::CommitOutcome::DryRun { fields } => fields,
            linguist_application::CommitOutcome::Committed(_) => {
                return Err("Preview unexpectedly performed a write".into());
            }
        };
        Ok(crate::commit_model::CommitPreview {
            before,
            after,
            media,
            model_changed,
        })
    }

    fn apply(
        &mut self,
        draft: &crate::draft::ReviewDraft,
    ) -> Result<crate::commit_model::SnapshotHistoryItem, String> {
        let request = self.request(draft, false)?;
        let outcome = self
            .runtime
            .block_on(commit_card(&self.commit, request))
            .map_err(|error| error.to_string())?;
        match outcome {
            linguist_application::CommitOutcome::Committed(receipt) => {
                Ok(crate::commit_model::SnapshotHistoryItem {
                    snapshot_id: receipt.snapshot_id,
                    note_id: receipt.note_id,
                })
            }
            linguist_application::CommitOutcome::DryRun { .. } => {
                Err("Apply unexpectedly remained dry-run".into())
            }
        }
    }

    fn restore(&mut self, snapshot_id: &str) -> Result<(), String> {
        self.restore_snapshot_id(snapshot_id)
    }
}

#[cfg(test)]
mod backend_tests {
    use super::*;

    #[test]
    fn deck_purpose_mapping_validates_and_preserves_field_configuration() {
        let mut config = linguist_config::NativeConfig::default();
        config.decks.insert(
            "japanese_vocab".into(),
            linguist_config::DeckConfig {
                fields: BTreeMap::from([("expression".into(), "Word".into())]),
                ..Default::default()
            },
        );
        set_deck_mapping(&mut config, "Japanese", "森の言葉", "2. Picture Words").unwrap();
        let mapped = &config.decks["japanese_vocab"];
        assert_eq!(mapped.deck_name.as_deref(), Some("森の言葉"));
        assert_eq!(mapped.model_name.as_deref(), Some("2. Picture Words"));
        assert_eq!(mapped.fields["expression"], "Word");
        assert!(set_deck_mapping(&mut config, "english_vocab", "森の言葉", "").is_err());
        assert!(set_deck_mapping(&mut config, "unknown", "Other", "").is_err());
        set_deck_mapping(&mut config, "japanese_vocab", "", "ignored").unwrap();
        assert!(config.decks["japanese_vocab"].deck_name.is_none());
        assert!(config.decks["japanese_vocab"].model_name.is_none());
    }

    #[test]
    fn missing_local_synthesizer_keeps_only_remote_voice_choices() {
        let voices = tts_voices(Vec::new());
        assert!(voices.iter().all(|voice| !voice.local));
        assert_eq!(
            linguist_audio::select_remote_voice(&voices, "zh-TW")
                .unwrap()
                .id,
            "zh-TW"
        );
        assert!(linguist_audio::select_voice(&voices, "zh-TW").is_none());
    }

    #[test]
    fn review_queue_shows_plain_expression_from_html_note() {
        let row = review_row(linguist_application::NoteInfo {
            note_id: 42,
            model_name: linguist_application::ModelName("Basic".into()),
            deck_names: Vec::new(),
            fields: BTreeMap::from([(
                "Word".into(),
                "<span style=\"color: red;\">猫&nbsp;好き</span>".into(),
            )]),
            tags: Vec::new(),
        });
        assert_eq!(row.expression, "猫 好き");
        assert_eq!(row.note_id, 42);
    }

    #[test]
    fn generation_uses_language_key_not_anki_deck_name() {
        let decks = BTreeMap::from([(
            "japanese_vocab".into(),
            linguist_config::DeckConfig {
                deck_name: Some("Japanese Words".into()),
                ..Default::default()
            },
        )]);
        let draft =
            crate::draft::ReviewDraft::injection(-1, "猫", "", "Japanese Words", "Vocabulary");
        assert_eq!(
            generation_deck_key(&draft, &decks).unwrap(),
            "japanese_vocab"
        );

        let mut explicit = draft.clone();
        explicit.language_key = Some("german_grammar".into());
        assert_eq!(
            generation_deck_key(&explicit, &decks).unwrap(),
            "german_grammar"
        );

        let unknown = crate::draft::ReviewDraft::injection(-2, "cat", "", "Unknown", "Model");
        assert!(generation_deck_key(&unknown, &decks).is_err());
    }

    #[test]
    fn imported_type_and_context_reach_card_generation() {
        let mut draft = crate::draft::ReviewDraft::injection(
            -1,
            "食べる",
            "parent-child example",
            "Japanese Words",
            "Vocabulary",
        );
        draft.type_tag = "causative form".into();
        let input = generation_input(&draft, "japanese_vocab", "食べる");
        assert_eq!(input.language_key, "japanese_vocab");
        assert_eq!(input.processed_data.source_note, "parent-child example");
        assert_eq!(input.processed_data.type_tag, "causative form");
        assert!(draft.meaning.is_empty());
    }

    #[test]
    fn csv_language_override_routes_duplicate_check_to_mapped_deck() {
        let config = linguist_config::NativeConfig {
            decks: BTreeMap::from([(
                "german_vocab".into(),
                linguist_config::DeckConfig {
                    deck_name: Some("Deutsch".into()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let preview =
            linguist_application::prepare_csv_input(&linguist_application::CsvIngestRequest {
                content: "Word,Language\ncat,japanese\nHaus,de\nchien,french\n".into(),
                deck_key: "Japanese Words".into(),
                language_key: "japanese_vocab".into(),
                type_tag: "vocab".into(),
                mapping: None,
            })
            .unwrap();
        let routed = route_ingestion_preview(
            IngestionPreview {
                rows: preview.rows,
                issues: preview.issues,
                duplicates: preview.duplicates,
            },
            &config,
            "japanese_vocab",
        );
        assert_eq!(routed.rows.len(), 2);
        assert_eq!(routed.rows[0].deck_key, "Japanese Words");
        assert_eq!(routed.rows[0].language_key, "japanese_vocab");
        assert_eq!(routed.rows[1].deck_key, "Deutsch");
        assert_eq!(routed.rows[1].language_key, "german_vocab");
        assert_eq!(routed.issues.len(), 1);
    }

    #[test]
    fn local_csv_urls_decode_without_accepting_remote_schemes() {
        assert_eq!(
            local_file_path("file:///tmp/words%20one.csv").unwrap(),
            PathBuf::from("/tmp/words one.csv")
        );
        assert!(local_file_path("https://example.test/words.csv").is_err());
        assert!(local_file_path("file://relative.csv").is_err());
    }

    #[test]
    fn native_field_mapping_preserves_shared_legacy_targets() {
        let mapping = configured_mapping(&BTreeMap::from([
            ("expression".into(), "Word".into()),
            ("meaning_text".into(), "Back".into()),
            ("kanji_construction".into(), "Back".into()),
        ]));
        assert_eq!(mapping.expression.as_deref(), Some("Word"));
        assert_eq!(mapping.meaning_text.as_deref(), Some("Back"));
        assert_eq!(mapping.kanji_construction.as_deref(), Some("Back"));
    }

    #[test]
    fn visual_existing_image_wins_over_discovered_replacement() {
        let mut document = CardDocument {
            schema_version: CONTRACT_VERSION,
            expression: "猫".into(),
            values: LogicalFields {
                meaning_image: Some("<img src=\"commons.jpg\">".into()),
                ..Default::default()
            },
            media: vec![
                linguist_core::MediaAsset {
                    filename: "commons.jpg".into(),
                    data_base64: "image".into(),
                },
                linguist_core::MediaAsset {
                    filename: "voice.wav".into(),
                    data_base64: "audio".into(),
                },
            ],
            obsolete_media: Vec::new(),
            issues: Vec::new(),
            tags: Vec::new(),
            provenance: BTreeMap::new(),
        };
        let originals = vec!["photo.png".into(), "dictionary.png".into()];
        let mut retained = vec!["photo.png".into()];
        apply_existing_image_result(
            &mut document,
            &originals,
            &mut retained,
            vec!["dictionary.png".into()],
        );
        assert_eq!(retained, ["photo.png"]);
        assert_eq!(document.obsolete_media, ["dictionary.png"]);
        assert_eq!(document.media[0].filename, "voice.wav");
    }

    #[test]
    fn dictionary_image_is_only_removed_when_replacement_exists() {
        let original = vec!["dictionary.png".into()];
        let mut without_replacement = CardDocument {
            schema_version: CONTRACT_VERSION,
            expression: "語".into(),
            values: LogicalFields::default(),
            media: Vec::new(),
            obsolete_media: Vec::new(),
            issues: Vec::new(),
            tags: Vec::new(),
            provenance: BTreeMap::new(),
        };
        let mut retained = Vec::new();
        apply_existing_image_result(
            &mut without_replacement,
            &original,
            &mut retained,
            original.clone(),
        );
        assert_eq!(retained, original);
        assert!(without_replacement.obsolete_media.is_empty());
    }

    #[test]
    fn dictionary_provider_uses_preset_then_deck_family() {
        assert_eq!(dictionary_provider("custom", "japanese_vocab"), "custom");
        assert_eq!(
            dictionary_provider("cambridge", "japanese_vocab"),
            "cambridge"
        );
        assert_eq!(dictionary_provider("", "english_vocab"), "cambridge");
        assert_eq!(dictionary_provider("", "taiwanese_vocab"), "moedict");
        assert_eq!(dictionary_provider("", "german_vocab"), "dict_cc");
        assert_eq!(dictionary_provider("", "japanese_vocab"), "jisho");
    }

    #[test]
    fn selected_deck_survives_refresh_and_missing_deck_falls_back() {
        let decks = vec!["Default".into(), "Japanese".into()];
        assert_eq!(
            choose_deck(&decks, Some("Japanese")).as_deref(),
            Some("Japanese")
        );
        assert_eq!(
            choose_deck(&decks, Some("Deleted")).as_deref(),
            Some("Default")
        );
        assert_eq!(choose_deck(&[], Some("Deleted")), None);
    }
}
