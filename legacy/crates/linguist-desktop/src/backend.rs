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
        #[qproperty(QString, theme_selection)]
        #[qproperty(QString, theme_red)]
        #[qproperty(QString, theme_yellow)]
        #[qproperty(QString, theme_green)]
        #[qproperty(QString, theme_cyan)]
        #[qproperty(QString, theme_blue)]
        #[qproperty(QString, theme_magenta)]
        #[qproperty(QString, anki_status)]
        #[qproperty(QString, ollama_status)]
        #[qproperty(QString, active_deck)]
        #[qproperty(QString, selection)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, queue_state)]
        #[qproperty(QString, queue_message)]
        #[qproperty(i32, deck_count)]
        #[qproperty(i32, model_count)]
        #[qproperty(i32, selected_deck_index)]
        #[qproperty(i32, review_row_count)]
        #[qproperty(i32, selected_review_index)]
        #[qproperty(i32, review_total)]
        #[qproperty(i32, review_page)]
        #[qproperty(i32, review_page_count)]
        #[qproperty(i32, review_page_start)]
        #[qproperty(i32, review_page_end)]
        #[qproperty(i32, review_search_match)]
        #[qproperty(i32, review_search_count)]
        #[qproperty(i32, review_navigation_serial)]
        #[qproperty(i32, review_content_serial)]
        #[qproperty(i32, mapping_source_count)]
        #[qproperty(i32, mapping_target_count)]
        #[qproperty(QString, mapping_purpose)]
        #[qproperty(QString, mapping_deck)]
        #[qproperty(QString, mapping_model)]
        #[qproperty(QString, mapping_tags)]
        #[qproperty(bool, mapping_dirty)]
        #[qproperty(bool, mapping_busy)]
        #[qproperty(QString, mapping_message)]
        #[qproperty(i32, mapping_render_serial)]
        #[qproperty(QString, draft_expression)]
        #[qproperty(QString, draft_meaning)]
        #[qproperty(QString, draft_examples)]
        #[qproperty(bool, draft_generated)]
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
        #[qproperty(i32, csv_column_count)]
        #[qproperty(i32, csv_expression_column)]
        #[qproperty(i32, csv_language_column)]
        #[qproperty(i32, csv_type_column)]
        #[qproperty(i32, csv_context_column)]
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
        #[cxx_name = "pollReviewLoad"]
        fn poll_review_load(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "selectItem"]
        fn select_item(self: Pin<&mut Self>, selection: &QString);
        #[qinvokable]
        #[cxx_name = "searchReview"]
        fn search_review(self: Pin<&mut Self>, query: &QString, direction: i32);
        #[qinvokable]
        #[cxx_name = "previousReviewPage"]
        fn previous_review_page(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "nextReviewPage"]
        fn next_review_page(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "reportError"]
        fn report_error(self: Pin<&mut Self>, message: &QString);
        #[qinvokable]
        #[cxx_name = "deckName"]
        fn deck_name(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "modelName"]
        fn model_name(self: &AppBackend, index: i32) -> QString;
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
        #[cxx_name = "mappingSourceName"]
        fn mapping_source_name(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingSourceHtml"]
        fn mapping_source_html(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingSourceImage"]
        fn mapping_source_image(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingTargetKey"]
        fn mapping_target_key(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingTargetLabel"]
        fn mapping_target_label(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingTargetHtml"]
        fn mapping_target_html(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingTargetImage"]
        fn mapping_target_image(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappingTargetHint"]
        fn mapping_target_hint(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "mappedSourceIndex"]
        fn mapped_source_index(self: &AppBackend, target_index: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "connectMappingField"]
        fn connect_mapping_field(self: Pin<&mut Self>, source_index: i32, target_index: i32);
        #[qinvokable]
        #[cxx_name = "clearMappingField"]
        fn clear_mapping_field(self: Pin<&mut Self>, target_index: i32);
        #[qinvokable]
        #[cxx_name = "setMappingPurpose"]
        fn choose_mapping_purpose(self: Pin<&mut Self>, purpose: &QString);
        #[qinvokable]
        #[cxx_name = "saveFieldMapping"]
        fn save_field_mapping(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "suggestFieldMapping"]
        fn suggest_field_mapping(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "inspectDeckMapping"]
        fn inspect_deck_mapping(self: Pin<&mut Self>, purpose: &QString, deck_name: &QString);
        #[qinvokable]
        #[cxx_name = "editDraftExpression"]
        fn edit_draft_expression(self: Pin<&mut Self>, value: &QString);
        #[qinvokable]
        #[cxx_name = "editDraftMeaning"]
        fn edit_draft_meaning(self: Pin<&mut Self>, value: &QString);

        #[qinvokable]
        #[cxx_name = "editDraftExamples"]
        fn edit_draft_examples(self: Pin<&mut Self>, value: &QString);
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
        #[cxx_name = "csvColumnName"]
        fn csv_column_name(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "remapCsvInput"]
        fn remap_csv_input(
            self: Pin<&mut Self>,
            expression: i32,
            language: i32,
            type_tag: i32,
            context: i32,
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
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use linguist_application::{
    BatchSelector, CommitRequest, CommitSource, CsvColumnMapping, CsvIngestRequest,
    DuplicateDecision, IngestionPreview, ManualIngestRequest, SelectorMetadataPort, commit_card,
    duplicate_decision_options, prepare_csv_input, prepare_manual_input, resolve_ingestion_preview,
    restore_snapshot, selector_preview,
};
use linguist_core::{
    CONTRACT_VERSION, CardBuildInput, CardDocument, CardMode, FieldMapping, LogicalFields,
    ProcessedCardData, Provenance, SourceKind,
};
use linguist_snapshots::SnapshotRepository;

use crate::controller::{ApplicationController, DesktopPort, DraftGenerationPort, GeneratedDraft};
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
    theme_selection: QString,
    theme_red: QString,
    theme_yellow: QString,
    theme_green: QString,
    theme_cyan: QString,
    theme_blue: QString,
    theme_magenta: QString,
    anki_status: QString,
    ollama_status: QString,
    active_deck: QString,
    selection: QString,
    error_message: QString,
    busy: bool,
    queue_state: QString,
    queue_message: QString,
    deck_count: i32,
    model_count: i32,
    selected_deck_index: i32,
    review_row_count: i32,
    selected_review_index: i32,
    review_total: i32,
    review_page: i32,
    review_page_count: i32,
    review_page_start: i32,
    review_page_end: i32,
    review_search_match: i32,
    review_search_count: i32,
    review_navigation_serial: i32,
    review_content_serial: i32,
    mapping_source_count: i32,
    mapping_target_count: i32,
    mapping_purpose: QString,
    mapping_deck: QString,
    mapping_model: QString,
    mapping_tags: QString,
    mapping_dirty: bool,
    mapping_busy: bool,
    mapping_message: QString,
    mapping_render_serial: i32,
    draft_expression: QString,
    draft_meaning: QString,
    draft_examples: QString,
    draft_generated: bool,
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
    csv_column_count: i32,
    csv_expression_column: i32,
    csv_language_column: i32,
    csv_type_column: i32,
    csv_context_column: i32,
    csv_header_names: Vec<String>,
    csv_source: Option<CsvImportSource>,
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
    review_browser: ReviewBrowser,
    model_names: Vec<String>,
    review_cache: Arc<Mutex<ReviewCache>>,
    review_load_result: Arc<Mutex<Option<ReviewLoadOutput>>>,
    review_generation: Arc<AtomicU64>,
    field_mapping: FieldMappingEditor,
    field_media_cache: Arc<Mutex<HashMap<String, String>>>,
    field_media_result: Arc<Mutex<Option<FieldMediaOutput>>>,
    field_media_generation: Arc<AtomicU64>,
    mapping_suggestion_result: Arc<Mutex<Option<MappingSuggestionOutput>>>,
    mapping_suggestion_generation: Arc<AtomicU64>,
    mapping_inspection_result: Arc<Mutex<Option<MappingInspectionOutput>>>,
    mapping_inspection_generation: Arc<AtomicU64>,
    batch_port: LocalBatchPort,
    batch_worker_active: Arc<AtomicBool>,
    batch_worker_error: Arc<Mutex<Option<String>>>,
    generation_adapter: Option<LiveGenerationAdapter>,
    controller: ApplicationController,
}

#[derive(Clone)]
struct CsvImportSource {
    content: String,
    deck_key: String,
    language_key: String,
    type_tag: String,
}

const REVIEW_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReviewIndexKey {
    deck: String,
    query: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReviewPageKey {
    index: ReviewIndexKey,
    offset: usize,
}

#[derive(Clone)]
struct CachedReviewPage {
    notes: Vec<linguist_application::NoteInfo>,
}

#[derive(Default)]
struct ReviewCache {
    indices: HashMap<ReviewIndexKey, Arc<Vec<i64>>>,
    pages: HashMap<ReviewPageKey, CachedReviewPage>,
}

#[derive(Default)]
struct ReviewBrowser {
    deck: String,
    query: String,
    ids: Arc<Vec<i64>>,
    offset: usize,
    cursor: usize,
    notes: Vec<linguist_application::NoteInfo>,
}

struct ReviewLoadRequest {
    preferred_deck: Option<String>,
    query: String,
    requested_offset: usize,
    requested_cursor: Option<usize>,
    initial_direction: i32,
    select_cursor: bool,
    check_services: bool,
    refresh_cache: bool,
}

#[derive(Clone)]
struct LoadedReviewPage {
    decks: Vec<String>,
    models: Vec<String>,
    deck: String,
    query: String,
    ids: Arc<Vec<i64>>,
    offset: usize,
    cursor: usize,
    notes: Vec<linguist_application::NoteInfo>,
    select_cursor: bool,
}

struct ReviewLoadOutput {
    generation: u64,
    anki: Option<Result<(), String>>,
    ollama: Option<Result<(), String>>,
    result: Result<LoadedReviewPage, String>,
}

const TARGET_FIELDS: [(&str, &str); 5] = [
    ("expression", "Expression"),
    ("meaning_image", "Meaning image"),
    ("meaning_text", "Meaning text"),
    ("kanji_construction", "Kanji construction"),
    ("audio", "Audio"),
];
const ALL_TARGET_FIELD_KEYS: [&str; 6] = [
    "expression",
    "meaning_image",
    "meaning_text",
    "examples",
    "kanji_construction",
    "audio",
];
const ENGLISH_VOCAB_TARGET_FIELDS: [(&str, &str); 4] = [
    ("expression", "Expression"),
    ("meaning_image", "Meaning image"),
    ("meaning_text", "Meaning text"),
    ("audio", "Audio"),
];
const JAPANESE_GRAMMAR_TARGET_FIELDS: [(&str, &str); 3] = [
    ("expression", "Grammar point"),
    ("meaning_text", "Explanation"),
    ("examples", "Examples"),
];

fn target_fields(purpose: &str) -> &'static [(&'static str, &'static str)] {
    match purpose {
        "english_vocab" => &ENGLISH_VOCAB_TARGET_FIELDS,
        "japanese_grammar" => &JAPANESE_GRAMMAR_TARGET_FIELDS,
        _ => &TARGET_FIELDS,
    }
}

#[derive(Clone, Default)]
struct FieldMappingEditor {
    note_id: Option<i64>,
    sources: Vec<(String, String)>,
    connections: Vec<Option<usize>>,
    hints: Vec<String>,
    purpose: String,
    deck: String,
    model: String,
    tags: Vec<String>,
    dirty: bool,
    message: String,
    media: HashMap<String, String>,
}

struct FieldMediaOutput {
    generation: u64,
    note_id: i64,
    media: HashMap<String, String>,
}

struct MappingSuggestionOutput {
    generation: u64,
    note_id: Option<i64>,
    result: Result<linguist_ollama::FieldMappingSuggestion, String>,
}
struct MappingInspectionOutput {
    generation: u64,
    result: Result<(FieldMappingEditor, linguist_application::NoteInfo), String>,
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
            theme_selection: palette.selection.into(),
            theme_red: palette.red.into(),
            theme_yellow: palette.yellow.into(),
            theme_green: palette.green.into(),
            theme_cyan: palette.cyan.into(),
            theme_blue: palette.blue.into(),
            theme_magenta: palette.magenta.into(),
            anki_status: state.anki.label().into(),
            ollama_status: state.ollama.label().into(),
            active_deck: state.active_deck.clone().into(),
            selection: state.selection.clone().into(),
            error_message: state.error.clone().into(),
            busy: state.busy,
            queue_state: controller.queue().state().label().into(),
            queue_message: controller.queue().state().message().into(),
            deck_count: queue_len(controller.queue().decks().len()),
            model_count: 0,
            selected_deck_index: queue_index(controller.queue().selected_deck_index()),
            review_row_count: queue_len(controller.queue().rows().len()),
            selected_review_index: queue_index(controller.queue().selected_index()),
            review_total: 0,
            review_page: 0,
            review_page_count: 0,
            review_page_start: 0,
            review_page_end: 0,
            review_search_match: 0,
            review_search_count: 0,
            review_navigation_serial: 0,
            review_content_serial: 0,
            mapping_source_count: 0,
            mapping_target_count: queue_len(TARGET_FIELDS.len()),
            mapping_purpose: QString::default(),
            mapping_deck: QString::default(),
            mapping_model: QString::default(),
            mapping_tags: QString::default(),
            mapping_dirty: false,
            mapping_busy: false,
            mapping_message: QString::default(),
            mapping_render_serial: 0,
            draft_expression: QString::default(),
            draft_meaning: QString::default(),
            draft_examples: QString::default(),
            draft_generated: false,
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
            csv_column_count: 0,
            csv_expression_column: 0,
            csv_language_column: -1,
            csv_type_column: -1,
            csv_context_column: -1,
            csv_header_names: Vec::new(),
            csv_source: None,
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
            review_browser: ReviewBrowser::default(),
            model_names: Vec::new(),
            review_cache: Arc::new(Mutex::new(ReviewCache::default())),
            review_load_result: Arc::new(Mutex::new(None)),
            review_generation: Arc::new(AtomicU64::new(0)),
            field_mapping: FieldMappingEditor::default(),
            field_media_cache: Arc::new(Mutex::new(HashMap::new())),
            field_media_result: Arc::new(Mutex::new(None)),
            field_media_generation: Arc::new(AtomicU64::new(0)),
            mapping_suggestion_result: Arc::new(Mutex::new(None)),
            mapping_suggestion_generation: Arc::new(AtomicU64::new(0)),
            mapping_inspection_result: Arc::new(Mutex::new(None)),
            mapping_inspection_generation: Arc::new(AtomicU64::new(0)),
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
            let report =
                linguist_config::import_legacy_file(&source).map_err(|error| error.to_string())?;
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
        self.as_mut().set_theme_accent(palette.accent.into());
        self.as_mut().set_theme_selection(palette.selection.into());
        self.as_mut().set_theme_red(palette.red.into());
        self.as_mut().set_theme_yellow(palette.yellow.into());
        self.as_mut().set_theme_green(palette.green.into());
        self.as_mut().set_theme_cyan(palette.cyan.into());
        self.as_mut().set_theme_blue(palette.blue.into());
        self.set_theme_magenta(palette.magenta.into());
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
        self.as_mut().rust_mut().controller.begin_refresh();
        if let Ok(mut cache) = self.as_ref().rust().review_cache.lock() {
            cache.indices.clear();
            cache.pages.clear();
        }
        sync_controller_state(self.as_mut());
        start_review_load(
            self,
            ReviewLoadRequest {
                preferred_deck: selected_deck,
                query: String::new(),
                requested_offset: 0,
                requested_cursor: Some(0),
                initial_direction: 1,
                select_cursor: false,
                check_services: true,
                refresh_cache: true,
            },
        );
    }

    pub fn poll_review_load(mut self: Pin<&mut Self>) {
        let inspection = self
            .as_ref()
            .rust()
            .mapping_inspection_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(inspection) = inspection
            && inspection.generation
                == self
                    .as_ref()
                    .rust()
                    .mapping_inspection_generation
                    .load(Ordering::Acquire)
        {
            match inspection.result {
                Ok((editor, note)) => {
                    self.as_mut().rust_mut().field_mapping = editor;
                    self.as_mut().set_mapping_busy(false);
                    sync_field_mapping_state(self.as_mut());
                    start_field_media_load(self.as_mut(), note);
                }
                Err(error) => {
                    self.as_mut().rust_mut().field_mapping.message =
                        format!("Could not inspect deck: {error}");
                    self.as_mut().set_mapping_busy(false);
                    sync_field_mapping_state(self.as_mut());
                }
            }
        }
        let suggestion = self
            .as_ref()
            .rust()
            .mapping_suggestion_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(suggestion) = suggestion
            && suggestion.generation
                == self
                    .as_ref()
                    .rust()
                    .mapping_suggestion_generation
                    .load(Ordering::Acquire)
            && suggestion.note_id == self.as_ref().rust().field_mapping.note_id
        {
            let mut state = self.as_mut().rust_mut();
            state.mapping_busy = false;
            match suggestion.result {
                Ok(suggestion) => {
                    let mut applied = 0;
                    let fields = target_fields(&state.field_mapping.purpose);
                    for assignment in suggestion.assignments {
                        let Some(target) = fields
                            .iter()
                            .position(|(key, _)| *key == assignment.logical_field)
                        else {
                            continue;
                        };
                        let Some(source) = state
                            .field_mapping
                            .sources
                            .iter()
                            .position(|(name, _)| name == &assignment.source_field)
                        else {
                            continue;
                        };
                        state.field_mapping.connections[target] = Some(source);
                        state.field_mapping.hints[target] = format!(
                            "{:.0}% · {}",
                            assignment.confidence.clamp(0.0, 1.0) * 100.0,
                            assignment.reason.trim()
                        );
                        applied += 1;
                    }
                    state.field_mapping.dirty = applied > 0;
                    state.field_mapping.message = format!(
                        "Ollama suggested {applied} connections · {}",
                        suggestion.summary.trim()
                    );
                }
                Err(error) => state.field_mapping.message = format!("Mapping failed: {error}"),
            }
            self.as_mut().set_mapping_busy(false);
            sync_field_mapping_state(self.as_mut());
        }
        let media_output = self
            .as_ref()
            .rust()
            .field_media_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(media_output) = media_output
            && media_output.generation
                == self
                    .as_ref()
                    .rust()
                    .field_media_generation
                    .load(Ordering::Acquire)
            && self.as_ref().rust().field_mapping.note_id == Some(media_output.note_id)
        {
            self.as_mut().rust_mut().field_mapping.media = media_output.media;
            let serial = self.as_ref().rust().mapping_render_serial.wrapping_add(1);
            self.as_mut().set_mapping_render_serial(serial);
        }
        let output = self
            .as_ref()
            .rust()
            .review_load_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let Some(output) = output else {
            return;
        };
        if output.generation
            != self
                .as_ref()
                .rust()
                .review_generation
                .load(Ordering::Acquire)
        {
            return;
        }
        if let (Some(anki), Some(ollama)) = (output.anki, output.ollama) {
            self.as_mut()
                .rust_mut()
                .controller
                .finish_service_checks(anki, ollama);
        }
        match output.result {
            Ok(page) => {
                let rows = page.notes.iter().cloned().map(review_row).collect();
                {
                    let mut state = self.as_mut().rust_mut();
                    state.model_names = page.models;
                    state.controller.finish_queue_load(
                        page.deck.clone(),
                        ReviewQueueData {
                            decks: page.decks,
                            rows,
                        },
                    );
                    state.review_browser = ReviewBrowser {
                        deck: page.deck,
                        query: page.query,
                        ids: page.ids,
                        offset: page.offset,
                        cursor: page.cursor,
                        notes: page.notes,
                    };
                    if page.select_cursor && !state.review_browser.notes.is_empty() {
                        let local = state
                            .review_browser
                            .cursor
                            .saturating_sub(state.review_browser.offset)
                            .min(state.review_browser.notes.len() - 1);
                        let note = state.review_browser.notes[local].clone();
                        state.controller.select_queue_index(local);
                        state.controller.hydrate_selected_note(note);
                    }
                    refresh_field_mapping(&mut state);
                }
                let serial = self.as_ref().review_content_serial().wrapping_add(1);
                self.as_mut().set_review_content_serial(serial);
                let model_count = queue_len(self.as_ref().rust().model_names.len());
                self.as_mut().set_model_count(model_count);
                sync_controller_state(self.as_mut());
                sync_review_browser_state(self.as_mut());
                sync_field_mapping_state(self.as_mut());
                start_selected_field_media_load(self);
            }
            Err(error) => {
                self.as_mut()
                    .rust_mut()
                    .controller
                    .finish_queue_error(error);
                sync_controller_state(self);
            }
        }
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

    pub fn search_review(mut self: Pin<&mut Self>, query: &QString, direction: i32) {
        let query = query.to_string().trim().to_owned();
        let (deck, same_query, total, current, offset) = {
            let binding = self.as_ref();
            let state = binding.rust();
            (
                state.review_browser.deck.clone(),
                state.review_browser.query == query,
                state.review_browser.ids.len(),
                state.review_browser.cursor,
                state.review_browser.offset,
            )
        };
        if deck.is_empty() {
            return;
        }
        if same_query && total > 0 {
            let cursor = if direction < 0 {
                (current + total - 1) % total
            } else {
                (current + 1) % total
            };
            let page_offset = cursor / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE;
            if page_offset == offset {
                let local = cursor - offset;
                {
                    let mut state = self.as_mut().rust_mut();
                    state.review_browser.cursor = cursor;
                    state.controller.select_queue_index(local);
                    if let Some(note) = state.review_browser.notes.get(local).cloned() {
                        state.controller.hydrate_selected_note(note);
                    }
                }
                sync_controller_state(self.as_mut());
                sync_review_browser_state(self);
                return;
            }
            self.as_mut().rust_mut().controller.begin_queue_loading();
            sync_controller_state(self.as_mut());
            start_review_load(
                self,
                ReviewLoadRequest {
                    preferred_deck: Some(deck),
                    query,
                    requested_offset: page_offset,
                    requested_cursor: Some(cursor),
                    initial_direction: direction,
                    select_cursor: true,
                    check_services: false,
                    refresh_cache: false,
                },
            );
            return;
        }
        self.as_mut().rust_mut().controller.begin_queue_loading();
        sync_controller_state(self.as_mut());
        start_review_load(
            self,
            ReviewLoadRequest {
                preferred_deck: Some(deck),
                query,
                requested_offset: 0,
                requested_cursor: None,
                initial_direction: direction,
                select_cursor: true,
                check_services: false,
                refresh_cache: false,
            },
        );
    }

    pub fn previous_review_page(self: Pin<&mut Self>) {
        move_review_page(self, -1);
    }

    pub fn next_review_page(self: Pin<&mut Self>) {
        move_review_page(self, 1);
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

    pub fn model_name(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().model_names.get(index))
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
            self.as_mut()
                .rust_mut()
                .controller
                .begin_deck_loading(index);
            sync_controller_state(self.as_mut());
            self.as_mut().rust_mut().review_browser = ReviewBrowser::default();
            self.as_mut().rust_mut().field_mapping = FieldMappingEditor::default();
            sync_review_browser_state(self.as_mut());
            sync_field_mapping_state(self.as_mut());
            start_review_load(
                self,
                ReviewLoadRequest {
                    preferred_deck: selected_deck,
                    query: String::new(),
                    requested_offset: 0,
                    requested_cursor: Some(0),
                    initial_direction: 1,
                    select_cursor: false,
                    check_services: false,
                    refresh_cache: false,
                },
            );
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
                let note_id = backend
                    .as_ref()
                    .rust()
                    .controller
                    .queue()
                    .rows()
                    .get(index)
                    .map(|row| row.note_id);
                if let Some(note) = backend
                    .as_ref()
                    .rust()
                    .review_browser
                    .notes
                    .iter()
                    .find(|note| Some(note.note_id) == note_id)
                    .cloned()
                {
                    backend
                        .as_mut()
                        .rust_mut()
                        .controller
                        .hydrate_selected_note(note);
                }
            }
            refresh_field_mapping(&mut backend.as_mut().rust_mut());
            sync_controller_state(self.as_mut());
            sync_field_mapping_state(self.as_mut());
            start_selected_field_media_load(self);
        }
    }

    pub fn mapping_source_name(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().field_mapping.sources.get(index))
            .map(|(name, _)| name.clone())
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_source_html(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().field_mapping.sources.get(index))
            .map(|(_, html)| {
                strip_image_tags(&embed_media_markup(
                    safe_field_markup(html),
                    &self.rust().field_mapping.media,
                ))
            })
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_source_image(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().field_mapping.sources.get(index))
            .and_then(|(_, html)| local_image_names(html).into_iter().next())
            .and_then(|name| self.rust().field_mapping.media.get(&name).cloned())
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_target_key(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| target_fields(&self.rust().field_mapping.purpose).get(index))
            .map(|(key, _)| *key)
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_target_label(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| target_fields(&self.rust().field_mapping.purpose).get(index))
            .map(|(_, label)| *label)
            .unwrap_or_default()
            .into()
    }

    fn generated_target_value(&self, index: i32) -> Option<String> {
        let editor = &self.rust().field_mapping;
        let draft = self.rust().controller.active_draft()?;
        if editor.note_id != Some(draft.note_id) {
            return None;
        }
        let document = draft.generated_document()?;
        let (key, _) = target_fields(&editor.purpose).get(usize::try_from(index).ok()?)?;
        match *key {
            "expression" => Some(
                document
                    .expression
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            ),
            "meaning_image" => document.values.meaning_image,
            "meaning_text" => document.values.meaning_text,
            "examples" => document.values.examples,
            "kanji_construction" => document.values.kanji_construction,
            "audio" => document.values.audio,
            _ => None,
        }
    }

    pub fn mapping_target_html(&self, index: i32) -> QString {
        self.generated_target_value(index)
            .map(|value| strip_image_tags(&safe_field_markup(&value)))
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_target_image(&self, index: i32) -> QString {
        self.generated_target_value(index)
            .and_then(|value| local_image_names(&value).into_iter().next())
            .and_then(|name| self.preview_media().get(&name).cloned())
            .unwrap_or_default()
            .into()
    }

    pub fn mapping_target_hint(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().field_mapping.hints.get(index))
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn mapped_source_index(&self, target_index: i32) -> i32 {
        usize::try_from(target_index)
            .ok()
            .and_then(|index| self.rust().field_mapping.connections.get(index))
            .and_then(|source| *source)
            .map(queue_len)
            .unwrap_or(-1)
    }

    pub fn connect_mapping_field(mut self: Pin<&mut Self>, source_index: i32, target_index: i32) {
        let (Ok(source), Ok(target)) =
            (usize::try_from(source_index), usize::try_from(target_index))
        else {
            return;
        };
        let mut state = self.as_mut().rust_mut();
        if source >= state.field_mapping.sources.len()
            || target >= state.field_mapping.connections.len()
        {
            return;
        }
        state.field_mapping.connections[target] = Some(source);
        state.field_mapping.hints[target].clear();
        state.field_mapping.dirty = true;
        state.field_mapping.message = "Mapping changed; save to reuse it".into();
        sync_field_mapping_state(self);
    }

    pub fn clear_mapping_field(mut self: Pin<&mut Self>, target_index: i32) {
        let Ok(target) = usize::try_from(target_index) else {
            return;
        };
        let mut state = self.as_mut().rust_mut();
        let Some(connection) = state.field_mapping.connections.get_mut(target) else {
            return;
        };
        *connection = None;
        if let Some(hint) = state.field_mapping.hints.get_mut(target) {
            hint.clear();
        }
        state.field_mapping.dirty = true;
        state.field_mapping.message = "Mapping changed; save to reuse it".into();
        sync_field_mapping_state(self);
    }

    pub fn choose_mapping_purpose(mut self: Pin<&mut Self>, purpose: &QString) {
        let Some(purpose) = linguist_application::canonical_language_key(&purpose.to_string())
        else {
            return;
        };
        let mut state = self.as_mut().rust_mut();
        if state.field_mapping.purpose != purpose {
            let existing = mapping_fields(&state.field_mapping);
            state.field_mapping.purpose = purpose;
            state.field_mapping.connections = target_fields(&state.field_mapping.purpose)
                .iter()
                .map(|(key, _)| {
                    existing.get(*key).and_then(|name| {
                        state
                            .field_mapping
                            .sources
                            .iter()
                            .position(|(source, _)| source == name)
                    })
                })
                .collect();
            state.field_mapping.hints = vec![String::new(); state.field_mapping.connections.len()];
            state.field_mapping.dirty = true;
            state.field_mapping.message = "Purpose changed; save to reuse it".into();
        }
        sync_field_mapping_state(self);
    }

    pub fn save_field_mapping(mut self: Pin<&mut Self>) {
        let result = (|| {
            let binding = self.as_ref();
            let mapping = &binding.rust().field_mapping;
            if mapping.purpose.is_empty() {
                return Err("Choose a deck purpose before saving".to_owned());
            }
            if mapping.deck.is_empty() || mapping.model.is_empty() {
                return Err("The selected card has no deck or note type".to_owned());
            }
            let fields = mapping_fields(mapping);
            if !fields.contains_key("expression") {
                return Err("Connect an expression field before saving".to_owned());
            }
            let mut config = runtime_config()?;
            if let Some((purpose, _)) = config.decks.iter().find(|(purpose, deck)| {
                purpose.as_str() != mapping.purpose
                    && deck.deck_name.as_deref() == Some(mapping.deck.as_str())
            }) {
                return Err(format!(
                    "This deck is already assigned to {purpose}; choose a different purpose"
                ));
            }
            let deck = config.decks.entry(mapping.purpose.clone()).or_default();
            deck.deck_name = Some(mapping.deck.clone());
            deck.model_name = Some(mapping.model.clone());
            deck.fields = fields;
            let path = native_config_path()?;
            linguist_config::save_native_replace(&path, &config).map_err(|error| error.to_string())
        })();
        let mut state = self.as_mut().rust_mut();
        match result {
            Ok(()) => {
                state.field_mapping.dirty = false;
                state.field_mapping.message = "Reusable field mapping saved".into();
            }
            Err(error) => state.field_mapping.message = error,
        }
        sync_field_mapping_state(self);
    }

    pub fn suggest_field_mapping(mut self: Pin<&mut Self>) {
        let editor = self.as_ref().rust().field_mapping.clone();
        if editor.sources.is_empty() || editor.purpose.is_empty() {
            self.as_mut().rust_mut().field_mapping.message =
                "Load a deck sample and choose its purpose first".into();
            sync_field_mapping_state(self);
            return;
        }
        let prompt = mapping_reasoning_prompt(&editor);
        let binding = self.as_ref();
        let state = binding.rust();
        let generation = state
            .mapping_suggestion_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let live_generation = state.mapping_suggestion_generation.clone();
        let result_slot = state.mapping_suggestion_result.clone();
        self.as_mut().rust_mut().mapping_busy = true;
        self.as_mut().rust_mut().field_mapping.message =
            "Reasoning about deck and card fields…".into();
        self.as_mut().set_mapping_busy(true);
        sync_field_mapping_state(self.as_mut());
        std::thread::spawn(move || {
            let result = (|| {
                let port = LiveDesktopPort::from_environment()?;
                let config = runtime_config()?;
                let model = if let Some(model) =
                    config.ollama_model.filter(|model| !model.trim().is_empty())
                {
                    model
                } else {
                    let models = port
                        .runtime
                        .block_on(port.ollama.models())
                        .map_err(|error| error.to_string())?;
                    choose_reasoning_model(&models)
                        .ok_or_else(|| "Ollama has no installed models".to_owned())?
                };
                port.runtime
                    .block_on(port.ollama.suggest_field_mapping(&model, &prompt))
                    .map_err(|error| error.to_string())
            })();
            if live_generation.load(Ordering::Acquire) == generation
                && let Ok(mut slot) = result_slot.lock()
            {
                *slot = Some(MappingSuggestionOutput {
                    generation,
                    note_id: editor.note_id,
                    result,
                });
            }
        });
    }

    pub fn inspect_deck_mapping(mut self: Pin<&mut Self>, purpose: &QString, deck_name: &QString) {
        let Some(purpose) = linguist_application::canonical_language_key(&purpose.to_string())
        else {
            self.as_mut().rust_mut().field_mapping.message = "Choose a deck purpose".into();
            sync_field_mapping_state(self);
            return;
        };
        let deck_name = deck_name.to_string().trim().to_owned();
        if deck_name.is_empty() {
            self.as_mut().rust_mut().field_mapping.message = "Choose an Anki deck".into();
            sync_field_mapping_state(self);
            return;
        }
        let binding = self.as_ref();
        let state = binding.rust();
        let generation = state
            .mapping_inspection_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let live_generation = state.mapping_inspection_generation.clone();
        let result_slot = state.mapping_inspection_result.clone();
        self.as_mut().set_mapping_busy(true);
        self.as_mut().rust_mut().field_mapping.message =
            format!("Inspecting a representative card from {deck_name}…");
        sync_field_mapping_state(self.as_mut());
        std::thread::spawn(move || {
            let result = (|| {
                let port = LiveDesktopPort::from_environment()?;
                let query = review_anki_query(&deck_name, "");
                let note_id = port
                    .runtime
                    .block_on(port.anki.find_notes(&query))
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Deck {deck_name:?} has no notes"))?;
                let note = port
                    .runtime
                    .block_on(port.anki.notes_info(&[note_id]))
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Anki did not return note {note_id}"))?;
                let editor =
                    field_mapping_for_note(&note, &purpose, &deck_name, &runtime_config()?);
                Ok((editor, note))
            })();
            if live_generation.load(Ordering::Acquire) == generation
                && let Ok(mut slot) = result_slot.lock()
            {
                *slot = Some(MappingInspectionOutput { generation, result });
            }
        });
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

    pub fn edit_draft_examples(mut self: Pin<&mut Self>, value: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .edit_draft(crate::draft::DraftField::Examples, value.to_string());
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
        if !self.rust().draft_generated {
            self.as_mut().rust_mut().controller.report_error(
                "Generate a card and review all proposed changes before running a dry run",
            );
            sync_controller_state(self);
            return;
        }
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
        if !self.rust().draft_generated {
            self.as_mut()
                .rust_mut()
                .controller
                .report_error("Generate and review the card before applying changes to Anki");
            sync_controller_state(self);
            return;
        }
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
        self.render_card_preview(template_index, back)
            .map(|rendered| embed_media_markup(rendered.html, &self.preview_media()))
            .unwrap_or_default()
            .into()
    }

    fn render_card_preview(
        &self,
        template_index: i32,
        back: bool,
    ) -> Option<crate::preview_model::RenderedCard> {
        let draft = self.rust().controller.active_draft()?;
        let document = draft.accepted_document()?;
        if !draft.pending().is_empty() {
            return None;
        }
        usize::try_from(template_index).ok().and_then(|index| {
            let config = runtime_config().ok()?;
            let purpose = generation_deck_key(draft, &config.decks).ok()?;
            crate::preview_model::render_managed_card_with_spec(
                &document,
                &linguist_core::managed_vocab_spec(purpose),
                index,
                if back {
                    crate::preview_model::CardFace::Back
                } else {
                    crate::preview_model::CardFace::Front
                },
            )
        })
    }

    fn preview_media(&self) -> HashMap<String, String> {
        let mut media = self.rust().field_mapping.media.clone();
        if let Some(document) = self
            .rust()
            .controller
            .active_draft()
            .and_then(crate::draft::ReviewDraft::generated_document)
        {
            media.extend(document.media.iter().map(|asset| {
                (
                    asset.filename.clone(),
                    media_data_uri(&asset.filename, &asset.data_base64),
                )
            }));
        }
        media
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
        clear_csv_preview(self.as_mut());
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
        clear_csv_preview(self.as_mut());
        self.as_mut().rust_mut().csv_source = Some(CsvImportSource {
            content: content.to_string(),
            deck_key: deck_key.to_string(),
            language_key: language_key.to_string(),
            type_tag: type_tag.to_string(),
        });
        show_csv_preview(self, None);
    }

    pub fn csv_column_name(&self, index: i32) -> QString {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rust().csv_header_names.get(index))
            .cloned()
            .unwrap_or_else(|| "—".into())
            .into()
    }

    pub fn remap_csv_input(
        self: Pin<&mut Self>,
        expression: i32,
        language: i32,
        type_tag: i32,
        context: i32,
    ) {
        let Ok(expression) = usize::try_from(expression) else {
            return;
        };
        show_csv_preview(
            self,
            Some(CsvColumnMapping {
                expression,
                language: usize::try_from(language).ok(),
                type_tag: usize::try_from(type_tag).ok(),
                context: usize::try_from(context).ok(),
            }),
        );
    }

    pub fn preview_csv_file(
        mut self: Pin<&mut Self>,
        file_url: &QString,
        deck_key: &QString,
        language_key: &QString,
        type_tag: &QString,
    ) {
        clear_ingestion_preview(self.as_mut());
        clear_csv_preview(self.as_mut());
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
        let (deck_key, deck_name) = match selector_job_identity(&selector, &runtime_config()) {
            Ok(identity) => identity,
            Err(error) => {
                self.as_mut().rust_mut().controller.report_error(error);
                sync_controller_state(self);
                return;
            }
        };
        let settings = BTreeMap::from([
            (
                "selector".into(),
                serde_json::to_value(&selector).expect("batch selector is serializable"),
            ),
            ("selection".into(), python_selection(&selector, &deck_key)),
        ]);
        let job = linguist_jobs::NewJob {
            deck_key,
            deck_name,
            dry_run,
            settings,
            items,
        };
        self.as_mut().rust_mut().selector_pending = None;
        self.batch_command(|controller, port| controller.create_batch(port, job));
    }
}

fn selector_job_identity(
    selector: &BatchSelector,
    config: &Result<linguist_config::NativeConfig, String>,
) -> Result<(String, String), String> {
    let deck = selector
        .deck
        .as_deref()
        .map(str::trim)
        .filter(|deck| !deck.is_empty())
        .ok_or_else(|| "A mapped deck is required for a selector batch".to_owned())?;
    let config = config.as_ref().map_err(Clone::clone)?;
    let matches = config
        .decks
        .iter()
        .filter(|(key, value)| key.as_str() == deck || value.deck_name.as_deref() == Some(deck))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(key, value)] => Ok((
            (*key).clone(),
            value.deck_name.clone().unwrap_or_else(|| deck.into()),
        )),
        [] => Err(format!(
            "Deck '{deck}' has no purpose mapping; configure it before creating a batch"
        )),
        _ => Err(format!("Deck '{deck}' has multiple purpose mappings")),
    }
}

fn python_selection(selector: &BatchSelector, deck_key: &str) -> serde_json::Value {
    serde_json::json!({
        "deck_key": deck_key,
        "date_from": selector.created_after.as_deref().unwrap_or_default(),
        "date_to": selector.created_before.as_deref().unwrap_or_default(),
        "model_name": selector.model.as_deref().unwrap_or_default(),
        "card_template": selector.template.as_deref().unwrap_or_default(),
        "query": selector.query,
        "required_tags": selector.tags,
        "excluded_tags": selector.excluded_tags,
        "media_scope": match selector.image {
            linguist_application::ImageFilter::Any => "all",
            linguist_application::ImageFilter::HasImage => "with",
            linguist_application::ImageFilter::NoImage => "without",
        },
        "limit": selector.limit,
    })
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

fn clear_csv_preview(mut backend: Pin<&mut qobject::AppBackend>) {
    backend.as_mut().rust_mut().csv_source = None;
    backend.as_mut().rust_mut().csv_header_names.clear();
    backend.as_mut().set_csv_headers(QString::default());
    backend.as_mut().set_csv_mapping(QString::default());
    backend.as_mut().set_csv_column_count(0);
    backend.as_mut().set_csv_expression_column(0);
    backend.as_mut().set_csv_language_column(-1);
    backend.as_mut().set_csv_type_column(-1);
    backend.as_mut().set_csv_context_column(-1);
}

fn show_csv_preview(mut backend: Pin<&mut qobject::AppBackend>, mapping: Option<CsvColumnMapping>) {
    clear_ingestion_preview(backend.as_mut());
    let Some(source) = backend.as_ref().rust().csv_source.clone() else {
        return;
    };
    let csv = match prepare_csv_input(&CsvIngestRequest {
        content: source.content,
        deck_key: source.deck_key,
        language_key: source.language_key.clone(),
        type_tag: source.type_tag,
        mapping,
    }) {
        Ok(csv) => csv,
        Err(error) => {
            backend.as_mut().rust_mut().controller.report_error(error);
            sync_controller_state(backend);
            return;
        }
    };
    let header = |column: Option<usize>| {
        column
            .and_then(|column| csv.headers.get(column))
            .cloned()
            .unwrap_or_else(|| "—".into())
    };
    let mapping_label = format!(
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
        .map(|config| route_ingestion_preview(prepared, &config, &source.language_key))
    {
        Ok(preview) => preview,
        Err(error) => {
            backend.as_mut().rust_mut().controller.report_error(error);
            sync_controller_state(backend);
            return;
        }
    };
    let display = build_ingestion_display(&preview);
    let row_count = queue_len(display.rows.len());
    let issue_count = queue_len(display.issues.len());
    let enqueue_count = pending_enqueue_count(&display.pending);
    backend.as_mut().rust_mut().manual_preview_rows = display.rows;
    backend.as_mut().rust_mut().manual_preview_issues = display.issues;
    backend.as_mut().rust_mut().manual_pending = display.pending;
    backend.as_mut().rust_mut().manual_options = display.options;
    backend.as_mut().set_manual_preview_count(row_count);
    backend.as_mut().set_manual_enqueue_count(enqueue_count);
    backend.as_mut().set_manual_issue_count(issue_count);
    backend.as_mut().rust_mut().csv_header_names = csv.headers.clone();
    backend
        .as_mut()
        .set_csv_column_count(queue_len(csv.headers.len()));
    backend
        .as_mut()
        .set_csv_expression_column(csv.mapping.expression as i32);
    backend
        .as_mut()
        .set_csv_language_column(csv.mapping.language.map_or(-1, |index| index as i32));
    backend
        .as_mut()
        .set_csv_type_column(csv.mapping.type_tag.map_or(-1, |index| index as i32));
    backend
        .as_mut()
        .set_csv_context_column(csv.mapping.context.map_or(-1, |index| index as i32));
    backend
        .as_mut()
        .set_csv_headers(csv.headers.join(" · ").into());
    backend.as_mut().set_csv_mapping(mapping_label.into());
    sync_controller_state(backend);
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

fn start_selected_field_media_load(qobject: Pin<&mut qobject::AppBackend>) {
    let note = {
        let binding = qobject.as_ref();
        let state = binding.rust();
        let selected = state
            .controller
            .queue()
            .selected_index()
            .and_then(|index| state.controller.queue().rows().get(index))
            .map(|row| row.note_id);
        state
            .review_browser
            .notes
            .iter()
            .find(|note| Some(note.note_id) == selected)
            .cloned()
    };
    let Some(note) = note else {
        return;
    };
    start_field_media_load(qobject, note);
}

fn start_field_media_load(
    qobject: Pin<&mut qobject::AppBackend>,
    note: linguist_application::NoteInfo,
) {
    let names = note
        .fields
        .values()
        .flat_map(|html| local_image_names(html))
        .collect::<BTreeSet<_>>();
    let binding = qobject.as_ref();
    let state = binding.rust();
    let generation = state.field_media_generation.fetch_add(1, Ordering::AcqRel) + 1;
    let cache = state.field_media_cache.clone();
    let result = state.field_media_result.clone();
    let live_generation = state.field_media_generation.clone();
    std::thread::spawn(move || {
        let mut media = HashMap::new();
        let port = LiveDesktopPort::from_environment().ok();
        for name in names {
            if live_generation.load(Ordering::Acquire) != generation {
                return;
            }
            let cached = cache
                .lock()
                .ok()
                .and_then(|cache| cache.get(&name).cloned());
            let uri = if let Some(uri) = cached {
                Some(uri)
            } else if let Some(port) = &port {
                port.runtime
                    .block_on(port.anki.retrieve_media_file(&name))
                    .ok()
                    .flatten()
                    .map(|file| media_data_uri(&file.filename, &file.data_base64))
            } else {
                None
            };
            if let Some(uri) = uri {
                if let Ok(mut cache) = cache.lock() {
                    cache.insert(name.clone(), uri.clone());
                }
                media.insert(name, uri);
            }
        }
        if live_generation.load(Ordering::Acquire) == generation
            && let Ok(mut slot) = result.lock()
        {
            *slot = Some(FieldMediaOutput {
                generation,
                note_id: note.note_id,
                media,
            });
        }
    });
}

fn local_image_names(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let lower = value.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find("src=") {
        let start = cursor + relative + 4;
        let Some(quote) = value[start..].chars().next() else {
            break;
        };
        if !matches!(quote, '"' | '\'') {
            cursor = start;
            continue;
        }
        let value_start = start + quote.len_utf8();
        let Some(end) = value[value_start..].find(quote) else {
            break;
        };
        let name = &value[value_start..value_start + end];
        if valid_local_media_name(name) {
            names.push(name.to_owned());
        }
        cursor = value_start + end + quote.len_utf8();
    }
    names
}

fn valid_local_media_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn media_data_uri(filename: &str, data_base64: &str) -> String {
    let lower = filename.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/jpeg"
    };
    format!("data:{mime};base64,{data_base64}")
}

fn embed_media_markup(mut html: String, media: &HashMap<String, String>) -> String {
    for (name, uri) in media {
        html = html.replace(&format!("linguist-media:///{name}"), uri);
        for quote in ['"', '\''] {
            html = html.replace(
                &format!("src={quote}{name}{quote}"),
                &format!("src={quote}{uri}{quote}"),
            );
        }
    }
    html
}

fn strip_image_tags(value: &str) -> String {
    let mut output = value.to_owned();
    loop {
        let lower = output.to_ascii_lowercase();
        let Some(start) = lower.find("<img") else {
            break;
        };
        let end = lower[start..]
            .find('>')
            .map(|offset| start + offset + 1)
            .unwrap_or(output.len());
        output.replace_range(start..end, "");
    }
    output
}

fn refresh_field_mapping(state: &mut AppBackendRust) {
    let Some(draft) = state.controller.active_draft() else {
        state.field_mapping = FieldMappingEditor::default();
        return;
    };
    if state.field_mapping.note_id == Some(draft.note_id) {
        return;
    }
    let config = runtime_config().unwrap_or_default();
    let purpose = draft
        .language_key
        .clone()
        .filter(|key| linguist_application::canonical_language_key(key).is_some())
        .or_else(|| {
            config.decks.iter().find_map(|(key, mapped)| {
                (mapped.deck_name.as_deref() == Some(draft.deck_name.as_str())
                    && mapped
                        .model_name
                        .as_deref()
                        .is_none_or(|model| model == draft.target_model))
                .then(|| key.clone())
            })
        })
        .unwrap_or_default();
    let configured = config
        .decks
        .get(&purpose)
        .map(|deck| deck.fields.clone())
        .filter(|fields| !fields.is_empty())
        .unwrap_or_else(|| inferred_mapping_fields(&draft.source_fields));
    let sources = ordered_mapping_sources(&draft.source_fields, &configured);
    let connections = target_fields(&purpose)
        .iter()
        .map(|(key, _)| {
            configured
                .get(*key)
                .and_then(|source| sources.iter().position(|(name, _)| name == source))
        })
        .collect();
    state.field_mapping = FieldMappingEditor {
        note_id: Some(draft.note_id),
        sources,
        connections,
        hints: vec![String::new(); target_fields(&purpose).len()],
        purpose,
        deck: draft.deck_name.clone(),
        model: draft.target_model.clone(),
        tags: draft.source_tags.clone(),
        dirty: false,
        message: String::new(),
        media: HashMap::new(),
    };
}

fn field_mapping_for_note(
    note: &linguist_application::NoteInfo,
    purpose: &str,
    deck: &str,
    config: &linguist_config::NativeConfig,
) -> FieldMappingEditor {
    let configured = config
        .decks
        .get(purpose)
        .map(|deck| deck.fields.clone())
        .filter(|fields| !fields.is_empty())
        .unwrap_or_else(|| inferred_mapping_fields(&note.fields));
    let sources = ordered_mapping_sources(&note.fields, &configured);
    let connections = target_fields(purpose)
        .iter()
        .map(|(key, _)| {
            configured
                .get(*key)
                .and_then(|source| sources.iter().position(|(name, _)| name == source))
        })
        .collect();
    FieldMappingEditor {
        note_id: Some(note.note_id),
        sources,
        connections,
        hints: vec![String::new(); target_fields(purpose).len()],
        purpose: purpose.into(),
        deck: deck.into(),
        model: note.model_name.0.clone(),
        tags: note.tags.clone(),
        dirty: false,
        message: "Review the inferred connections or ask Ollama for a suggestion".into(),
        media: HashMap::new(),
    }
}

fn ordered_mapping_sources(
    fields: &BTreeMap<String, String>,
    mapping: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut ordered = Vec::with_capacity(fields.len());
    for key in ALL_TARGET_FIELD_KEYS {
        let Some(name) = mapping.get(key) else {
            continue;
        };
        if let Some(value) = fields.get(name)
            && !ordered.iter().any(|(existing, _)| existing == name)
        {
            ordered.push((name.clone(), value.clone()));
        }
    }
    let remaining = fields
        .iter()
        .filter(|(name, _)| !ordered.iter().any(|(existing, _)| existing == *name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    ordered.extend(remaining);
    ordered
}

fn inferred_mapping_fields(fields: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mapping = mapping_from_fields(fields);
    [
        ("expression", mapping.expression),
        ("meaning_image", mapping.meaning_image),
        ("meaning_text", mapping.meaning_text),
        ("examples", mapping.examples),
        ("kanji_construction", mapping.kanji_construction),
        ("audio", mapping.audio),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
    .collect()
}

fn mapping_fields(editor: &FieldMappingEditor) -> BTreeMap<String, String> {
    target_fields(&editor.purpose)
        .iter()
        .enumerate()
        .filter_map(|(target, (key, _))| {
            let source = editor.connections.get(target).and_then(|source| *source)?;
            let name = editor.sources.get(source).map(|(name, _)| name.clone())?;
            Some(((*key).to_owned(), name))
        })
        .collect()
}

fn mapping_reasoning_prompt(editor: &FieldMappingEditor) -> String {
    let targets = target_fields(&editor.purpose)
        .iter()
        .map(|(key, label)| serde_json::json!({"key": key, "meaning": label}))
        .collect::<Vec<_>>();
    let sources = editor
        .sources
        .iter()
        .map(|(name, value)| {
            let plain = linguist_core::normalize_expression(value);
            let excerpt = plain.chars().take(400).collect::<String>();
            serde_json::json!({"name": name, "sample": excerpt})
        })
        .collect::<Vec<_>>();
    format!(
        "You are mapping one existing Anki note type into a language-learning card schema. Reason from field names and representative content. Assign a source only when it semantically fits. A source may be reused when one legacy field combines concepts. Use exact target keys and exact source names. Do not invent fields. English vocabulary intentionally has no kanji construction. Return every confident mapping and explain uncertainty.\nContext:\n{}",
        serde_json::json!({
            "purpose": editor.purpose,
            "deck": editor.deck,
            "note_type": editor.model,
            "tags": editor.tags,
            "target_schema": targets,
            "source_fields": sources,
        })
    )
}

fn choose_reasoning_model(models: &[String]) -> Option<String> {
    models
        .iter()
        .find(|model| {
            let model = model.to_ascii_lowercase();
            ["reason", "qwen", "deepseek", "gpt-oss"]
                .iter()
                .any(|token| model.contains(token))
        })
        .or_else(|| models.first())
        .cloned()
}

fn sync_field_mapping_state(mut qobject: Pin<&mut qobject::AppBackend>) {
    let (source_count, purpose, deck, model, tags, dirty, message) = {
        let binding = qobject.as_ref();
        let editor = &binding.rust().field_mapping;
        (
            queue_len(editor.sources.len()),
            editor.purpose.clone(),
            editor.deck.clone(),
            editor.model.clone(),
            editor.tags.join(" · "),
            editor.dirty,
            editor.message.clone(),
        )
    };
    qobject.as_mut().set_mapping_source_count(source_count);
    qobject
        .as_mut()
        .set_mapping_target_count(queue_len(target_fields(&purpose).len()));
    qobject.as_mut().set_mapping_purpose(purpose.into());
    qobject.as_mut().set_mapping_deck(deck.into());
    qobject.as_mut().set_mapping_model(model.into());
    qobject.as_mut().set_mapping_tags(tags.into());
    qobject.as_mut().set_mapping_dirty(dirty);
    qobject.as_mut().set_mapping_message(message.into());
    let serial = qobject.as_ref().mapping_render_serial().wrapping_add(1);
    qobject.set_mapping_render_serial(serial);
}

fn safe_field_markup(value: &str) -> String {
    let mut safe = value.to_owned();
    for tag in ["script", "iframe", "object", "embed", "form"] {
        safe = remove_html_block(&safe, tag);
    }
    safe = strip_remote_attribute(&safe, "src");
    strip_remote_attribute(&safe, "href")
}

fn remove_html_block(value: &str, tag: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let lower = value.to_ascii_lowercase();
    let mut cursor = 0;
    let opening = format!("<{tag}");
    let closing = format!("</{tag}>");
    while let Some(relative) = lower[cursor..].find(&opening) {
        let start = cursor + relative;
        output.push_str(&value[cursor..start]);
        let after = lower[start..]
            .find(&closing)
            .map(|end| start + end + closing.len())
            .or_else(|| lower[start..].find('>').map(|end| start + end + 1))
            .unwrap_or(value.len());
        cursor = after;
    }
    output.push_str(&value[cursor..]);
    output
}

fn strip_remote_attribute(value: &str, attribute: &str) -> String {
    let mut output = value.to_owned();
    for quote in ['"', '\''] {
        for scheme in ["http://", "https://", "//"] {
            let needle = format!("{attribute}={quote}{scheme}");
            while let Some(start) = output.to_ascii_lowercase().find(&needle) {
                let value_start = start + attribute.len() + 2;
                let end = output[value_start..]
                    .find(quote)
                    .map(|end| value_start + end)
                    .unwrap_or(output.len());
                output.replace_range(value_start..end, "");
            }
        }
    }
    output
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
    let examples = qobject
        .as_ref()
        .rust()
        .controller
        .active_draft()
        .map(|draft| draft.examples.clone())
        .unwrap_or_default();
    qobject.as_mut().set_draft_examples(examples.into());
    let generated = qobject
        .as_ref()
        .rust()
        .controller
        .active_draft()
        .is_some_and(|draft| draft.accepted_document().is_some() && draft.pending().is_empty());
    qobject.as_mut().set_draft_generated(generated);
    let revision = qobject.as_ref().mapping_render_serial().wrapping_add(1);
    qobject.as_mut().set_mapping_render_serial(revision);
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
    let mut config = if path.exists() {
        linguist_config::load_native(&path).map_err(|error| error.to_string())?
    } else {
        linguist_config::NativeConfig::default()
    };
    if let Some(defaults) = local_mapping_defaults()? {
        merge_mapping_defaults(&mut config, defaults);
    }
    Ok(config)
}

fn merge_mapping_defaults(
    config: &mut linguist_config::NativeConfig,
    defaults: BTreeMap<String, linguist_config::DeckConfig>,
) {
    for (purpose, default) in defaults {
        let configured = config.decks.entry(purpose).or_default();
        if configured.deck_name.is_none() {
            configured.deck_name = default.deck_name;
        }
        if configured.model_name.is_none() {
            configured.model_name = default.model_name;
        }
        if configured.ocr_languages.is_empty() {
            configured.ocr_languages = default.ocr_languages;
        }
        for (logical, physical) in default.fields {
            configured.fields.entry(logical).or_insert(physical);
        }
    }
}

fn local_mapping_defaults() -> Result<Option<BTreeMap<String, linguist_config::DeckConfig>>, String>
{
    let path = std::env::var_os("LINGUIST_LOCAL_MAPPING_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            let path = native_config_path()
                .ok()?
                .with_file_name("default-mappings.local.json");
            path.exists().then_some(path)
        })
        .or_else(|| {
            let path = PathBuf::from("local-deck-mappings.json");
            path.exists().then_some(path)
        });
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid {}: {error}", path.display()))
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

fn start_review_load(qobject: Pin<&mut qobject::AppBackend>, request: ReviewLoadRequest) {
    let binding = qobject.as_ref();
    let state = binding.rust();
    let generation = state.review_generation.fetch_add(1, Ordering::AcqRel) + 1;
    let cache = state.review_cache.clone();
    let slot = state.review_load_result.clone();
    let live_generation = state.review_generation.clone();
    std::thread::spawn(move || {
        let (anki, ollama, result) = match LiveDesktopPort::from_environment() {
            Ok(port) => {
                let anki = request.check_services.then(|| port.anki_available());
                let ollama = request.check_services.then(|| port.ollama_available());
                let result = load_review_page(&request, &cache, &port);
                (anki, ollama, result)
            }
            Err(error) => (
                request.check_services.then(|| Err(error.clone())),
                request.check_services.then(|| Err(error.clone())),
                Err(error),
            ),
        };
        let preload = result.as_ref().ok().cloned();
        if live_generation.load(Ordering::Acquire) == generation
            && let Ok(mut output) = slot.lock()
        {
            *output = Some(ReviewLoadOutput {
                generation,
                anki,
                ollama,
                result,
            });
        }
        if let Some(page) = preload
            && live_generation.load(Ordering::Acquire) == generation
        {
            preload_review_neighbors(&page, &cache, &live_generation, generation);
        }
    });
}

fn load_review_page(
    request: &ReviewLoadRequest,
    cache: &Arc<Mutex<ReviewCache>>,
    port: &LiveDesktopPort,
) -> Result<LoadedReviewPage, String> {
    let decks = port.decks()?;
    let models = port.models().unwrap_or_default();
    let deck = choose_deck(&decks, request.preferred_deck.as_deref())
        .ok_or_else(|| "No Anki decks are available".to_owned())?;
    let key = ReviewIndexKey {
        deck: deck.clone(),
        query: request.query.clone(),
    };
    if request.refresh_cache
        && let Ok(mut cache) = cache.lock()
    {
        cache.indices.remove(&key);
        cache.pages.retain(|page, _| page.index != key);
    }
    let cached_ids = cache
        .lock()
        .ok()
        .and_then(|cache| cache.indices.get(&key).cloned());
    let ids = if let Some(ids) = cached_ids {
        ids
    } else {
        let query = review_anki_query(&deck, &request.query);
        let ids = Arc::new(
            port.runtime
                .block_on(port.anki.find_notes(&query))
                .map_err(|error| error.to_string())?,
        );
        if let Ok(mut cache) = cache.lock() {
            cache.indices.insert(key.clone(), ids.clone());
        }
        ids
    };
    let (offset, cursor) = review_position(
        ids.len(),
        request.requested_offset,
        request.requested_cursor,
        request.initial_direction,
    );
    let page_key = ReviewPageKey { index: key, offset };
    let cached_page = cache
        .lock()
        .ok()
        .and_then(|cache| cache.pages.get(&page_key).cloned());
    let notes = if let Some(page) = cached_page {
        page.notes
    } else {
        let notes = fetch_review_page(port, &ids, offset)?;
        if let Ok(mut cache) = cache.lock() {
            cache.pages.insert(
                page_key,
                CachedReviewPage {
                    notes: notes.clone(),
                },
            );
        }
        notes
    };
    Ok(LoadedReviewPage {
        decks,
        models,
        deck,
        query: request.query.clone(),
        ids,
        offset,
        cursor,
        notes,
        select_cursor: request.select_cursor,
    })
}

fn fetch_review_page(
    port: &LiveDesktopPort,
    ids: &[i64],
    offset: usize,
) -> Result<Vec<linguist_application::NoteInfo>, String> {
    let end = (offset + REVIEW_PAGE_SIZE).min(ids.len());
    if offset >= end {
        return Ok(Vec::new());
    }
    let notes = port
        .runtime
        .block_on(port.anki.notes_info(&ids[offset..end]))
        .map_err(|error| error.to_string())?;
    let mut by_id = notes
        .into_iter()
        .map(|note| (note.note_id, note))
        .collect::<HashMap<_, _>>();
    Ok(ids[offset..end]
        .iter()
        .filter_map(|id| by_id.remove(id))
        .collect())
}

fn preload_review_neighbors(
    page: &LoadedReviewPage,
    cache: &Arc<Mutex<ReviewCache>>,
    generation: &AtomicU64,
    expected_generation: u64,
) {
    let offsets = [
        page.offset.checked_sub(REVIEW_PAGE_SIZE),
        (page.offset + REVIEW_PAGE_SIZE < page.ids.len()).then_some(page.offset + REVIEW_PAGE_SIZE),
    ];
    let Ok(port) = LiveDesktopPort::from_environment() else {
        return;
    };
    for offset in offsets.into_iter().flatten() {
        if generation.load(Ordering::Acquire) != expected_generation {
            return;
        }
        let key = ReviewPageKey {
            index: ReviewIndexKey {
                deck: page.deck.clone(),
                query: page.query.clone(),
            },
            offset,
        };
        if cache
            .lock()
            .is_ok_and(|cache| cache.pages.contains_key(&key))
        {
            continue;
        }
        let Ok(notes) = fetch_review_page(&port, &page.ids, offset) else {
            continue;
        };
        if let Ok(mut cache) = cache.lock() {
            cache.pages.insert(key, CachedReviewPage { notes });
        }
    }
}

fn review_position(
    total: usize,
    requested_offset: usize,
    requested_cursor: Option<usize>,
    initial_direction: i32,
) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let cursor = requested_cursor
        .unwrap_or_else(|| if initial_direction < 0 { total - 1 } else { 0 })
        .min(total - 1);
    let last_offset = (total - 1) / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE;
    let offset = if requested_cursor.is_some() {
        cursor / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE
    } else if requested_offset > 0 {
        requested_offset.min(last_offset) / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE
    } else {
        cursor / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE
    };
    (
        offset,
        cursor
            .max(offset)
            .min((offset + REVIEW_PAGE_SIZE - 1).min(total - 1)),
    )
}

fn review_anki_query(deck: &str, query: &str) -> String {
    let deck = deck.replace('\\', "\\\\").replace('"', "\\\"");
    let query = query.trim().replace('\\', "\\\\").replace('"', "\\\"");
    if query.is_empty() {
        format!("deck:\"{deck}\"")
    } else {
        format!("deck:\"{deck}\" \"{query}\"")
    }
}

fn move_review_page(mut qobject: Pin<&mut qobject::AppBackend>, direction: i32) {
    let (deck, query, total, offset) = {
        let binding = qobject.as_ref();
        let state = binding.rust();
        (
            state.review_browser.deck.clone(),
            state.review_browser.query.clone(),
            state.review_browser.ids.len(),
            state.review_browser.offset,
        )
    };
    let next = if direction < 0 {
        offset.saturating_sub(REVIEW_PAGE_SIZE)
    } else {
        (offset + REVIEW_PAGE_SIZE)
            .min(total.saturating_sub(1) / REVIEW_PAGE_SIZE * REVIEW_PAGE_SIZE)
    };
    if deck.is_empty() || total == 0 || next == offset {
        return;
    }
    qobject.as_mut().rust_mut().controller.begin_queue_loading();
    sync_controller_state(qobject.as_mut());
    start_review_load(
        qobject,
        ReviewLoadRequest {
            preferred_deck: Some(deck),
            query,
            requested_offset: next,
            requested_cursor: Some(next),
            initial_direction: direction,
            select_cursor: false,
            check_services: false,
            refresh_cache: false,
        },
    );
}

fn sync_review_browser_state(mut qobject: Pin<&mut qobject::AppBackend>) {
    let (total, offset, cursor, end, searching) = {
        let binding = qobject.as_ref();
        let state = binding.rust();
        let total = state.review_browser.ids.len();
        (
            total,
            state.review_browser.offset,
            state.review_browser.cursor,
            (state.review_browser.offset + state.review_browser.notes.len()).min(total),
            !state.review_browser.query.is_empty(),
        )
    };
    let pages = total.div_ceil(REVIEW_PAGE_SIZE);
    qobject.as_mut().set_review_total(queue_len(total));
    qobject.as_mut().set_review_page(if total == 0 {
        0
    } else {
        queue_len(offset / REVIEW_PAGE_SIZE + 1)
    });
    qobject.as_mut().set_review_page_count(queue_len(pages));
    qobject
        .as_mut()
        .set_review_page_start(if total == 0 { 0 } else { queue_len(offset + 1) });
    qobject.as_mut().set_review_page_end(queue_len(end));
    qobject
        .as_mut()
        .set_review_search_count(if searching { queue_len(total) } else { 0 });
    qobject
        .as_mut()
        .set_review_search_match(if searching && total > 0 {
            queue_len(cursor + 1)
        } else {
            0
        });
    let serial = qobject.as_ref().review_navigation_serial().wrapping_add(1);
    qobject.set_review_navigation_serial(serial);
}

struct LiveDesktopPort {
    runtime: tokio::runtime::Runtime,
    anki: linguist_anki::AnkiConnectTransport,
    ollama: linguist_ollama::OllamaClient,
}
impl LiveDesktopPort {
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
        })
    }
    fn decks(&self) -> Result<Vec<String>, String> {
        self.runtime
            .block_on(self.anki.deck_names())
            .map(|decks| decks.into_iter().map(|deck| deck.0).collect())
            .map_err(|error| error.to_string())
    }
    fn models(&self) -> Result<Vec<String>, String> {
        self.runtime
            .block_on(self.anki.model_names())
            .map(|models| models.into_iter().map(|model| model.0).collect())
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
        let preview = self
            .runtime
            .block_on(self.anki.preview(selector, usize::MAX))
            .map_err(|error| error.to_string())?;
        Ok(preview
            .notes
            .into_iter()
            .map(|note| linguist_jobs::BatchItemSeed {
                note_id: note.note_id,
                word: note.expression,
            })
            .collect())
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
        choose_deck(&self.decks()?, None).ok_or_else(|| "No Anki decks are available".into())
    }
    fn review_queue(&self) -> Result<ReviewQueueData, String> {
        let decks = self.decks()?;
        let Some(deck) = choose_deck(&decks, None) else {
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
        ids.truncate(REVIEW_PAGE_SIZE);
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
            self.image_with_dictionary(expression, deck_key, &Default::default())
                .await
        })
    }

    fn image_with_dictionary<'a>(
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
            use std::sync::{Arc, atomic::AtomicBool};
            let hint = dictionary
                .definition
                .split([';', '\n'])
                .map(str::trim)
                .filter(|term| !term.is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            let search = self.image_search.with_meaning_hint(&hint);
            let result = linguist_media::discover_image(
                &search,
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
            self.apply_existing_image_policy(draft, &mut document, deck_key)
                .await;
            Ok(document)
        })
    }

    async fn apply_existing_image_policy(
        &self,
        draft: &crate::draft::ReviewDraft,
        document: &mut CardDocument,
        deck_key: &str,
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
            let evidence = self.ocr.recognize_bytes(
                &bytes,
                self.ocr_languages(deck_key),
                Arc::new(AtomicBool::new(false)),
            );
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

    fn ocr_languages<'a>(&'a self, deck_key: &str) -> &'a str {
        deck_ocr_languages(&self.decks, deck_key)
    }
}

fn deck_ocr_languages<'a>(
    decks: &'a BTreeMap<String, linguist_config::DeckConfig>,
    deck_key: &str,
) -> &'a str {
    decks
        .get(deck_key)
        .map(|deck| deck.ocr_languages.trim())
        .filter(|languages| !languages.is_empty())
        .unwrap_or_else(|| match deck_key.split('_').next().unwrap_or_default() {
            "japanese" => "jpn+eng+vie",
            "taiwanese" => "chi_tra+eng+vie",
            "german" => "deu+eng",
            _ => "eng",
        })
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
        if let Some(examples) = &document.values.examples {
            changes.push(crate::draft::GeneratedChange {
                field: crate::draft::DraftField::Examples,
                value: examples.clone(),
                provenance: "Ollama grammar examples".into(),
            });
        }
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
            examples: Some(draft.examples.clone()),
            kanji_construction: Some(draft.kanji.clone()),
            audio: Some(draft.audio.join("<br/>")),
        };
        let config = runtime_config()?;
        let configured_entry = config.decks.iter().find(|(key, configured)| {
            key.as_str() == draft.deck_name
                || configured.deck_name.as_deref() == Some(draft.deck_name.as_str())
        });
        let purpose = configured_entry
            .map(|(key, _)| key.as_str())
            .or(draft.language_key.as_deref())
            .unwrap_or("japanese_vocab");
        let deck_config = configured_entry.map(|(_, configured)| configured);
        let managed_spec = linguist_core::managed_vocab_spec(purpose);
        let managed_mapping = managed_field_mapping(purpose);
        let (deck_name, mut target_model, source, mut mapping, tags) = match draft.mode {
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
        // Source mappings describe legacy input. Vocabulary output always uses
        // its own managed schema, never the legacy physical field names.
        if matches!(purpose, "japanese_vocab" | "english_vocab") {
            target_model = managed_spec.model_name.clone();
            mapping = managed_field_mapping(purpose);
        }
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
                .block_on(self.commit.managed_template_plan(purpose))
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

fn managed_field_mapping(purpose: &str) -> FieldMapping {
    FieldMapping {
        expression: Some("Expression".into()),
        meaning_image: (purpose != "japanese_grammar").then(|| "Picture".into()),
        meaning_text: Some(if purpose == "japanese_grammar" {
            "Explanation".into()
        } else {
            "Meaning".into()
        }),
        examples: (purpose == "japanese_grammar").then(|| "Examples".into()),
        kanji_construction: (purpose == "japanese_vocab").then(|| "Kanji".into()),
        audio: (purpose != "japanese_grammar").then(|| "Audio".into()),
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
        examples: field("examples"),
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
        examples: find(&["Examples", "Example Sentences"]),
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
    fn review_pages_clamp_offsets_and_search_directions() {
        assert_eq!(review_position(0, 500, Some(500), 1), (0, 0));
        assert_eq!(review_position(250, 0, Some(135), 1), (100, 135));
        assert_eq!(review_position(250, 999, None, 1), (200, 200));
        assert_eq!(review_position(250, 0, None, -1), (200, 249));
    }

    #[test]
    fn review_search_queries_cover_the_whole_deck_and_escape_input() {
        assert_eq!(review_anki_query("Japanese", ""), "deck:\"Japanese\"");
        assert_eq!(
            review_anki_query("A \\\"deck", "say \\\"hi"),
            "deck:\"A \\\\\\\"deck\" \"say \\\\\\\"hi\""
        );
    }

    #[test]
    fn reusable_field_mapping_keeps_schema_keys_and_source_names() {
        let editor = FieldMappingEditor {
            sources: vec![
                ("Word".into(), "猫".into()),
                ("Picture".into(), "<img src='cat.jpg'>".into()),
                ("Definition".into(), "cat".into()),
            ],
            connections: vec![Some(0), Some(1), Some(2), None, None],
            ..Default::default()
        };
        assert_eq!(
            mapping_fields(&editor),
            BTreeMap::from([
                ("expression".into(), "Word".into()),
                ("meaning_image".into(), "Picture".into()),
                ("meaning_text".into(), "Definition".into()),
            ])
        );
    }

    #[test]
    fn english_mapping_omits_kanji_and_orders_sources_by_schema() {
        let fields = BTreeMap::from([
            ("Extra".into(), "x".into()),
            ("Picture".into(), "image".into()),
            ("Pronunciation".into(), "audio".into()),
            ("Word".into(), "term".into()),
            ("Definition".into(), "meaning".into()),
        ]);
        let mapping = BTreeMap::from([
            ("expression".into(), "Word".into()),
            ("meaning_image".into(), "Picture".into()),
            ("meaning_text".into(), "Definition".into()),
            ("audio".into(), "Pronunciation".into()),
        ]);
        assert_eq!(
            target_fields("english_vocab")
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            ["expression", "meaning_image", "meaning_text", "audio"]
        );
        assert_eq!(
            ordered_mapping_sources(&fields, &mapping)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["Word", "Picture", "Definition", "Pronunciation", "Extra"]
        );
        assert_eq!(
            target_fields("japanese_grammar")
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            ["expression", "meaning_text", "examples"]
        );
        let grammar = managed_field_mapping("japanese_grammar");
        assert_eq!(grammar.meaning_text.as_deref(), Some("Explanation"));
        assert_eq!(grammar.examples.as_deref(), Some("Examples"));
        assert_eq!(grammar.meaning_image, None);
        assert_eq!(grammar.audio, None);
        assert_eq!(grammar.kanji_construction, None);
    }

    #[test]
    fn mapping_prompt_contains_bounded_samples_and_exact_schema() {
        let editor = FieldMappingEditor {
            purpose: "english_vocab".into(),
            deck: "Moonlit Manuscripts".into(),
            model: "2. Picture Words".into(),
            sources: vec![("Word".into(), format!("<b>{}</b>", "x".repeat(600)))],
            ..Default::default()
        };
        let prompt = mapping_reasoning_prompt(&editor);
        assert!(prompt.contains("Moonlit Manuscripts"));
        assert!(prompt.contains("meaning_image"));
        assert!(!prompt.contains("kanji_construction"));
        assert!(prompt.len() < 1_800);
        assert_eq!(
            choose_reasoning_model(&["gemma".into(), "qwen3:8b".into()]),
            Some("qwen3:8b".into())
        );
    }

    #[test]
    fn local_defaults_fill_gaps_without_overwriting_user_mapping() {
        let mut config = linguist_config::NativeConfig::default();
        config.decks.insert(
            "english_vocab".into(),
            linguist_config::DeckConfig {
                deck_name: Some("My deck".into()),
                fields: BTreeMap::from([("expression".into(), "Term".into())]),
                ..Default::default()
            },
        );
        merge_mapping_defaults(
            &mut config,
            BTreeMap::from([(
                "english_vocab".into(),
                linguist_config::DeckConfig {
                    deck_name: Some("Default deck".into()),
                    model_name: Some("Model".into()),
                    fields: BTreeMap::from([
                        ("expression".into(), "Word".into()),
                        ("audio".into(), "Sound".into()),
                    ]),
                    ..Default::default()
                },
            )]),
        );
        let merged = &config.decks["english_vocab"];
        assert_eq!(merged.deck_name.as_deref(), Some("My deck"));
        assert_eq!(merged.model_name.as_deref(), Some("Model"));
        assert_eq!(merged.fields["expression"], "Term");
        assert_eq!(merged.fields["audio"], "Sound");
    }

    #[test]
    fn field_renderer_embeds_local_media_and_blocks_active_remote_markup() {
        let markup = embed_media_markup(
            safe_field_markup(
                "<script>bad()</script><a href='https://bad.test'>x</a><img src='cat.png'>",
            ),
            &HashMap::from([("cat.png".into(), "data:image/png;base64,Y2F0".into())]),
        );
        assert!(!markup.contains("<script"));
        assert!(!markup.contains("https://bad.test"));
        assert!(markup.contains("data:image/png;base64,Y2F0"));
        assert!(!strip_image_tags(&markup).contains("<img"));
        assert_eq!(
            local_image_names("<img src=\"safe.webp\"><img src='../bad'>"),
            ["safe.webp"]
        );
    }

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
    fn imported_ocr_languages_override_safe_family_defaults() {
        let decks = BTreeMap::from([(
            "japanese_vocab".into(),
            linguist_config::DeckConfig {
                ocr_languages: "jpn+eng".into(),
                ..Default::default()
            },
        )]);
        assert_eq!(deck_ocr_languages(&decks, "japanese_vocab"), "jpn+eng");
        assert_eq!(
            deck_ocr_languages(&decks, "taiwanese_vocab"),
            "chi_tra+eng+vie"
        );
        assert_eq!(deck_ocr_languages(&decks, "german_vocab"), "deu+eng");
        assert_eq!(deck_ocr_languages(&decks, "english_vocab"), "eng");
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
    fn selector_uses_purpose_key_and_python_compatible_settings() {
        let config = Ok(linguist_config::NativeConfig {
            decks: BTreeMap::from([(
                "japanese_vocab".into(),
                linguist_config::DeckConfig {
                    deck_name: Some("Japanese Words".into()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        let selector = BatchSelector {
            deck: Some("Japanese Words".into()),
            created_after: Some("2026-09-01".into()),
            created_before: Some("2026-09-21".into()),
            tags: vec!["jlpt_n3".into()],
            excluded_tags: vec!["blocked".into()],
            image: linguist_application::ImageFilter::HasImage,
            limit: 50,
            ..Default::default()
        };
        assert_eq!(
            selector_job_identity(&selector, &config).unwrap(),
            ("japanese_vocab".into(), "Japanese Words".into())
        );
        let legacy = python_selection(&selector, "japanese_vocab");
        assert_eq!(legacy["deck_key"], "japanese_vocab");
        assert_eq!(legacy["required_tags"][0], "jlpt_n3");
        assert_eq!(legacy["excluded_tags"][0], "blocked");
        assert_eq!(legacy["media_scope"], "with");
        assert_eq!(legacy["limit"], 50);
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
