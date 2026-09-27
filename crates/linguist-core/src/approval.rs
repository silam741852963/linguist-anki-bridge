//! Content approval is scoped evidence; it never authorizes external mutations.
use crate::{
    Severity,
    canonical::ContractError,
    records::{Approval, PlanRevision},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub digest: String,
    pub item_ids: Option<Vec<uuid::Uuid>>,
    pub actor: String,
    pub accepted_warnings: Vec<String>,
}
pub fn build(
    plan: &PlanRevision,
    request: &ApprovalRequest,
    approved_at: String,
) -> Result<Approval, ContractError> {
    if request.plan_id != plan.id
        || request.revision != plan.revision
        || request.digest != plan.approval_digest()?
    {
        return Err(ContractError("APPROVAL_REVISION_CONFLICT".into()));
    }
    if request.actor.trim().is_empty()
        || request.actor.chars().count() > 200
        || request.actor.chars().any(char::is_control)
    {
        return Err(ContractError("INVALID_APPROVAL_ACTOR".into()));
    }
    let item_ids = request
        .item_ids
        .clone()
        .unwrap_or_else(|| plan.documents.iter().map(|doc| doc.id).collect());
    let ids: BTreeSet<_> = item_ids.iter().copied().collect();
    if ids.len() != item_ids.len() {
        return Err(ContractError("DUPLICATE_APPROVAL_ITEM".into()));
    }
    let evidence = crate::plan_validation::inspect(plan)?;
    if ids
        .iter()
        .any(|id| !evidence.items.iter().any(|item| item.document_id == *id))
    {
        return Err(ContractError("APPROVAL_ITEM_NOT_FOUND".into()));
    }
    if evidence
        .items
        .iter()
        .filter(|item| ids.contains(&item.document_id))
        .any(|item| !item.content_ready)
    {
        return Err(ContractError("DOCUMENT_NOT_READY: selected items contain errors, unresolved reviews or stale rendered outputs".into()));
    }
    let warnings: BTreeSet<_> = evidence
        .items
        .iter()
        .filter(|item| ids.contains(&item.document_id))
        .flat_map(|item| &item.issues)
        .filter(|issue| issue.severity == Severity::Warning)
        .map(|issue| issue.code.clone())
        .collect();
    let accepted: BTreeSet<_> = request.accepted_warnings.iter().cloned().collect();
    if accepted.len() != request.accepted_warnings.len() {
        return Err(ContractError("DUPLICATE_WARNING_ACCEPTANCE".into()));
    }
    if !accepted.is_subset(&warnings) {
        return Err(ContractError("UNKNOWN_WARNING_ACCEPTANCE".into()));
    }
    if accepted != warnings {
        return Err(ContractError(format!(
            "DOCUMENT_NOT_READY: explicitly accept warnings: {}",
            warnings
                .difference(&accepted)
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        )));
    }
    Ok(Approval {
        plan_id: plan.id,
        revision: plan.revision,
        digest: request.digest.clone(),
        item_ids,
        actor: request.actor.clone(),
        approved_at,
        accepted_warnings: accepted.into_iter().collect(),
    })
}
