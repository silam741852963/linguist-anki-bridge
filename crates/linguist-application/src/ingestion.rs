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
pub struct CsvColumnMapping {
    pub expression: usize,
    pub language: Option<usize>,
    pub type_tag: Option<usize>,
    pub context: Option<usize>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsvIngestRequest {
    pub content: String,
    pub deck_key: String,
    pub language_key: String,
    pub type_tag: String,
    pub mapping: Option<CsvColumnMapping>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsvPreview {
    pub headers: Vec<String>,
    pub mapping: CsvColumnMapping,
    pub rows: Vec<InputRow>,
    pub issues: Vec<RowIssue>,
    pub duplicates: Vec<usize>,
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
pub fn prepare_csv_input(request: &CsvIngestRequest) -> Result<CsvPreview, String> {
    let records = parse_csv(&request.content)?;
    let Some(headers) = records.first() else {
        return Err("CSV has no header row".into());
    };
    let headers = headers.clone();
    let mapping = request
        .mapping
        .clone()
        .unwrap_or_else(|| infer_mapping(&headers));
    if mapping.expression >= headers.len() {
        return Err("Expression column is out of range".into());
    }
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    let mut duplicates = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, record) in records.into_iter().skip(1).enumerate() {
        let line = index + 2;
        let get = |column: Option<usize>| {
            column
                .and_then(|column| record.get(column))
                .map(String::as_str)
                .unwrap_or_default()
                .trim()
        };
        let expression = normalize_expression(
            record
                .get(mapping.expression)
                .map(String::as_str)
                .unwrap_or_default(),
        );
        if expression.is_empty() {
            issues.push(RowIssue {
                line,
                message: "Expression is empty".into(),
            });
            continue;
        }
        if !seen.insert(expression.clone()) {
            duplicates.push(line)
        }
        let language = get(mapping.language);
        let type_tag = get(mapping.type_tag);
        rows.push(InputRow {
            ordinal: rows.len() + 1,
            expression,
            context: get(mapping.context).into(),
            deck_key: request.deck_key.clone(),
            language_key: if language.is_empty() {
                request.language_key.clone()
            } else {
                language.into()
            },
            type_tag: if type_tag.is_empty() {
                request.type_tag.clone()
            } else {
                type_tag.into()
            },
        });
    }
    Ok(CsvPreview {
        headers,
        mapping,
        rows,
        issues,
        duplicates,
    })
}
fn infer_mapping(headers: &[String]) -> CsvColumnMapping {
    let find = |aliases: &[&str]| {
        headers.iter().position(|header| {
            aliases
                .iter()
                .any(|alias| header.trim().eq_ignore_ascii_case(alias))
        })
    };
    CsvColumnMapping {
        expression: find(&["word", "expression", "vocabulary", "front", "term"]).unwrap_or(0),
        language: find(&["language", "lang", "deck"]),
        type_tag: find(&["type", "word_type", "part_of_speech"]),
        context: find(&["note", "notes", "context", "meaning"]),
    }
}
fn parse_csv(content: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err("CSV has an unclosed quoted field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row)
    }
    Ok(rows)
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
    #[test]
    fn maps_aliases_and_validates_csv_rows() {
        let preview = prepare_csv_input(&CsvIngestRequest {
            content: "Word,Language,Type,Note\n\"食,べる\",japanese_vocab,verb,\"a, b\"\n,ja,,"
                .into(),
            deck_key: "japanese_vocab".into(),
            language_key: "japanese_vocab".into(),
            type_tag: String::new(),
            mapping: None,
        })
        .unwrap();
        assert_eq!(preview.rows[0].expression, "食,べる");
        assert_eq!(preview.rows[0].context, "a, b");
        assert_eq!(preview.issues.len(), 1);
    }
}
