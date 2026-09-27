//! Typed review decisions resolve known review issues; they never waive structural errors.
use crate::{
    Issue, Severity,
    canonical::ContractError,
    records::{PlanRevision, ReviewChoice, ReviewDecision},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolutionRequest {
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub input_digest: String,
    pub actor: String,
    pub choice: ReviewChoice,
}
#[derive(Debug, Serialize)]
pub struct ResolutionResult {
    pub revision: PlanRevision,
    pub decision_id: uuid::Uuid,
    pub ready: bool,
}
pub fn resolve(
    base: &PlanRevision,
    request: &ResolutionRequest,
    created_at: String,
) -> Result<ResolutionResult, ContractError> {
    if request.schema_version != 2 {
        return Err(ContractError("UNSUPPORTED_RESOLUTION_VERSION".into()));
    }
    if request.base_revision != base.revision || request.base_digest != base.approval_digest()? {
        return Err(ContractError("REVIEW_BASE_CONFLICT".into()));
    }
    if request.actor.trim().is_empty()
        || request.actor.chars().count() > 200
        || request.actor.chars().any(char::is_control)
    {
        return Err(ContractError("INVALID_REVIEW_ACTOR".into()));
    }
    let mut candidate = base.clone();
    let document = candidate
        .documents
        .iter_mut()
        .find(|doc| doc.id == request.document_id)
        .ok_or_else(|| ContractError("REVIEW_DOCUMENT_NOT_FOUND".into()))?;
    if request.input_digest != document.semantic_digest()? {
        return Err(ContractError("REVIEW_INPUT_CONFLICT".into()));
    }
    document.issues.retain(|issue| issue.stage != "validation");
    let issues = crate::validation::validate(document);
    let issue = issues
        .iter()
        .find(|issue| issue.id == request.issue_id)
        .ok_or_else(|| ContractError("REVIEW_ISSUE_NOT_UNRESOLVED".into()))?;
    if issue.severity != Severity::Review {
        return Err(ContractError("ISSUE_CANNOT_BE_WAIVED".into()));
    }
    match &request.choice {
        ReviewChoice::ContentVerified { evidence_ids } if issue.code == "GENERATED_FACT_REVIEW" => {
            let selected: BTreeSet<_> = evidence_ids.iter().map(|id| id.to_string()).collect();
            let required: BTreeSet<_> = issue.source_refs.iter().cloned().collect();
            if selected.len() != evidence_ids.len() || selected.is_empty() || selected != required {
                return Err(ContractError("REVIEW_EVIDENCE_MISMATCH".into()));
            }
            if evidence_ids
                .iter()
                .any(|id| !document.evidence.iter().any(|evidence| evidence.id == *id))
            {
                return Err(ContractError("REVIEW_EVIDENCE_MISSING".into()));
            }
        }
        ReviewChoice::Sense(key) if issue.code == "DICTIONARY_SENSE_REVIEW" => {
            let crate::LearningContent::Vocabulary(vocab) = &mut document.content else {
                return Err(ContractError("REVIEW_KIND_CONFLICT".into()));
            };
            let choices: Vec<_> = vocab
                .dictionary
                .iter()
                .flat_map(|entry| {
                    entry
                        .senses
                        .iter()
                        .filter(|sense| sense.key == *key)
                        .map(move |sense| (entry, sense))
                })
                .collect();
            if choices.len() != 1 {
                return Err(ContractError("DICTIONARY_SENSE_CONFLICT".into()));
            }
            let (entry, sense) = choices[0];
            if !entry
                .forms
                .iter()
                .chain(&entry.readings)
                .any(|form| form == &vocab.expression)
            {
                return Err(ContractError("DICTIONARY_EXPRESSION_CONFLICT".into()));
            }
            let readings = dictionary_readings(entry, &vocab.expression)?;
            if document.target_language.as_str().split('-').next() == Some("en")
                && readings.is_empty()
            {
                if !vocab.reading.is_empty() {
                    return Err(ContractError(
                        "CAPABILITY_UNAVAILABLE: this dictionary does not expose lexical readings"
                            .into(),
                    ));
                }
            } else if vocab.reading.is_empty() {
                if readings.len() != 1 {
                    return Err(ContractError("CAPABILITY_UNAVAILABLE: multiple dictionary readings require a typed reading selection".into()));
                }
                vocab.reading = readings.iter().next().unwrap().clone();
            } else if !readings.contains(&vocab.reading) {
                return Err(ContractError("DICTIONARY_READING_CONFLICT".into()));
            }
            for example in &sense.examples {
                if !vocab.examples.iter().any(|existing| {
                    existing.sentence == example.sentence
                        && existing.translation == example.translation
                }) {
                    vocab.examples.push(example.clone());
                }
            }
            vocab.meaning = sense.definitions.join("; ");
            vocab.sense_key = key.clone();
            // Decisions bound to the previous semantic document must not survive selected facts changing.
            let old_ids: BTreeSet<_> = document.reviews.iter().map(|review| review.id).collect();
            document.reviews.clear();
            candidate
                .review_decisions
                .retain(|decision| !old_ids.contains(&decision.id));
        }
        _ => {
            return Err(ContractError(
                "CAPABILITY_UNAVAILABLE: this issue/decision pipeline is not implemented".into(),
            ));
        }
    }
    let decision = ReviewDecision {
        id: uuid::Uuid::new_v4(),
        issue_id: request.issue_id.clone(),
        input_digest: document.semantic_digest()?,
        actor: request.actor.clone(),
        created_at,
        choice: request.choice.clone(),
    };
    document.reviews.push(decision.clone());
    if crate::validation::validate(document)
        .iter()
        .any(|issue| issue.id == request.issue_id)
    {
        return Err(ContractError("REVIEW_DECISION_NOT_APPLICABLE".into()));
    }
    candidate.review_decisions.push(decision.clone());
    candidate.revision = base
        .revision
        .checked_add(1)
        .ok_or_else(|| ContractError("REVISION_LIMIT".into()))?;
    candidate.parent_digest = Some(request.base_digest.clone());
    candidate.rendered.clear();
    let mut ready = true;
    for document in &mut candidate.documents {
        document.issues.retain(|issue| issue.stage != "validation");
        document.issues = crate::validation::validate(document);
        let empty = BTreeMap::new();
        let fields = document
            .sources
            .first()
            .map(|source| &source.fields)
            .unwrap_or(&empty);
        match crate::render::render(document, fields) {
            Ok(rendered) => candidate.rendered.push(rendered),
            Err(_) => {
                ready = false;
                if document
                    .issues
                    .iter()
                    .all(|issue| issue.severity == Severity::Warning)
                {
                    document.issues.push(Issue::new(
                        "EFFECTIVE_RENDER_BLOCKED",
                        Severity::Error,
                        None,
                        "Effective fields cannot render; edit the plan.",
                    ));
                }
            }
        }
        ready &= document
            .issues
            .iter()
            .all(|issue| issue.severity == Severity::Warning);
    }
    Ok(ResolutionResult {
        revision: candidate,
        decision_id: decision.id,
        ready,
    })
}

/// Keep written-form/reading associations when a provider supplied them.
pub fn dictionary_readings(
    entry: &crate::DictionaryEntry,
    expression: &str,
) -> Result<BTreeSet<String>, ContractError> {
    if let Some(values) = entry.metadata.get("written_form_pairs_json") {
        if values.len() != 1 {
            return Err(ContractError("DICTIONARY_FORM_MAPPING_INVALID".into()));
        }
        let pairs: Vec<serde_json::Value> = crate::canonical::parse(values[0].as_bytes())?;
        let mut readings = BTreeSet::new();
        for pair in pairs {
            let reading = pair
                .get("reading")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let word = pair
                .get("word")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if (word == expression || reading == expression) && !reading.is_empty() {
                readings.insert(reading.to_owned());
            }
        }
        Ok(readings)
    } else if entry.readings.iter().any(|reading| reading == expression) {
        Ok(BTreeSet::from([expression.to_owned()]))
    } else {
        Ok(entry.readings.iter().cloned().collect())
    }
}
