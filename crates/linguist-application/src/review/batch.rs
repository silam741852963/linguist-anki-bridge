//! `plans resolve-batch`: many review decisions applied in one call.
//!
//! The batch is bound to one plan revision and digest, and each decision to
//! its document's semantic digest at that revision. Decisions are applied in
//! a fixed order so content-changing ones go first (sense, then cue and
//! exercise repairs, media roles, history, source verifications and generated
//! facts). Between two decisions on the same document only this batch has
//! changed it, so later decisions are re-bound to the current digest. Every
//! decision still runs through the single-decision `resolve`, each result is
//! published as its own revision, and the batch stops at the first conflict.
pub use linguist_core::review::{BatchDecision, ResolutionBatch};
use linguist_core::{
    document::Task,
    records::{PlanRevision, ReviewChoice},
    review::ResolutionRequest,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Application order; lower goes first. Cue and exercise repairs drop every
/// decision that is not rebindable, so they precede media roles,
/// verifications and generated-fact decisions.
pub fn rank(decision: &BatchDecision) -> u8 {
    if decision.history_map.is_some() {
        return 3;
    }
    match &decision.choice {
        Some(
            ReviewChoice::Sense(_)
            | ReviewChoice::SenseWithReading { .. }
            | ReviewChoice::Expression { .. }
            | ReviewChoice::Segmentation(_)
            | ReviewChoice::Anchor(_)
            | ReviewChoice::Duplicate { .. },
        ) => 0,
        Some(ReviewChoice::Cue { .. } | ReviewChoice::Exercise { .. }) => 1,
        Some(
            ReviewChoice::SourceMediaRole { .. }
            | ReviewChoice::Media(_)
            | ReviewChoice::MissingMedia { .. },
        ) => 2,
        Some(ReviewChoice::NativeHistory { .. }) => 3,
        Some(ReviewChoice::SourceContentVerified { .. } | ReviewChoice::SourceFieldDropped { .. }) => {
            4
        }
        Some(ReviewChoice::ContentVerified { .. } | ReviewChoice::ContentRejected { .. }) => 5,
        None => 6,
    }
}

/// Parse `SOURCE_ORDINAL=TASK` entries as `plans resolve-history --map` does.
pub fn parse_history_map(entries: &[String]) -> Result<Vec<(u16, Task)>, String> {
    entries
        .iter()
        .map(|entry| {
            let (ordinal, task) = entry
                .split_once('=')
                .ok_or("NATIVE_HISTORY_MAP_INVALID: use SOURCE_ORDINAL=TASK")?;
            let ordinal: u16 = ordinal
                .trim()
                .parse()
                .map_err(|_| "NATIVE_HISTORY_MAP_INVALID: use SOURCE_ORDINAL=TASK")?;
            let task: Task = serde_json::from_value(serde_json::json!(task.trim()))
                .map_err(|_| "NATIVE_HISTORY_MAP_INVALID: unknown task")?;
            Ok((ordinal, task))
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct AppliedDecision {
    /// Position in the submitted file.
    pub index: usize,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub revision: u32,
    pub digest: String,
    pub decision_id: uuid::Uuid,
}

#[derive(Debug, Serialize)]
pub struct BatchConflict {
    pub index: usize,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct BatchOutcome {
    pub schema_version: u16,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub digest: String,
    pub ready: bool,
    pub applied: Vec<AppliedDecision>,
    /// The first decision that did not apply; later ones were not tried.
    pub conflict: Option<BatchConflict>,
    pub not_attempted: usize,
}

/// Builds a native-history request for one document of the current revision.
pub type HistoryRequest<'a> =
    dyn FnMut(&PlanRevision, uuid::Uuid, &[(u16, Task)], &str) -> Result<ResolutionRequest, String>
        + 'a;

/// Apply `batch` on `base`, publishing each child revision in `store`;
/// `history` is only called for `history_map` decisions.
pub fn resolve_batch(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    batch: &ResolutionBatch,
    created_at: &dyn Fn() -> Result<String, String>,
    history: &mut HistoryRequest<'_>,
) -> Result<BatchOutcome, String> {
    if batch.schema_version != 2 {
        return Err("UNSUPPORTED_RESOLUTION_VERSION".into());
    }
    let base_digest = base.approval_digest().map_err(|e| e.to_string())?;
    if batch.base_revision != base.revision || batch.base_digest != base_digest {
        return Err("REVIEW_BASE_CONFLICT".into());
    }
    if batch.decisions.is_empty() {
        return Err("REVIEW_BATCH_EMPTY".into());
    }
    for decision in &batch.decisions {
        if decision.choice.is_some() == decision.history_map.is_some() {
            return Err(format!(
                "REVIEW_BATCH_DECISION_INVALID: {} needs exactly one of choice and history_map",
                decision.issue_id
            ));
        }
    }
    let start: BTreeMap<uuid::Uuid, String> = base
        .documents
        .iter()
        .map(|doc| Ok((doc.id, doc.semantic_digest().map_err(|e| e.to_string())?)))
        .collect::<Result<_, String>>()?;
    let mut order: Vec<usize> = (0..batch.decisions.len()).collect();
    order.sort_by_key(|&index| rank(&batch.decisions[index]));

    let mut current = base.clone();
    let mut digest = base_digest;
    let mut ready = false;
    let mut applied = Vec::new();
    let mut conflict = None;
    for (position, &index) in order.iter().enumerate() {
        let decision = &batch.decisions[index];
        let step = (|| -> Result<_, String> {
            let original = start
                .get(&decision.document_id)
                .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?;
            if *original != decision.input_digest {
                return Err("REVIEW_INPUT_CONFLICT".into());
            }
            let document = current
                .documents
                .iter()
                .find(|doc| doc.id == decision.document_id)
                .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?;
            let request = match (&decision.choice, &decision.history_map) {
                (Some(choice), None) => ResolutionRequest {
                    schema_version: 2,
                    base_revision: current.revision,
                    base_digest: digest.clone(),
                    document_id: decision.document_id,
                    issue_id: decision.issue_id.clone(),
                    input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
                    actor: batch.actor.clone(),
                    choice: choice.clone(),
                },
                (None, Some(map)) => {
                    let request = history(
                        &current,
                        decision.document_id,
                        &parse_history_map(map)?,
                        &batch.actor,
                    )?;
                    if request.issue_id != decision.issue_id {
                        return Err("REVIEW_ISSUE_CONFLICT".into());
                    }
                    request
                }
                _ => unreachable!(),
            };
            let result = super::resolve(store, &current, &request, created_at()?)?;
            let published = store
                .publish_revision(&result.revision)
                .map_err(|e| e.to_string())?;
            Ok((result, published))
        })();
        match step {
            Ok((result, published)) => {
                applied.push(AppliedDecision {
                    index,
                    document_id: decision.document_id,
                    issue_id: decision.issue_id.clone(),
                    revision: result.revision.revision,
                    digest: published.clone(),
                    decision_id: result.decision_id,
                });
                ready = result.ready;
                current = result.revision;
                digest = published;
            }
            Err(error) => {
                conflict = Some(BatchConflict {
                    index,
                    document_id: decision.document_id,
                    issue_id: decision.issue_id.clone(),
                    error,
                });
                let not_attempted = order.len() - position - 1;
                return Ok(BatchOutcome {
                    schema_version: 2,
                    plan_id: current.id,
                    revision: current.revision,
                    digest,
                    ready: false,
                    applied,
                    conflict,
                    not_attempted,
                });
            }
        }
    }
    Ok(BatchOutcome {
        schema_version: 2,
        plan_id: current.id,
        revision: current.revision,
        digest,
        ready,
        applied,
        conflict,
        not_attempted: 0,
    })
}

/// A decisions file for every open review issue of `plan`, with each issue's
/// templates under `options`. `choice` is left null: nothing is decided for
/// the reviewer, and a null choice is refused when the file is submitted.
pub fn template(plan: &PlanRevision, actor: &str) -> Result<serde_json::Value, String> {
    let mut decisions = Vec::new();
    let mut after = 0u32;
    loop {
        let page = super::inspection::page(plan, None, after, 1000)?;
        for entry in page["issues"].as_array().into_iter().flatten() {
            if entry["issue"]["severity"] != "review" {
                continue;
            }
            let identity = &entry["request_identity"];
            let templates = entry["templates"].as_array().cloned().unwrap_or_default();
            let mut decision = serde_json::json!({
                "document_id": identity["document_id"],
                "issue_id": identity["issue_id"],
                "input_digest": identity["input_digest"],
            });
            if entry["issue"]["code"] == "SOURCE_NATIVE_HISTORY_REVIEW" {
                decision["history_map"] = serde_json::json!([]);
            } else {
                decision["choice"] = serde_json::Value::Null;
                decision["options"] = serde_json::json!({
                    "issue": entry["issue"],
                    "templates": templates,
                    "manual_media_decision_required": entry["manual_media_decision_required"],
                });
            }
            decisions.push(decision);
        }
        match page["next_index"].as_u64() {
            Some(next) => after = next as u32,
            None => break,
        }
    }
    Ok(serde_json::json!({
        "schema_version": 2,
        "base_revision": plan.revision,
        "base_digest": plan.approval_digest().map_err(|e| e.to_string())?,
        "actor": actor,
        "decisions": decisions,
    }))
}
