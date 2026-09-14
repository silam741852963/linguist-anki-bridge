//! Input-only manual and pasted-list preparation. Qt chooses how to display it.

use crate::{ExactExpressionRequest, ExpressionResolution, NoteInfo, resolve_exact_expression};
use linguist_core::normalize_expression;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualIngestRequest {
    pub raw: String,
    pub deck_key: String,
    pub language_key: String,
    pub type_tag: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputRow {
    pub ordinal: usize,
    pub expression: String,
    pub context: String,
    pub deck_key: String,
    pub language_key: String,
    pub type_tag: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowIssue {
    pub line: usize,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionPreview {
    pub rows: Vec<InputRow>,
    pub issues: Vec<RowIssue>,
    pub duplicates: Vec<usize>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DuplicateDecision {
    Inject,
    Modernize { note: NoteInfo },
    Ambiguous { matches: Vec<NoteInfo> },
    Skip,
}

pub fn prepare_manual_input(request: &ManualIngestRequest) -> IngestionPreview {
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();
    for (line, raw) in request.raw.lines().enumerate() {
        let line_number = line + 1;
        let mut columns = raw.splitn(2, '\t');
        let expression = normalize_expression(columns.next().unwrap_or_default());
        let context = columns.next().unwrap_or_default().trim().into();
        if expression.is_empty() {
            if !raw.trim().is_empty() {
                issues.push(RowIssue {
                    line: line_number,
                    message: "Expression is empty".into(),
                });
            }
            continue;
        }
        if !seen.insert(expression.clone()) {
            duplicates.push(line_number);
        }
        rows.push(InputRow {
            ordinal: rows.len() + 1,
            expression,
            context,
            deck_key: request.deck_key.clone(),
            language_key: request.language_key.clone(),
            type_tag: request.type_tag.clone(),
        });
    }
    IngestionPreview {
        rows,
        issues,
        duplicates,
    }
}
pub fn resolve_ingestion_preview(
    preview: &IngestionPreview,
    candidates: impl IntoIterator<Item = NoteInfo>,
) -> Vec<DuplicateDecision> {
    let notes = candidates.into_iter().collect::<Vec<_>>();
    preview
        .rows
        .iter()
        .map(|row| {
            match resolve_exact_expression(
                &ExactExpressionRequest {
                    deck_key: row.deck_key.clone(),
                    expression: row.expression.clone(),
                    preferred_fields: vec!["Expression".into(), "Word".into()],
                },
                notes.clone(),
            ) {
                ExpressionResolution::Inject { .. } => DuplicateDecision::Inject,
                ExpressionResolution::Modernize { note, .. } => {
                    DuplicateDecision::Modernize { note }
                }
                ExpressionResolution::Ambiguous { matches, .. } => {
                    DuplicateDecision::Ambiguous { matches }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeckName, ModelName};
    use std::collections::BTreeMap;
    fn note(expression: &str) -> NoteInfo {
        NoteInfo {
            note_id: 3,
            model_name: ModelName("Card".into()),
            deck_names: vec![DeckName("Deck".into())],
            fields: BTreeMap::from([("Expression".into(), expression.into())]),
            tags: vec![],
        }
    }
    #[test]
    fn parses_context_and_flags_duplicate_paste_rows() {
        let preview = prepare_manual_input(&ManualIngestRequest {
            raw: "食べる\tmeal verb\n <b>食べる</b> \n新語\tcontext".into(),
            deck_key: "japanese_vocab".into(),
            language_key: "japanese_vocab".into(),
            type_tag: "vocab".into(),
        });
        assert_eq!(preview.rows.len(), 3);
        assert_eq!(preview.rows[0].context, "meal verb");
        assert_eq!(preview.duplicates, vec![2]);
    }
    #[test]
    fn makes_duplicate_decisions_without_picking_ambiguous_note() {
        let preview = prepare_manual_input(&ManualIngestRequest {
            raw: "食べる\n新語".into(),
            deck_key: "japanese_vocab".into(),
            language_key: "japanese_vocab".into(),
            type_tag: String::new(),
        });
        let decisions = resolve_ingestion_preview(&preview, [note("食べる")]);
        assert!(matches!(decisions[0], DuplicateDecision::Modernize { .. }));
        assert_eq!(decisions[1], DuplicateDecision::Inject);
    }
}
