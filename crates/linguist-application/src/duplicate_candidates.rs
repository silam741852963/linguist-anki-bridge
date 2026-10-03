//! Time-bound, read-only managed-model candidate inspection for authored adds.
//! Search syntax is only a candidate locator; it cannot authorize skip or apply.
use linguist_core::{LearningContent, records::PlanRevision};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub trait CandidateReader {
    fn find_notes(&self, query: &str) -> Result<Vec<String>, String>;
    fn notes_info(&self, ids: &[String]) -> Result<Vec<Value>, String>;
}

impl CandidateReader for linguist_anki::Client {
    fn find_notes(&self, query: &str) -> Result<Vec<String>, String> {
        linguist_anki::Client::find_notes(self, query)
    }
    fn notes_info(&self, ids: &[String]) -> Result<Vec<Value>, String> {
        linguist_anki::Client::notes_info(self, ids)
    }
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub note_id: String,
    pub model_matches: bool,
    pub compared_fields_match: bool,
    pub relation: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u16,
    pub plan_id: Uuid,
    pub revision: u32,
    pub plan_digest: String,
    pub document_id: Uuid,
    pub observed_at_unix_seconds: u64,
    pub candidates: Vec<Candidate>,
    pub search_scope: &'static str,
    pub collection_duplicate_check_complete: bool,
    pub semantic_identity_verified: bool,
    pub apply_eligible: bool,
}

fn search_value(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err("DUPLICATE_QUERY_UNSUPPORTED_INPUT".into());
    }
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('*', "\\*")
        .replace('_', "\\_"))
}

pub fn inspect(
    plan: &PlanRevision,
    document_id: Uuid,
    reader: &dyn CandidateReader,
    max_candidates: usize,
    max_query_chars: usize,
) -> Result<Report, String> {
    if max_candidates == 0 {
        return Err("DUPLICATE_CANDIDATE_LIMIT_INVALID".into());
    }
    let document = plan
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    if document.sources.is_empty()
        || document.sources.iter().any(|source| {
            !matches!(
                source.kind.as_str(),
                "authored_json_v2" | "authored_csv_v1" | "authored_inline_v1"
            )
        })
    {
        return Err("DUPLICATE_CANDIDATES_ADD_ONLY".into());
    }
    let rendered = plan
        .rendered
        .iter()
        .find(|r| r.document_id == document_id)
        .ok_or("DOCUMENT_NOT_RENDERED")?;
    let (field, value, compared): (&str, &str, &[&str]) = match &document.content {
        LearningContent::Vocabulary(v) => (
            "Expression",
            &v.expression,
            &["Expression", "Language", "Reading", "SenseKey", "Meaning"],
        ),
        LearningContent::Grammar(g) => (
            "Pattern",
            &g.pattern,
            &["Pattern", "Language", "UseKey", "Meaning", "Formation"],
        ),
    };
    // Search all models: a legacy Basic or Picture Words note can be a candidate.
    // Anki search is approximate; only the returned fields are compared below.
    let mut query = format!("\"{}\"", search_value(value)?);
    let scope = plan
        .settings
        .values
        .get("selection.duplicate_scope")
        .and_then(Value::as_str)
        .unwrap_or("collection");
    let search_scope = match scope {
        "collection" => "collection_text_candidates",
        "target_deck" => {
            let suffix = if field == "Expression" {
                "vocab"
            } else {
                "grammar"
            };
            let prefix = match document
                .target_language
                .as_str()
                .split('-')
                .next()
                .unwrap_or("")
            {
                "ja" => "japanese",
                "en" => "english",
                _ => return Err("DUPLICATE_TARGET_DECK_PURPOSE_UNRESOLVED".into()),
            };
            let key = format!("purposes.{prefix}_{suffix}.target_deck");
            let deck = plan
                .settings
                .values
                .get(&key)
                .and_then(Value::as_str)
                .ok_or("DUPLICATE_TARGET_DECK_UNCONFIGURED")?;
            query = format!("{} {query}", linguist_anki::deck_query(deck)?);
            "target_deck_text_candidates"
        }
        _ => return Err("DUPLICATE_SCOPE_INVALID".into()),
    };
    if query.chars().count() > max_query_chars {
        return Err("DUPLICATE_QUERY_TOO_LARGE".into());
    }
    let ids = reader.find_notes(&query)?;
    if ids.len() > max_candidates {
        return Err(
            "DUPLICATE_CANDIDATES_TOO_MANY: narrow the input or increase selection.max_notes"
                .into(),
        );
    }
    let rows = reader.notes_info(&ids)?;
    if rows.len() != ids.len() {
        return Err("ANKI_INFO_COUNT_CONFLICT".into());
    }
    let mut candidates = Vec::with_capacity(ids.len());
    for (id, row) in ids.into_iter().zip(rows) {
        if row
            .get("noteId")
            .map(linguist_anki::wire_id)
            .transpose()?
            .as_deref()
            != Some(id.as_str())
        {
            return Err("ANKI_INFO_ID_CONFLICT".into());
        }
        let model_matches =
            row.get("modelName").and_then(Value::as_str) == Some(rendered.model.name.as_str());
        let fields = row
            .get("fields")
            .and_then(Value::as_object)
            .ok_or("ANKI_NOTE_FIELDS_INVALID")?;
        let compared_fields_match = model_matches
            && compared.iter().all(|field| {
                fields
                    .get(*field)
                    .and_then(|entry| entry.get("value"))
                    .and_then(Value::as_str)
                    == rendered.fields.get(*field).map(String::as_str)
            });
        let raw = |field: &str| {
            fields
                .get(field)
                .and_then(|entry| entry.get("value"))
                .and_then(Value::as_str)
        };
        let expected = |field: &str| rendered.fields.get(field).map(String::as_str);
        let sense_field = if field == "Expression" {
            "SenseKey"
        } else {
            "UseKey"
        };
        let relation = if !model_matches || raw(field) != expected(field) {
            "search_hit_unresolved"
        } else if raw("Language") != expected("Language") {
            "same_primary_other_language"
        } else if raw(sense_field) != expected(sense_field) {
            "same_primary_other_sense_or_use"
        } else if compared_fields_match {
            "same_compared_fields"
        } else {
            "same_identity_fields_other_content"
        };
        candidates.push(Candidate {
            note_id: id,
            model_matches,
            compared_fields_match,
            relation,
        });
    }
    Ok(Report {
        schema_version: 2,
        plan_id: plan.id,
        revision: plan.revision,
        plan_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        document_id,
        observed_at_unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "CLOCK_INVALID")?
            .as_secs(),
        candidates,
        search_scope,
        collection_duplicate_check_complete: false,
        semantic_identity_verified: false,
        apply_eligible: false,
    })
}

/// Record a candidate report as a reviewable duplicate decision point in a new
/// revision. Only the plan's latest revision with the given digest may be
/// extended. With no candidates nothing is recorded: an empty search is not
/// proof of absence.
pub fn record(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    digest: &str,
    report: &Report,
    created_at: u64,
) -> Result<Option<PlanRevision>, String> {
    use linguist_core::{
        Provenance, canonical,
        records::Evidence,
        validation::{self, Issue, Severity},
    };
    if store.latest_revision(base.id)? != base.revision
        || base.approval_digest().map_err(|e| e.to_string())? != digest
        || report.plan_id != base.id
        || report.revision != base.revision
        || report.plan_digest != digest
    {
        return Err("DUPLICATE_RECORD_BASE_CONFLICT".into());
    }
    if report.candidates.is_empty() {
        return Ok(None);
    }
    let mut child = base.clone();
    let document = child
        .documents
        .iter_mut()
        .find(|document| document.id == report.document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    let issue_id = format!("COLLECTION_DUPLICATE_REVIEW:{}", document.id);
    document
        .issues
        .retain(|issue| issue.id != issue_id && issue.stage != "validation");
    document
        .evidence
        .retain(|evidence| evidence.field != "duplicates");
    document
        .reviews
        .retain(|review| review.issue_id != issue_id);
    document.evidence.push(Evidence {
        id: Uuid::new_v4(),
        field: "duplicates".into(),
        provenance: Provenance::Provider,
        source_id: None,
        region_id: None,
        target: None,
        source_span: None,
        language: document.target_language.clone(),
        claim: serde_json::to_string(&serde_json::json!({
            "candidates": report.candidates,
            "search_scope": report.search_scope,
            "observed_at_unix_seconds": report.observed_at_unix_seconds,
            "recorded_at_unix_seconds": created_at,
            "collection_duplicate_check_complete": false,
        }))
        .map_err(|e| e.to_string())?,
        source_url: None,
        ambiguous: true,
    });
    let mut issue = Issue::new(
        "COLLECTION_DUPLICATE_REVIEW",
        Severity::Review,
        Some("duplicates"),
        "Existing notes may duplicate this item; choose create_new or skip for a candidate note.",
    );
    issue.id = issue_id;
    issue.stage = "duplicates".into();
    issue.source_refs = report
        .candidates
        .iter()
        .map(|c| c.note_id.clone())
        .collect();
    document.issues.push(issue);
    document.issues = validation::validate(document);
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("DUPLICATE_REVISION_LIMIT")?;
    child.parent_digest = Some(digest.into());
    child.binding = None;
    // The item is no longer ready; resolve re-renders it after the decision.
    child
        .rendered
        .retain(|rendered| rendered.document_id != report.document_id);
    let sources: Vec<_> = child.documents.iter().flat_map(|d| &d.sources).collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    store.publish_revision(&child)?;
    Ok(Some(child))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_value_escapes_anki_specials_and_rejects_controls() {
        assert_eq!(
            search_value("a\\b\"*_&<>").unwrap(),
            "a\\\\b\\\"\\*\\_&amp;&lt;&gt;"
        );
        assert!(search_value("a\nb").is_err());
        assert!(search_value("  ").is_err());
    }
}
