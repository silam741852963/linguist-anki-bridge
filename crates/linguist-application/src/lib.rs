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
use linguist_core::{CardDocument, normalize_expression};

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
}
