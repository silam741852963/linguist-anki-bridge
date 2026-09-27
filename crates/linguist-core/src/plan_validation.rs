//! Validation evidence refers to an exact immutable revision, never an apply authorization.
use crate::{Issue, Severity, canonical::ContractError, records::PlanRevision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemValidation {
    pub document_id: uuid::Uuid,
    pub semantic_digest: String,
    pub content_ready: bool,
    pub staged_render_matches: bool,
    pub issues: Vec<Issue>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationEvidence {
    pub schema_version: u16,
    pub id: uuid::Uuid,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub plan_digest: String,
    pub content_ready: bool,
    pub items: Vec<ItemValidation>,
    pub live_checked: bool,
    pub apply_eligible: bool,
}
pub fn inspect(plan: &PlanRevision) -> Result<ValidationEvidence, ContractError> {
    if plan.schema_version != 2 || plan.revision == 0 {
        return Err(ContractError("INVALID_PLAN_REVISION".into()));
    }
    let ids: BTreeSet<_> = plan.documents.iter().map(|d| d.id).collect();
    if ids.len() != plan.documents.len() {
        return Err(ContractError("DUPLICATE_PLAN_DOCUMENT".into()));
    }
    let render_ids: BTreeSet<_> = plan.rendered.iter().map(|r| r.document_id).collect();
    if render_ids.len() != plan.rendered.len() || !render_ids.is_subset(&ids) {
        return Err(ContractError("INVALID_PLAN_RENDER_REFERENCES".into()));
    }
    let mut items = Vec::new();
    for document in &plan.documents {
        let mut doc = document.clone();
        // Stored validation diagnostics are observations, not permanent content decisions.
        doc.issues.retain(|issue| issue.stage != "validation");
        let mut issues = crate::validation::validate(&doc);
        let empty = BTreeMap::new();
        let fields = doc.sources.first().map(|s| &s.fields).unwrap_or(&empty);
        let staged = plan.rendered.iter().find(|r| r.document_id == doc.id);
        let staged_render_matches = match crate::render::render(&doc, fields) {
            Ok(rendered) if staged == Some(&rendered) => true,
            Ok(_) => {
                issues.push(Issue::new(
                    "STAGED_RENDER_MISMATCH",
                    Severity::Error,
                    None,
                    "Staged output is missing or stale; create a corrected plan revision.",
                ));
                false
            }
            Err(_) => {
                if issues
                    .iter()
                    .all(|issue| issue.severity == Severity::Warning)
                {
                    issues.push(Issue::new(
                        "EFFECTIVE_RENDER_BLOCKED",
                        Severity::Error,
                        None,
                        "Effective field intents cannot render; edit the plan to correct them.",
                    ));
                }
                false
            }
        };
        // A source diagnostic may overlap a recomputed one. Return one stable observation.
        let mut seen = BTreeSet::new();
        issues.retain(|issue| {
            seen.insert((issue.id.clone(), issue.code.clone(), issue.stage.clone()))
        });
        let content_ready = staged_render_matches
            && issues
                .iter()
                .all(|issue| issue.severity == Severity::Warning);
        items.push(ItemValidation {
            document_id: doc.id,
            semantic_digest: doc.semantic_digest()?,
            content_ready,
            staged_render_matches,
            issues,
        });
    }
    Ok(ValidationEvidence {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        plan_id: plan.id,
        revision: plan.revision,
        plan_digest: plan.approval_digest()?,
        content_ready: items.iter().all(|item| item.content_ready),
        items,
        live_checked: false,
        apply_eligible: false,
    })
}
