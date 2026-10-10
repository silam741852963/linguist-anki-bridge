//! ALG-SPLIT: apply one reviewed grammar split group. Stable child operation
//! IDs and one source snapshot are recorded before any write. Every non-anchor
//! unit is created as a fresh note with its own marker, journal and receipt;
//! only after all of them verify is the anchor updated or migrated in place,
//! keeping its retained history. Any failure leaves an exact partial group.
//! Resuming reconciles unresolved units and never recreates a verified one.
use crate::apply::{self, ApplyItemOutcome, ApplyPort, ApplyRequest, ReconcileRequest, Role, Slot};
use crate::backup::Result;
use linguist_core::records::{OperationState, PlanRevision};
use linguist_store::{
    Store,
    lease::LeaseToken,
    restore::{SplitAttempt, SplitExecutionRecord},
};
use serde::Serialize;
use uuid::Uuid;

pub struct SplitApplyRequest<'a> {
    /// The current invocation's explicit `--apply`.
    pub apply: bool,
    pub plan_id: Uuid,
    pub revision: u32,
    pub digest: &'a str,
    pub grammar_group: Uuid,
    /// One approval that covers every unit of the group.
    pub approval_id: Uuid,
    pub checkpoint_id: Uuid,
    pub protected_manifest_digest: &'a str,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
    pub max_media_bytes: u64,
    pub accept_schema_change: bool,
    pub now_ms: u64,
    /// Execution ID used when no execution exists yet; the caller may
    /// pre-allocate it so the checkpoint carries the same group.
    pub new_execution_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnitStatus {
    pub document_id: Uuid,
    pub role: &'static str,
    pub attempts: Vec<Uuid>,
    pub operation_id: Uuid,
    pub state: Option<OperationState>,
    /// `verified`, `not_started`, `failed_before_write`, `needs_recovery`,
    /// `in_progress` or `restored`.
    pub status: &'static str,
    pub note_id: Option<i64>,
    pub next_command: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SplitOutcome {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub revision: u32,
    pub grammar_group: Uuid,
    pub source_snapshot: Uuid,
    /// `complete`, `partial`, `not_started` or `rolled_back`.
    pub state: &'static str,
    pub units: Vec<UnitStatus>,
    pub issues: Vec<String>,
    pub next_command: Option<String>,
}

fn resume_command(plan: Uuid, revision: u32, group: Uuid) -> String {
    format!("linguist-anki-bridge apply {plan} --revision {revision} --split-group {group} --apply")
}

fn latest(attempts: &[SplitAttempt], document: Uuid) -> Result<SplitAttempt> {
    attempts
        .iter()
        .filter(|a| a.document == document)
        .max_by_key(|a| a.sequence)
        .cloned()
        .ok_or_else(|| "SPLIT_EXECUTION_CORRUPT".into())
}

fn request_for<'a>(
    request: &SplitApplyRequest<'a>,
    item_id: Uuid,
    execution: Uuid,
) -> ApplyRequest<'a> {
    ApplyRequest {
        apply: request.apply,
        plan_id: request.plan_id,
        revision: request.revision,
        digest: request.digest,
        item_id,
        approval_id: request.approval_id,
        checkpoint_id: request.checkpoint_id,
        group_id: Some(execution),
        protected_manifest_digest: request.protected_manifest_digest,
        reuse_max_age_seconds: request.reuse_max_age_seconds,
        max_package_bytes: request.max_package_bytes,
        max_media_bytes: request.max_media_bytes,
        accept_schema_change: request.accept_schema_change,
        now_ms: request.now_ms,
    }
}

fn units(plan: &PlanRevision, group: Uuid) -> Result<(Uuid, Vec<Uuid>)> {
    let group = plan
        .grammar_groups
        .iter()
        .find(|g| g.id == group)
        .ok_or("SPLIT_GROUP_NOT_FOUND")?;
    let children = group
        .units
        .iter()
        .copied()
        .filter(|id| *id != group.anchor_document)
        .collect();
    Ok((group.anchor_document, children))
}

/// Apply or resume one reviewed grammar split group under a held lease.
pub fn apply_group(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &SplitApplyRequest,
) -> Result<SplitOutcome> {
    if !request.apply {
        return Err("APPLY_FLAG_REQUIRED: the current invocation must pass --apply".into());
    }
    store.validate_lease(lease)?;
    let plan = store.revision(request.plan_id, request.revision)?;
    let (anchor, children) = units(&plan, request.grammar_group)?;
    let record = match store.split_execution_for(
        request.plan_id,
        request.revision,
        request.grammar_group,
    )? {
        Some(record) => record,
        None => start(store, port, request, anchor, &children)?,
    };
    drive_group(store, lease, port, request, &record)
}

/// ALG-SPLIT steps 1-2: check every unit, snapshot the source once and
/// allocate stable operation IDs, all before the first write.
fn start(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    request: &SplitApplyRequest,
    anchor: Uuid,
    children: &[Uuid],
) -> Result<SplitExecutionRecord> {
    let execution = request.new_execution_id.unwrap_or_else(Uuid::new_v4);
    for child in children {
        apply::check_item(
            store,
            port,
            &request_for(request, *child, execution),
            Role::SplitChild,
        )
        .map_err(|code| format!("SPLIT_PREFLIGHT_FAILED: unit {child}: {code}"))?;
    }
    let anchor_check = apply::check_item(
        store,
        port,
        &request_for(request, anchor, execution),
        Role::SplitAnchor,
    )
    .map_err(|code| format!("SPLIT_PREFLIGHT_FAILED: anchor {anchor}: {code}"))?;
    let source = anchor_check
        .observed()
        .ok_or("SPLIT_ANCHOR_SOURCE_MISSING")?
        .clone();
    let snapshot = apply::fresh_snapshot(
        store,
        port,
        execution,
        Some(&source),
        anchor_check.model(),
        anchor_check.migration(),
        request.now_ms,
    )?;
    let record = SplitExecutionRecord {
        execution_id: execution,
        plan_id: request.plan_id,
        revision: request.revision,
        grammar_group: request.grammar_group,
        anchor_document: anchor,
        children: children.to_vec(),
        source_snapshot: snapshot.id,
        created_ms: request.now_ms,
    };
    let first: Vec<(Uuid, Uuid)> = children
        .iter()
        .chain(std::iter::once(&anchor))
        .map(|unit| (*unit, Uuid::new_v4()))
        .collect();
    store.publish_split_execution(&record, &first)?;
    Ok(record)
}

/// What one unit's latest attempt shows, read locally.
enum Unit {
    Verified,
    Restored,
    /// No journal and no intent: nothing was ever sent for this attempt.
    Unsent,
    /// A new attempt is safe: no effect, or only verified reusable media.
    Retry,
    Unresolved,
}

fn unit_state(store: &Store, operation: Uuid) -> Result<Unit> {
    match store.journal(operation) {
        Ok(version) => Ok(match version.journal.state {
            OperationState::Committed => Unit::Verified,
            OperationState::Restored => Unit::Restored,
            OperationState::FailedBeforeWrite => Unit::Retry,
            _ if apply::superseded_safely(&version.journal) => Unit::Retry,
            _ => Unit::Unresolved,
        }),
        Err(code) if code == "JOURNAL_NOT_FOUND" => match store.apply_operation(operation) {
            // An intent without a journal was never dispatched.
            Ok(_) => Ok(Unit::Retry),
            Err(code) if code == "APPLY_OPERATION_NOT_FOUND" => Ok(Unit::Unsent),
            Err(code) => Err(code),
        },
        Err(code) => Err(code),
    }
}

/// ALG-SPLIT steps 2-4 for an allocated execution.
fn drive_group(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &SplitApplyRequest,
    record: &SplitExecutionRecord,
) -> Result<SplitOutcome> {
    let mut issues = Vec::new();
    let order: Vec<(Uuid, Role)> = record
        .children
        .iter()
        .map(|c| (*c, Role::SplitChild))
        .chain(std::iter::once((record.anchor_document, Role::SplitAnchor)))
        .collect();
    'units: for (unit, role) in order {
        // One checkpoint plus many units can outlast a lease period (WP-23).
        crate::backup::keep_writer_lease(store, lease)?;
        loop {
            let attempt = latest(&store.split_attempts(record.execution_id)?, unit)?;
            match unit_state(store, attempt.operation)? {
                Unit::Verified => continue 'units,
                Unit::Restored => {
                    issues.push(format!(
                        "SPLIT_GROUP_ROLLED_BACK: unit {unit} was restored; prepare a new plan revision to apply again"
                    ));
                    break 'units;
                }
                Unit::Retry => {
                    store.append_split_attempt(record.execution_id, unit)?;
                }
                Unit::Unsent => {
                    let slot = Slot {
                        operation: attempt.operation,
                        main_step: Uuid::new_v4(),
                        role,
                    };
                    let item = request_for(request, unit, record.execution_id);
                    match apply::apply_item_with(store, lease, port, &item, &slot) {
                        Ok(ApplyItemOutcome {
                            state: OperationState::Committed,
                            ..
                        }) => continue 'units,
                        Ok(outcome) => {
                            issues.extend(outcome.issues.iter().map(|i| i.code.clone()));
                            break 'units;
                        }
                        Err(code) => {
                            issues.push(format!("unit {unit}: {code}"));
                            break 'units;
                        }
                    }
                }
                Unit::Unresolved => {
                    // Reconcile the same operation; never a new attempt.
                    let result = apply::reconcile(
                        store,
                        lease,
                        port,
                        &ReconcileRequest {
                            operation_id: attempt.operation,
                            apply: true,
                            rebind: None,
                        },
                    );
                    match result {
                        Ok(outcome) if outcome.state == OperationState::Committed => {
                            continue 'units;
                        }
                        // Proven no effect: the next pass starts one new attempt.
                        Ok(outcome) if outcome.state == OperationState::FailedBeforeWrite => {
                            continue;
                        }
                        Ok(outcome) => {
                            issues.extend(outcome.issues.iter().map(|i| i.code.clone()));
                            break 'units;
                        }
                        Err(code) => {
                            issues.push(format!("unit {unit}: {code}"));
                            break 'units;
                        }
                    }
                }
            }
        }
    }
    let mut outcome = status(store, record.execution_id)?;
    outcome.issues = issues;
    Ok(outcome)
}

/// Local group status from stored attempts and journals; reads no collection.
pub fn status(store: &Store, execution: Uuid) -> Result<SplitOutcome> {
    let record = store.split_execution(execution)?;
    let attempts = store.split_attempts(execution)?;
    let mut units_out = Vec::new();
    let order: Vec<(Uuid, &'static str)> = record
        .children
        .iter()
        .map(|c| (*c, "child"))
        .chain(std::iter::once((record.anchor_document, "anchor")))
        .collect();
    for (unit, role) in order {
        let latest = latest(&attempts, unit)?;
        let journal = match store.journal(latest.operation) {
            Ok(version) => Some(version.journal),
            Err(code) if code == "JOURNAL_NOT_FOUND" => None,
            Err(code) => return Err(code),
        };
        let state = journal.as_ref().map(|j| j.state);
        let status = match (&journal, state) {
            (None, _) => "not_started",
            (_, Some(OperationState::Committed)) => "verified",
            (_, Some(OperationState::Restored)) => "restored",
            (_, Some(OperationState::FailedBeforeWrite)) => "failed_before_write",
            (Some(j), _) if apply::superseded_safely(j) => "failed_before_write",
            (_, Some(OperationState::NeedsRecovery)) => "needs_recovery",
            _ => "in_progress",
        };
        let note_id = match &journal {
            Some(j)
                if matches!(
                    j.state,
                    OperationState::Committed | OperationState::Restored
                ) =>
            {
                store
                    .snapshot(j.snapshot_id)?
                    .after
                    .and_then(|receipt| receipt.readback)
                    .and_then(|readback| readback.note_ids.first().cloned())
                    .and_then(|id| String::from(id).parse().ok())
            }
            _ => None,
        };
        units_out.push(UnitStatus {
            document_id: unit,
            role,
            attempts: attempts
                .iter()
                .filter(|a| a.document == unit)
                .map(|a| a.operation)
                .collect(),
            operation_id: latest.operation,
            state,
            status,
            note_id,
            next_command: matches!(status, "needs_recovery" | "in_progress")
                .then(|| apply::reconcile_command_for(latest.operation)),
        });
    }
    let state = if units_out.iter().any(|u| u.status == "restored") {
        "rolled_back"
    } else if units_out.iter().all(|u| u.status == "verified") {
        "complete"
    } else if units_out.iter().all(|u| u.status == "not_started") {
        "not_started"
    } else {
        "partial"
    };
    Ok(SplitOutcome {
        execution_id: execution,
        plan_id: record.plan_id,
        revision: record.revision,
        grammar_group: record.grammar_group,
        source_snapshot: record.source_snapshot,
        state,
        next_command: matches!(state, "partial" | "not_started")
            .then(|| resume_command(record.plan_id, record.revision, record.grammar_group)),
        units: units_out,
        issues: vec![],
    })
}
