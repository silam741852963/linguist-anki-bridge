//! ALG-JOB for simulate and apply jobs: immutable definitions, explicit run
//! authority, durable controls with confirmed stop acknowledgements, a single
//! serialized collection writer, retry classification with per-item attempt
//! budgets, group halts on shared faults, reconciliation before any new
//! dispatch, grouped rollback previews, audits and local tombstones.
//!
//! Every native effect goes through the WP-11 `apply` driver. This module adds
//! no effect of its own: an apply job is a durable, resumable loop over
//! `apply_item` with stable operation IDs recorded before each dispatch.
use crate::apply::{self, ApplyPort, ApplyRequest, ReconcileRequest, Role, SimulatedItem, Slot};
use linguist_core::records::{JobMode, OperationState, ResolvedSettings};
use linguist_store::{
    Store,
    apply_job::{
        ApplyJobDefinition, ApplyJobItem, ApplyJobReceipt, ApplyJobStage, ApplyJobStatus,
        ItemOutcome, JobControl, JobTombstone, RetryClass, StopReason, fold, item_ref,
    },
    lease::{LeaseToken, Resource},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;

/// Error codes produced when a fault is shared by every item of the group:
/// collection identity, session, shared schema/model, approval scope or local
/// durability. They halt the group whatever `jobs.on_item_error` says.
const HALT_PREFIXES: &[&str] = &[
    "APPLY_IDENTITY_MISMATCH",
    "SESSION_CHANGED",
    "APPLY_BINDING_WEAK",
    "APPLY_REMOTE_MUTATION_UNAVAILABLE",
    "APPLY_LOCAL_DURABILITY_FAILED",
    "APPLY_REVISION_STALE",
    "APPLY_DIGEST_MISMATCH",
    "APPLY_APPROVAL_MISMATCH",
    "APPLY_MODEL_",
    "MODEL_",
    "CHECKPOINT_",
    "CAPABILITY_UNAVAILABLE",
    "LEASE_",
    "STORE_",
    "APPLY_OPERATION_CORRUPT",
    "APPLY_INTENT_CORRUPT",
];

/// Dependency/transport failures that leave no effect and may succeed later.
const TRANSIENT_PREFIXES: &[&str] = &[
    "ANKI_READ_TIMEOUT",
    "ANKI_DEPENDENCY_UNAVAILABLE",
    "ANKI_HTTP_FAILURE",
    "APPLY_OWNER_UNAVAILABLE",
    "JOB_ITEM_INTERRUPTED",
];

/// Unknown or partial native outcomes: reconcile, never retry.
const RECONCILE_PREFIXES: &[&str] = &[
    "APPLY_RECOVERY_REQUIRED",
    "APPLY_OUTCOME_UNKNOWN",
    "APPLY_KNOWN_PARTIAL",
    "APPLY_NATIVE_PENDING",
    "APPLY_RESUBMIT",
    "APPLY_ABSENCE_UNPROVEN",
    "APPLY_RECONCILE_REVIEW_REQUIRED",
    "APPLY_REJECTED_WITH_EFFECT",
];

/// Retry classification of one stable error code.
pub fn classify(code: &str) -> RetryClass {
    let has = |list: &[&str]| list.iter().any(|p| code.starts_with(p));
    if has(RECONCILE_PREFIXES) {
        RetryClass::Reconcile
    } else if has(HALT_PREFIXES) {
        RetryClass::Halt
    } else if has(TRANSIENT_PREFIXES) {
        RetryClass::Transient
    } else {
        RetryClass::Permanent
    }
}

/// The stable code of an error message (`CODE: detail`).
fn code_of(message: &str) -> String {
    let code: String = message
        .split(':')
        .next()
        .unwrap_or(message)
        .trim()
        .chars()
        .take(100)
        .collect();
    if !code.is_empty()
        && code
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
    {
        code
    } else {
        "JOB_ITEM_ERROR_UNCLASSIFIED".into()
    }
}

pub struct CreateRequest<'a> {
    pub mode: JobMode,
    pub plan_id: Uuid,
    pub revision: u32,
    pub digest: &'a str,
    pub approval_id: Uuid,
    /// Empty selects every approved item in plan order.
    pub item_ids: Vec<Uuid>,
    pub checkpoint_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub accept_schema_change: bool,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreateOutcome {
    pub job_id: Uuid,
    pub mode: JobMode,
    pub digest: String,
    pub plan_id: Uuid,
    pub revision: u32,
    pub item_count: usize,
    pub state: &'static str,
    pub worker_started: bool,
}

fn validate_job_settings(settings: &ResolvedSettings) -> Result<()> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "jobs.max_item_attempts",
        "jobs.on_item_error",
        "jobs.lease_seconds",
        "jobs.heartbeat_seconds",
        "jobs.pause_poll_ms",
        "backup.reuse_max_age_seconds",
        "backup.max_package_gb",
        "media.max_asset_mb",
        "anki.commit_interval_seconds",
        "anki.native_adapter",
    ] {
        registry.validate_value(key, settings.values.get(key).ok_or("JOB_SETTING_MISSING")?)?;
    }
    crate::apply::require_native_adapter(&settings.values)?;
    let lease = settings.values["jobs.lease_seconds"].as_u64().unwrap();
    if settings.values["jobs.heartbeat_seconds"].as_u64().unwrap() * 3 >= lease {
        return Err("JOB_HEARTBEAT_INTERVAL_INVALID".into());
    }
    Ok(())
}

/// OP-35 for simulate/apply: freeze an approved, ready revision's items with
/// the settings and authority scope. Creation never runs the job.
pub fn create(
    store: &mut Store,
    settings: &ResolvedSettings,
    request: &CreateRequest,
) -> Result<CreateOutcome> {
    if request.mode == JobMode::Prepare {
        return Err("JOB_MODE_INVALID: prepare jobs are created from note selectors".into());
    }
    validate_job_settings(settings)?;
    if request.mode == JobMode::Apply && request.checkpoint_id.is_none() {
        return Err(
            "JOB_CHECKPOINT_REQUIRED: apply jobs name the verified checkpoint every item depends on"
                .into(),
        );
    }
    // A checkpoint created for a group (`backup create --group G`) makes G the
    // job ID, so every item reuses it as the same group; otherwise items need
    // the cross-group reuse rule (age and protected manifest).
    let mut adopted = None;
    if let Some(checkpoint) = request.checkpoint_id {
        let record = store.checkpoint(checkpoint)?;
        if let Some(group) = record.evidence["group_id"].as_str() {
            let group = Uuid::parse_str(group).map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?;
            let taken = match store.apply_job(group) {
                Ok(_) => true,
                Err(code) if code == "APPLY_JOB_NOT_FOUND" => store.preparation_job(group).is_ok(),
                Err(code) => return Err(code),
            };
            if taken {
                return Err(
                    "JOB_CHECKPOINT_GROUP_IN_USE: the checkpoint's group already names another job"
                        .into(),
                );
            }
            adopted = Some(group);
        }
    }
    let plan = store.revision(request.plan_id, request.revision)?;
    let approval = store.approval(request.approval_id)?.approval;
    let order: Vec<Uuid> = plan.documents.iter().map(|d| d.id).collect();
    let requested: BTreeSet<Uuid> = request.item_ids.iter().copied().collect();
    if requested.len() != request.item_ids.len() {
        return Err("JOB_ITEM_DUPLICATE".into());
    }
    if let Some(unknown) = requested.iter().find(|id| !order.contains(id)) {
        return Err(format!("APPLY_ITEM_NOT_FOUND: {unknown}"));
    }
    let items: Vec<Uuid> = order
        .into_iter()
        .filter(|id| {
            if requested.is_empty() {
                approval.item_ids.contains(id)
            } else {
                requested.contains(id)
            }
        })
        .collect();
    if items.is_empty() {
        return Err("JOB_INPUT_REQUIRED_OR_LIMIT: the approval covers no item".into());
    }
    if items.len() > linguist_store::apply_job::MAX_APPLY_JOB_ITEMS {
        return Err("JOB_INPUT_LIMIT_EXCEEDED".into());
    }
    // Every item must be approved, ready and standalone now; nothing is read from Anki.
    for item in &items {
        apply::authorize(
            store,
            request.plan_id,
            request.revision,
            request.digest,
            *item,
            request.approval_id,
        )?;
    }
    let id = adopted.unwrap_or_else(Uuid::new_v4);
    let definition = ApplyJobDefinition {
        schema_version: 1,
        job: linguist_core::records::Job {
            id,
            mode: request.mode,
            settings: settings.clone(),
            plan_refs: items
                .iter()
                .map(|i| item_ref(request.plan_id, request.revision, *i))
                .collect(),
            item_ids: items.clone(),
            pause_requested: false,
            cancel_requested: false,
        },
        plan_id: request.plan_id,
        revision: request.revision,
        plan_digest: request.digest.into(),
        approval_id: request.approval_id,
        checkpoint_id: request.checkpoint_id,
        protected_manifest_digest: request.protected_manifest_digest.into(),
        accept_schema_change: request.accept_schema_change,
        created_ms: request.now_ms,
    };
    let digest = store.create_apply_job(&definition)?;
    Ok(CreateOutcome {
        job_id: id,
        mode: request.mode,
        digest,
        plan_id: request.plan_id,
        revision: request.revision,
        item_count: items.len(),
        state: "queued",
        worker_started: false,
    })
}

pub struct RunRequest {
    pub job: Uuid,
    /// The current invocation's explicit `--apply`.
    pub apply: bool,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemReport {
    pub item_id: Uuid,
    pub attempt: u16,
    pub operation_id: Option<Uuid>,
    pub outcome: ItemOutcome,
    pub code: Option<String>,
    pub retry: RetryClass,
    pub simulated: Option<SimulatedItem>,
    pub receipt_digest: Option<String>,
    pub next_command: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunReport {
    pub job_id: Uuid,
    pub mode: JobMode,
    pub reconciled: Vec<ItemReport>,
    pub dispatched: Vec<ItemReport>,
    pub stop: StopReason,
    pub stop_code: Option<String>,
    pub status: ApplyJobStatus,
    pub items_remaining: usize,
    pub writes_enabled: bool,
    pub next_commands: Vec<String>,
    pub exit_code: u8,
}

fn request_for<'a>(def: &'a ApplyJobDefinition, item: Uuid, now_ms: u64) -> ApplyRequest<'a> {
    let values = &def.job.settings.values;
    let gib = 1024 * 1024 * 1024;
    ApplyRequest {
        apply: def.job.mode == JobMode::Apply,
        plan_id: def.plan_id,
        revision: def.revision,
        digest: &def.plan_digest,
        item_id: item,
        approval_id: def.approval_id,
        checkpoint_id: def.checkpoint_id.unwrap_or(Uuid::nil()),
        group_id: Some(def.job.id),
        protected_manifest_digest: &def.protected_manifest_digest,
        reuse_max_age_seconds: values["backup.reuse_max_age_seconds"].as_u64().unwrap(),
        max_package_bytes: values["backup.max_package_gb"].as_u64().unwrap() * gib,
        max_media_bytes: values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024,
        accept_schema_change: def.accept_schema_change,
        now_ms,
    }
}

fn state(store: &Store, def: &ApplyJobDefinition) -> Result<(Vec<ApplyJobItem>, ApplyJobStatus)> {
    let events = store.apply_job_events(def.job.id)?;
    let worker = store.job_worker_status(def.job.id)?;
    Ok(fold(def, &events, &worker))
}

fn latest_control(events: &[ApplyJobReceipt]) -> Option<(u32, JobControl)> {
    events.iter().rev().find_map(|r| match r.event.stage {
        ApplyJobStage::Control { action } => Some((r.event.sequence, action)),
        _ => None,
    })
}

/// Classified result of one apply attempt.
fn from_apply(
    result: &Result<apply::ApplyItemOutcome>,
) -> (ItemOutcome, Option<String>, RetryClass) {
    match result {
        Ok(outcome) => match outcome.state {
            OperationState::Committed => (ItemOutcome::Committed, None, RetryClass::None),
            OperationState::FailedBeforeWrite => {
                let code = outcome
                    .issues
                    .last()
                    .map(|i| i.code.clone())
                    .unwrap_or_else(|| "APPLY_REJECTED_BEFORE_WRITE".into());
                let class = match classify(&code) {
                    RetryClass::Reconcile => RetryClass::Permanent,
                    class => class,
                };
                (ItemOutcome::FailedBeforeWrite, Some(code), class)
            }
            _ => {
                let code = outcome
                    .issues
                    .iter()
                    .rev()
                    .map(|i| i.code.as_str())
                    .find(|c| classify(c) == RetryClass::Reconcile)
                    .unwrap_or("APPLY_RECOVERY_REQUIRED")
                    .to_owned();
                (
                    ItemOutcome::NeedsRecovery,
                    Some(code),
                    RetryClass::Reconcile,
                )
            }
        },
        Err(message) => {
            let code = code_of(message);
            let class = classify(&code);
            let outcome = if class == RetryClass::Reconcile {
                ItemOutcome::NeedsRecovery
            } else {
                ItemOutcome::Failed
            };
            (outcome, Some(code), class)
        }
    }
}

struct Worker<'a> {
    store: &'a mut Store,
    lease: LeaseToken,
    seconds: u64,
    job: Uuid,
}

impl Worker<'_> {
    fn append(&mut self, stage: ApplyJobStage) -> Result<ApplyJobReceipt> {
        self.store
            .append_apply_job_event(self.job, stage, &self.lease)
    }
    fn renew(&mut self) -> Result<()> {
        self.store.renew_lease(&self.lease, self.seconds)
    }
    fn stop(
        &mut self,
        reason: StopReason,
        control_sequence: Option<u32>,
        code: Option<String>,
    ) -> Result<()> {
        self.append(ApplyJobStage::Stopped {
            reason,
            control_sequence,
            idle: false,
            code,
        })
        .map(|_| ())
    }
}

/// OP-39/OP-41 for simulate/apply jobs: ALG-JOB under the job lease. Apply
/// mode also needs the current `--apply` and the single collection-writer
/// lease; simulate mode never writes and refuses `--apply`.
pub fn run(
    store: &mut Store,
    writer: Option<&LeaseToken>,
    port: &mut dyn ApplyPort,
    request: &RunRequest,
) -> Result<RunReport> {
    let def = store.apply_job(request.job)?;
    if store.job_tombstone(request.job)?.is_some() {
        return Err("JOB_TOMBSTONED: a deleted job cannot run".into());
    }
    match (def.job.mode, request.apply) {
        (JobMode::Simulate, true) => {
            return Err(
                "JOB_MODE_NEVER_WRITES: simulate jobs perform preflight only; create an apply job"
                    .into(),
            );
        }
        (JobMode::Apply, false) => {
            return Err(
                "APPLY_FLAG_REQUIRED: apply jobs need --apply on every run, resume and retry"
                    .into(),
            );
        }
        (JobMode::Prepare, _) => return Err("APPLY_JOB_NOT_FOUND".into()),
        _ => {}
    }
    validate_job_settings(&def.job.settings)?;
    let writer = match def.job.mode {
        JobMode::Apply => {
            let writer = writer.ok_or("APPLY_WRITER_LEASE_REQUIRED")?;
            store.validate_lease(writer)?;
            Some(writer)
        }
        _ => None,
    };
    let seconds = def.job.settings.values["jobs.lease_seconds"]
        .as_u64()
        .unwrap();
    // Refuses a live owner even after its lease expired.
    let lease = store.acquire_lease(&Resource::JobWorker(def.job.id), seconds)?;
    let mut worker = Worker {
        store,
        lease,
        seconds,
        job: def.job.id,
    };
    let result = drive(&mut worker, writer, port, &def, request.now_ms);
    let released = worker.store.release_lease(&worker.lease);
    let (reconciled, dispatched, stop, stop_code) = result?;
    released?;
    let (items, status) = state(worker.store, &def)?;
    let mut next_commands = Vec::new();
    for item in &items {
        if item.state == "needs_recovery"
            && let Some(op) = item.operation_id
        {
            next_commands.push(apply::reconcile_command_for(op));
        }
    }
    let flag = if def.job.mode == JobMode::Apply {
        " --apply"
    } else {
        ""
    };
    match stop {
        StopReason::Paused => next_commands.push(format!("lab jobs resume {}{flag}", def.job.id)),
        StopReason::Halted | StopReason::ItemErrorPolicy => {
            next_commands.push(format!("lab jobs items {}", def.job.id));
            next_commands.push(format!("lab jobs retry {} --failed{flag}", def.job.id));
        }
        _ => {}
    }
    let remaining = items
        .iter()
        .filter(|i| i.state == "pending" || i.retry_requested)
        .count();
    let exit_code = match stop {
        StopReason::RecoveryRequired => 7,
        StopReason::Halted => 5,
        _ if status.counts.contains_key("needs_recovery") => 7,
        _ if items
            .iter()
            .any(|i| matches!(i.state.as_str(), "failed" | "failed_before_write")) =>
        {
            4
        }
        _ => 0,
    };
    Ok(RunReport {
        job_id: def.job.id,
        mode: def.job.mode,
        reconciled,
        dispatched,
        stop,
        stop_code,
        status,
        items_remaining: remaining,
        writes_enabled: def.job.mode == JobMode::Apply,
        next_commands,
        exit_code,
    })
}

type Driven = (Vec<ItemReport>, Vec<ItemReport>, StopReason, Option<String>);

fn drive(
    worker: &mut Worker,
    writer: Option<&LeaseToken>,
    port: &mut dyn ApplyPort,
    def: &ApplyJobDefinition,
    now_ms: u64,
) -> Result<Driven> {
    let max = def.max_attempts();
    let mut reconciled = Vec::new();
    // 1. Account for every unfinished or unknown item before anything else.
    let (items, _) = state(worker.store, def)?;
    for item in items
        .iter()
        .filter(|i| i.state == "started" || i.state == "needs_recovery")
    {
        worker.renew()?;
        let report = account(worker, writer, port, item)?;
        reconciled.push(report);
    }
    if reconciled
        .iter()
        .any(|r| r.outcome == ItemOutcome::NeedsRecovery)
    {
        let code = reconciled
            .iter()
            .find(|r| r.outcome == ItemOutcome::NeedsRecovery)
            .and_then(|r| r.code.clone());
        worker.stop(StopReason::RecoveryRequired, None, code.clone())?;
        return Ok((reconciled, vec![], StopReason::RecoveryRequired, code));
    }
    // 2. Honour a durable pause/cancel only after accounting is complete.
    let events = worker.store.apply_job_events(def.job.id)?;
    if let Some((sequence, action)) = latest_control(&events)
        && action != JobControl::Resume
    {
        let reason = acknowledge(worker, &events, sequence, action)?;
        return Ok((reconciled, vec![], reason, None));
    }
    // 3. Dispatch eligible items in frozen order through the single writer.
    let (items, _) = state(worker.store, def)?;
    let eligible: Vec<ApplyJobItem> = items
        .into_iter()
        .filter(|i| {
            i.state == "pending"
                || (i.attempt < max
                    && (i.retry_requested && i.retry_eligible
                        || (i.state == "failed_before_write"
                            && i.error_code
                                .as_deref()
                                .is_some_and(|c| c.starts_with("JOB_ITEM_INTERRUPTED")))))
        })
        .collect();
    let mut dispatched = Vec::new();
    // `anki.commit_interval_seconds`: minimum gap between note mutations.
    let interval = std::time::Duration::from_secs_f64(
        def.job.settings.values["anki.commit_interval_seconds"]
            .as_f64()
            .unwrap_or(0.25),
    );
    let mut last_write: Option<std::time::Instant> = None;
    for item in eligible {
        if def.job.mode == JobMode::Apply
            && let Some(last) = last_write
        {
            std::thread::sleep(interval.saturating_sub(last.elapsed()));
        }
        worker.renew()?;
        let attempt = item.attempt + 1;
        let slot = Slot {
            operation: Uuid::new_v4(),
            main_step: Uuid::new_v4(),
            role: Role::Standalone,
        };
        let apply_mode = def.job.mode == JobMode::Apply;
        // The start records the stable operation IDs before any intent, and
        // re-checks the newest control inside the same transaction.
        match worker.append(ApplyJobStage::Started {
            item_id: item.item_id,
            attempt,
            operation_id: apply_mode.then_some(slot.operation),
            main_step: apply_mode.then_some(slot.main_step),
        }) {
            Ok(_) => {}
            Err(code) if code == "APPLY_JOB_CONTROL_BLOCKS_DISPATCH" => {
                let events = worker.store.apply_job_events(def.job.id)?;
                let (sequence, action) =
                    latest_control(&events).ok_or("APPLY_JOB_EVENT_CORRUPT")?;
                let reason = acknowledge(worker, &events, sequence, action)?;
                return Ok((reconciled, dispatched, reason, None));
            }
            Err(code) => return Err(code),
        }
        let request = request_for(def, item.item_id, now_ms);
        let (outcome, code, retry, simulated, receipt, next, operation) = if apply_mode {
            let result = apply::apply_item_with(
                worker.store,
                writer.ok_or("APPLY_WRITER_LEASE_REQUIRED")?,
                port,
                &request,
                &slot,
            );
            last_write = Some(std::time::Instant::now());
            let (outcome, code, retry) = from_apply(&result);
            let (receipt, next, op) = match &result {
                Ok(o) => (
                    o.receipt_digest.clone(),
                    o.next_command.clone(),
                    o.operation_id,
                ),
                Err(_) => (None, None, None),
            };
            (
                outcome,
                code,
                retry,
                None,
                receipt,
                next,
                op.or(Some(slot.operation)),
            )
        } else {
            match apply::simulate_item(worker.store, port, &request, def.checkpoint_id.is_some()) {
                Ok(sim) => (
                    ItemOutcome::Simulated,
                    None,
                    RetryClass::None,
                    Some(sim),
                    None,
                    None,
                    None,
                ),
                Err(message) => {
                    let code = code_of(&message);
                    let class = classify(&code);
                    (
                        ItemOutcome::Failed,
                        Some(code),
                        class,
                        None,
                        None,
                        None,
                        None,
                    )
                }
            }
        };
        // Operation IDs that never got an intent are not reported as operations.
        let operation = operation.filter(|op| worker.store.apply_operation(*op).is_ok());
        worker.append(ApplyJobStage::Finished {
            item_id: item.item_id,
            attempt,
            operation_id: operation,
            outcome,
            code: code.clone(),
            retry,
        })?;
        dispatched.push(ItemReport {
            item_id: item.item_id,
            attempt,
            operation_id: operation,
            outcome,
            code: code.clone(),
            retry,
            simulated,
            receipt_digest: receipt,
            next_command: next,
        });
        let stop = match retry {
            RetryClass::Reconcile => Some(StopReason::RecoveryRequired),
            RetryClass::Halt => Some(StopReason::Halted),
            _ if !matches!(outcome, ItemOutcome::Committed | ItemOutcome::Simulated)
                && def.job.settings.values["jobs.on_item_error"] == "stop" =>
            {
                Some(StopReason::ItemErrorPolicy)
            }
            _ => None,
        };
        if let Some(reason) = stop {
            worker.stop(reason, None, code.clone())?;
            return Ok((reconciled, dispatched, reason, code));
        }
    }
    worker.stop(StopReason::Idle, None, None)?;
    Ok((reconciled, dispatched, StopReason::Idle, None))
}

/// Confirm a pause/cancel request once nothing is in flight.
fn acknowledge(
    worker: &mut Worker,
    events: &[ApplyJobReceipt],
    sequence: u32,
    action: JobControl,
) -> Result<StopReason> {
    let reason = if action == JobControl::Cancel {
        StopReason::Cancelled
    } else {
        StopReason::Paused
    };
    let acked = events.iter().any(|r| {
        r.event.sequence > sequence && matches!(r.event.stage, ApplyJobStage::Stopped { .. })
    });
    if !acked {
        worker.stop(reason, Some(sequence), None)?;
    }
    Ok(reason)
}

/// Resolve one started or unknown item from durable evidence, never by
/// resetting it: a missing intent proves no effect; an intent is reconciled.
fn account(
    worker: &mut Worker,
    writer: Option<&LeaseToken>,
    port: &mut dyn ApplyPort,
    item: &ApplyJobItem,
) -> Result<ItemReport> {
    let mut report = ItemReport {
        item_id: item.item_id,
        attempt: item.attempt,
        operation_id: None,
        outcome: ItemOutcome::FailedBeforeWrite,
        code: Some("JOB_ITEM_INTERRUPTED_BEFORE_INTENT".into()),
        retry: RetryClass::Transient,
        simulated: None,
        receipt_digest: None,
        next_command: None,
    };
    let operation = item
        .operation_id
        .or_else(|| item.operations.last().copied());
    let journal = match operation {
        // Simulate attempts and apply attempts without an intent had no effect.
        None => None,
        Some(op) => match worker.store.apply_operation(op) {
            Err(code) if code == "APPLY_OPERATION_NOT_FOUND" => None,
            Err(code) => return Err(code),
            Ok(_) => match worker.store.journal(op) {
                // The intent precedes the journal; effects follow the journal.
                Err(code) if code == "JOURNAL_NOT_FOUND" => {
                    report.operation_id = Some(op);
                    report.code = Some("JOB_ITEM_INTERRUPTED_BEFORE_JOURNAL".into());
                    None
                }
                Err(code) => return Err(code),
                Ok(version) => Some((op, version)),
            },
        },
    };
    if let Some((op, version)) = journal {
        report.operation_id = Some(op);
        let state = if version.pending_recovery || !terminal(version.journal.state) {
            let writer = writer.ok_or("APPLY_WRITER_LEASE_REQUIRED")?;
            match apply::reconcile(
                worker.store,
                writer,
                port,
                &ReconcileRequest {
                    operation_id: op,
                    apply: true,
                    rebind: None,
                },
            ) {
                Ok(outcome) => {
                    report.receipt_digest = outcome.receipt_digest.clone();
                    report.next_command = outcome.next_command.clone();
                    if !matches!(
                        outcome.state,
                        OperationState::Committed | OperationState::FailedBeforeWrite
                    ) {
                        report.code = outcome
                            .issues
                            .iter()
                            .rev()
                            .map(|i| i.code.clone())
                            .find(|c| classify(c) == RetryClass::Reconcile)
                            .or(Some("APPLY_RECOVERY_REQUIRED".into()));
                    }
                    outcome.state
                }
                Err(message) => {
                    report.code = Some(code_of(&message));
                    report.next_command = Some(apply::reconcile_command_for(op));
                    OperationState::NeedsRecovery
                }
            }
        } else {
            version.journal.state
        };
        let (outcome, code, retry) = match state {
            OperationState::Committed => (ItemOutcome::Committed, None, RetryClass::None),
            OperationState::Restored | OperationState::Compensated => (
                ItemOutcome::Committed,
                Some("APPLY_OPERATION_RESTORED".into()),
                RetryClass::None,
            ),
            // Reconciliation proved the attempt had no effect.
            OperationState::FailedBeforeWrite => (
                ItemOutcome::FailedBeforeWrite,
                Some("APPLY_REJECTED_BEFORE_WRITE".into()),
                RetryClass::Transient,
            ),
            _ => (
                ItemOutcome::NeedsRecovery,
                report.code.clone(),
                RetryClass::Reconcile,
            ),
        };
        report.outcome = outcome;
        report.code = code;
        report.retry = retry;
    }
    let unchanged = item.state == "needs_recovery" && report.outcome == ItemOutcome::NeedsRecovery;
    if !unchanged {
        worker.append(ApplyJobStage::Finished {
            item_id: item.item_id,
            attempt: item.attempt.max(1),
            operation_id: report.operation_id,
            outcome: report.outcome,
            code: report.code.clone(),
            retry: report.retry,
        })?;
    }
    Ok(report)
}

fn terminal(state: OperationState) -> bool {
    matches!(
        state,
        OperationState::Committed
            | OperationState::FailedBeforeWrite
            | OperationState::Restored
            | OperationState::Compensated
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct RetryRefusal {
    pub item_id: Uuid,
    pub state: String,
    pub code: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RetryOutcome {
    pub job_id: Uuid,
    pub accepted: Vec<Uuid>,
    pub refused: Vec<RetryRefusal>,
    pub envelope: Option<ApplyJobReceipt>,
}

/// OP-42 envelope: classify the named (or every failed) item and record a
/// retry for eligible ones only. Running them is a separate `run` call.
pub fn request_retry(
    store: &mut Store,
    job: Uuid,
    item_ids: &[Uuid],
    failed: bool,
) -> Result<RetryOutcome> {
    let def = store.apply_job(job)?;
    if store.job_tombstone(job)?.is_some() {
        return Err("JOB_TOMBSTONED".into());
    }
    if item_ids.is_empty() != failed {
        return Err("JOB_RETRY_SELECTION_REQUIRED: name --item-id values or --failed".into());
    }
    let (items, _) = state(store, &def)?;
    let selected: Vec<&ApplyJobItem> = if failed {
        items
            .iter()
            .filter(|i| {
                matches!(
                    i.state.as_str(),
                    "failed" | "failed_before_write" | "needs_recovery"
                )
            })
            .collect()
    } else {
        let mut out = Vec::new();
        for id in item_ids {
            out.push(
                items
                    .iter()
                    .find(|i| i.item_id == *id)
                    .ok_or_else(|| format!("APPLY_JOB_ITEM_NOT_FOUND: {id}"))?,
            );
        }
        out
    };
    let max = def.max_attempts();
    let mut accepted = Vec::new();
    let mut refused = Vec::new();
    for item in selected {
        let code = match (item.state.as_str(), item.retry) {
            ("needs_recovery", _) | (_, Some(RetryClass::Reconcile)) => {
                Some("JOB_RETRY_RECONCILE_FIRST")
            }
            ("pending" | "started", _) => Some("JOB_RETRY_NOT_FAILED"),
            ("committed" | "simulated", _) => Some("JOB_RETRY_ALREADY_SUCCEEDED"),
            (_, Some(RetryClass::Permanent)) => Some("JOB_RETRY_REQUIRES_NEW_REVISION"),
            _ if item.attempt >= max => Some("JOB_RETRY_ATTEMPTS_EXHAUSTED"),
            _ if item.retry_requested => Some("JOB_RETRY_ALREADY_REQUESTED"),
            _ => None,
        };
        match code {
            Some(code) => refused.push(RetryRefusal {
                item_id: item.item_id,
                state: item.state.clone(),
                code: code.into(),
            }),
            None => accepted.push(item.item_id),
        }
    }
    let envelope = if accepted.is_empty() {
        None
    } else {
        Some(store.request_apply_job_retry(job, &accepted)?)
    };
    Ok(RetryOutcome {
        job_id: job,
        accepted,
        refused,
        envelope,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlOutcome {
    pub job_id: Uuid,
    pub control: ApplyJobReceipt,
    pub worker_stopped_confirmed: bool,
    pub acknowledgement: Option<ApplyJobReceipt>,
    pub status: ApplyJobStatus,
}

/// OP-40/OP-41/OP-43 request. Confirmation comes from a worker
/// acknowledgement, or from an idle job with no lease holder and nothing in flight.
pub fn request_control(store: &mut Store, job: Uuid, action: JobControl) -> Result<ControlOutcome> {
    let def = store.apply_job(job)?;
    let (control, ack) = store.request_apply_job_control(job, action)?;
    let (_, status) = state(store, &def)?;
    Ok(ControlOutcome {
        job_id: job,
        control,
        worker_stopped_confirmed: status.worker_stopped_confirmed,
        acknowledgement: ack,
        status,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct JobView {
    pub definition: ApplyJobDefinition,
    pub digest: String,
    pub status: ApplyJobStatus,
    pub worker: linguist_store::apply_job::WorkerStatus,
    pub recovery_required: Vec<ApplyJobItem>,
    pub tombstone: Option<JobTombstone>,
    pub next_commands: Vec<String>,
}

/// OP-37 for simulate/apply jobs. Local read only.
pub fn show(store: &Store, job: Uuid) -> Result<JobView> {
    let definition = store.apply_job(job)?;
    let digest = store.apply_job_digest(job)?;
    let worker = store.job_worker_status(job)?;
    let (items, status) = state(store, &definition)?;
    let recovery_required: Vec<ApplyJobItem> = items
        .into_iter()
        .filter(|i| matches!(i.state.as_str(), "needs_recovery" | "started"))
        .collect();
    let flag = if definition.job.mode == JobMode::Apply {
        " --apply"
    } else {
        ""
    };
    let mut next_commands: Vec<String> = recovery_required
        .iter()
        .filter_map(|i| i.operation_id.map(apply::reconcile_command_for))
        .collect();
    match status.state.as_str() {
        "paused" | "pause_requested" => next_commands.push(format!("lab jobs resume {job}{flag}")),
        "pending" | "interrupted" | "needs_recovery" => {
            next_commands.push(format!("lab jobs run {job}{flag}"))
        }
        "halted" | "finished_with_failures" => {
            next_commands.push(format!("lab jobs retry {job} --failed{flag}"))
        }
        _ => {}
    }
    Ok(JobView {
        tombstone: store.job_tombstone(job)?,
        definition,
        digest,
        status,
        worker,
        recovery_required,
        next_commands,
    })
}

/// OP-38 for simulate/apply jobs: a bounded page in frozen input order.
pub fn items(store: &Store, job: Uuid, after_index: u32, limit: u32) -> Result<Vec<ApplyJobItem>> {
    if !(1..=10000).contains(&limit) {
        return Err("INVALID_PAGE_LIMIT".into());
    }
    let definition = store.apply_job(job)?;
    let (items, _) = state(store, &definition)?;
    Ok(items
        .into_iter()
        .skip(after_index as usize)
        .take(limit as usize)
        .collect())
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditItem {
    pub item_id: Uuid,
    pub state: String,
    pub operation_id: Option<Uuid>,
    pub journal_state: Option<OperationState>,
    pub receipt_digest: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditReport {
    pub job_id: Uuid,
    pub scope: &'static str,
    pub definition_digest: String,
    pub events_verified: u32,
    pub head_digest: Option<String>,
    pub plan_verified: bool,
    pub approval_verified: bool,
    pub partial: bool,
    pub items: Vec<AuditItem>,
    pub issues: Vec<String>,
    pub native_verified: bool,
    pub worker_liveness: String,
}

/// OP-46 for simulate/apply jobs: verify the definition, the whole event
/// chain, the plan/approval binding and, per item, that the recorded
/// outcome agrees with its apply operation journal and receipt. A partially
/// applied batch is reported explicitly. Local only; nothing is repaired.
pub fn audit(store: &Store, job: Uuid) -> Result<AuditReport> {
    let def = store.apply_job(job)?;
    let digest = store.apply_job_digest(job)?;
    let events = store.apply_job_events(job)?;
    let worker = store.job_worker_status(job)?;
    let (items, status) = fold(&def, &events, &worker);
    let mut issues = Vec::new();
    let plan_verified = match store.revision(def.plan_id, def.revision) {
        Ok(plan) => plan.approval_digest().is_ok_and(|d| d == def.plan_digest),
        Err(_) => false,
    };
    if !plan_verified {
        issues.push("AUDIT_PLAN_DIGEST_MISMATCH".into());
    }
    let approval_verified = store.approval(def.approval_id).is_ok_and(|a| {
        a.approval.plan_id == def.plan_id
            && a.approval.revision == def.revision
            && a.approval.digest == def.plan_digest
            && def
                .job
                .item_ids
                .iter()
                .all(|i| a.approval.item_ids.contains(i))
    });
    if !approval_verified {
        issues.push("AUDIT_APPROVAL_SCOPE_MISMATCH".into());
    }
    // Every recorded operation must belong to this job's plan item and group.
    let mut audit_items = Vec::new();
    for item in &items {
        let mut item_issues = Vec::new();
        let mut journal_state = None;
        let mut receipt = None;
        for op in &item.operations {
            match store.apply_operation(*op) {
                Ok(record) => {
                    if record.plan_id != def.plan_id
                        || record.revision != def.revision
                        || record.item_id != item.item_id
                    {
                        item_issues.push("AUDIT_OPERATION_ITEM_MISMATCH".into());
                    }
                }
                Err(code) if code == "APPLY_OPERATION_NOT_FOUND" => continue,
                Err(code) => item_issues.push(code),
            }
            if let Ok(version) = store.journal(*op) {
                if version.journal.group_id != Some(job) {
                    item_issues.push("AUDIT_OPERATION_GROUP_MISMATCH".into());
                }
                if Some(*op) == item.operation_id {
                    journal_state = Some(version.journal.state);
                    receipt = store
                        .snapshot(version.journal.snapshot_id)
                        .ok()
                        .and_then(|snapshot| snapshot.after)
                        .and_then(|after| {
                            linguist_core::canonical::digest("lab-apply-receipt-v1", &after).ok()
                        });
                }
            }
        }
        let consistent = match (item.state.as_str(), journal_state) {
            ("committed", Some(s)) => terminal(s) && s != OperationState::FailedBeforeWrite,
            ("committed", None) => false,
            ("needs_recovery", Some(s)) => !terminal(s) || s == OperationState::NeedsRecovery,
            ("failed_before_write", Some(s)) => s == OperationState::FailedBeforeWrite,
            ("started", _) => true,
            (_, None) => true,
            _ => true,
        };
        if !consistent {
            item_issues.push("AUDIT_ITEM_STATE_DIVERGES_FROM_JOURNAL".into());
        }
        if item.state == "committed" && def.job.mode == JobMode::Apply && receipt.is_none() {
            item_issues.push("AUDIT_RECEIPT_MISSING".into());
        }
        issues.extend(item_issues.iter().cloned());
        audit_items.push(AuditItem {
            item_id: item.item_id,
            state: item.state.clone(),
            operation_id: item.operation_id,
            journal_state,
            receipt_digest: receipt,
            issues: item_issues,
        });
    }
    let succeeded = items
        .iter()
        .filter(|i| matches!(i.state.as_str(), "committed" | "simulated"))
        .count();
    let partial = succeeded > 0 && succeeded < items.len();
    Ok(AuditReport {
        job_id: job,
        scope: "local_full_history",
        definition_digest: digest,
        events_verified: status.event_count,
        head_digest: status.head_digest,
        plan_verified,
        approval_verified,
        partial,
        items: audit_items,
        issues,
        native_verified: false,
        worker_liveness: worker.owner,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct DeletePreview {
    pub job_id: Uuid,
    pub kind: String,
    pub executed: bool,
    pub tombstone: JobTombstone,
    pub tombstone_digest: Option<String>,
    pub retained: Vec<&'static str>,
    pub anki_deletion: bool,
}

/// OP-45 for every job kind: preview or record a local tombstone of a
/// terminal job. Active, interrupted or unresolved jobs are refused; all
/// referenced snapshots, journals, receipts, assets, events and plans remain.
pub fn delete(store: &mut Store, job: Uuid, execute: bool, now_ms: u64) -> Result<DeletePreview> {
    if store.job_tombstone(job)?.is_some() {
        return Err("JOB_ALREADY_TOMBSTONED".into());
    }
    if store.job_worker_status(job)?.lease == "active" {
        return Err("JOB_DELETE_WORKER_ACTIVE: a worker holds or last held the job lease without releasing it; run or recover the job first".into());
    }
    let tombstone = match store.apply_job(job) {
        Ok(def) => {
            let digest = store.apply_job_digest(job)?;
            let (items, status) = state(store, &def)?;
            let terminal_state = matches!(
                status.state.as_str(),
                "completed" | "cancelled" | "finished_with_failures" | "halted"
            );
            if !terminal_state {
                return Err(format!(
                    "JOB_DELETE_NOT_TERMINAL: job state is {}; finish, cancel or reconcile it first",
                    status.state
                ));
            }
            if items
                .iter()
                .any(|i| matches!(i.state.as_str(), "started" | "needs_recovery"))
            {
                return Err("JOB_DELETE_RECOVERY_REQUIRED".into());
            }
            JobTombstone {
                schema_version: 1,
                job_id: job,
                kind: match def.job.mode {
                    JobMode::Simulate => "simulate".into(),
                    _ => "apply".into(),
                },
                definition_digest: digest,
                final_state: status.state,
                summary: status.counts,
                retained_operations: items.iter().flat_map(|i| i.operations.clone()).collect(),
                retained_plan: Some((def.plan_id, def.revision)),
                created_ms: now_ms,
            }
        }
        Err(code) if code == "APPLY_JOB_NOT_FOUND" => {
            let def = store.preparation_job(job)?;
            let mut summary = BTreeMap::new();
            for offset in (0..def.job.item_ids.len()).step_by(1000) {
                for item in store.preparation_items(job, offset as u32, 1000)? {
                    if item.state == "started" {
                        return Err("JOB_DELETE_RECOVERY_REQUIRED: an interrupted read needs `jobs recover` first".into());
                    }
                    *summary.entry(item.state).or_insert(0u32) += 1;
                }
            }
            let control = store.preparation_control(job)?;
            let cancelled = control.as_ref().is_some_and(|c| {
                c.event.action == linguist_store::preparation_control::ControlAction::Cancel
            });
            let plan = match store.revision(job, 1) {
                Ok(_) => Some((job, 1)),
                Err(code) if code == "PLAN_NOT_FOUND" => None,
                Err(code) => return Err(code),
            };
            let all_captured = summary.keys().all(|k| k == "captured");
            let final_state = if cancelled {
                "cancelled"
            } else if plan.is_some() && all_captured {
                "completed"
            } else {
                return Err("JOB_DELETE_NOT_TERMINAL: a preparation job is terminal after its draft is published or it is cancelled".into());
            };
            JobTombstone {
                schema_version: 1,
                job_id: job,
                kind: "prepare".into(),
                definition_digest: linguist_core::canonical::digest("preparation-definition", &def)
                    .map_err(|e| e.to_string())?,
                final_state: final_state.into(),
                summary,
                retained_operations: vec![],
                retained_plan: plan,
                created_ms: now_ms,
            }
        }
        Err(code) => return Err(code),
    };
    let digest = if execute {
        Some(store.tombstone_job(&tombstone)?)
    } else {
        None
    };
    Ok(DeletePreview {
        job_id: job,
        kind: tombstone.kind.clone(),
        executed: execute,
        tombstone,
        tombstone_digest: digest,
        retained: vec![
            "definition",
            "events and controls",
            "plans and revisions",
            "snapshots",
            "journals",
            "receipts",
            "assets",
        ],
        anki_deletion: false,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct JobRollbackItem {
    pub item_id: Uuid,
    pub operation_id: Uuid,
    pub snapshot_id: Uuid,
    pub state: OperationState,
    pub restore: Option<crate::restore::LocalRestorePreview>,
    pub error: Option<String>,
}

/// OP-44 local plan for an apply job: one restore preview per receipt-backed
/// operation the job recorded (or the selected items), from stored records.
/// Unknown outcomes are listed with their reconcile command.
pub fn rollback_preview(
    store: &Store,
    job: Uuid,
    item_ids: &[Uuid],
) -> Result<Vec<JobRollbackItem>> {
    let def = store.apply_job(job)?;
    let (items, _) = state(store, &def)?;
    let mut out = Vec::new();
    for item in items {
        if !item_ids.is_empty() && !item_ids.contains(&item.item_id) {
            continue;
        }
        for op in &item.operations {
            let Ok(version) = store.journal(*op) else {
                continue;
            };
            let preview = crate::restore::local_preview(store, version.journal.snapshot_id);
            out.push(JobRollbackItem {
                item_id: item.item_id,
                operation_id: *op,
                snapshot_id: version.journal.snapshot_id,
                state: version.journal.state,
                error: preview.as_ref().err().cloned(),
                restore: preview.ok(),
            });
        }
    }
    if out.is_empty() {
        return Err("ROLLBACK_GROUP_EMPTY: the job recorded no apply operation".into());
    }
    Ok(out)
}
