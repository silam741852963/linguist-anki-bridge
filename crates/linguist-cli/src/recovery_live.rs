//! Read-only native ledger observations; no collection-state reconciliation or writes.
use linguist_anki::{
    Client,
    native::{MutationVariant, NativeOperationStatus},
};
use linguist_core::records::StepState;
use linguist_store::journal::JournalVersion;
use serde::Serialize;

#[derive(Serialize)]
pub struct LiveStep {
    pub step_id: uuid::Uuid,
    pub local_state: StepState,
    pub native_status: Option<NativeOperationStatus>,
    pub intent_conflicts: Vec<&'static str>,
    pub read_error: Option<String>,
}

#[derive(Serialize)]
pub struct LiveJournal {
    pub journal_id: uuid::Uuid,
    pub binding_conflicts: Vec<&'static str>,
    pub steps: Vec<LiveStep>,
}

#[derive(Serialize)]
pub struct LiveReport {
    pub native_manifest_digest: Option<String>,
    pub capability_error: Option<String>,
    pub status_action_advertised: bool,
    pub status_reads_completed: u32,
    pub journals: Vec<LiveJournal>,
    pub native_effects_verified: bool,
    pub reconciliation_available: bool,
}

impl LiveReport {
    pub fn unavailable(error: String) -> Self {
        Self {
            native_manifest_digest: None,
            capability_error: Some(error),
            status_action_advertised: false,
            status_reads_completed: 0,
            journals: vec![],
            native_effects_verified: false,
            reconciliation_available: false,
        }
    }

    pub fn dependency_unavailable(&self) -> bool {
        self.capability_error.is_some()
            || self
                .journals
                .iter()
                .any(|journal| journal.steps.iter().any(|step| step.read_error.is_some()))
    }
}

pub fn inspect(
    journals: &[JournalVersion],
    client: &Client,
    configured_endpoint: &str,
    max_steps: usize,
) -> Result<LiveReport, String> {
    let steps = journals.iter().try_fold(0usize, |count, version| {
        count
            .checked_add(
                version
                    .journal
                    .steps
                    .iter()
                    .filter(|step| step.state != StepState::IntentRecorded)
                    .count(),
            )
            .ok_or("RECOVERY_LIVE_STEP_LIMIT")
    })?;
    if steps > max_steps {
        return Err("RECOVERY_LIVE_STEP_LIMIT".into());
    }
    let inspected = match client.native_capabilities() {
        Ok(value) => value,
        Err(error) => return Ok(LiveReport::unavailable(error)),
    };
    let advertised = inspected
        .declaration
        .actions
        .iter()
        .any(|action| action == "labOperationStatus");
    let mut report = LiveReport {
        native_manifest_digest: Some(inspected.manifest_digest.clone()),
        capability_error: (!advertised).then(|| "ANKI_NATIVE_STATUS_UNAVAILABLE".into()),
        status_action_advertised: advertised,
        status_reads_completed: 0,
        journals: vec![],
        native_effects_verified: false,
        reconciliation_available: false,
    };
    for version in journals {
        let journal = &version.journal;
        let mut binding_conflicts = vec![];
        if journal.binding.endpoint != configured_endpoint {
            binding_conflicts.push("endpoint_changed");
        }
        if journal.binding.bridge_id != inspected.declaration.bridge_id {
            binding_conflicts.push("bridge_changed");
        }
        if journal.binding.capability_digest != inspected.manifest_digest {
            binding_conflicts.push("capability_changed");
        }
        match &inspected.declaration.collection_session {
            Some(session) => {
                if session.lineage_id != journal.binding.lineage_id {
                    binding_conflicts.push("lineage_changed");
                }
                if session.session_epoch != journal.binding.session_epoch {
                    binding_conflicts.push("session_changed");
                }
                if session.profile_fingerprint != journal.binding.profile_fingerprint {
                    binding_conflicts.push("profile_changed");
                }
                if session.path_fingerprint != journal.binding.path_fingerprint {
                    binding_conflicts.push("path_changed");
                }
            }
            None => binding_conflicts.push("current_session_unavailable"),
        }
        let mut observed = LiveJournal {
            journal_id: journal.id,
            binding_conflicts,
            steps: vec![],
        };
        for step in &journal.steps {
            if step.state == StepState::IntentRecorded {
                continue;
            }
            let mut observation = LiveStep {
                step_id: step.id,
                local_state: step.state,
                native_status: None,
                intent_conflicts: vec![],
                read_error: None,
            };
            if !advertised {
                observation.read_error = Some("ANKI_NATIVE_STATUS_UNAVAILABLE".into());
            } else if journal.binding.bridge_id != inspected.declaration.bridge_id {
                observation.read_error = Some("ANKI_NATIVE_BRIDGE_CONFLICT".into());
            } else {
                match client.native_operation_status(journal.binding.lineage_id, step.id) {
                    Ok(status) => {
                        report.status_reads_completed += 1;
                        if status.payload_digest != step.payload_digest {
                            observation.intent_conflicts.push("payload_changed");
                        }
                        if status.approved_digest != journal.approval_digest {
                            observation.intent_conflicts.push("approval_changed");
                        }
                        if status.session_epoch != journal.binding.session_epoch {
                            observation
                                .intent_conflicts
                                .push("operation_session_changed");
                        }
                        let expected: Option<MutationVariant> =
                            serde_json::from_value(serde_json::json!(step.action)).ok();
                        if expected != Some(status.variant) {
                            observation
                                .intent_conflicts
                                .push("variant_changed_or_unknown");
                        }
                        observation.native_status = Some(status);
                    }
                    Err(error) => observation.read_error = Some(error),
                }
            }
            observed.steps.push(observation);
        }
        report.journals.push(observed);
    }
    Ok(report)
}
