//! Typed review decisions resolve known review issues; they never waive structural errors.
mod cue;
mod media;
use crate::{
    Issue, Severity,
    canonical::ContractError,
    records::{PlanRevision, ReviewChoice, ReviewDecision},
};
pub(crate) use media::source_media_matches;
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
    if issue.severity != Severity::Review && !cue::applicable(document, issue, &request.choice) {
        return Err(ContractError("ISSUE_CANNOT_BE_WAIVED".into()));
    }
    match &request.choice {
        ReviewChoice::Cue { .. } | ReviewChoice::Exercise { .. } => {
            cue::repair(document, issue, &request.choice)?;
            let old_ids: BTreeSet<_> = document.reviews.iter().map(|review| review.id).collect();
            document.reviews.clear();
            candidate
                .review_decisions
                .retain(|decision| !old_ids.contains(&decision.id));
        }
        ReviewChoice::SourceMediaRole {
            source_id,
            asset_digest,
            original_filename,
            role,
            attribution,
            license,
            ..
        } => {
            if !source_media_matches(document, issue, &request.choice, false) {
                return Err(ContractError(
                    "REVIEW_SOURCE_MEDIA_EVIDENCE_MISMATCH".into(),
                ));
            }
            let prior = document.semantic_digest()?;
            let asset = document
                .media
                .iter_mut()
                .find(|asset| {
                    asset.source_id == Some(*source_id)
                        && asset.digest == *asset_digest
                        && asset.original_filename.as_ref() == Some(original_filename)
                })
                .unwrap();
            asset.role = *role;
            asset.filename =
                media::role_filename(asset_digest, &asset.mime, original_filename, *role)
                    .ok_or_else(|| ContractError("REVIEW_MEDIA_TYPE_CONFLICT".into()))?;
            asset.attribution = attribution.clone();
            asset.license = license.clone();
            if document.semantic_digest()? != prior {
                let old_ids: BTreeSet<_> = document.reviews.iter().map(|r| r.id).collect();
                document.reviews.clear();
                candidate
                    .review_decisions
                    .retain(|r| !old_ids.contains(&r.id));
            }
        }
        ReviewChoice::SourceContentVerified {
            source_id,
            evidence_ids,
        } if matches!(
            issue.code.as_str(),
            "SOURCE_HTML_TEXT_REVIEW" | "SOURCE_EXAMPLES_REVIEW"
        ) =>
        {
            if !source_content_verified(document, issue, *source_id, evidence_ids) {
                return Err(ContractError("REVIEW_SOURCE_EVIDENCE_MISMATCH".into()));
            }
        }
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
        ReviewChoice::Sense(key) | ReviewChoice::SenseWithReading { key, .. }
            if issue.code == "DICTIONARY_SENSE_REVIEW" =>
        {
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
            if let ReviewChoice::SenseWithReading { reading, .. } = &request.choice {
                if !readings.contains(reading) {
                    return Err(ContractError("DICTIONARY_READING_CONFLICT".into()));
                }
                vocab.reading = reading.clone();
            }
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
        let observations: Vec<_> = document
            .issues
            .iter()
            .filter(|issue| {
                issue.stage == "capture"
                    && matches!(
                        issue.code.as_str(),
                        "SOURCE_HTML_TEXT_REVIEW"
                            | "SOURCE_EXAMPLES_REVIEW"
                            | "SOURCE_MEDIA_CONTENT_REVIEW"
                            | "SOURCE_MEDIA_FORMAT_REVIEW"
                            | "SOURCE_AUDIO_COMPLETENESS_REVIEW"
                    )
            })
            .cloned()
            .collect();
        document.issues = crate::validation::validate(document);
        for mut observation in observations {
            if !document
                .issues
                .iter()
                .any(|issue| issue.id == observation.id)
            {
                observation.severity = Severity::Warning;
                document.issues.push(observation);
            }
        }
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

/// Only derived content review, never source task/history/identity or structural issues.
pub(crate) fn source_content_verified(
    document: &crate::LearningDocument,
    issue: &Issue,
    source_id: uuid::Uuid,
    selected: &[uuid::Uuid],
) -> bool {
    if issue.stage != "capture"
        || !matches!(
            issue.code.as_str(),
            "SOURCE_HTML_TEXT_REVIEW" | "SOURCE_EXAMPLES_REVIEW"
        )
        || issue.source_refs != vec![source_id.to_string()]
        || !document.sources.iter().any(|source| source.id == source_id)
    {
        return false;
    }
    let required: BTreeSet<_> = document
        .evidence
        .iter()
        .filter(|e| {
            e.source_id == Some(source_id)
                && e.provenance == crate::Provenance::Source
                && Some(&e.field) == issue.field.as_ref()
        })
        .map(|e| e.id)
        .collect();
    let chosen: BTreeSet<_> = selected.iter().copied().collect();
    !required.is_empty() && chosen.len() == selected.len() && chosen == required
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
