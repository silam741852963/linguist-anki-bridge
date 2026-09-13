//! Use-case boundaries for the native rewrite.
//!
//! Adapters for AnkiConnect, dictionaries, OCR, Ollama, media, and SQLite
//! implement these ports. The Qt layer consumes application events and does
//! not call providers directly.

use std::{collections::BTreeMap, future::Future, pin::Pin};

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
}
