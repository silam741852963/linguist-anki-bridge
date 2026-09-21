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
    pub source_line: usize,
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

/// Normalize the language aliases accepted by the Python importer.
pub fn canonical_language_key(value: &str) -> Option<String> {
    let key = value.trim().to_ascii_lowercase().replace(' ', "_");
    let key = match key.as_str() {
        "japanese" | "ja" => "japanese_vocab",
        "english" | "en" => "english_vocab",
        "taiwanese" | "zh_tw" | "zh-tw" => "taiwanese_vocab",
        "german" | "de" => "german_vocab",
        _ => key.as_str(),
    };
    matches!(
        key,
        "japanese_vocab"
            | "japanese_grammar"
            | "english_vocab"
            | "english_grammar"
            | "taiwanese_vocab"
            | "taiwanese_grammar"
            | "german_vocab"
            | "german_grammar"
    )
    .then(|| key.to_owned())
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
            source_line: line_number,
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
    let mut headers = headers.fields.clone();
    if let Some(first) = headers.first_mut() {
        *first = first.trim_start_matches('\u{feff}').to_owned();
    }
    if headers.iter().all(|header| header.trim().is_empty()) {
        return Err("CSV has no usable headers".into());
    }
    let mapping = request
        .mapping
        .clone()
        .unwrap_or_else(|| infer_mapping(&headers));
    if mapping.expression >= headers.len() {
        return Err("Expression column is out of range".into());
    }
    for (name, column) in [
        ("Language", mapping.language),
        ("Type", mapping.type_tag),
        ("Context", mapping.context),
    ] {
        if column.is_some_and(|column| column >= headers.len()) {
            return Err(format!("{name} column is out of range"));
        }
    }
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    let mut duplicates = Vec::new();
    let mut seen = BTreeSet::new();
    for record in records.into_iter().skip(1) {
        let line = record.line;
        if record.fields.len() == 1 && record.fields[0].trim().is_empty() {
            continue;
        }
        if record.fields.len() != headers.len() {
            issues.push(RowIssue {
                line,
                message: format!(
                    "Expected {} columns, found {}",
                    headers.len(),
                    record.fields.len()
                ),
            });
            continue;
        }
        let get = |column: Option<usize>| {
            column
                .and_then(|column| record.fields.get(column))
                .map(String::as_str)
                .unwrap_or_default()
                .trim()
        };
        let expression = normalize_expression(
            record
                .fields
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
            source_line: line,
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
            let normalized = header
                .chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>();
            aliases.iter().any(|alias| normalized == *alias)
        })
    };
    CsvColumnMapping {
        expression: find(&["word", "expression", "vocabulary", "front", "term"]).unwrap_or(0),
        language: find(&["language", "lang", "deck"]),
        type_tag: find(&["type", "wordtype", "partofspeech", "pos"]),
        context: find(&["note", "notes", "context", "meaning", "definition"]),
    }
}
struct CsvRecord {
    fields: Vec<String>,
    line: usize,
}
#[derive(Clone, Copy)]
enum CsvFieldState {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}
fn parse_csv(content: &str) -> Result<Vec<CsvRecord>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut state = CsvFieldState::Start;
    let mut line = 1;
    let mut row_start = 1;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' && chars.peek() == Some(&'\n') {
            continue;
        }
        match (state, ch) {
            (CsvFieldState::Quoted, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (CsvFieldState::Quoted, '"') => state = CsvFieldState::AfterQuote,
            (CsvFieldState::Quoted, '\n') => {
                field.push('\n');
                line += 1;
            }
            (CsvFieldState::Quoted, character) => field.push(character),
            (CsvFieldState::Start, '"') => state = CsvFieldState::Quoted,
            (CsvFieldState::Unquoted | CsvFieldState::AfterQuote, '"') => {
                return Err(format!("Unexpected quote on CSV line {line}"));
            }
            (_, ',') => {
                row.push(std::mem::take(&mut field));
                state = CsvFieldState::Start;
            }
            (_, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(CsvRecord {
                    fields: std::mem::take(&mut row),
                    line: row_start,
                });
                line += 1;
                row_start = line;
                state = CsvFieldState::Start;
            }
            (CsvFieldState::AfterQuote, character) => {
                return Err(format!(
                    "Unexpected character '{character}' after quote on CSV line {line}"
                ));
            }
            (CsvFieldState::Start, character) => {
                field.push(character);
                state = CsvFieldState::Unquoted;
            }
            (CsvFieldState::Unquoted, character) => field.push(character),
        }
    }
    if matches!(state, CsvFieldState::Quoted) {
        return Err("CSV has an unclosed quoted field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(CsvRecord {
            fields: row,
            line: row_start,
        })
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

/// Choices are explicit: an ambiguous Anki match is never selected by default.
/// A repeated input row also defaults to Skip, even if Anki has no match.
pub fn duplicate_decision_options(
    decision: &DuplicateDecision,
    repeated_in_input: bool,
) -> Vec<DuplicateDecision> {
    let mut choices = match decision {
        DuplicateDecision::Inject => vec![DuplicateDecision::Inject, DuplicateDecision::Skip],
        DuplicateDecision::Modernize { note } => vec![
            DuplicateDecision::Modernize { note: note.clone() },
            DuplicateDecision::Inject,
            DuplicateDecision::Skip,
        ],
        DuplicateDecision::Ambiguous { matches } => {
            let mut matches = matches.clone();
            matches.sort_by_key(|note| note.note_id);
            matches.dedup_by_key(|note| note.note_id);
            let mut choices = vec![DuplicateDecision::Skip, DuplicateDecision::Inject];
            choices.extend(
                matches
                    .into_iter()
                    .map(|note| DuplicateDecision::Modernize { note }),
            );
            choices
        }
        DuplicateDecision::Skip => vec![DuplicateDecision::Skip],
    };
    if repeated_in_input {
        if let Some(index) = choices
            .iter()
            .position(|choice| matches!(choice, DuplicateDecision::Skip))
        {
            choices.swap(0, index);
        }
    }
    choices
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
        assert_eq!(preview.rows[0].source_line, 1);
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
    fn repeated_and_ambiguous_rows_require_explicit_choice() {
        let repeated = duplicate_decision_options(&DuplicateDecision::Inject, true);
        assert_eq!(
            repeated,
            [DuplicateDecision::Skip, DuplicateDecision::Inject]
        );

        let mut first = note("食べる");
        first.note_id = 42;
        let mut second = note("食べる");
        second.note_id = 7;
        let choices = duplicate_decision_options(
            &DuplicateDecision::Ambiguous {
                matches: vec![first.clone(), second.clone(), first],
            },
            false,
        );
        assert_eq!(choices[0], DuplicateDecision::Skip);
        assert_eq!(choices[1], DuplicateDecision::Inject);
        assert!(matches!(&choices[2], DuplicateDecision::Modernize { note } if note.note_id == 7));
        assert!(matches!(&choices[3], DuplicateDecision::Modernize { note } if note.note_id == 42));
        assert_eq!(choices.len(), 4);
        assert_eq!(
            duplicate_decision_options(&DuplicateDecision::Skip, false),
            [DuplicateDecision::Skip]
        );
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
        assert_eq!(
            canonical_language_key(" Japanese ").as_deref(),
            Some("japanese_vocab")
        );
        assert_eq!(
            canonical_language_key("de").as_deref(),
            Some("german_vocab")
        );
        assert_eq!(
            canonical_language_key("japanese_grammar").as_deref(),
            Some("japanese_grammar")
        );
        assert!(canonical_language_key("unknown").is_none());
    }

    #[test]
    fn csv_mapping_handles_bom_aliases_multiline_and_bad_rows() {
        let base = CsvIngestRequest {
            content: "\u{feff}Term,Part of Speech,Notes\r\n\"猫\",noun,\"line one\r\nline two\"\r\n犬,verb\r\n"
                .into(),
            deck_key: "Japanese".into(),
            language_key: "japanese_vocab".into(),
            type_tag: String::new(),
            mapping: None,
        };
        let preview = prepare_csv_input(&base).unwrap();
        assert_eq!(preview.headers[0], "Term");
        assert_eq!(preview.mapping.type_tag, Some(1));
        assert_eq!(preview.mapping.context, Some(2));
        assert_eq!(preview.rows[0].context, "line one\nline two");
        assert_eq!(preview.rows[0].source_line, 2);
        assert_eq!(preview.issues[0].line, 4);
        assert!(preview.issues[0].message.contains("Expected 3 columns"));

        let malformed = CsvIngestRequest {
            content: format!("{}鳥,\"noun\"oops,note\n", base.content),
            ..base
        };
        assert!(
            prepare_csv_input(&malformed)
                .unwrap_err()
                .contains("after quote")
        );
    }

    #[test]
    fn csv_explicit_mapping_rejects_out_of_range_columns() {
        let request = CsvIngestRequest {
            content: "Term,Notes\n猫,context\n".into(),
            deck_key: "Japanese".into(),
            language_key: "japanese_vocab".into(),
            type_tag: String::new(),
            mapping: Some(CsvColumnMapping {
                expression: 0,
                language: None,
                type_tag: None,
                context: Some(2),
            }),
        };
        assert_eq!(
            prepare_csv_input(&request).unwrap_err(),
            "Context column is out of range"
        );

        let remapped = prepare_csv_input(&CsvIngestRequest {
            content: "Ignore,Term,Notes\nunused,猫,cat example\n".into(),
            mapping: Some(CsvColumnMapping {
                expression: 1,
                language: None,
                type_tag: None,
                context: Some(2),
            }),
            ..request
        })
        .unwrap();
        assert_eq!(remapped.rows[0].expression, "猫");
        assert_eq!(remapped.rows[0].context, "cat example");
    }
}
