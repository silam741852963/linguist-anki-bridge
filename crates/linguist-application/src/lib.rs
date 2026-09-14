//! Use-case boundaries for the native rewrite.
//!
//! Adapters for AnkiConnect, dictionaries, OCR, Ollama, media, and SQLite
//! implement these ports. The Qt layer consumes application events and does
//! not call providers directly.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use linguist_core::{
    CardDocument, CardMode, FieldMapping, ManagedTemplatePlan, ModelTemplate, ObservedModel,
    SnapshotDocument, SnapshotOriginalNote, normalize_expression,
};
use serde::{Deserialize, Serialize};

mod ingestion;
mod selector;
pub use ingestion::{
    CsvColumnMapping, CsvIngestRequest, CsvPreview, DuplicateDecision, IngestionPreview, InputRow,
    ManualIngestRequest, RowIssue, prepare_csv_input, prepare_manual_input,
    resolve_ingestion_preview,
};
pub use selector::{
    BatchSelector, CompletionFilter, ImageFilter, SelectorMetadataPort, SelectorPreview,
    selector_preview,
};

pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, PortError>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortError {
    pub operation: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for PortError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for PortError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSummary {
    pub note_id: i64,
    pub expression: String,
    pub deck_key: String,
    pub model_name: String,
}

/// Read-only collection data exposed to use cases and eventually to the GUI.
/// Adapter-specific wire shapes must not cross this boundary.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeckName(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelName(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelFields {
    pub model_name: ModelName,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardTemplate {
    pub name: String,
    pub front: String,
    pub back: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelTemplates {
    pub model_name: ModelName,
    pub templates: Vec<CardTemplate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelStyling {
    pub model_name: ModelName,
    pub css: String,
}

pub fn observe_model(
    fields: &ModelFields,
    templates: &ModelTemplates,
    styling: &ModelStyling,
) -> Result<ObservedModel, String> {
    if fields.model_name != templates.model_name || fields.model_name != styling.model_name {
        return Err("model fields, templates, and styling names differ".into());
    }
    Ok(ObservedModel {
        model_name: fields.model_name.0.clone(),
        fields: fields.fields.clone(),
        templates: templates
            .templates
            .iter()
            .map(|template| ModelTemplate {
                name: template.name.clone(),
                front: template.front.clone(),
                back: template.back.clone(),
            })
            .collect(),
        css: styling.css.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteInfo {
    pub note_id: i64,
    pub model_name: ModelName,
    pub deck_names: Vec<DeckName>,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaFile {
    pub filename: String,
    pub data_base64: String,
}

pub trait MediaPort: Send + Sync {
    fn retrieve_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, Option<MediaFile>>;
    fn store_media<'a>(&'a self, media: &'a MediaFile) -> PortFuture<'a, ()>;
    fn delete_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, ()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaChange {
    pub filename: String,
    pub data_base64: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MediaPlan {
    pub stage: Vec<MediaChange>,
    pub remove_obsolete: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMediaTransaction {
    staged: Vec<MediaFile>,
    obsolete: Vec<String>,
    before: BTreeMap<String, Option<MediaFile>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaTransactionError {
    pub phase: &'static str,
    pub message: String,
    pub rollback_errors: Vec<String>,
}

impl std::fmt::Display for MediaTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "media {}: {}", self.phase, self.message)
    }
}

impl std::error::Error for MediaTransactionError {}

impl PreparedMediaTransaction {
    pub async fn prepare<P: MediaPort + ?Sized>(
        port: &P,
        plan: MediaPlan,
    ) -> Result<Self, MediaTransactionError> {
        let mut staged = BTreeMap::<String, String>::new();
        for change in plan.stage {
            validate_media(&change)?;
            match staged.get(&change.filename) {
                Some(existing) if existing != &change.data_base64 => {
                    return Err(transaction_error(
                        "validate",
                        format!("conflicting media {}", change.filename),
                    ));
                }
                Some(_) => {}
                None => {
                    staged.insert(change.filename, change.data_base64);
                }
            }
        }
        let obsolete = plan
            .remove_obsolete
            .into_iter()
            .filter(|filename| !filename.is_empty())
            .collect::<BTreeSet<_>>();
        if let Some(filename) = obsolete
            .iter()
            .find(|filename| staged.contains_key(*filename))
        {
            return Err(transaction_error(
                "validate",
                format!("media {filename} cannot be staged and removed"),
            ));
        }
        let staged = staged
            .into_iter()
            .map(|(filename, data_base64)| MediaFile {
                filename,
                data_base64,
            })
            .collect::<Vec<_>>();
        let obsolete = obsolete.into_iter().collect::<Vec<_>>();
        let mut before = BTreeMap::new();
        for filename in staged
            .iter()
            .map(|media| media.filename.as_str())
            .chain(obsolete.iter().map(String::as_str))
        {
            let existing = port
                .retrieve_media(filename)
                .await
                .map_err(|error| transaction_error("snapshot", format!("{filename}: {error}")))?;
            before.insert(filename.into(), existing);
        }
        Ok(Self {
            staged,
            obsolete,
            before,
        })
    }

    pub fn before(&self) -> &BTreeMap<String, Option<MediaFile>> {
        &self.before
    }

    pub async fn commit<P: MediaPort + ?Sized>(
        &self,
        port: &P,
    ) -> Result<(), MediaTransactionError> {
        let mut touched = Vec::new();
        for media in &self.staged {
            touched.push(media.filename.clone());
            if let Err(error) = port.store_media(media).await {
                return Err(self
                    .rollback_after(port, touched, "stage", error.to_string())
                    .await);
            }
            match port.retrieve_media(&media.filename).await {
                Ok(Some(stored)) if same_media(&stored.data_base64, &media.data_base64) => {}
                Ok(Some(_)) => {
                    return Err(self
                        .rollback_after(
                            port,
                            touched,
                            "verify",
                            format!("{} content differs", media.filename),
                        )
                        .await);
                }
                Ok(None) => {
                    return Err(self
                        .rollback_after(
                            port,
                            touched,
                            "verify",
                            format!("{} missing after store", media.filename),
                        )
                        .await);
                }
                Err(error) => {
                    return Err(self
                        .rollback_after(port, touched, "verify", error.to_string())
                        .await);
                }
            }
        }
        for filename in &self.obsolete {
            touched.push(filename.clone());
            if let Err(error) = port.delete_media(filename).await {
                return Err(self
                    .rollback_after(port, touched, "remove", error.to_string())
                    .await);
            }
        }
        Ok(())
    }

    /// Mutation phase for the card's new media. Caller owns rollback after
    /// this succeeds, so note/model work can happen before obsolete removal.
    pub async fn stage_and_verify<P: MediaPort + ?Sized>(
        &self,
        port: &P,
    ) -> Result<(), MediaTransactionError> {
        for media in &self.staged {
            port.store_media(media)
                .await
                .map_err(|error| transaction_error("stage", error.to_string()))?;
            match port.retrieve_media(&media.filename).await {
                Ok(Some(stored)) if same_media(&stored.data_base64, &media.data_base64) => {}
                Ok(Some(_)) => {
                    return Err(transaction_error(
                        "verify",
                        format!("{} content differs", media.filename),
                    ));
                }
                Ok(None) => {
                    return Err(transaction_error(
                        "verify",
                        format!("{} missing after store", media.filename),
                    ));
                }
                Err(error) => return Err(transaction_error("verify", error.to_string())),
            }
        }
        Ok(())
    }

    /// Final media phase. Call only after snapshot finalization succeeds.
    pub async fn remove_obsolete<P: MediaPort + ?Sized>(
        &self,
        port: &P,
    ) -> Result<(), MediaTransactionError> {
        for filename in &self.obsolete {
            port.delete_media(filename)
                .await
                .map_err(|error| transaction_error("remove", error.to_string()))?;
        }
        Ok(())
    }

    pub async fn rollback<P: MediaPort + ?Sized>(
        &self,
        port: &P,
    ) -> Result<(), MediaTransactionError> {
        let names = self.before.keys().cloned().collect();
        let errors = self.restore(port, names).await;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(MediaTransactionError {
                phase: "rollback",
                message: "could not restore all media".into(),
                rollback_errors: errors,
            })
        }
    }

    async fn rollback_after<P: MediaPort + ?Sized>(
        &self,
        port: &P,
        touched: Vec<String>,
        phase: &'static str,
        message: String,
    ) -> MediaTransactionError {
        let errors = self.restore(port, touched).await;
        MediaTransactionError {
            phase,
            message,
            rollback_errors: errors,
        }
    }

    async fn restore<P: MediaPort + ?Sized>(
        &self,
        port: &P,
        mut names: Vec<String>,
    ) -> Vec<String> {
        names.reverse();
        names.dedup();
        let mut errors = Vec::new();
        for filename in names {
            let result = match self.before.get(&filename).cloned().flatten() {
                Some(media) => port.store_media(&media).await,
                None => port.delete_media(&filename).await,
            };
            if let Err(error) = result {
                errors.push(format!("{filename}: {error}"));
            }
        }
        errors
    }
}

fn validate_media(change: &MediaChange) -> Result<(), MediaTransactionError> {
    if change.filename.is_empty() || change.filename.contains('/') || change.filename.contains('\\')
    {
        return Err(transaction_error("validate", "unsafe media filename"));
    }
    if change.data_base64.is_empty() || STANDARD.decode(&change.data_base64).is_err() {
        return Err(transaction_error(
            "validate",
            format!("invalid base64 for {}", change.filename),
        ));
    }
    Ok(())
}

fn same_media(left: &str, right: &str) -> bool {
    STANDARD.decode(left).ok() == STANDARD.decode(right).ok()
}

fn transaction_error(phase: &'static str, message: impl Into<String>) -> MediaTransactionError {
    MediaTransactionError {
        phase,
        message: message.into(),
        rollback_errors: vec![],
    }
}

/// A caller-provided expression plus the fields that semantically represent it
/// in a legacy note type. The application owns this policy so every importer
/// reaches the same modernization/injection decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactExpressionRequest {
    pub deck_key: String,
    pub expression: String,
    pub preferred_fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpressionResolution {
    Inject {
        deck_key: String,
        expression: String,
    },
    Modernize {
        deck_key: String,
        expression: String,
        note: NoteInfo,
    },
    /// Never choose an arbitrary duplicate note. The caller can surface this
    /// to the user or apply an explicit import policy later.
    Ambiguous {
        deck_key: String,
        expression: String,
        matches: Vec<NoteInfo>,
    },
}

/// Resolve a local exact match after an adapter has performed its indexed
/// candidate search. HTML markup and whitespace cannot create false matches.
pub fn resolve_exact_expression(
    request: &ExactExpressionRequest,
    candidates: impl IntoIterator<Item = NoteInfo>,
) -> ExpressionResolution {
    let expression = normalize_expression(&request.expression);
    let matches = if expression.is_empty() {
        Vec::new()
    } else {
        candidates
            .into_iter()
            .filter(|note| note_matches_expression(note, &expression, &request.preferred_fields))
            .collect::<Vec<_>>()
    };
    match matches.len() {
        0 => ExpressionResolution::Inject {
            deck_key: request.deck_key.clone(),
            expression,
        },
        1 => ExpressionResolution::Modernize {
            deck_key: request.deck_key.clone(),
            expression,
            note: matches.into_iter().next().expect("one checked match"),
        },
        _ => ExpressionResolution::Ambiguous {
            deck_key: request.deck_key.clone(),
            expression,
            matches,
        },
    }
}

fn note_matches_expression(note: &NoteInfo, expression: &str, preferred_fields: &[String]) -> bool {
    let mut names = preferred_fields
        .iter()
        .filter(|name| note.fields.contains_key(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for name in note.fields.keys() {
        let lower = name.to_lowercase();
        if !names.contains(name)
            && ["expression", "word", "front", "vocab"]
                .iter()
                .any(|token| lower.contains(token))
        {
            names.push(name.clone());
        }
    }
    if names.is_empty() {
        names.extend(note.fields.keys().cloned());
    }
    names.into_iter().any(|name| {
        note.fields
            .get(&name)
            .is_some_and(|value| normalize_expression(value) == expression)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceNote {
    pub summary: NoteSummary,
    pub fields: Vec<(String, String)>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReceipt {
    pub note_id: i64,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitSource {
    pub note_id: i64,
    pub model_name: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRequest {
    pub mode: CardMode,
    pub dry_run: bool,
    pub deck_key: String,
    pub deck_name: String,
    pub target_model: String,
    pub source: Option<CommitSource>,
    pub document: CardDocument,
    pub field_mapping: FieldMapping,
    pub template_plan: ManagedTemplatePlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotCapture {
    pub word: String,
    pub mode: CardMode,
    pub deck_key: String,
    pub source: Option<CommitSource>,
    pub document: CardDocument,
    pub media_before: BTreeMap<String, Option<MediaFile>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotHandle(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateMutation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteMutation {
    pub note_id: i64,
    pub created: bool,
}

pub trait CommitPort: MediaPort {
    fn backup_deck<'a>(&'a self, deck_name: &'a str) -> PortFuture<'a, ()>;
    fn capture_snapshot<'a>(
        &'a self,
        capture: &'a SnapshotCapture,
    ) -> PortFuture<'a, SnapshotHandle>;
    fn finalize_snapshot<'a>(
        &'a self,
        snapshot: &'a SnapshotHandle,
        post_write_state: &'a PostWriteState,
    ) -> PortFuture<'a, ()>;
    fn fail_snapshot<'a>(
        &'a self,
        snapshot: &'a SnapshotHandle,
        error: &'a str,
    ) -> PortFuture<'a, ()>;
    fn apply_template<'a>(
        &'a self,
        plan: &'a ManagedTemplatePlan,
    ) -> PortFuture<'a, TemplateMutation>;
    fn rollback_template<'a>(&'a self, mutation: &'a TemplateMutation) -> PortFuture<'a, ()>;
    fn update_note<'a>(
        &'a self,
        source: &'a CommitSource,
        fields: &'a BTreeMap<String, String>,
        target_model: &'a str,
    ) -> PortFuture<'a, NoteMutation>;
    fn create_note<'a>(
        &'a self,
        deck_name: &'a str,
        model_name: &'a str,
        fields: &'a BTreeMap<String, String>,
        tags: &'a [String],
    ) -> PortFuture<'a, NoteMutation>;
    fn rollback_note<'a>(
        &'a self,
        mutation: &'a NoteMutation,
        source: Option<&'a CommitSource>,
    ) -> PortFuture<'a, ()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    DryRun { fields: BTreeMap<String, String> },
    Committed(CommitReceipt),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitError {
    pub phase: &'static str,
    pub message: String,
    pub rollback_errors: Vec<String>,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "commit {}: {}", self.phase, self.message)
    }
}

impl std::error::Error for CommitError {}

pub async fn commit_card<P: CommitPort + ?Sized>(
    port: &P,
    request: CommitRequest,
) -> Result<CommitOutcome, CommitError> {
    let fields = validate_commit(&request)?;
    if request.dry_run {
        return Ok(CommitOutcome::DryRun { fields });
    }
    port.backup_deck(&request.deck_name)
        .await
        .map_err(|error| commit_error("backup", error.to_string()))?;

    let media = PreparedMediaTransaction::prepare(
        port,
        MediaPlan {
            stage: request
                .document
                .media
                .iter()
                .map(|asset| MediaChange {
                    filename: asset.filename.clone(),
                    data_base64: asset.data_base64.clone(),
                })
                .collect(),
            remove_obsolete: request.document.obsolete_media.clone(),
        },
    )
    .await
    .map_err(|error| commit_error(error.phase, error.message))?;
    let snapshot = port
        .capture_snapshot(&SnapshotCapture {
            word: request.document.expression.clone(),
            mode: request.mode,
            deck_key: request.deck_key.clone(),
            source: request.source.clone(),
            document: request.document.clone(),
            media_before: media.before().clone(),
        })
        .await
        .map_err(|error| commit_error("snapshot", error.to_string()))?;

    let mut template_mutation = None;
    let mut note_mutation = None;
    let result = async {
        media
            .stage_and_verify(port)
            .await
            .map_err(|error| commit_error(error.phase, error.message))?;
        template_mutation = Some(
            port.apply_template(&request.template_plan)
                .await
                .map_err(|error| commit_error("template", error.to_string()))?,
        );
        let mutation = match request.mode {
            CardMode::Modernize => {
                let source = request.source.as_ref().ok_or_else(|| {
                    commit_error("validate", "modernization requires source note")
                })?;
                port.update_note(source, &fields, &request.target_model)
                    .await
                    .map_err(|error| commit_error("note", error.to_string()))?
            }
            CardMode::Inject => port
                .create_note(
                    &request.deck_name,
                    &request.target_model,
                    &fields,
                    &request.document.tags,
                )
                .await
                .map_err(|error| commit_error("note", error.to_string()))?,
        };
        note_mutation = Some(mutation.clone());
        let post_write_state = PostWriteState {
            note: PostWriteNote {
                note_id: mutation.note_id,
                model_name: request.target_model.clone(),
                deck_name: request.deck_name.clone(),
                fields: fields.clone(),
                tags: match request.mode {
                    CardMode::Modernize => request
                        .source
                        .as_ref()
                        .map(|source| source.tags.clone())
                        .unwrap_or_default(),
                    CardMode::Inject => request.document.tags.clone(),
                },
            },
            media: request
                .document
                .media
                .iter()
                .map(|asset| (asset.filename.clone(), Some(asset.data_base64.clone())))
                .chain(
                    request
                        .document
                        .obsolete_media
                        .iter()
                        .cloned()
                        .map(|filename| (filename, None)),
                )
                .collect(),
        };
        port.finalize_snapshot(&snapshot, &post_write_state)
            .await
            .map_err(|error| commit_error("finalize snapshot", error.to_string()))?;
        media
            .remove_obsolete(port)
            .await
            .map_err(|error| commit_error(error.phase, error.message))?;
        Ok::<CommitOutcome, CommitError>(CommitOutcome::Committed(CommitReceipt {
            note_id: mutation.note_id,
            snapshot_id: snapshot.0.clone(),
        }))
    }
    .await;
    match result {
        Ok(outcome) => Ok(outcome),
        Err(mut error) => {
            if let Some(mutation) = note_mutation.as_ref() {
                if let Err(rollback) = port.rollback_note(mutation, request.source.as_ref()).await {
                    error.rollback_errors.push(format!("note: {rollback}"));
                }
            }
            if let Some(mutation) = template_mutation.as_ref() {
                if let Err(rollback) = port.rollback_template(mutation).await {
                    error.rollback_errors.push(format!("template: {rollback}"));
                }
            }
            if let Err(rollback) = media.rollback(port).await {
                error.rollback_errors.extend(rollback.rollback_errors);
            }
            if let Err(rollback) = port.fail_snapshot(&snapshot, &error.message).await {
                error.rollback_errors.push(format!("snapshot: {rollback}"));
            }
            Err(error)
        }
    }
}

fn validate_commit(request: &CommitRequest) -> Result<BTreeMap<String, String>, CommitError> {
    if !request.document.ready() {
        return Err(commit_error("validate", "card document is not ready"));
    }
    if request.mode == CardMode::Modernize && request.source.is_none() {
        return Err(commit_error(
            "validate",
            "modernization requires source note",
        ));
    }
    request
        .document
        .map_fields(&request.field_mapping)
        .map_err(|error| commit_error("validate", error.to_string()))
}

fn commit_error(phase: &'static str, message: impl Into<String>) -> CommitError {
    CommitError {
        phase,
        message: message.into(),
        rollback_errors: vec![],
    }
}

/// Snapshot extension populated by native commit adapters. It records the
/// state produced by the commit, so restore never overwrites a later edit.
pub const NATIVE_POST_WRITE_EXTENSION: &str = "native_post_write_v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PostWriteNote {
    pub note_id: i64,
    pub model_name: String,
    pub deck_name: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PostWriteState {
    pub note: PostWriteNote,
    /// Every file changed by the commit, after the successful write. `None`
    /// represents obsolete media that was removed.
    pub media: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreOutcome {
    DryRun,
    AlreadyRestored,
    Restored {
        restored_note_id: Option<i64>,
        removed_note_ids: Vec<i64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreError {
    Invalid(String),
    Conflict { note_id: i64, reason: String },
    Port(PortError),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid snapshot: {message}"),
            Self::Conflict { note_id, reason } => {
                write!(
                    formatter,
                    "newer write conflicts with note {note_id}: {reason}"
                )
            }
            Self::Port(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RestoreError {}

impl From<PortError> for RestoreError {
    fn from(error: PortError) -> Self {
        Self::Port(error)
    }
}

/// Collection mutations needed to restore a versioned snapshot. The adapter
/// must make `restore_note` restore model, deck, fields, and tags as one
/// collection-level operation.
pub trait RestorePort: MediaPort {
    fn snapshot_restored<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, bool>;
    fn note_info<'a>(&'a self, note_id: i64) -> PortFuture<'a, Option<NoteInfo>>;
    fn delete_notes<'a>(&'a self, note_ids: &'a [i64]) -> PortFuture<'a, ()>;
    fn restore_note<'a>(&'a self, original: &'a SnapshotOriginalNote) -> PortFuture<'a, ()>;
    fn mark_snapshot_restored<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, ()>;
}

/// Restore a Python or native snapshot without touching unrelated collection
/// state. Native snapshots additionally reject a note that no longer equals
/// the state recorded immediately after commit.
pub async fn restore_snapshot<P: RestorePort + ?Sized>(
    port: &P,
    document: &SnapshotDocument,
) -> Result<RestoreOutcome, RestoreError> {
    let snapshot = &document.snapshot;
    if document.schema_version != linguist_core::CONTRACT_VERSION {
        return Err(RestoreError::Invalid("unsupported schema version".into()));
    }
    if snapshot.dry_run {
        return Ok(RestoreOutcome::DryRun);
    }
    if snapshot.status == "reverted" || port.snapshot_restored(&snapshot.id).await? {
        return Ok(RestoreOutcome::AlreadyRestored);
    }

    let original = snapshot.original_note.as_ref();
    let original_note_id = original.and_then(|note| note.note_id);
    let result_note_id = snapshot.result_note_id;
    if original.is_some() && original_note_id.is_none() {
        return Err(RestoreError::Invalid("original note has no note id".into()));
    }
    if original_note_id.is_none() && result_note_id.is_none() {
        return Err(RestoreError::Invalid(
            "no original or committed note id".into(),
        ));
    }

    if let Some(expected) = post_write_state(document)? {
        let current = port
            .note_info(expected.note.note_id)
            .await?
            .ok_or_else(|| RestoreError::Conflict {
                note_id: expected.note.note_id,
                reason: "note no longer exists".into(),
            })?;
        ensure_post_write_matches(&current, &expected.note)?;
        for (filename, expected_media) in &expected.media {
            let current_media = port.retrieve_media(filename).await?;
            let matches = match (expected_media, current_media) {
                (Some(expected), Some(current)) => same_media(expected, &current.data_base64),
                (None, None) => true,
                _ => false,
            };
            if !matches {
                return Err(RestoreError::Conflict {
                    note_id: expected.note.note_id,
                    reason: format!("media changed: {filename}"),
                });
            }
        }
    } else if let Some(note_id) = original_note_id {
        if port.note_info(note_id).await?.is_none() {
            return Err(RestoreError::Conflict {
                note_id,
                reason: "original note no longer exists".into(),
            });
        }
    }

    let mut deleted = BTreeSet::new();
    if original.is_none() {
        if let Some(note_id) = result_note_id {
            deleted.insert(note_id);
        }
    }
    deleted.extend(snapshot.created_note_ids.iter().copied());
    let removed_note_ids: Vec<_> = deleted.into_iter().collect();
    if !removed_note_ids.is_empty() {
        port.delete_notes(&removed_note_ids).await?;
    }

    for (filename, previous) in &snapshot.media_before {
        match previous {
            Some(data_base64) => {
                port.store_media(&MediaFile {
                    filename: filename.clone(),
                    data_base64: data_base64.clone(),
                })
                .await?;
            }
            None => port.delete_media(filename).await?,
        }
    }
    if let Some(original) = original {
        port.restore_note(original).await?;
    }
    port.mark_snapshot_restored(&snapshot.id).await?;
    Ok(RestoreOutcome::Restored {
        restored_note_id: original_note_id,
        removed_note_ids,
    })
}

fn post_write_state(document: &SnapshotDocument) -> Result<Option<PostWriteState>, RestoreError> {
    document
        .snapshot
        .extensions
        .get(NATIVE_POST_WRITE_EXTENSION)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                RestoreError::Invalid(format!("invalid native post-write state: {error}"))
            })
        })
        .transpose()
}

fn ensure_post_write_matches(
    current: &NoteInfo,
    expected: &PostWriteNote,
) -> Result<(), RestoreError> {
    let same_deck = current
        .deck_names
        .iter()
        .any(|deck| deck.0 == expected.deck_name);
    let same_tags: BTreeSet<_> = current.tags.iter().collect();
    let expected_tags: BTreeSet<_> = expected.tags.iter().collect();
    let reason = if current.model_name.0 != expected.model_name {
        Some("model changed")
    } else if !same_deck {
        Some("deck changed")
    } else if current.fields != expected.fields {
        Some("fields changed")
    } else if same_tags != expected_tags {
        Some("tags changed")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(RestoreError::Conflict {
            note_id: expected.note_id,
            reason: reason.into(),
        }),
        None => Ok(()),
    }
}

pub trait AnkiPort: Send + Sync {
    fn note<'a>(&'a self, note_id: i64) -> PortFuture<'a, SourceNote>;
    fn find_exact<'a>(
        &'a self,
        deck_key: &'a str,
        expression: &'a str,
    ) -> PortFuture<'a, Option<SourceNote>>;
    fn commit<'a>(
        &'a self,
        source: Option<&'a SourceNote>,
        document: &'a CardDocument,
    ) -> PortFuture<'a, CommitReceipt>;
    fn restore<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, ()>;
}

pub trait EnrichmentPort: Send + Sync {
    fn modernize<'a>(&'a self, note: &'a SourceNote) -> PortFuture<'a, CardDocument>;
    fn inject<'a>(
        &'a self,
        deck_key: &'a str,
        expression: &'a str,
        context: &'a str,
    ) -> PortFuture<'a, CardDocument>;
}

pub trait JobPort: Send + Sync {
    fn pause<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
    fn resume<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
    fn retry_failures<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, u64>;
    fn rollback<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn note(note_id: i64, fields: &[(&str, &str)]) -> NoteInfo {
        NoteInfo {
            note_id,
            model_name: ModelName("Legacy".into()),
            deck_names: vec![DeckName("Japanese".into())],
            fields: fields
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into()))
                .collect(),
            tags: vec![],
        }
    }

    fn request(expression: &str) -> ExactExpressionRequest {
        ExactExpressionRequest {
            deck_key: "japanese_vocab".into(),
            expression: expression.into(),
            preferred_fields: vec!["Expression".into(), "Word".into()],
        }
    }

    #[test]
    fn exact_matching_ignores_html_and_whitespace_without_substring_matches() {
        let resolution = resolve_exact_expression(
            &request("  食べる "),
            [
                note(1, &[("Expression", "<b>食べる</b>")]),
                note(2, &[("Expression", "食べるもの")]),
            ],
        );
        assert!(matches!(
            resolution,
            ExpressionResolution::Modernize { note, .. } if note.note_id == 1
        ));
    }

    #[test]
    fn missing_or_whitespace_expression_selects_injection() {
        let missing = resolve_exact_expression(&request("新語"), [note(1, &[("Word", "旧語")])]);
        assert!(matches!(missing, ExpressionResolution::Inject { .. }));
        let whitespace =
            resolve_exact_expression(&request(" \t\n "), [note(1, &[("Word", "旧語")])]);
        assert_eq!(
            whitespace,
            ExpressionResolution::Inject {
                deck_key: "japanese_vocab".into(),
                expression: "".into(),
            }
        );
    }

    #[test]
    fn semantic_field_fallback_and_duplicates_are_explicit() {
        let fallback = resolve_exact_expression(
            &request("俳優"),
            [note(1, &[("Vocabulary", "俳優"), ("Meaning", "actor")])],
        );
        assert!(matches!(fallback, ExpressionResolution::Modernize { .. }));

        let duplicate = resolve_exact_expression(
            &request("俳優"),
            [note(1, &[("Word", "俳優")]), note(2, &[("Front", "俳優")])],
        );
        assert!(matches!(
            duplicate,
            ExpressionResolution::Ambiguous { matches, .. } if matches.len() == 2
        ));
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum MediaOperation {
        Retrieve,
        Store,
        Delete,
    }

    #[derive(Default)]
    struct MockMediaPort {
        media: Mutex<BTreeMap<String, String>>,
        calls: Mutex<Vec<MediaOperation>>,
        fail: Mutex<Option<(MediaOperation, usize)>>,
        corrupt_once: Mutex<Option<String>>,
    }

    impl MockMediaPort {
        fn with_media(media: &[(&str, &str)]) -> Self {
            Self {
                media: Mutex::new(
                    media
                        .iter()
                        .map(|(name, data)| ((*name).into(), (*data).into()))
                        .collect(),
                ),
                ..Self::default()
            }
        }

        fn fail_at(&self, operation: MediaOperation, count: usize) {
            *self.fail.lock().unwrap() = Some((operation, count));
        }

        fn check(&self, operation: MediaOperation) -> Result<(), PortError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(operation);
            let occurrence = calls
                .iter()
                .filter(|current| **current == operation)
                .count();
            let mut failure = self.fail.lock().unwrap();
            if *failure == Some((operation, occurrence)) {
                *failure = None;
                return Err(PortError {
                    operation: "mock media",
                    message: "injected failure".into(),
                    retryable: false,
                });
            }
            Ok(())
        }

        fn state(&self) -> BTreeMap<String, String> {
            self.media.lock().unwrap().clone()
        }
    }

    impl MediaPort for MockMediaPort {
        fn retrieve_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, Option<MediaFile>> {
            Box::pin(async move {
                self.check(MediaOperation::Retrieve)?;
                Ok(self
                    .media
                    .lock()
                    .unwrap()
                    .get(filename)
                    .cloned()
                    .map(|data_base64| MediaFile {
                        filename: filename.into(),
                        data_base64,
                    }))
            })
        }

        fn store_media<'a>(&'a self, media: &'a MediaFile) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.check(MediaOperation::Store)?;
                let corrupt = {
                    let mut corrupt_once = self.corrupt_once.lock().unwrap();
                    if corrupt_once.as_deref() == Some(&media.filename) {
                        *corrupt_once = None;
                        true
                    } else {
                        false
                    }
                };
                let data = if corrupt {
                    "Y29ycnVwdA==".into()
                } else {
                    media.data_base64.clone()
                };
                self.media
                    .lock()
                    .unwrap()
                    .insert(media.filename.clone(), data);
                Ok(())
            })
        }

        fn delete_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.check(MediaOperation::Delete)?;
                self.media.lock().unwrap().remove(filename);
                Ok(())
            })
        }
    }

    fn media_plan() -> MediaPlan {
        MediaPlan {
            stage: vec![
                MediaChange {
                    filename: "new-a.jpg".into(),
                    data_base64: "bmV3LWE=".into(),
                },
                MediaChange {
                    filename: "new-b.mp3".into(),
                    data_base64: "bmV3LWI=".into(),
                },
            ],
            remove_obsolete: vec!["old.jpg".into()],
        }
    }

    #[tokio::test]
    async fn media_transaction_stages_verifies_and_removes_obsolete_media() {
        let port = MockMediaPort::with_media(&[("old.jpg", "b2xk")]);
        let transaction = PreparedMediaTransaction::prepare(&port, media_plan())
            .await
            .unwrap();
        assert_eq!(
            transaction.before()["old.jpg"]
                .as_ref()
                .unwrap()
                .data_base64,
            "b2xk"
        );
        transaction.commit(&port).await.unwrap();
        assert_eq!(
            port.state(),
            BTreeMap::from([
                ("new-a.jpg".into(), "bmV3LWE=".into()),
                ("new-b.mp3".into(), "bmV3LWI=".into()),
            ])
        );
    }

    #[tokio::test]
    async fn media_transaction_captures_before_every_mutation_and_rolls_back_each_boundary() {
        for (operation, count, corrupt) in [
            (MediaOperation::Store, 1, false),
            (MediaOperation::Store, 2, false),
            (MediaOperation::Delete, 1, false),
            (MediaOperation::Store, 0, true),
        ] {
            let port = MockMediaPort::with_media(&[("old.jpg", "b2xk")]);
            let before = port.state();
            let transaction = PreparedMediaTransaction::prepare(&port, media_plan())
                .await
                .unwrap();
            if corrupt {
                *port.corrupt_once.lock().unwrap() = Some("new-a.jpg".into());
            } else {
                port.fail_at(operation, count);
            }
            let error = transaction.commit(&port).await.unwrap_err();
            assert!(matches!(error.phase, "stage" | "verify" | "remove"));
            assert!(error.rollback_errors.is_empty());
            assert_eq!(port.state(), before);
        }
    }

    #[tokio::test]
    async fn media_preflight_failure_and_conflicts_never_mutate() {
        let port = MockMediaPort::with_media(&[("old.jpg", "b2xk")]);
        let before = port.state();
        port.fail_at(MediaOperation::Retrieve, 1);
        assert!(matches!(
            PreparedMediaTransaction::prepare(&port, media_plan()).await,
            Err(MediaTransactionError {
                phase: "snapshot",
                ..
            })
        ));
        assert_eq!(port.state(), before);

        let conflict = MediaPlan {
            stage: vec![
                MediaChange {
                    filename: "same.jpg".into(),
                    data_base64: "YQ==".into(),
                },
                MediaChange {
                    filename: "same.jpg".into(),
                    data_base64: "Yg==".into(),
                },
            ],
            remove_obsolete: vec!["same.jpg".into()],
        };
        assert!(matches!(
            PreparedMediaTransaction::prepare(&port, conflict).await,
            Err(MediaTransactionError {
                phase: "validate",
                ..
            })
        ));
        assert_eq!(port.state(), before);
    }

    #[derive(Default)]
    struct MockCommitPort {
        media: Mutex<BTreeMap<String, String>>,
        events: Mutex<Vec<String>>,
        fail: Mutex<Option<String>>,
        note_fields: Mutex<BTreeMap<i64, BTreeMap<String, String>>>,
        next_note: Mutex<i64>,
    }

    impl MockCommitPort {
        fn new() -> Self {
            Self {
                media: Mutex::new(BTreeMap::from([("old.jpg".into(), "b2xk".into())])),
                note_fields: Mutex::new(BTreeMap::from([(
                    42,
                    BTreeMap::from([("Expression".into(), "old".into())]),
                )])),
                next_note: Mutex::new(100),
                ..Self::default()
            }
        }

        fn fail(&self, phase: &str) {
            *self.fail.lock().unwrap() = Some(phase.into());
        }

        fn event(&self, phase: &str) -> Result<(), PortError> {
            self.events.lock().unwrap().push(phase.into());
            let mut failure = self.fail.lock().unwrap();
            if failure.as_deref() == Some(phase) {
                *failure = None;
                Err(PortError {
                    operation: "mock commit",
                    message: format!("{phase} failed"),
                    retryable: false,
                })
            } else {
                Ok(())
            }
        }

        fn media_state(&self) -> BTreeMap<String, String> {
            self.media.lock().unwrap().clone()
        }
    }

    impl MediaPort for MockCommitPort {
        fn retrieve_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, Option<MediaFile>> {
            Box::pin(async move {
                self.event("retrieve")?;
                Ok(self
                    .media
                    .lock()
                    .unwrap()
                    .get(filename)
                    .cloned()
                    .map(|data_base64| MediaFile {
                        filename: filename.into(),
                        data_base64,
                    }))
            })
        }

        fn store_media<'a>(&'a self, media: &'a MediaFile) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.event("store")?;
                self.media
                    .lock()
                    .unwrap()
                    .insert(media.filename.clone(), media.data_base64.clone());
                Ok(())
            })
        }

        fn delete_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.event("delete")?;
                self.media.lock().unwrap().remove(filename);
                Ok(())
            })
        }
    }

    impl CommitPort for MockCommitPort {
        fn backup_deck<'a>(&'a self, _: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move { self.event("backup") })
        }

        fn capture_snapshot<'a>(
            &'a self,
            _: &'a SnapshotCapture,
        ) -> PortFuture<'a, SnapshotHandle> {
            Box::pin(async move {
                self.event("capture")?;
                Ok(SnapshotHandle("snapshot-1".into()))
            })
        }

        fn finalize_snapshot<'a>(
            &'a self,
            _: &'a SnapshotHandle,
            _: &'a PostWriteState,
        ) -> PortFuture<'a, ()> {
            Box::pin(async move { self.event("finalize") })
        }

        fn fail_snapshot<'a>(&'a self, _: &'a SnapshotHandle, _: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move { self.event("fail snapshot") })
        }

        fn apply_template<'a>(
            &'a self,
            _: &'a ManagedTemplatePlan,
        ) -> PortFuture<'a, TemplateMutation> {
            Box::pin(async move {
                self.event("template")?;
                Ok(TemplateMutation)
            })
        }

        fn rollback_template<'a>(&'a self, _: &'a TemplateMutation) -> PortFuture<'a, ()> {
            Box::pin(async move { self.event("rollback template") })
        }

        fn update_note<'a>(
            &'a self,
            source: &'a CommitSource,
            fields: &'a BTreeMap<String, String>,
            _: &'a str,
        ) -> PortFuture<'a, NoteMutation> {
            Box::pin(async move {
                self.event("update")?;
                self.note_fields
                    .lock()
                    .unwrap()
                    .insert(source.note_id, fields.clone());
                Ok(NoteMutation {
                    note_id: source.note_id,
                    created: false,
                })
            })
        }

        fn create_note<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            fields: &'a BTreeMap<String, String>,
            _: &'a [String],
        ) -> PortFuture<'a, NoteMutation> {
            Box::pin(async move {
                self.event("create")?;
                let mut next = self.next_note.lock().unwrap();
                let note_id = *next;
                *next += 1;
                self.note_fields
                    .lock()
                    .unwrap()
                    .insert(note_id, fields.clone());
                Ok(NoteMutation {
                    note_id,
                    created: true,
                })
            })
        }

        fn rollback_note<'a>(
            &'a self,
            mutation: &'a NoteMutation,
            source: Option<&'a CommitSource>,
        ) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.event("rollback note")?;
                if mutation.created {
                    self.note_fields.lock().unwrap().remove(&mutation.note_id);
                } else if let Some(source) = source {
                    self.note_fields
                        .lock()
                        .unwrap()
                        .insert(source.note_id, source.fields.clone());
                }
                Ok(())
            })
        }
    }

    fn commit_request(mode: CardMode, dry_run: bool) -> CommitRequest {
        let source = (mode == CardMode::Modernize).then(|| CommitSource {
            note_id: 42,
            model_name: "Legacy".into(),
            fields: BTreeMap::from([("Expression".into(), "old".into())]),
            tags: vec!["old".into()],
        });
        CommitRequest {
            mode,
            dry_run,
            deck_key: "japanese_vocab".into(),
            deck_name: "Japanese".into(),
            target_model: "Linguist Japanese Vocabulary".into(),
            source,
            document: CardDocument {
                schema_version: linguist_core::CONTRACT_VERSION,
                expression: "新語".into(),
                values: linguist_core::LogicalFields {
                    meaning_text: Some("new meaning".into()),
                    ..Default::default()
                },
                media: vec![linguist_core::MediaAsset {
                    filename: "new.jpg".into(),
                    data_base64: "bmV3".into(),
                }],
                obsolete_media: vec!["old.jpg".into()],
                issues: vec![],
                tags: vec!["new".into()],
                provenance: BTreeMap::new(),
            },
            field_mapping: linguist_core::FieldMapping {
                expression: Some("Expression".into()),
                meaning_text: Some("Meaning".into()),
                ..Default::default()
            },
            template_plan: ManagedTemplatePlan::NoChange,
        }
    }

    #[tokio::test]
    async fn commit_dry_run_performs_no_external_mutation() {
        let port = MockCommitPort::new();
        assert!(matches!(
            commit_card(&port, commit_request(CardMode::Inject, true)).await,
            Ok(CommitOutcome::DryRun { .. })
        ));
        assert!(port.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn commit_orders_backup_snapshot_note_and_obsolete_media_last() {
        for mode in [CardMode::Modernize, CardMode::Inject] {
            let port = MockCommitPort::new();
            let outcome = commit_card(&port, commit_request(mode, false))
                .await
                .unwrap();
            assert!(matches!(outcome, CommitOutcome::Committed(_)));
            let events = port.events.lock().unwrap().clone();
            let index = |name: &str| events.iter().position(|event| event == name).unwrap();
            assert!(index("backup") < index("capture"));
            assert!(index("capture") < index("store"));
            assert!(index("store") < index("template"));
            assert!(
                index("template")
                    < index(if mode == CardMode::Modernize {
                        "update"
                    } else {
                        "create"
                    })
            );
            assert!(index("finalize") < index("delete"));
        }
    }

    #[tokio::test]
    async fn commit_rolls_back_modernize_and_inject_failures_at_every_mutation_step() {
        for mode in [CardMode::Modernize, CardMode::Inject] {
            for phase in [
                "backup",
                "capture",
                "store",
                "template",
                if mode == CardMode::Modernize {
                    "update"
                } else {
                    "create"
                },
                "finalize",
                "delete",
            ] {
                let port = MockCommitPort::new();
                let media_before = port.media_state();
                let notes_before = port.note_fields.lock().unwrap().clone();
                port.fail(phase);
                assert!(
                    commit_card(&port, commit_request(mode, false))
                        .await
                        .is_err(),
                    "{mode:?} {phase}"
                );
                assert_eq!(port.media_state(), media_before, "{mode:?} {phase}");
                assert_eq!(
                    *port.note_fields.lock().unwrap(),
                    notes_before,
                    "{mode:?} {phase}"
                );
            }
        }
    }

    #[derive(Default)]
    struct MockRestorePort {
        media: Mutex<BTreeMap<String, String>>,
        notes: Mutex<BTreeMap<i64, NoteInfo>>,
        restored: Mutex<BTreeSet<String>>,
        events: Mutex<Vec<String>>,
    }

    impl MockRestorePort {
        fn with_note(note: NoteInfo) -> Self {
            Self {
                notes: Mutex::new(BTreeMap::from([(note.note_id, note)])),
                ..Self::default()
            }
        }
    }

    impl MediaPort for MockRestorePort {
        fn retrieve_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, Option<MediaFile>> {
            Box::pin(async move {
                Ok(self
                    .media
                    .lock()
                    .unwrap()
                    .get(filename)
                    .cloned()
                    .map(|data_base64| MediaFile {
                        filename: filename.into(),
                        data_base64,
                    }))
            })
        }

        fn store_media<'a>(&'a self, media: &'a MediaFile) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("store:{}", media.filename));
                self.media
                    .lock()
                    .unwrap()
                    .insert(media.filename.clone(), media.data_base64.clone());
                Ok(())
            })
        }

        fn delete_media<'a>(&'a self, filename: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("delete media:{filename}"));
                self.media.lock().unwrap().remove(filename);
                Ok(())
            })
        }
    }

    impl RestorePort for MockRestorePort {
        fn snapshot_restored<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, bool> {
            Box::pin(async move { Ok(self.restored.lock().unwrap().contains(snapshot_id)) })
        }

        fn note_info<'a>(&'a self, note_id: i64) -> PortFuture<'a, Option<NoteInfo>> {
            Box::pin(async move { Ok(self.notes.lock().unwrap().get(&note_id).cloned()) })
        }

        fn delete_notes<'a>(&'a self, note_ids: &'a [i64]) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("delete notes:{note_ids:?}"));
                let mut notes = self.notes.lock().unwrap();
                for note_id in note_ids {
                    notes.remove(note_id);
                }
                Ok(())
            })
        }

        fn restore_note<'a>(&'a self, original: &'a SnapshotOriginalNote) -> PortFuture<'a, ()> {
            Box::pin(async move {
                let note_id = original.note_id.expect("validated original note id");
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("restore:{note_id}"));
                self.notes.lock().unwrap().insert(
                    note_id,
                    NoteInfo {
                        note_id,
                        model_name: ModelName(original.model_name.clone()),
                        deck_names: vec![DeckName(original.deck_name.clone())],
                        fields: original.fields.clone(),
                        tags: original.tags.clone(),
                    },
                );
                Ok(())
            })
        }

        fn mark_snapshot_restored<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, ()> {
            Box::pin(async move {
                self.events.lock().unwrap().push("mark restored".into());
                self.restored.lock().unwrap().insert(snapshot_id.into());
                Ok(())
            })
        }
    }

    fn python_snapshot() -> SnapshotDocument {
        serde_json::from_str(include_str!("../../../contracts/fixtures/snapshot.v1.json")).unwrap()
    }

    #[tokio::test]
    async fn restore_python_snapshot_restores_mock_collection_once() {
        let snapshot = python_snapshot();
        let port = MockRestorePort::with_note(NoteInfo {
            note_id: 42,
            model_name: ModelName("Linguist Japanese Vocabulary".into()),
            deck_names: vec![DeckName("Japanese".into())],
            fields: BTreeMap::from([("Expression".into(), "new".into())]),
            tags: vec!["new".into()],
        });
        port.media.lock().unwrap().extend(BTreeMap::from([
            ("image.jpg".into(), "new-base64".into()),
            ("new.mp3".into(), "new-audio".into()),
        ]));

        assert!(matches!(
            restore_snapshot(&port, &snapshot).await,
            Ok(RestoreOutcome::Restored {
                restored_note_id: Some(42),
                ..
            })
        ));
        let restored = port.notes.lock().unwrap().get(&42).cloned().unwrap();
        assert_eq!(restored.model_name.0, "Legacy Japanese");
        assert_eq!(restored.deck_names, vec![DeckName("Japanese".into())]);
        assert_eq!(restored.fields["Word"], "俳優");
        assert_eq!(restored.tags, vec!["legacy"]);
        assert_eq!(
            port.media.lock().unwrap().get("image.jpg"),
            Some(&"old-base64".into())
        );
        assert!(!port.media.lock().unwrap().contains_key("new.mp3"));
        let state = (
            port.media.lock().unwrap().clone(),
            port.notes.lock().unwrap().clone(),
            port.events.lock().unwrap().clone(),
        );

        assert_eq!(
            restore_snapshot(&port, &snapshot).await,
            Ok(RestoreOutcome::AlreadyRestored)
        );
        assert_eq!(port.media.lock().unwrap().clone(), state.0);
        assert_eq!(port.notes.lock().unwrap().clone(), state.1);
        assert_eq!(port.events.lock().unwrap().clone(), state.2);
    }

    #[tokio::test]
    async fn restore_injection_removes_created_notes_and_media() {
        let mut snapshot = python_snapshot();
        snapshot.snapshot.original_note = None;
        snapshot.snapshot.result_note_id = Some(88);
        snapshot.snapshot.created_note_ids = vec![89, 88];
        let port = MockRestorePort::with_note(note(88, &[("Expression", "new")]));
        port.notes
            .lock()
            .unwrap()
            .insert(89, note(89, &[("Expression", "sibling")]));
        port.media
            .lock()
            .unwrap()
            .insert("new.mp3".into(), "new-audio".into());

        assert_eq!(
            restore_snapshot(&port, &snapshot).await,
            Ok(RestoreOutcome::Restored {
                restored_note_id: None,
                removed_note_ids: vec![88, 89],
            })
        );
        assert!(port.notes.lock().unwrap().is_empty());
        assert!(!port.media.lock().unwrap().contains_key("new.mp3"));
    }

    #[tokio::test]
    async fn restore_blocks_native_snapshot_when_note_has_newer_write() {
        let mut snapshot = python_snapshot();
        snapshot.snapshot.extensions.insert(
            NATIVE_POST_WRITE_EXTENSION.into(),
            serde_json::to_value(PostWriteState {
                note: PostWriteNote {
                    note_id: 42,
                    model_name: "Linguist Japanese Vocabulary".into(),
                    deck_name: "Japanese".into(),
                    fields: BTreeMap::from([("Expression".into(), "expected".into())]),
                    tags: vec!["new".into()],
                },
                media: BTreeMap::new(),
            })
            .unwrap(),
        );
        let port = MockRestorePort::with_note(NoteInfo {
            note_id: 42,
            model_name: ModelName("Linguist Japanese Vocabulary".into()),
            deck_names: vec![DeckName("Japanese".into())],
            fields: BTreeMap::from([("Expression".into(), "edited later".into())]),
            tags: vec!["new".into()],
        });
        let before = port.notes.lock().unwrap().clone();

        assert!(matches!(
            restore_snapshot(&port, &snapshot).await,
            Err(RestoreError::Conflict { note_id: 42, .. })
        ));
        assert_eq!(*port.notes.lock().unwrap(), before);
        assert!(port.events.lock().unwrap().is_empty());
    }
}
