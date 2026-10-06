//! Typed edits create a child revision; source archives and parent data stay immutable.
use crate::{FieldIntent, Issue, Severity, canonical::ContractError, records::PlanRevision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanPatch {
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u16,
    pub base_digest: String,
    pub items: Vec<ItemPatch>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemPatch {
    pub document_id: uuid::Uuid,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldIntent<String>>,
    #[serde(default)]
    pub personal_notes: FieldIntent<String>,
}
#[derive(Debug, Serialize)]
pub struct EditResult {
    pub revision: PlanRevision,
    pub ready: bool,
    pub changed: bool,
    pub invalidated_review_ids: Vec<uuid::Uuid>,
}
pub fn apply_patch(
    base: &PlanRevision,
    patch: &PlanPatch,
    save_draft: bool,
) -> Result<EditResult, ContractError> {
    if patch.schema_version != 2 {
        return Err(ContractError("UNSUPPORTED_PATCH_VERSION".into()));
    }
    if patch.base_digest != base.approval_digest()? {
        return Err(ContractError("PLAN_EDIT_BASE_CONFLICT".into()));
    }
    let mut candidate = base.clone();
    let mut ids = BTreeSet::new();
    let mut invalidated_review_ids = Vec::new();
    for item in &patch.items {
        if !ids.insert(item.document_id) {
            return Err(ContractError("DUPLICATE_PATCH_DOCUMENT".into()));
        }
        let document = candidate
            .documents
            .iter_mut()
            .find(|d| d.id == item.document_id)
            .ok_or_else(|| ContractError("PATCH_DOCUMENT_NOT_FOUND".into()))?;
        let old_digest = document.semantic_digest()?;
        for (key, intent) in &item.fields {
            // Identity branch: a missing vocabulary sense key may be assigned
            // once (for example after a revamp capture); an existing one is
            // identity and is never rewritten by a patch.
            if key == "SenseKey" {
                let crate::LearningContent::Vocabulary(vocab) = &mut document.content else {
                    return Err(ContractError("PATCH_TYPED_FIELD_REQUIRED:SenseKey".into()));
                };
                match intent {
                    FieldIntent::Set(value)
                        if vocab.sense_key.trim().is_empty() && !value.trim().is_empty() =>
                    {
                        vocab.sense_key = value.trim().to_owned();
                        continue;
                    }
                    FieldIntent::Keep => continue,
                    _ => {
                        return Err(ContractError(
                            "PATCH_IDENTITY_IMMUTABLE: SenseKey can only fill an empty sense key"
                                .into(),
                        ));
                    }
                }
            }
            // Identity, tasks, cues and media require separate typed pipeline branches.
            if !matches!(
                key.as_str(),
                "Meaning"
                    | "Reading"
                    | "Pronunciation"
                    | "Usage"
                    | "Kanji"
                    | "Formation"
                    | "Source"
            ) || !crate::model::for_document(document).fields.contains(key)
            {
                return Err(ContractError(format!("PATCH_TYPED_FIELD_REQUIRED:{key}")));
            }
            document.edits.insert(key.clone(), intent.clone());
        }
        document.personal_notes = item
            .personal_notes
            .resolve(Some(&document.personal_notes))?
            .unwrap_or_default();
        if old_digest != document.semantic_digest()? {
            invalidated_review_ids.extend(document.reviews.iter().map(|review| review.id));
            document.reviews.clear();
        }
    }
    let changed = candidate.documents != base.documents;
    if !changed {
        return Ok(EditResult {
            revision: candidate,
            ready: base.documents.iter().all(|doc| {
                let empty = BTreeMap::new();
                let source = doc.sources.first().map(|s| &s.fields).unwrap_or(&empty);
                crate::validation::validate(doc)
                    .iter()
                    .all(|issue| issue.severity == Severity::Warning)
                    && crate::render::render(doc, source).is_ok()
            }),
            changed: false,
            invalidated_review_ids,
        });
    }
    invalidated_review_ids.extend(candidate.review_decisions.iter().map(|review| review.id));
    candidate.review_decisions.clear();
    candidate.revision = base
        .revision
        .checked_add(1)
        .ok_or_else(|| ContractError("REVISION_LIMIT".into()))?;
    candidate.parent_digest = Some(patch.base_digest.clone());
    candidate.rendered.clear();
    let mut ready = true;
    for document in &mut candidate.documents {
        document.issues.retain(|issue| issue.stage != "validation");
        document.issues = crate::validation::validate(document);
        let empty = BTreeMap::new();
        let source = document
            .sources
            .first()
            .map(|source| &source.fields)
            .unwrap_or(&empty);
        match crate::render::render(document, source) {
            Ok(rendered) => candidate.rendered.push(rendered),
            Err(_) => {
                ready = false;
                if !document
                    .issues
                    .iter()
                    .any(|issue| issue.severity != Severity::Warning)
                {
                    document.issues.push(Issue::new(
                        "EFFECTIVE_RENDER_BLOCKED",
                        Severity::Error,
                        None,
                        "Effective edited fields cannot be rendered; correct the typed patch.",
                    ));
                }
            }
        }
        ready &= document
            .issues
            .iter()
            .all(|issue| issue.severity == Severity::Warning);
    }
    if !ready && !save_draft {
        return Err(ContractError("DOCUMENT_NOT_READY: use --save-draft to retain an explicitly invalid or review-needed child revision".into()));
    }
    invalidated_review_ids.sort();
    invalidated_review_ids.dedup();
    Ok(EditResult {
        revision: candidate,
        ready,
        changed,
        invalidated_review_ids,
    })
}
