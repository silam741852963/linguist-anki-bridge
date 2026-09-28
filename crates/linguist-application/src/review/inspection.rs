//! Compact, paginated review identities and templates, without raw source archives.
use linguist_core::{
    LearningContent, Severity,
    records::{PlanRevision, ReviewChoice},
};

pub fn page(
    plan: &PlanRevision,
    document_id: Option<uuid::Uuid>,
    after_index: u32,
    limit: u32,
) -> Result<serde_json::Value, String> {
    if !(1..=1000).contains(&limit) {
        return Err("PLAN_REVIEW_LIMIT_INVALID".into());
    }
    if document_id.is_some_and(|id| !plan.documents.iter().any(|document| document.id == id)) {
        return Err("PLAN_DOCUMENT_NOT_FOUND".into());
    }
    let validation = linguist_core::plan_validation::inspect(plan).map_err(|e| e.to_string())?;
    let mut issues = Vec::new();
    for item in &validation.items {
        if document_id.is_some_and(|id| id != item.document_id) {
            continue;
        }
        let document = plan
            .documents
            .iter()
            .find(|document| document.id == item.document_id)
            .unwrap();
        for issue in item
            .issues
            .iter()
            .filter(|issue| issue.severity != Severity::Warning)
        {
            issues.push((document, item, issue));
        }
    }
    let total = issues.len();
    let entries: Vec<_> = issues.iter().enumerate().skip(after_index as usize).take(limit as usize).map(|(index, (document, item, issue))| {
        let choices = linguist_core::review::decision_templates(document, issue);
        let manual_media = matches!(issue.code.as_str(), "SOURCE_MEDIA_CONTENT_REVIEW" | "SOURCE_MEDIA_FORMAT_REVIEW" | "SOURCE_AUDIO_COMPLETENESS_REVIEW");
        let templates: Vec<_> = choices.iter().map(|choice| {
            let (key, reading) = match choice {
                ReviewChoice::Sense(key) => (Some(key), None),
                ReviewChoice::SenseWithReading { key, reading } => (Some(key), Some(reading)),
                _ => (None, None),
            };
            let definitions = key.and_then(|key| match &document.content {
                LearningContent::Vocabulary(vocab) => vocab.dictionary.iter().flat_map(|entry| &entry.senses).find(|sense| sense.key == *key).map(|sense| sense.definitions.clone()),
                _ => None,
            });
            serde_json::json!({"choice":choice,"definitions":definitions,"reading":reading,"requires_authored_content":matches!(choice, ReviewChoice::Cue { .. } | ReviewChoice::Exercise { .. })})
        }).collect();
        serde_json::json!({"index":index,"request_identity":{"schema_version":2,"base_revision":plan.revision,"base_digest":validation.plan_digest,"document_id":document.id,"issue_id":issue.id,"input_digest":item.semantic_digest},"issue":issue,"actor_required":true,"resolution_available":!templates.is_empty() || manual_media,"manual_media_decision_required":manual_media,"templates":templates})
    }).collect();
    let end = (after_index as usize).saturating_add(entries.len());
    let next = if end < total { Some(end) } else { None };
    Ok(
        serde_json::json!({"schema_version":2,"plan_id":plan.id,"revision":plan.revision,"digest":validation.plan_digest,"scope":"unresolved_issue_page","issues":entries,"total_issues":total,"next_index":next,"archives_included":false,"native_verified":false,"apply_eligible":false}),
    )
}
