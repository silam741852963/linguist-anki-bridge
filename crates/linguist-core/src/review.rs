//! Typed review decisions resolve known review issues; they never waive structural errors.
mod cue;
mod media;
use crate::{
    Issue, Severity,
    canonical::ContractError,
    records::{MediaRole, PlanRevision, ReviewChoice, ReviewDecision},
};
pub(crate) use media::source_media_matches;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolutionRequest {
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub input_digest: String,
    pub actor: String,
    pub choice: ReviewChoice,
}
/// `plans resolve-batch`: decisions bound to one revision and, each, to its
/// document's semantic digest at that revision.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolutionBatch {
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub actor: String,
    pub decisions: Vec<BatchDecision>,
}

/// One decision. Exactly one of `choice` and `history_map` is set;
/// `history_map` resolves SOURCE_NATIVE_HISTORY_REVIEW from live companion
/// evidence like `plans resolve-history`. `options` is informational (the
/// templates `plans resolve-batch --template` printed) and is ignored.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchDecision {
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub input_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<ReviewChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_map: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ResolutionResult {
    pub revision: PlanRevision,
    pub decision_id: uuid::Uuid,
    pub ready: bool,
}
/// Supported fact choices and repair skeletons; every submitted decision is revalidated.
pub fn decision_templates(document: &crate::LearningDocument, issue: &Issue) -> Vec<ReviewChoice> {
    let mut choices = Vec::new();
    if issue.code == "SOURCE_MEDIA_MISSING_REVIEW"
        && let (Some(filename), Some(source)) = (issue.field.as_ref(), issue.source_refs.first())
        && let Ok(source_id) = source.parse()
    {
        let choice = ReviewChoice::MissingMedia {
            source_id,
            filename: filename.clone(),
        };
        if missing_media_matches(document, issue, &choice) {
            choices.push(choice);
        }
    }
    for task in [
        crate::Task::Production,
        crate::Task::Spelling,
        crate::Task::Recognition,
    ] {
        // Suggestions come only from current facts and are leak-checked; the
        // reviewer still has to submit (and may edit) the text.
        let suggestion = if task == crate::Task::Recognition {
            crate::cues::suggest_recognition(document)
        } else {
            crate::cues::suggest(document, task)
        };
        let choice = ReviewChoice::Cue {
            task,
            text: suggestion.unwrap_or_default(),
        };
        if cue::applicable(document, issue, &choice) {
            choices.push(choice);
        }
    }
    let (prompt, answer) = crate::cues::suggest_exercise(document).unwrap_or_default();
    let exercise = ReviewChoice::Exercise { prompt, answer };
    if cue::applicable(document, issue, &exercise) {
        choices.push(exercise);
    }
    if issue.code == "GRAMMAR_SEGMENTATION_REVIEW" && issue.severity == Severity::Review {
        let ids: Vec<uuid::Uuid> = issue
            .source_refs
            .iter()
            .filter_map(|id| uuid::Uuid::parse_str(id).ok())
            .collect();
        if ids.len() >= 2 {
            choices.push(ReviewChoice::Segmentation(ids.clone()));
        }
        choices.extend(
            ids.into_iter()
                .map(|id| ReviewChoice::Segmentation(vec![id])),
        );
    }
    for region in &document.regions {
        let choice = ReviewChoice::Expression {
            region_id: region.id,
        };
        if expression_applicable(document, issue, &choice) {
            choices.push(choice);
        }
    }
    if CANDIDATE_REVIEW_CODES.contains(&issue.code.as_str()) && issue.severity == Severity::Review {
        choices.extend(issue.source_refs.iter().cloned().map(ReviewChoice::Media));
        choices.push(ReviewChoice::Media(String::new()));
    }
    if issue.code == "COLLECTION_DUPLICATE_REVIEW" && issue.severity == Severity::Review {
        for note_id in &issue.source_refs {
            if let Ok(note_id) = crate::AnkiId::try_from(note_id.clone()) {
                for action in DUPLICATE_ACTIONS {
                    choices.push(ReviewChoice::Duplicate {
                        note_id: note_id.clone(),
                        action: action.into(),
                    });
                }
            }
        }
    }
    if issue.code == "DICTIONARY_SENSE_REVIEW"
        && issue.severity == Severity::Review
        && let crate::LearningContent::Vocabulary(vocab) = &document.content
    {
        let mut key_counts = BTreeMap::<&str, usize>::new();
        for sense in vocab.dictionary.iter().flat_map(|entry| &entry.senses) {
            *key_counts.entry(&sense.key).or_default() += 1;
        }
        for entry in &vocab.dictionary {
            if !entry
                .forms
                .iter()
                .chain(&entry.readings)
                .any(|form| form == &vocab.expression)
            {
                continue;
            }
            let Ok(readings) = dictionary_readings(entry, &vocab.expression) else {
                continue;
            };
            for sense in &entry.senses {
                if key_counts.get(sense.key.as_str()) != Some(&1) {
                    continue;
                }
                if readings.is_empty()
                    && document.target_language.as_str().split('-').next() == Some("en")
                    && vocab.reading.is_empty()
                {
                    choices.push(ReviewChoice::Sense(sense.key.clone()));
                } else {
                    choices.extend(
                        readings
                            .iter()
                            .map(|reading| ReviewChoice::SenseWithReading {
                                key: sense.key.clone(),
                                reading: reading.clone(),
                            }),
                    );
                }
            }
        }
    }
    if issue.source_refs.len() == 1
        && let Ok(source_id) = uuid::Uuid::parse_str(&issue.source_refs[0])
    {
        let evidence_ids: Vec<_> = document
            .evidence
            .iter()
            .filter(|evidence| {
                evidence.source_id == Some(source_id)
                    && matches!(
                        evidence.provenance,
                        crate::Provenance::Source | crate::Provenance::Ocr
                    )
                    && Some(&evidence.field) == issue.field.as_ref()
            })
            .map(|evidence| evidence.id)
            .collect();
        if source_content_verified(document, issue, source_id, &evidence_ids) {
            choices.push(ReviewChoice::SourceContentVerified {
                source_id,
                evidence_ids,
            });
        }
    }
    let dropped_field = match (issue.code.as_str(), issue.source_refs.as_slice()) {
        ("SOURCE_UNMAPPED_FIELD_REVIEW" | "SOURCE_MEDIA_DISCOVERY_REVIEW", [source]) => {
            issue.field.as_ref().map(|f| (source, f))
        }
        ("SOURCE_STRUCTURED_ROLE_REVIEW", [source, field]) => Some((source, field)),
        _ => None,
    };
    if let Some((source, field)) = dropped_field
        && let Ok(source_id) = uuid::Uuid::parse_str(source)
        && source_field_dropped(document, issue, source_id, field)
    {
        choices.push(ReviewChoice::SourceFieldDropped {
            source_id,
            field: field.clone(),
        });
    }
    if issue.code == "GENERATED_FACT_REVIEW" && issue.severity == Severity::Review {
        let ids: Option<Vec<_>> = issue
            .source_refs
            .iter()
            .map(|id| uuid::Uuid::parse_str(id).ok())
            .collect();
        if let Some(evidence_ids) = ids
            && !evidence_ids.is_empty()
            && evidence_ids.iter().collect::<BTreeSet<_>>().len() == evidence_ids.len()
            && evidence_ids
                .iter()
                .all(|id| document.evidence.iter().any(|evidence| evidence.id == *id))
        {
            let single = (evidence_ids.len() == 1).then(|| evidence_ids[0]);
            choices.push(ReviewChoice::ContentVerified { evidence_ids });
            if let Some(evidence_id) = single {
                choices.push(ReviewChoice::ContentRejected { evidence_id });
            }
        }
    }
    choices
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
    if issue.severity != Severity::Review
        && !cue::applicable(document, issue, &request.choice)
        && !expression_applicable(document, issue, &request.choice)
    {
        return Err(ContractError("ISSUE_CANNOT_BE_WAIVED".into()));
    }
    match &request.choice {
        ReviewChoice::Cue { .. } | ReviewChoice::Exercise { .. } => {
            cue::repair(document, issue, &request.choice)?;
            // State-checked decisions are revalidated below; all others are invalidated.
            let old_ids: BTreeSet<_> = document
                .reviews
                .iter()
                .filter(|review| !rebindable(&review.choice))
                .map(|review| review.id)
                .collect();
            document
                .reviews
                .retain(|review| !old_ids.contains(&review.id));
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
        ) || OCR_REVIEW_CODES.contains(&issue.code.as_str()) =>
        {
            if !source_content_verified(document, issue, *source_id, evidence_ids) {
                return Err(ContractError("REVIEW_SOURCE_EVIDENCE_MISMATCH".into()));
            }
        }
        ReviewChoice::SourceFieldDropped { source_id, field }
            if matches!(
                issue.code.as_str(),
                "SOURCE_UNMAPPED_FIELD_REVIEW"
                    | "SOURCE_STRUCTURED_ROLE_REVIEW"
                    | "SOURCE_MEDIA_DISCOVERY_REVIEW"
            ) =>
        {
            if !source_field_dropped(document, issue, *source_id, field) {
                return Err(ContractError("REVIEW_SOURCE_FIELD_MISMATCH".into()));
            }
        }
        ReviewChoice::Segmentation(ids) if issue.code == "GRAMMAR_SEGMENTATION_REVIEW" => {
            if !segmentation_valid(issue, ids) {
                return Err(ContractError("REVIEW_SEGMENTATION_INVALID".into()));
            }
            if ids.len() == 1 {
                let text = document
                    .regions
                    .iter()
                    .find(|region| region.id == ids[0])
                    .map(|region| region.text.trim().to_owned())
                    .unwrap_or_default();
                if let crate::LearningContent::Grammar(grammar) = &mut document.content
                    && grammar.pattern.trim().is_empty()
                {
                    // The chosen region becomes the pattern verbatim (operators kept).
                    grammar.pattern = text;
                    let old_ids: BTreeSet<_> =
                        document.reviews.iter().map(|review| review.id).collect();
                    document.reviews.clear();
                    candidate
                        .review_decisions
                        .retain(|decision| !old_ids.contains(&decision.id));
                }
            }
        }
        ReviewChoice::ContentVerified { evidence_ids } if issue.code == "SOURCE_CLAIM_CONFLICT" => {
            let selected: BTreeSet<_> = evidence_ids.iter().map(|id| id.to_string()).collect();
            let required: BTreeSet<_> = issue.source_refs.iter().cloned().collect();
            if selected.len() != evidence_ids.len() || selected != required {
                return Err(ContractError("REVIEW_EVIDENCE_MISMATCH".into()));
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
        ReviewChoice::ContentRejected { evidence_id } if issue.code == "GENERATED_FACT_REVIEW" => {
            if issue.source_refs != vec![evidence_id.to_string()] {
                return Err(ContractError("REVIEW_EVIDENCE_MISMATCH".into()));
            }
            reject_generated(document, *evidence_id)?;
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
        ReviewChoice::Expression { region_id } => {
            if !expression_applicable(document, issue, &request.choice) {
                return Err(ContractError("REVIEW_EXPRESSION_REGION_INVALID".into()));
            }
            let text = document
                .regions
                .iter()
                .find(|region| region.id == *region_id)
                .map(|region| region.text.trim().to_owned())
                .unwrap_or_default();
            if let crate::LearningContent::Vocabulary(vocab) = &mut document.content {
                vocab.expression = text;
            }
            // Identity changed: every earlier decision is invalidated.
            let old_ids: BTreeSet<_> = document.reviews.iter().map(|review| review.id).collect();
            document.reviews.clear();
            candidate
                .review_decisions
                .retain(|decision| !old_ids.contains(&decision.id));
        }
        ReviewChoice::Media(digest) if CANDIDATE_REVIEW_CODES.contains(&issue.code.as_str()) => {
            select_candidate(document, issue, digest)?;
        }
        ReviewChoice::Duplicate { note_id, action }
            if issue.code == "COLLECTION_DUPLICATE_REVIEW" =>
        {
            if !duplicate_choice_matches(issue, note_id, action) {
                return Err(ContractError("REVIEW_DUPLICATE_CHOICE_INVALID".into()));
            }
        }
        ReviewChoice::MissingMedia { .. } if issue.code == "SOURCE_MEDIA_MISSING_REVIEW" => {
            if !missing_media_matches(document, issue, &request.choice) {
                return Err(ContractError("REVIEW_MISSING_MEDIA_MISMATCH".into()));
            }
        }
        ReviewChoice::NativeHistory {
            source_id,
            task_map,
            ..
        } if issue.code == "SOURCE_NATIVE_HISTORY_REVIEW" => {
            // The task map is part of the document; content facts that other
            // decisions were checked against are unchanged, so those decisions
            // move to the new digest and are re-validated below.
            let prior = document.semantic_digest()?;
            document.task_maps.retain(|map| map.source_id != *source_id);
            document.task_maps.push(task_map.clone());
            // Every mapped card keeps its task: studied history is never
            // dropped to satisfy new-note task defaults.
            for entry in &task_map.entries {
                if !document.requested_tasks.contains(&entry.target_task) {
                    document.requested_tasks.push(entry.target_task);
                }
            }
            let current = document.semantic_digest()?;
            let moved: BTreeSet<_> = document
                .reviews
                .iter()
                .filter(|review| review.input_digest == prior)
                .map(|review| review.id)
                .collect();
            for review in &mut document.reviews {
                if moved.contains(&review.id) {
                    review.input_digest = current.clone();
                }
            }
            for decision in &mut candidate.review_decisions {
                if moved.contains(&decision.id) {
                    decision.input_digest = current.clone();
                }
            }
            if !native_history_matches(document, issue, &request.choice) {
                return Err(ContractError("REVIEW_NATIVE_HISTORY_MISMATCH".into()));
            }
        }
        // Accept ALG-SPLIT for this unit: the named anchor keeps the source
        // note and its history; every other unit becomes a fresh note.
        ReviewChoice::Anchor(anchor) if issue.code == "GRAMMAR_SPLIT_NATIVE_REVIEW" => {
            if !split_anchor_matches(document, *anchor) {
                return Err(ContractError("REVIEW_SPLIT_ANCHOR_MISMATCH".into()));
            }
        }
        _ => {
            return Err(ContractError(
                "CAPABILITY_UNAVAILABLE: this issue/decision pipeline is not implemented".into(),
            ));
        }
    }
    // A typed cue/media/duplicate decision does not change the facts that sense,
    // media and duplicate decisions are checked against. Rebind those that still
    // validate to the new content digest; drop any that no longer hold.
    if !matches!(
        request.choice,
        ReviewChoice::Sense(_) | ReviewChoice::SenseWithReading { .. }
    ) {
        let current = document.semantic_digest()?;
        let mut rebound = BTreeSet::new();
        for review in &mut document.reviews {
            if rebindable(&review.choice) && review.input_digest != current {
                review.input_digest = current.clone();
                rebound.insert(review.id);
            }
        }
        let open: BTreeSet<_> = crate::validation::validate(document)
            .into_iter()
            .map(|issue| issue.id)
            .collect();
        let stale: BTreeSet<_> = document
            .reviews
            .iter()
            .filter(|review| rebound.contains(&review.id) && open.contains(&review.issue_id))
            .map(|review| review.id)
            .collect();
        document
            .reviews
            .retain(|review| !stale.contains(&review.id));
        candidate
            .review_decisions
            .retain(|decision| !stale.contains(&decision.id));
        for decision in &mut candidate.review_decisions {
            if let Some(review) = document.reviews.iter().find(|r| r.id == decision.id) {
                decision.input_digest = review.input_digest.clone();
            }
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
            .filter(|issue| crate::validation::reopenable(issue))
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

/// Staged provider media candidates: a reviewer picks one digest or declines ("").
pub const CANDIDATE_REVIEW_CODES: [&str; 2] = ["IMAGE_CANDIDATE_REVIEW", "AUDIO_CANDIDATE_REVIEW"];
/// Duplicate decisions are recorded intent; apply enforces them separately.
pub const DUPLICATE_ACTIONS: [&str; 2] = ["create_new", "skip"];

fn candidate_role(issue: &Issue) -> MediaRole {
    if issue.code == "AUDIO_CANDIDATE_REVIEW" {
        MediaRole::Audio
    } else {
        MediaRole::Picture
    }
}

/// Exactly the chosen candidate renders in the issue's role; every other
/// candidate stays archive-only. "" declines all candidates.
pub(crate) fn candidate_choice_matches(
    document: &crate::LearningDocument,
    issue: &Issue,
    digest: &str,
) -> bool {
    let role = candidate_role(issue);
    (digest.is_empty() || issue.source_refs.iter().any(|d| d == digest))
        && issue.source_refs.iter().all(|candidate| {
            document.media.iter().any(|asset| {
                asset.digest == *candidate
                    && asset.source_id.is_none()
                    && if candidate == digest {
                        asset.role == role
                    } else {
                        asset.role == MediaRole::Archive
                    }
            })
        })
}

fn select_candidate(
    document: &mut crate::LearningDocument,
    issue: &Issue,
    digest: &str,
) -> Result<(), ContractError> {
    if !digest.is_empty() && !issue.source_refs.iter().any(|d| d == digest) {
        return Err(ContractError("REVIEW_CANDIDATE_UNKNOWN".into()));
    }
    let role = candidate_role(issue);
    // Rendering roles require a decoded-inspection receipt for that candidate.
    if !digest.is_empty()
        && !document.evidence.iter().any(|e| {
            e.target
                == Some(crate::records::EvidenceTarget::MediaAsset {
                    digest: digest.into(),
                })
                && serde_json::from_str::<serde_json::Value>(&e.claim)
                    .is_ok_and(|claim| claim["inspection"]["mime"].is_string())
        })
    {
        return Err(ContractError("REVIEW_CANDIDATE_UNINSPECTED".into()));
    }
    for candidate in &issue.source_refs {
        let asset = document
            .media
            .iter_mut()
            .find(|asset| asset.digest == *candidate && asset.source_id.is_none())
            .ok_or_else(|| ContractError("REVIEW_CANDIDATE_MISSING".into()))?;
        if candidate == digest {
            asset.filename =
                media::role_filename(&asset.digest, &asset.mime, &asset.filename, role)
                    .ok_or_else(|| ContractError("REVIEW_MEDIA_TYPE_CONFLICT".into()))?;
            asset.role = role;
        } else {
            asset.role = MediaRole::Archive;
        }
    }
    Ok(())
}

/// An empty vocabulary expression may be taken from one single-line OCR region.
fn expression_applicable(
    document: &crate::LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> bool {
    let ReviewChoice::Expression { region_id } = choice else {
        return false;
    };
    issue.code == "REQUIRED_CONTENT"
        && issue.field.as_deref() == Some("expression")
        && matches!(&document.content, crate::LearningContent::Vocabulary(v) if v.expression.trim().is_empty())
        && document.regions.iter().any(|region| {
            region.id == *region_id
                && !region.text.trim().is_empty()
                && !region.text.contains('\n')
                && region.text.chars().count() <= 200
        })
}

/// Decisions whose validity is fully re-checked against document state.
/// Decisions checked against facts other than generated content (sense,
/// media, source history, duplicates, segmentation, split anchor).
pub fn rebindable(choice: &ReviewChoice) -> bool {
    matches!(
        choice,
        ReviewChoice::NativeHistory { .. }
            | ReviewChoice::MissingMedia { .. }
            | ReviewChoice::Sense(_)
            | ReviewChoice::SenseWithReading { .. }
            | ReviewChoice::Media(_)
            | ReviewChoice::Duplicate { .. }
            | ReviewChoice::Segmentation(_)
            | ReviewChoice::Anchor(_)
    )
}

/// True when this split unit's recorded request names `anchor` as the unit
/// that keeps the source note.
pub(crate) fn split_anchor_matches(document: &crate::LearningDocument, anchor: uuid::Uuid) -> bool {
    document
        .sources
        .iter()
        .filter(|source| {
            matches!(
                source.kind.as_str(),
                crate::records::GRAMMAR_SPLIT_SOURCE | crate::records::VOCABULARY_SPLIT_SOURCE
            )
        })
        .filter_map(|source| source.fields.get("split_request"))
        .filter_map(|raw| crate::canonical::parse::<serde_json::Value>(raw.as_bytes()).ok())
        .any(|request| request["document_id"].as_str() == Some(anchor.to_string().as_str()))
}

/// Segmentation names one or more recorded candidate regions, in reading order.
pub(crate) fn segmentation_valid(issue: &Issue, ids: &[uuid::Uuid]) -> bool {
    let positions: Option<Vec<usize>> = ids
        .iter()
        .map(|id| issue.source_refs.iter().position(|r| *r == id.to_string()))
        .collect();
    !ids.is_empty() && positions.is_some_and(|positions| positions.windows(2).all(|w| w[0] < w[1]))
}

pub(crate) fn duplicate_choice_matches(
    issue: &Issue,
    note_id: &crate::AnkiId,
    action: &str,
) -> bool {
    let note_id = String::from(note_id.clone());
    DUPLICATE_ACTIONS.contains(&action) && issue.source_refs.contains(&note_id)
}

/// OCR observations a reviewer may confirm after inspecting the image and every
/// recorded region; manual transcription is a separate content edit.
pub const OCR_REVIEW_CODES: [&str; 3] = [
    "OCR_TEXT_REVIEW",
    "IMAGE_CLASSIFICATION_REVIEW",
    "OCR_FAILED_REVIEW",
];

/// A `MissingMedia` decision holds while the source still references the
/// file and the document has no archived bytes for it.
pub(crate) fn missing_media_matches(
    document: &crate::LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> bool {
    let ReviewChoice::MissingMedia {
        source_id,
        filename,
    } = choice
    else {
        return false;
    };
    issue.code == "SOURCE_MEDIA_MISSING_REVIEW"
        && issue.field.as_ref() == Some(filename)
        && issue.source_refs == [source_id.to_string()]
        && document
            .sources
            .iter()
            .any(|s| s.id == *source_id && s.media_refs.contains(filename))
        && !document.media.iter().any(|m| {
            m.source_id == Some(*source_id)
                && (m.original_filename.as_ref() == Some(filename) || &m.filename == filename)
        })
}

/// A `NativeHistory` decision holds while its evidence digest, source model
/// and task map still describe the document: every observed card's template
/// ordinal is mapped (retained), and the map is the document's map for it.
pub(crate) fn native_history_matches(
    document: &crate::LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> bool {
    let ReviewChoice::NativeHistory {
        source_id,
        cards,
        evidence_digest,
        task_map,
    } = choice
    else {
        return false;
    };
    let Some(source) = document.sources.iter().find(|s| s.id == *source_id) else {
        return false;
    };
    let ordinals: BTreeSet<u16> = cards.iter().map(|card| card.ordinal).collect();
    let ids: BTreeSet<String> = cards
        .iter()
        .map(|card| String::from(card.card_id.clone()))
        .collect();
    issue.code == "SOURCE_NATIVE_HISTORY_REVIEW"
        && issue.source_refs == [source_id.to_string()]
        && source.kind == "anki_read_capture_v2"
        && !cards.is_empty()
        && ordinals.len() == cards.len()
        && ids.len() == cards.len()
        && cards.iter().all(|card| {
            card.history_digest.len() == 64
                && card.history_digest.bytes().all(|b| b.is_ascii_hexdigit())
        })
        && task_map.validate().is_ok()
        && task_map.source_id == *source_id
        && task_map.source_model_digest == source.model_manifest
        && document
            .task_maps
            .iter()
            .filter(|m| m.source_id == *source_id)
            .collect::<Vec<_>>()
            == [task_map]
        && ordinals.iter().all(|ordinal| {
            task_map
                .entries
                .iter()
                .any(|e| e.source_ordinal == *ordinal)
        })
        && task_map
            .entries
            .iter()
            .all(|e| ordinals.contains(&e.source_ordinal))
        && crate::records::native_history_digest(source, cards)
            .is_ok_and(|digest| digest == *evidence_digest)
}

/// Remove one generated fact and its evidence. Examples are removed by index
/// and later example evidence is re-pointed; text fields are cleared.
fn reject_generated(
    document: &mut crate::LearningDocument,
    evidence_id: uuid::Uuid,
) -> Result<(), ContractError> {
    let evidence = document
        .evidence
        .iter()
        .find(|e| e.id == evidence_id && e.provenance == crate::Provenance::Generated)
        .cloned()
        .ok_or_else(|| ContractError("REVIEW_EVIDENCE_MISSING".into()))?;
    let field = evidence.field.as_str();
    match (&mut document.content, field) {
        (crate::LearningContent::Vocabulary(v), "usage") => v.usage.clear(),
        (crate::LearningContent::Vocabulary(v), "nuance") => v.nuance.clear(),
        (crate::LearningContent::Vocabulary(v), "collocations") => v.collocations.clear(),
        // A rejected generated dictionary entry goes, with any sense chosen from it.
        (crate::LearningContent::Vocabulary(v), "dictionary") => {
            let chosen = v.dictionary.iter().any(|e| {
                e.provider == crate::document::GENERATED_DICTIONARY_PROVIDER
                    && e.senses.iter().any(|s| s.key == v.sense_key)
            });
            v.dictionary
                .retain(|e| e.provider != crate::document::GENERATED_DICTIONARY_PROVIDER);
            if chosen {
                v.sense_key.clear();
                v.meaning.clear();
                v.reading.clear();
            }
        }
        (crate::LearningContent::Grammar(g), "usage") => g.usage.clear(),
        (crate::LearningContent::Grammar(g), "meaning") => g.meaning.clear(),
        (crate::LearningContent::Grammar(g), "formation") => g.formation.clear(),
        (content, "examples") => {
            let Some(crate::records::EvidenceTarget::Example { index }) = evidence.target else {
                return Err(ContractError("REVIEW_EVIDENCE_TARGET_INVALID".into()));
            };
            let examples = match content {
                crate::LearningContent::Vocabulary(v) => &mut v.examples,
                crate::LearningContent::Grammar(g) => &mut g.examples,
            };
            if examples
                .get(index)
                .is_none_or(|e| !e.evidence_ids.contains(&evidence_id))
            {
                return Err(ContractError("REVIEW_EVIDENCE_TARGET_INVALID".into()));
            }
            examples.remove(index);
            for e in &mut document.evidence {
                if let Some(crate::records::EvidenceTarget::Example { index: later }) =
                    &mut e.target
                    && *later > index
                {
                    *later -= 1;
                }
            }
        }
        _ => return Err(ContractError("REVIEW_REJECTION_UNSUPPORTED".into())),
    }
    document.evidence.retain(|e| e.id != evidence_id);
    Ok(())
}

/// Dropping a source field is valid only for that exact capture issue and only
/// while the field's original value is archived: an unmapped field, the text
/// of a field mapped to picture/audio, or a field's remote or unsafe media
/// reference (local files keep their own reviews in each case).
pub(crate) fn source_field_dropped(
    document: &crate::LearningDocument,
    issue: &Issue,
    source_id: uuid::Uuid,
    field: &str,
) -> bool {
    let named = match issue.code.as_str() {
        "SOURCE_UNMAPPED_FIELD_REVIEW" | "SOURCE_MEDIA_DISCOVERY_REVIEW" => {
            issue.field.as_deref() == Some(field)
                && issue.source_refs == vec![source_id.to_string()]
        }
        "SOURCE_STRUCTURED_ROLE_REVIEW" => {
            issue.source_refs == vec![source_id.to_string(), field.to_owned()]
        }
        _ => false,
    };
    named
        && issue.stage == "capture"
        && document.sources.iter().any(|source| {
            source.id == source_id
                && source.fields.contains_key(field)
                && document.archives.iter().any(|archive| {
                    archive.source_id == source_id
                        && archive.original_fields.get(field) == source.fields.get(field)
                })
        })
}

/// Only derived content review, never source task/history/identity or structural issues.
pub(crate) fn source_content_verified(
    document: &crate::LearningDocument,
    issue: &Issue,
    source_id: uuid::Uuid,
    selected: &[uuid::Uuid],
) -> bool {
    let provenance = match issue.stage.as_str() {
        "capture"
            if matches!(
                issue.code.as_str(),
                "SOURCE_HTML_TEXT_REVIEW" | "SOURCE_EXAMPLES_REVIEW"
            ) =>
        {
            crate::Provenance::Source
        }
        "ocr" if OCR_REVIEW_CODES.contains(&issue.code.as_str()) => crate::Provenance::Ocr,
        _ => return false,
    };
    if issue.source_refs != vec![source_id.to_string()]
        || !document.sources.iter().any(|source| source.id == source_id)
    {
        return false;
    }
    let required: BTreeSet<_> = document
        .evidence
        .iter()
        .filter(|e| {
            e.source_id == Some(source_id)
                && e.provenance == provenance
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
