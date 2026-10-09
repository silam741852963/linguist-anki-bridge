//! `plans resolve-batch`: many review decisions applied in one call.
//!
//! The batch is bound to one plan revision and digest, and each decision to
//! its document's semantic digest at that revision. Decisions are applied in
//! a fixed order (see `rank`). Between two decisions on the same document only
//! this batch has changed it, so each decision is re-bound to the current
//! digest and runs through the single-decision `resolve`; each result is
//! published as its own revision.
//!
//! Some decisions change content and so invalidate earlier ones on the same
//! document (a source media role clears every decision; a cue repair drops
//! the non-rebindable ones). After the ordered pass, any applied decision
//! whose issue is open again is re-applied unchanged, revalidated against the
//! new content, until nothing reopens (at most `MAX_PASSES`). The batch stops
//! at the first decision that does not apply.
pub use linguist_core::review::{BatchDecision, ResolutionBatch};
use linguist_core::{
    document::Task,
    records::{PlanRevision, ReviewChoice},
    review::ResolutionRequest,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Ordered pass plus re-application passes.
pub const MAX_PASSES: usize = 4;

/// Application order; lower goes first. Source media roles clear every
/// decision of their document, so they go first; then identity (sense,
/// expression, segmentation), cue repairs, native history, source
/// verifications, candidate media and generated facts.
pub fn rank(decision: &BatchDecision) -> u8 {
    // Assertions are checked once every decision applied.
    if decision.expect_resolved {
        return 8;
    }
    if decision.history_map.is_some() {
        return 3;
    }
    match &decision.choice {
        Some(ReviewChoice::SourceMediaRole { .. } | ReviewChoice::MissingMedia { .. }) => 0,
        Some(
            ReviewChoice::Sense(_)
            | ReviewChoice::SenseWithReading { .. }
            | ReviewChoice::Expression { .. }
            | ReviewChoice::Segmentation(_)
            | ReviewChoice::Anchor(_)
            | ReviewChoice::Duplicate { .. },
        ) => 1,
        Some(ReviewChoice::Cue { .. } | ReviewChoice::Exercise { .. }) => 2,
        Some(ReviewChoice::NativeHistory { .. }) => 3,
        Some(
            ReviewChoice::SourceContentVerified { .. } | ReviewChoice::SourceFieldDropped { .. },
        ) => 4,
        Some(ReviewChoice::Media(_)) => 5,
        Some(ReviewChoice::ContentVerified { .. } | ReviewChoice::ContentRejected { .. }) => 6,
        None => 7,
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
    /// 1 for the ordered pass; later passes re-apply reopened decisions.
    pub pass: usize,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub revision: u32,
    pub digest: String,
    pub decision_id: uuid::Uuid,
}

/// An `expect_resolved` entry whose issue was closed by the batch.
#[derive(Debug, Serialize)]
pub struct VerifiedClosed {
    pub index: usize,
    pub document_id: uuid::Uuid,
    pub issue_id: String,
}

#[derive(Debug, Serialize)]
pub struct BatchConflict {
    pub index: usize,
    pub pass: usize,
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
    pub passes: usize,
    pub applied: Vec<AppliedDecision>,
    pub verified_closed: Vec<VerifiedClosed>,
    /// The first entry that did not apply; the batch stopped there.
    pub conflict: Option<BatchConflict>,
    /// Entries never tried because the batch stopped first.
    pub not_attempted: usize,
}

/// Builds a native-history request for one document of the current revision.
pub type HistoryRequest<'a> = dyn FnMut(&PlanRevision, uuid::Uuid, &[(u16, Task)], &str) -> Result<ResolutionRequest, String>
    + 'a;

/// True when `issue_id` is an open issue of the document in `plan`.
fn open(plan: &PlanRevision, document_id: uuid::Uuid, issue_id: &str) -> Result<bool, String> {
    let mut document = plan
        .documents
        .iter()
        .find(|doc| doc.id == document_id)
        .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?
        .clone();
    document.issues.retain(|issue| issue.stage != "validation");
    Ok(linguist_core::validation::validate(&document)
        .iter()
        .any(|issue| issue.id == issue_id))
}

struct Run<'a, 'h> {
    store: &'a mut linguist_store::Store,
    batch: &'a ResolutionBatch,
    created_at: &'a dyn Fn() -> Result<String, String>,
    history: &'a mut HistoryRequest<'h>,
    current: PlanRevision,
    digest: String,
    ready: bool,
}
impl Run<'_, '_> {
    /// Resolve one entry against the current revision and publish the child.
    fn apply(&mut self, index: usize) -> Result<(u32, String, uuid::Uuid), String> {
        let decision = &self.batch.decisions[index];
        let document = self
            .current
            .documents
            .iter()
            .find(|doc| doc.id == decision.document_id)
            .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?;
        let request = match (&decision.choice, &decision.history_map) {
            (Some(choice), None) => ResolutionRequest {
                schema_version: 2,
                base_revision: self.current.revision,
                base_digest: self.digest.clone(),
                document_id: decision.document_id,
                issue_id: decision.issue_id.clone(),
                input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
                actor: self.batch.actor.clone(),
                choice: choice.clone(),
            },
            (None, Some(map)) => {
                let request = (self.history)(
                    &self.current,
                    decision.document_id,
                    &parse_history_map(map)?,
                    &self.batch.actor,
                )?;
                if request.issue_id != decision.issue_id {
                    return Err("REVIEW_ISSUE_CONFLICT".into());
                }
                request
            }
            _ => unreachable!(),
        };
        let result = super::resolve(self.store, &self.current, &request, (self.created_at)()?)?;
        let published = self
            .store
            .publish_revision(&result.revision)
            .map_err(|e| e.to_string())?;
        self.ready = result.ready;
        self.current = result.revision;
        self.digest = published.clone();
        Ok((self.current.revision, published, result.decision_id))
    }
}

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
    let start: BTreeMap<uuid::Uuid, String> = base
        .documents
        .iter()
        .map(|doc| Ok((doc.id, doc.semantic_digest().map_err(|e| e.to_string())?)))
        .collect::<Result<_, String>>()?;
    for decision in &batch.decisions {
        let set = [
            decision.choice.is_some(),
            decision.history_map.is_some(),
            decision.expect_resolved,
        ];
        if set.iter().filter(|set| **set).count() != 1 {
            return Err(format!(
                "REVIEW_BATCH_DECISION_INVALID: {} needs exactly one of choice, history_map and expect_resolved",
                decision.issue_id
            ));
        }
    }
    let mut order: Vec<usize> = (0..batch.decisions.len()).collect();
    order.sort_by_key(|&index| rank(&batch.decisions[index]));
    let (decisions, assertions): (Vec<usize>, Vec<usize>) = order
        .into_iter()
        .partition(|&index| !batch.decisions[index].expect_resolved);

    let mut run = Run {
        store,
        batch,
        created_at,
        history,
        current: base.clone(),
        digest: base_digest,
        ready: false,
    };
    let mut applied = Vec::new();
    let mut verified_closed = Vec::new();
    let mut attempted = 0usize;
    let mut passes = 0usize;
    let mut queue = decisions.clone();
    let conflict = 'run: {
        while !queue.is_empty() {
            passes += 1;
            if passes > MAX_PASSES {
                let index = queue[0];
                break 'run Some((index, passes - 1, "REVIEW_BATCH_NOT_STABLE".to_owned()));
            }
            for &index in &queue {
                let decision = &batch.decisions[index];
                if passes == 1 {
                    attempted += 1;
                    if start.get(&decision.document_id) != Some(&decision.input_digest) {
                        break 'run Some((index, passes, "REVIEW_INPUT_CONFLICT".to_owned()));
                    }
                }
                match run.apply(index) {
                    Ok((revision, digest, decision_id)) => applied.push(AppliedDecision {
                        index,
                        pass: passes,
                        document_id: decision.document_id,
                        issue_id: decision.issue_id.clone(),
                        revision,
                        digest,
                        decision_id,
                    }),
                    Err(error) => break 'run Some((index, passes, error)),
                }
            }
            // Decisions a later content change invalidated are re-applied.
            let mut reopened = Vec::new();
            for &index in &decisions {
                let decision = &batch.decisions[index];
                match open(&run.current, decision.document_id, &decision.issue_id) {
                    Ok(true) => reopened.push(index),
                    Ok(false) => {}
                    Err(error) => break 'run Some((index, passes, error)),
                }
            }
            queue = reopened;
        }
        for &index in &assertions {
            attempted += 1;
            let decision = &batch.decisions[index];
            if start.get(&decision.document_id) != Some(&decision.input_digest) {
                break 'run Some((index, passes, "REVIEW_INPUT_CONFLICT".to_owned()));
            }
            match open(&run.current, decision.document_id, &decision.issue_id) {
                Ok(false) => verified_closed.push(VerifiedClosed {
                    index,
                    document_id: decision.document_id,
                    issue_id: decision.issue_id.clone(),
                }),
                Ok(true) => break 'run Some((index, passes, "REVIEW_ISSUE_STILL_OPEN".to_owned())),
                Err(error) => break 'run Some((index, passes, error)),
            }
        }
        None
    };
    Ok(BatchOutcome {
        schema_version: 2,
        plan_id: run.current.id,
        revision: run.current.revision,
        digest: run.digest,
        ready: conflict.is_none() && run.ready,
        passes,
        applied,
        verified_closed,
        conflict: conflict.map(|(index, pass, error)| BatchConflict {
            index,
            pass,
            document_id: batch.decisions[index].document_id,
            issue_id: batch.decisions[index].issue_id.clone(),
            error,
        }),
        not_attempted: batch.decisions.len() - attempted,
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
            } else if entry["issue"]["code"] == "SOURCE_TASK_MAPPING_REVIEW" {
                // Closed by the native history task map, not by a decision.
                decision["expect_resolved"] = true.into();
                decision["options"] = serde_json::json!({"issue": entry["issue"]});
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
