//! ALG-MODEL over an injected native port: exact-manifest reuse, name-collision
//! blocking, a verified checkpoint before creation, journal-before-call and
//! evidence-based reconciliation. Shared models are never overwritten or deleted.
use crate::backup::{DependentScope, PortFailure, Result, binding_digest, require_checkpoint};
use crate::checkpoint::CoverageRequirement;
use linguist_core::{
    canonical,
    model::{ManagedModel, Template},
    records::{CollectionBinding, JournalStep, OperationJournal, OperationState, StepState},
    validation::{Issue, Severity},
};
use linguist_store::{
    Store, checkpoint::ModelOperationRecord, journal::JournalVersion, lease::LeaseToken,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Actual note type read back from the collection, templates in ordinal order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedModel {
    pub id: i64,
    pub name: String,
    pub fields: Vec<String>,
    pub templates: Vec<Template>,
    pub css: String,
}

/// Companion ledger evidence for one operation UUID. Only a created receipt
/// that names the model and manifest can adopt an uncertain result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OperationEvidence {
    Absent,
    FailedBeforeWrite,
    Created {
        model_id: i64,
        manifest_digest: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInstallRequest {
    pub operation_id: Uuid,
    pub binding: CollectionBinding,
    pub manifest: ManagedModel,
    pub manifest_digest: String,
    pub expected_absent: bool,
}

pub trait ModelPort {
    /// Every note type whose name is exactly `name`.
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>>;
    fn install_model(
        &mut self,
        request: &ModelInstallRequest,
    ) -> std::result::Result<i64, PortFailure>;
    fn operation_evidence(&mut self, operation_id: Uuid) -> Result<OperationEvidence>;
}

/// RI-05 canonical manifest digest (version excluded); see
/// `linguist_core::model::ManifestProjection`.
pub fn manifest_digest(model: &ManagedModel) -> Result<String> {
    model.manifest_digest().map_err(|e| e.to_string())
}

fn observed_digest(model: &ObservedModel) -> Result<String> {
    linguist_core::model::ManifestProjection::new(
        &model.name,
        &model.fields,
        &model.templates,
        &model.css,
    )
    .digest()
    .map_err(|e| e.to_string())
}

/// Exact name, field order, template names/ordinals/bytes and CSS.
pub fn exact_match(observed: &ObservedModel, target: &ManagedModel) -> bool {
    let mut templates = target.templates.clone();
    templates.sort_by_key(|template| template.ordinal);
    observed.name == target.name
        && observed.fields == target.fields
        && observed.templates == templates
        && observed.css == target.css
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum ModelProposal {
    Create,
    Reuse {
        model_id: i64,
    },
    NameCollision {
        model_id: i64,
        differences: Vec<&'static str>,
    },
    Ambiguous {
        model_ids: Vec<i64>,
    },
}

fn differences(observed: &ObservedModel, target: &ManagedModel) -> Vec<&'static str> {
    let mut templates = target.templates.clone();
    templates.sort_by_key(|template| template.ordinal);
    let mut out = Vec::new();
    if observed.fields != target.fields {
        out.push("fields");
    }
    if observed.templates != templates {
        out.push("templates");
    }
    if observed.css != target.css {
        out.push("css");
    }
    out
}

pub fn propose(observed: &[ObservedModel], target: &ManagedModel) -> ModelProposal {
    match observed {
        [] => ModelProposal::Create,
        [one] if exact_match(one, target) => ModelProposal::Reuse { model_id: one.id },
        [one] => ModelProposal::NameCollision {
            model_id: one.id,
            differences: differences(one, target),
        },
        many => ModelProposal::Ambiguous {
            model_ids: many.iter().map(|model| model.id).collect(),
        },
    }
}

pub struct InstallRequest<'a> {
    pub binding: CollectionBinding,
    pub target: ManagedModel,
    pub checkpoint_id: Uuid,
    pub group_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInstallOutcome {
    pub model_name: String,
    pub manifest_digest: String,
    pub action: &'static str,
    pub model_id: Option<i64>,
    pub operation_id: Option<Uuid>,
    pub journal_state: Option<OperationState>,
    pub checkpoint_id: Option<Uuid>,
    pub verified: bool,
    pub needs_recovery: bool,
    pub issues: Vec<Issue>,
}

fn issue(code: &str, message: impl Into<String>) -> Issue {
    let mut issue = Issue::new(code, Severity::Error, None, message);
    issue.stage = "model".into();
    issue
}

/// Refuses a new attempt while an earlier operation for the name is unresolved.
fn unresolved_for(store: &Store, name: &str) -> Result<Option<Uuid>> {
    for record in store.model_operations_named(name)? {
        match store.journal(record.operation_id) {
            Ok(version) if version.pending_recovery => return Ok(Some(record.operation_id)),
            Ok(_) => {}
            // The pre-state record is written before the journal; a missing journal
            // means nothing was dispatched for that operation.
            Err(code) if code == "JOURNAL_NOT_FOUND" => {}
            Err(code) => return Err(code),
        }
    }
    Ok(None)
}

struct Journal<'a> {
    store: &'a mut Store,
    version: JournalVersion,
}

impl Journal<'_> {
    fn advance(&mut self, change: impl FnOnce(&mut OperationJournal)) -> Result<()> {
        let mut next = self.version.journal.clone();
        change(&mut next);
        self.version = self.store.append_journal(&next, Some(&self.version))?;
        Ok(())
    }
}

fn outcome(
    target: &ManagedModel,
    digest: &str,
    action: &'static str,
    model_id: Option<i64>,
    journal: Option<&OperationJournal>,
    checkpoint_id: Option<Uuid>,
) -> ModelInstallOutcome {
    let state = journal.map(|j| j.state);
    ModelInstallOutcome {
        model_name: target.name.clone(),
        manifest_digest: digest.into(),
        action,
        model_id,
        operation_id: journal.map(|j| j.id),
        journal_state: state,
        checkpoint_id,
        verified: matches!(action, "reused" | "created"),
        needs_recovery: state == Some(OperationState::NeedsRecovery),
        issues: journal.map(|j| j.issues.clone()).unwrap_or_default(),
    }
}

/// Installs or reuses one managed model under an already-held writer lease.
/// Every refusal before dispatch performs zero collection mutations.
pub fn install(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ModelPort,
    request: InstallRequest,
) -> Result<ModelInstallOutcome> {
    store.validate_lease(lease)?;
    let target = &request.target;
    let digest = manifest_digest(target)?;
    if let Some(operation) = unresolved_for(store, &target.name)? {
        return Err(format!(
            "MODEL_INSTALL_RECOVERY_REQUIRED: reconcile operation {operation} first"
        ));
    }
    let observed = port.models_named(&target.name)?;
    match propose(&observed, target) {
        ModelProposal::Reuse { model_id } => {
            return Ok(outcome(
                target,
                &digest,
                "reused",
                Some(model_id),
                None,
                None,
            ));
        }
        ModelProposal::NameCollision { differences, .. } => {
            return Err(format!(
                "MODEL_NAME_COLLISION: existing same-name model differs in {}; never overwritten",
                differences.join(",")
            ));
        }
        ModelProposal::Ambiguous { .. } => return Err("MODEL_NAME_AMBIGUOUS".into()),
        ModelProposal::Create => {}
    }
    // Model creation is a schema action: collection scope with scheduling and media.
    let authorization = require_checkpoint(
        store,
        request.checkpoint_id,
        &DependentScope {
            binding: &request.binding,
            requirement: CoverageRequirement {
                scheduling: true,
                media: true,
                schema: true,
            },
            note_ids: &[],
            card_ids: &[],
            model_ids: &[],
            media_names: &[],
            group_id: request.group_id,
            protected_manifest_digest: request.protected_manifest_digest,
            now_ms: request.now_ms,
            reuse_max_age_seconds: request.reuse_max_age_seconds,
            max_package_bytes: request.max_package_bytes,
        },
    )?;
    let operation = Uuid::new_v4();
    let install = ModelInstallRequest {
        operation_id: operation,
        binding: request.binding.clone(),
        manifest: target.clone(),
        manifest_digest: digest.clone(),
        expected_absent: true,
    };
    store.publish_model_operation(&ModelOperationRecord {
        operation_id: operation,
        model_name: target.name.clone(),
        manifest_digest: digest.clone(),
        created_ms: request.now_ms,
        evidence: serde_json::json!({
            "schema_version": 1,
            "observed_same_name": observed,
            "binding_digest": binding_digest(&request.binding)?,
            "checkpoint": authorization,
            "manifest": target,
        }),
    })?;
    let journal = OperationJournal {
        id: operation,
        group_id: request.group_id,
        approval_digest: format!("lab-model-v1:{digest}"),
        binding: request.binding.clone(),
        snapshot_id: operation,
        backup_id: request.checkpoint_id,
        state: OperationState::Prepared,
        steps: vec![JournalStep {
            id: Uuid::new_v4(),
            action: "install_model".into(),
            payload_digest: canonical::digest("lab-model-install-request-v1", &install)
                .map_err(|e| e.to_string())?,
            precondition_digest: canonical::digest(
                "lab-model-absent-v1",
                &(&target.name, binding_digest(&request.binding)?),
            )
            .map_err(|e| e.to_string())?,
            expected_post_digest: digest.clone(),
            state: StepState::IntentRecorded,
            observed_digest: None,
        }],
        issues: vec![],
    };
    let version = store.append_journal(&journal, None)?;
    let mut journal = Journal { store, version };
    journal.advance(|j| j.state = OperationState::Preflight)?;
    journal.advance(|j| j.state = OperationState::Checkpointed)?;
    journal.advance(|j| {
        j.state = OperationState::Mutating;
        j.steps[0].state = StepState::RequestStarted;
    })?;
    let response = port.install_model(&install);
    match response {
        Ok(model_id) => {
            let actual = port.models_named(&target.name);
            settle_observed(
                &mut journal,
                target,
                &digest,
                Some(model_id),
                actual,
                request.checkpoint_id,
            )
        }
        Err(PortFailure::Rejected(reason)) => {
            // A rejection counts as no effect only if the name is still absent.
            let actual = port.models_named(&target.name);
            if matches!(&actual, Ok(models) if models.is_empty()) {
                let observed = canonical::digest("lab-model-failure-v1", &reason).ok();
                journal.advance(|j| {
                    j.steps[0].state = StepState::ObservedFailure;
                    j.steps[0].observed_digest = observed;
                    j.state = OperationState::FailedBeforeWrite;
                    j.issues
                        .push(issue("MODEL_INSTALL_REJECTED", reason.clone()));
                })?;
                return Err(format!("MODEL_INSTALL_REJECTED: {reason}"));
            }
            mark_unknown(
                &mut journal,
                "MODEL_INSTALL_REJECTED_WITH_EFFECT",
                &format!(
                    "adapter rejected the request ({reason}) but the name exists or could not be read"
                ),
            )?;
            Ok(outcome(
                target,
                &digest,
                "needs_recovery",
                None,
                Some(&journal.version.journal),
                Some(request.checkpoint_id),
            ))
        }
        Err(PortFailure::Unknown(reason)) => {
            mark_unknown(
                &mut journal,
                "MODEL_INSTALL_OUTCOME_UNKNOWN",
                &format!("install response lost: {reason}"),
            )?;
            Ok(outcome(
                target,
                &digest,
                "needs_recovery",
                None,
                Some(&journal.version.journal),
                Some(request.checkpoint_id),
            ))
        }
    }
}

fn mark_unknown(journal: &mut Journal, code: &str, detail: &str) -> Result<()> {
    journal.advance(|j| {
        j.steps[0].state = StepState::Unknown;
        j.state = OperationState::NeedsRecovery;
        j.issues.push(issue(
            code,
            format!(
                "{detail}; collection effect is uncertain, reconcile before any dependent write"
            ),
        ));
    })
}

/// After an observed success: verify the actual read-back. A partial or
/// different model needs recovery and is never deleted as compensation.
fn settle_observed(
    journal: &mut Journal,
    target: &ManagedModel,
    digest: &str,
    claimed_id: Option<i64>,
    actual: Result<Vec<ObservedModel>>,
    checkpoint_id: Uuid,
) -> Result<ModelInstallOutcome> {
    let models = match actual {
        Ok(models) => models,
        Err(_) => {
            mark_unknown(
                journal,
                "MODEL_READBACK_UNAVAILABLE",
                "read-back failed after success",
            )?;
            return Ok(outcome(
                target,
                digest,
                "needs_recovery",
                None,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ));
        }
    };
    let found = match models.as_slice() {
        [one] if Some(one.id) == claimed_id || claimed_id.is_none() => Some(one),
        _ => None,
    };
    let observed = found.map(observed_digest).transpose()?;
    journal.advance(|j| {
        j.steps[0].state = StepState::ObservedSuccess;
        j.steps[0].observed_digest = observed
            .clone()
            .or_else(|| canonical::digest("lab-model-readback-v1", &models).ok());
        j.state = OperationState::Verifying;
    })?;
    match found {
        Some(model) if exact_match(model, target) => {
            journal.advance(|j| {
                j.steps[0].state = StepState::Verified;
                j.steps[0].observed_digest = Some(digest.into());
                j.state = OperationState::Committed;
            })?;
            Ok(outcome(
                target,
                digest,
                "created",
                Some(model.id),
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ))
        }
        _ => {
            journal.advance(|j| {
                j.state = OperationState::NeedsRecovery;
                j.issues.push(issue(
                    "MODEL_PARTIAL_REQUIRES_RECOVERY",
                    "read-back differs from the manifest; notes must not use this model and it is not deleted automatically",
                ));
            })?;
            Ok(outcome(
                target,
                digest,
                "needs_recovery",
                found.map(|m| m.id),
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ))
        }
    }
}

/// ALG-MODEL step 3 reconciliation. Adoption needs the exact name, the exact
/// manifest and companion operation evidence naming the same model; the name
/// alone is never enough. No request is re-sent.
pub fn reconcile(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ModelPort,
    operation_id: Uuid,
) -> Result<ModelInstallOutcome> {
    store.validate_lease(lease)?;
    let record = store.model_operation(operation_id)?;
    let target: ManagedModel = serde_json::from_value(record.evidence["manifest"].clone())
        .map_err(|_| "MODEL_OPERATION_CORRUPT")?;
    if manifest_digest(&target)? != record.manifest_digest {
        return Err("MODEL_OPERATION_CORRUPT".into());
    }
    let version = store.journal(operation_id)?;
    let checkpoint_id = version.journal.backup_id;
    let digest = record.manifest_digest.clone();
    let mut journal = Journal { store, version };
    match journal.version.journal.state {
        OperationState::Committed => {
            let models = port.models_named(&target.name)?;
            let id = models.first().map(|m| m.id);
            return Ok(outcome(
                &target,
                &digest,
                "created",
                id,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ));
        }
        OperationState::FailedBeforeWrite => {
            return Ok(outcome(
                &target,
                &digest,
                "not_created",
                None,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ));
        }
        _ => {}
    }
    let step = journal.version.journal.steps[0].state;
    if step == StepState::IntentRecorded {
        // Never dispatched: close the operation without any collection effect.
        let state = journal.version.journal.state;
        if matches!(
            state,
            OperationState::Prepared | OperationState::Preflight | OperationState::Checkpointed
        ) {
            journal.advance(|j| {
                j.state = OperationState::FailedBeforeWrite;
                j.issues.push(issue(
                    "MODEL_INSTALL_NOT_DISPATCHED",
                    "request was never started",
                ));
            })?;
            return Ok(outcome(
                &target,
                &digest,
                "not_created",
                None,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ));
        }
    }
    if step == StepState::RequestStarted {
        // Crash between durable request_started and any observation.
        mark_unknown(
            &mut journal,
            "MODEL_INSTALL_OUTCOME_UNKNOWN",
            "request started without a recorded observation",
        )?;
    }
    let models = port.models_named(&target.name)?;
    let evidence = port.operation_evidence(operation_id)?;
    let exact: Vec<&ObservedModel> = models.iter().filter(|m| exact_match(m, &target)).collect();
    match (&evidence, models.as_slice(), exact.as_slice()) {
        (
            OperationEvidence::Created {
                model_id,
                manifest_digest: evidence_digest,
            },
            [_],
            [model],
        ) if *evidence_digest == digest && model.id == *model_id => {
            let model_id = model.id;
            journal.advance(|j| {
                j.steps[0].state = StepState::Verified;
                j.steps[0].observed_digest = Some(digest.clone());
                j.state = OperationState::Verifying;
            })?;
            journal.advance(|j| j.state = OperationState::Committed)?;
            Ok(outcome(
                &target,
                &digest,
                "created",
                Some(model_id),
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ))
        }
        (OperationEvidence::FailedBeforeWrite, [], _)
            if journal.version.journal.steps[0].state == StepState::Unknown =>
        {
            let observed = canonical::digest("lab-model-failure-v1", "reconciled_absent").ok();
            journal.advance(|j| {
                j.steps[0].state = StepState::ObservedFailure;
                j.steps[0].observed_digest = observed;
                j.state = OperationState::FailedBeforeWrite;
                j.issues.push(issue(
                    "MODEL_INSTALL_RECONCILED_ABSENT",
                    "companion evidence and read-back both show no model was created",
                ));
            })?;
            Ok(outcome(
                &target,
                &digest,
                "not_created",
                None,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ))
        }
        _ => {
            let code = if !models.is_empty() && exact.is_empty() {
                "MODEL_PARTIAL_REQUIRES_RECOVERY"
            } else {
                "MODEL_RECONCILE_EVIDENCE_INSUFFICIENT"
            };
            let already = journal
                .version
                .journal
                .issues
                .last()
                .is_some_and(|last| last.code == code);
            if !already {
                journal.advance(|j| {
                    j.issues.push(issue(
                        code,
                        "name or manifest alone cannot prove this operation's outcome",
                    ))
                })?;
            }
            Ok(outcome(
                &target,
                &digest,
                "needs_recovery",
                None,
                Some(&journal.version.journal),
                Some(checkpoint_id),
            ))
        }
    }
}
