//! Immutable simulate/apply job definitions, their hash-linked event history,
//! worker stop acknowledgements and local job tombstones. Storing any record
//! never calls Anki, dispatches an effect or authorizes a write; worker events
//! require the matching job lease inside the same SQLite transaction.
use crate::{Result, Store, sql};
use linguist_core::{
    canonical,
    records::{Job, JobMode},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS apply_jobs(
 id TEXT PRIMARY KEY, mode TEXT NOT NULL CHECK(mode IN('simulate','apply')),
 plan_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), item_count INTEGER NOT NULL CHECK(item_count>0),
 created_ms INTEGER NOT NULL CHECK(created_ms>=0), body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS apply_job_events(
 job_id TEXT NOT NULL REFERENCES apply_jobs(id), sequence INTEGER NOT NULL CHECK(sequence>0),
 kind TEXT NOT NULL, item_id TEXT, digest TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(job_id,sequence));
CREATE INDEX IF NOT EXISTS apply_job_events_kind ON apply_job_events(job_id,kind,sequence);
CREATE TABLE IF NOT EXISTS preparation_stop_acks(
 job_id TEXT NOT NULL REFERENCES preparation_jobs(id), control_sequence INTEGER NOT NULL CHECK(control_sequence>0),
 digest TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(job_id,control_sequence));
CREATE TABLE IF NOT EXISTS job_tombstones(
 job_id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN('prepare','simulate','apply')),
 digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE TRIGGER IF NOT EXISTS apply_jobs_no_update BEFORE UPDATE ON apply_jobs BEGIN SELECT RAISE(ABORT,'job definitions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS apply_jobs_no_delete BEFORE DELETE ON apply_jobs BEGIN SELECT RAISE(ABORT,'job deletion is tombstoning only'); END;
CREATE TRIGGER IF NOT EXISTS apply_job_events_no_update BEFORE UPDATE ON apply_job_events BEGIN SELECT RAISE(ABORT,'job events are immutable'); END;
CREATE TRIGGER IF NOT EXISTS apply_job_events_no_delete BEFORE DELETE ON apply_job_events BEGIN SELECT RAISE(ABORT,'job event retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS preparation_stop_acks_no_update BEFORE UPDATE ON preparation_stop_acks BEGIN SELECT RAISE(ABORT,'stop acknowledgements are immutable'); END;
CREATE TRIGGER IF NOT EXISTS preparation_stop_acks_no_delete BEFORE DELETE ON preparation_stop_acks BEGIN SELECT RAISE(ABORT,'stop acknowledgement retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS job_tombstones_no_update BEFORE UPDATE ON job_tombstones BEGIN SELECT RAISE(ABORT,'job tombstones are immutable'); END;
CREATE TRIGGER IF NOT EXISTS job_tombstones_no_delete BEFORE DELETE ON job_tombstones BEGIN SELECT RAISE(ABORT,'job tombstones are permanent'); END;
";

/// Upper bound on the items one simulate/apply job may freeze.
pub const MAX_APPLY_JOB_ITEMS: usize = 10_000;
/// Upper bound on one job's event history; the fold reads the whole chain.
pub const MAX_APPLY_JOB_EVENTS: u32 = 200_000;

/// Immutable simulate/apply job. `job.item_ids` are the approved plan item IDs
/// in frozen plan order; `job.settings` is the frozen effective configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyJobDefinition {
    pub schema_version: u16,
    pub job: Job,
    pub plan_id: Uuid,
    pub revision: u32,
    pub plan_digest: String,
    pub approval_id: Uuid,
    /// Required for apply mode; simulate checks it when present.
    pub checkpoint_id: Option<Uuid>,
    pub protected_manifest_digest: String,
    pub accept_schema_change: bool,
    pub created_ms: u64,
}

impl ApplyJobDefinition {
    pub fn validate(&self) -> Result<()> {
        let job = &self.job;
        let invalid = self.schema_version != 1
            || job.id.is_nil()
            || self.plan_id.is_nil()
            || self.approval_id.is_nil()
            || self.revision == 0
            || job.mode == JobMode::Prepare
            || job.item_ids.is_empty()
            || job.item_ids.len() > MAX_APPLY_JOB_ITEMS
            || job.item_ids.len() != job.plan_refs.len()
            || job.item_ids.iter().any(Uuid::is_nil)
            || job.pause_requested
            || job.cancel_requested
            || (job.mode == JobMode::Apply && self.checkpoint_id.is_none())
            || self.protected_manifest_digest.is_empty()
            || self.protected_manifest_digest.len() > 200
            || self.plan_digest.is_empty();
        let unique: std::collections::BTreeSet<_> = job.item_ids.iter().collect();
        if invalid || unique.len() != job.item_ids.len() {
            return Err("APPLY_JOB_DEFINITION_INVALID".into());
        }
        for (item, reference) in job.item_ids.iter().zip(&job.plan_refs) {
            if reference != &item_ref(self.plan_id, self.revision, *item) {
                return Err("APPLY_JOB_DEFINITION_INVALID".into());
            }
        }
        for key in [
            "jobs.max_item_attempts",
            "jobs.on_item_error",
            "jobs.lease_seconds",
            "jobs.heartbeat_seconds",
            "jobs.pause_poll_ms",
            "storage.state_dir",
        ] {
            if !job.settings.values.contains_key(key) {
                return Err("APPLY_JOB_SETTING_MISSING".into());
            }
        }
        Ok(())
    }
    pub fn max_attempts(&self) -> u16 {
        self.job.settings.values["jobs.max_item_attempts"]
            .as_u64()
            .unwrap_or(1)
            .clamp(1, 10) as u16
    }
}

pub fn item_ref(plan: Uuid, revision: u32, item: Uuid) -> String {
    format!("plan:{plan}:{revision}:item:{item}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobControl {
    Pause,
    Resume,
    Cancel,
}

/// Final result of one item attempt as the worker recorded it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemOutcome {
    /// Apply mode: a verified receipt exists.
    Committed,
    /// Simulate mode: every preflight check passed; nothing was written.
    Simulated,
    /// Proven to have had no collection effect.
    FailedBeforeWrite,
    /// A refusal or validation failure before any intent.
    Failed,
    /// Unknown or partial native outcome: reconcile, never retry.
    NeedsRecovery,
}

/// Retry classification. Only `transient` and `halt` are retry-eligible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    /// Success; nothing to retry.
    None,
    /// Dependency/transport failure; eligible within the attempt budget.
    Transient,
    /// A shared identity/schema/model/durability fault halted the group. The
    /// item itself may be retried once the shared fault is resolved.
    Halt,
    /// Item content or approval failed validation; needs a new plan revision.
    Permanent,
    /// Unknown mutation outcome; reconcile first.
    Reconcile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Paused,
    Cancelled,
    /// A shared fault stopped the group.
    Halted,
    /// `jobs.on_item_error=stop` after an item failure.
    ItemErrorPolicy,
    /// Unknown outcomes remain after reconciliation.
    RecoveryRequired,
    /// No eligible item remains.
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyJobStage {
    /// Durable dispatch record. Apply attempts carry the stable operation and
    /// main-step IDs allocated before any intent.
    Started {
        item_id: Uuid,
        attempt: u16,
        operation_id: Option<Uuid>,
        main_step: Option<Uuid>,
    },
    Finished {
        item_id: Uuid,
        attempt: u16,
        operation_id: Option<Uuid>,
        outcome: ItemOutcome,
        code: Option<String>,
        retry: RetryClass,
    },
    Control {
        action: JobControl,
    },
    /// Explicit retry envelope: the named items may start one more attempt.
    Retry {
        item_ids: Vec<Uuid>,
    },
    /// Worker stop acknowledgement. `control_sequence` names the control
    /// event it confirms; `idle` means no worker held the lease when the
    /// request was recorded.
    Stopped {
        reason: StopReason,
        control_sequence: Option<u32>,
        idle: bool,
        code: Option<String>,
    },
}

impl ApplyJobStage {
    fn kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::Finished { .. } => "finished",
            Self::Control { .. } => "control",
            Self::Retry { .. } => "retry",
            Self::Stopped { .. } => "stopped",
        }
    }
    fn item(&self) -> Option<Uuid> {
        match self {
            Self::Started { item_id, .. } | Self::Finished { item_id, .. } => Some(*item_id),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyJobEvent {
    pub schema_version: u16,
    pub job_id: Uuid,
    pub sequence: u32,
    pub parent_digest: Option<String>,
    /// Lease generation of the worker that wrote it; requests carry none.
    pub lease_generation: Option<i64>,
    pub stage: ApplyJobStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApplyJobReceipt {
    pub digest: String,
    pub event: ApplyJobEvent,
}

/// Latest durable state of one item, in frozen input order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApplyJobItem {
    pub index: u32,
    pub item_id: Uuid,
    pub input_ref: String,
    /// pending, started, committed, simulated, failed_before_write, failed, needs_recovery
    pub state: String,
    pub attempt: u16,
    pub operation_id: Option<Uuid>,
    pub operations: Vec<Uuid>,
    pub error_code: Option<String>,
    pub retry: Option<RetryClass>,
    /// A retry envelope after the latest finish authorizes one more attempt.
    pub retry_requested: bool,
    /// Classification metadata only: an eligible class within the budget.
    pub retry_eligible: bool,
    pub event_sequence: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    /// none, released or active
    pub lease: String,
    pub expired: bool,
    /// alive, absent, unknown or not_applicable
    pub owner: String,
    pub generation: Option<i64>,
}

impl WorkerStatus {
    pub fn running(&self) -> bool {
        self.lease == "active" && self.owner != "absent"
    }
}

/// Derived job summary. Requested controls are separate from confirmed stops.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApplyJobStatus {
    pub state: String,
    pub control: Option<JobControl>,
    pub control_sequence: Option<u32>,
    pub worker_stopped_confirmed: bool,
    pub last_stop: Option<StopReason>,
    pub counts: BTreeMap<String, u32>,
    pub event_count: u32,
    pub head_digest: Option<String>,
}

/// Fold a verified event chain into item summaries and the job status.
pub fn fold(
    definition: &ApplyJobDefinition,
    events: &[ApplyJobReceipt],
    worker: &WorkerStatus,
) -> (Vec<ApplyJobItem>, ApplyJobStatus) {
    let max = definition.max_attempts();
    let mut items: Vec<ApplyJobItem> = definition
        .job
        .item_ids
        .iter()
        .zip(&definition.job.plan_refs)
        .enumerate()
        .map(|(index, (item, reference))| ApplyJobItem {
            index: index as u32,
            item_id: *item,
            input_ref: reference.clone(),
            state: "pending".into(),
            attempt: 0,
            operation_id: None,
            operations: vec![],
            error_code: None,
            retry: None,
            retry_requested: false,
            retry_eligible: false,
            event_sequence: None,
        })
        .collect();
    let position: BTreeMap<Uuid, usize> = definition
        .job
        .item_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    let mut control = None;
    let mut control_sequence = None;
    let mut acked = None;
    let mut last_stop = None;
    for receipt in events {
        let sequence = receipt.event.sequence;
        match &receipt.event.stage {
            ApplyJobStage::Started {
                item_id,
                attempt,
                operation_id,
                ..
            } => {
                let item = &mut items[position[item_id]];
                item.state = "started".into();
                item.attempt = *attempt;
                item.operation_id = *operation_id;
                if let Some(op) = operation_id {
                    item.operations.push(*op);
                }
                item.error_code = None;
                item.retry = None;
                item.retry_requested = false;
                item.event_sequence = Some(sequence);
            }
            ApplyJobStage::Finished {
                item_id,
                outcome,
                code,
                retry,
                operation_id,
                ..
            } => {
                let item = &mut items[position[item_id]];
                item.state = serde_json::to_value(outcome)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                item.operation_id = *operation_id;
                item.error_code = code.clone();
                item.retry = Some(*retry);
                item.retry_requested = false;
                item.event_sequence = Some(sequence);
            }
            ApplyJobStage::Retry { item_ids } => {
                for id in item_ids {
                    if let Some(i) = position.get(id) {
                        items[*i].retry_requested = true;
                    }
                }
            }
            ApplyJobStage::Control { action } => {
                control = Some(*action);
                control_sequence = Some(sequence);
            }
            ApplyJobStage::Stopped {
                reason,
                control_sequence: confirms,
                ..
            } => {
                last_stop = Some(*reason);
                if confirms.is_some() {
                    acked = *confirms;
                }
            }
        }
    }
    for item in &mut items {
        item.retry_eligible = matches!(item.retry, Some(RetryClass::Transient | RetryClass::Halt))
            && item.attempt < max;
    }
    let mut counts = BTreeMap::new();
    for item in &items {
        *counts.entry(item.state.clone()).or_insert(0) += 1;
    }
    let stopping = matches!(control, Some(JobControl::Pause | JobControl::Cancel));
    let confirmed = stopping && acked.is_some() && acked == control_sequence;
    let in_flight = counts.contains_key("started");
    let state = match control {
        Some(JobControl::Cancel) if confirmed => "cancelled",
        Some(JobControl::Cancel) => "cancel_requested",
        Some(JobControl::Pause) if confirmed => "paused",
        Some(JobControl::Pause) => "pause_requested",
        _ if in_flight && worker.running() => "running",
        _ if in_flight => "interrupted",
        _ if counts.contains_key("needs_recovery") => "needs_recovery",
        _ if worker.running() => "running",
        _ if last_stop == Some(StopReason::Halted) => "halted",
        _ if counts.contains_key("pending") => "pending",
        _ if items
            .iter()
            .all(|i| matches!(i.state.as_str(), "committed" | "simulated")) =>
        {
            "completed"
        }
        _ => "finished_with_failures",
    };
    let status = ApplyJobStatus {
        state: state.into(),
        control,
        control_sequence,
        worker_stopped_confirmed: confirmed,
        last_stop,
        counts,
        event_count: events.len() as u32,
        head_digest: events.last().map(|r| r.digest.clone()),
    };
    (items, status)
}

/// Durable stop acknowledgement for a preparation job's control request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationStopAck {
    pub schema_version: u16,
    pub job_id: Uuid,
    pub control_sequence: u32,
    pub control_digest: String,
    pub action: crate::preparation_control::ControlAction,
    pub lease_generation: Option<i64>,
    pub idle: bool,
}

/// Local metadata tombstone. Referenced snapshots, journals, receipts, assets
/// and events remain; nothing in Anki is deleted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTombstone {
    pub schema_version: u16,
    pub job_id: Uuid,
    pub kind: String,
    pub definition_digest: String,
    pub final_state: String,
    pub summary: BTreeMap<String, u32>,
    pub retained_operations: Vec<Uuid>,
    pub retained_plan: Option<(Uuid, u32)>,
    pub created_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobListing {
    pub id: Uuid,
    pub kind: String,
    pub digest: String,
    pub item_count: u32,
    pub event_count: u32,
    pub tombstoned: bool,
}

fn generation(token: &crate::lease::LeaseToken) -> i64 {
    token.generation()
}

pub(crate) fn tombstoned(connection: &rusqlite::Connection, job: Uuid) -> Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM job_tombstones WHERE job_id=?1",
            [job.to_string()],
            |_| Ok(()),
        )
        .optional()
        .map(|r| r.is_some())
        .map_err(sql)
}

fn decode_event(job: Uuid, sequence: u32, digest: &str, body: &[u8]) -> Result<ApplyJobEvent> {
    let event: ApplyJobEvent = canonical::parse(body).map_err(|_| "APPLY_JOB_EVENT_CORRUPT")?;
    if event.schema_version != 1
        || event.job_id != job
        || event.sequence != sequence
        || canonical::digest("apply-job-event", &event).map_err(|e| e.to_string())? != digest
    {
        return Err("APPLY_JOB_EVENT_CORRUPT".into());
    }
    Ok(event)
}

fn head(tx: &rusqlite::Connection, job: Uuid) -> Result<Option<(u32, String)>> {
    tx.query_row(
        "SELECT sequence,digest FROM apply_job_events WHERE job_id=?1 ORDER BY sequence DESC LIMIT 1",
        [job.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(sql)
}

fn latest_control(tx: &rusqlite::Connection, job: Uuid) -> Result<Option<(u32, JobControl)>> {
    let row: Option<(u32, String, Vec<u8>)> = tx
        .query_row(
            "SELECT sequence,digest,body FROM apply_job_events WHERE job_id=?1 AND kind='control' ORDER BY sequence DESC LIMIT 1",
            [job.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    row.map(
        |(sequence, digest, body)| match decode_event(job, sequence, &digest, &body)?.stage {
            ApplyJobStage::Control { action } => Ok((sequence, action)),
            _ => Err("APPLY_JOB_EVENT_CORRUPT".into()),
        },
    )
    .transpose()
}

fn insert(
    tx: &rusqlite::Connection,
    job: Uuid,
    stage: ApplyJobStage,
    lease_generation: Option<i64>,
) -> Result<ApplyJobReceipt> {
    let previous = head(tx, job)?;
    let sequence = previous
        .as_ref()
        .map_or(Some(1), |h| h.0.checked_add(1))
        .filter(|s| *s <= MAX_APPLY_JOB_EVENTS)
        .ok_or("APPLY_JOB_EVENT_LIMIT")?;
    let event = ApplyJobEvent {
        schema_version: 1,
        job_id: job,
        sequence,
        parent_digest: previous.map(|h| h.1),
        lease_generation,
        stage,
    };
    let digest = canonical::digest("apply-job-event", &event).map_err(|e| e.to_string())?;
    let body = canonical::bytes(&event).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO apply_job_events(job_id,sequence,kind,item_id,digest,body) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            job.to_string(),
            sequence,
            event.stage.kind(),
            event.stage.item().map(|i| i.to_string()),
            digest,
            body
        ],
    )
    .map_err(sql)?;
    Ok(ApplyJobReceipt { digest, event })
}

fn code_valid(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 100
        && code
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

/// True when any item's newest item event is an unfinished start.
fn in_flight(tx: &rusqlite::Connection, job: Uuid) -> Result<bool> {
    let mut statement = tx
        .prepare("SELECT kind FROM apply_job_events e WHERE job_id=?1 AND item_id IS NOT NULL AND sequence=(SELECT MAX(sequence) FROM apply_job_events WHERE job_id=e.job_id AND item_id=e.item_id)")
        .map_err(sql)?;
    let kinds = statement
        .query_map([job.to_string()], |r| r.get::<_, String>(0))
        .map_err(sql)?;
    for kind in kinds {
        if kind.map_err(sql)? == "started" {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Store {
    /// Create-new publication of a validated simulate/apply job.
    pub fn create_apply_job(&mut self, definition: &ApplyJobDefinition) -> Result<String> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        definition.validate()?;
        // The approval must exist and cover every frozen item of this revision.
        let approval = self.approval(definition.approval_id)?.approval;
        if approval.plan_id != definition.plan_id
            || approval.revision != definition.revision
            || definition
                .job
                .item_ids
                .iter()
                .any(|i| !approval.item_ids.contains(i))
        {
            return Err("APPLY_JOB_APPROVAL_CONFLICT".into());
        }
        let body = canonical::bytes(definition).map_err(|e| e.to_string())?;
        let digest =
            canonical::digest("apply-job-definition", definition).map_err(|e| e.to_string())?;
        let mode = match definition.job.mode {
            JobMode::Simulate => "simulate",
            JobMode::Apply => "apply",
            JobMode::Prepare => return Err("APPLY_JOB_DEFINITION_INVALID".into()),
        };
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let clash: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM preparation_jobs WHERE id=?1)",
                [definition.job.id.to_string()],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if clash {
            return Err("APPLY_JOB_CONFLICT".into());
        }
        tx.execute(
            "INSERT INTO apply_jobs(id,mode,plan_id,revision,item_count,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                definition.job.id.to_string(),
                mode,
                definition.plan_id.to_string(),
                definition.revision,
                definition.job.item_ids.len() as u32,
                i64::try_from(definition.created_ms).map_err(|_| "APPLY_JOB_DEFINITION_INVALID")?,
                digest,
                body
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                "APPLY_JOB_CONFLICT".to_owned()
            }
            e => sql(e),
        })?;
        tx.commit().map_err(sql)?;
        Ok(digest)
    }

    pub fn apply_job(&self, id: Uuid) -> Result<ApplyJobDefinition> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM apply_jobs WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("APPLY_JOB_NOT_FOUND")?;
        let definition: ApplyJobDefinition =
            canonical::parse(&body).map_err(|_| "APPLY_JOB_CORRUPT")?;
        if definition.job.id != id
            || canonical::digest("apply-job-definition", &definition).map_err(|e| e.to_string())?
                != digest
            || definition.validate().is_err()
        {
            return Err("APPLY_JOB_CORRUPT".into());
        }
        Ok(definition)
    }

    pub fn apply_job_digest(&self, id: Uuid) -> Result<String> {
        self.apply_job(id)?;
        self.connection
            .query_row(
                "SELECT body_digest FROM apply_jobs WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(sql)
    }

    /// Every event of one job, with digests, sequences and parent links verified.
    pub fn apply_job_events(&self, id: Uuid) -> Result<Vec<ApplyJobReceipt>> {
        let definition = self.apply_job(id)?;
        let mut statement = self
            .connection
            .prepare("SELECT sequence,kind,item_id,digest,body FROM apply_job_events WHERE job_id=?1 ORDER BY sequence")
            .map_err(sql)?;
        let rows = statement
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })
            .map_err(sql)?;
        let items: std::collections::BTreeSet<Uuid> =
            definition.job.item_ids.iter().copied().collect();
        let mut out: Vec<ApplyJobReceipt> = Vec::new();
        for row in rows {
            let (sequence, kind, item, digest, body) = row.map_err(sql)?;
            let event = decode_event(id, sequence, &digest, &body)?;
            let expected_parent = out.last().map(|r| r.digest.clone());
            if sequence as usize != out.len() + 1
                || event.parent_digest != expected_parent
                || event.stage.kind() != kind
                || event.stage.item().map(|i| i.to_string()) != item
                || event.stage.item().is_some_and(|i| !items.contains(&i))
            {
                return Err("APPLY_JOB_EVENT_CORRUPT".into());
            }
            out.push(ApplyJobReceipt { digest, event });
        }
        Ok(out)
    }

    /// Worker transition. The job lease is validated and, for a start, the
    /// newest control request is checked inside the same transaction.
    pub fn append_apply_job_event(
        &mut self,
        job: Uuid,
        stage: ApplyJobStage,
        worker: &crate::lease::LeaseToken,
    ) -> Result<ApplyJobReceipt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let definition = self.apply_job(job)?;
        match &stage {
            ApplyJobStage::Started {
                item_id, attempt, ..
            } => {
                if !definition.job.item_ids.contains(item_id)
                    || *attempt == 0
                    || *attempt > definition.max_attempts()
                {
                    return Err("APPLY_JOB_EVENT_INVALID".into());
                }
            }
            ApplyJobStage::Finished {
                item_id,
                attempt,
                code,
                ..
            } => {
                if !definition.job.item_ids.contains(item_id)
                    || *attempt == 0
                    || code.as_deref().is_some_and(|c| !code_valid(c))
                {
                    return Err("APPLY_JOB_EVENT_INVALID".into());
                }
            }
            ApplyJobStage::Stopped { code, .. } => {
                if code.as_deref().is_some_and(|c| !code_valid(c)) {
                    return Err("APPLY_JOB_EVENT_INVALID".into());
                }
            }
            ApplyJobStage::Control { .. } | ApplyJobStage::Retry { .. } => {
                return Err("APPLY_JOB_EVENT_INVALID".into());
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if tombstoned(&tx, job)? {
            return Err("JOB_TOMBSTONED".into());
        }
        crate::lease::validate_job_worker_token(&tx, worker, job)?;
        if let ApplyJobStage::Started { item_id, .. } = &stage {
            if latest_control(&tx, job)?.is_some_and(|(_, action)| action != JobControl::Resume) {
                return Err("APPLY_JOB_CONTROL_BLOCKS_DISPATCH".into());
            }
            // One unfinished start per item: a second start would lose the first outcome.
            let last: Option<String> = tx
                .query_row(
                    "SELECT kind FROM apply_job_events WHERE job_id=?1 AND item_id=?2 ORDER BY sequence DESC LIMIT 1",
                    params![job.to_string(), item_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if last.as_deref() == Some("started") {
                return Err("APPLY_JOB_ITEM_IN_FLIGHT".into());
            }
        }
        if let ApplyJobStage::Finished {
            item_id, attempt, ..
        } = &stage
        {
            let last: Option<(String, Vec<u8>, u32, String)> = tx
                .query_row(
                    "SELECT kind,body,sequence,digest FROM apply_job_events WHERE job_id=?1 AND item_id=?2 ORDER BY sequence DESC LIMIT 1",
                    params![job.to_string(), item_id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(sql)?;
            // A finish follows its own start, or replaces an unresolved
            // outcome of the same attempt after reconciliation.
            let matches = last.is_some_and(|(_, body, sequence, digest)| {
                decode_event(job, sequence, &digest, &body).is_ok_and(|e| match e.stage {
                    ApplyJobStage::Started { attempt: a, .. } => a == *attempt,
                    ApplyJobStage::Finished {
                        attempt: a,
                        outcome: ItemOutcome::NeedsRecovery,
                        ..
                    } => a == *attempt,
                    _ => false,
                })
            });
            if !matches {
                return Err("APPLY_JOB_FINISH_WITHOUT_START".into());
            }
        }
        let receipt = insert(&tx, job, stage, Some(generation(worker)))?;
        tx.commit().map_err(sql)?;
        Ok(receipt)
    }

    /// Append (or reuse) a pause/resume/cancel request. When no worker holds
    /// the job lease and no item is in flight, the stop is confirmed in the
    /// same transaction as an idle acknowledgement.
    pub fn request_apply_job_control(
        &mut self,
        job: Uuid,
        action: JobControl,
    ) -> Result<(ApplyJobReceipt, Option<ApplyJobReceipt>)> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        self.apply_job(job)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if tombstoned(&tx, job)? {
            return Err("JOB_TOMBSTONED".into());
        }
        let previous = latest_control(&tx, job)?;
        let control = match previous {
            Some((sequence, current)) if current == action => {
                let (digest, body): (String, Vec<u8>) = tx
                    .query_row(
                        "SELECT digest,body FROM apply_job_events WHERE job_id=?1 AND sequence=?2",
                        params![job.to_string(), sequence],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(sql)?;
                ApplyJobReceipt {
                    event: decode_event(job, sequence, &digest, &body)?,
                    digest,
                }
            }
            Some((_, JobControl::Cancel)) => return Err("APPLY_JOB_CANCEL_IS_TERMINAL".into()),
            _ => insert(&tx, job, ApplyJobStage::Control { action }, None)?,
        };
        let mut ack = None;
        if action != JobControl::Resume {
            let worker = worker_status(&tx, job)?;
            let acked: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM apply_job_events WHERE job_id=?1 AND kind='stopped' AND sequence>?2)",
                    params![job.to_string(), control.event.sequence],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if !acked && worker.lease != "active" && !in_flight(&tx, job)? {
                ack = Some(insert(
                    &tx,
                    job,
                    ApplyJobStage::Stopped {
                        reason: if action == JobControl::Cancel {
                            StopReason::Cancelled
                        } else {
                            StopReason::Paused
                        },
                        control_sequence: Some(control.event.sequence),
                        idle: true,
                        code: None,
                    },
                    None,
                )?);
            }
        }
        tx.commit().map_err(sql)?;
        Ok((control, ack))
    }

    /// Record an explicit retry envelope. Eligibility is decided by the caller
    /// from the folded state; the store refuses cancelled jobs and unknown items.
    pub fn request_apply_job_retry(
        &mut self,
        job: Uuid,
        item_ids: &[Uuid],
    ) -> Result<ApplyJobReceipt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let definition = self.apply_job(job)?;
        if item_ids.is_empty()
            || item_ids.len() > MAX_APPLY_JOB_ITEMS
            || item_ids
                .iter()
                .any(|i| !definition.job.item_ids.contains(i))
        {
            return Err("APPLY_JOB_RETRY_INVALID".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if tombstoned(&tx, job)? {
            return Err("JOB_TOMBSTONED".into());
        }
        if latest_control(&tx, job)?.is_some_and(|(_, a)| a == JobControl::Cancel) {
            return Err("APPLY_JOB_CANCEL_IS_TERMINAL".into());
        }
        let receipt = insert(
            &tx,
            job,
            ApplyJobStage::Retry {
                item_ids: item_ids.to_vec(),
            },
            None,
        )?;
        tx.commit().map_err(sql)?;
        Ok(receipt)
    }

    pub fn job_worker_status(&self, job: Uuid) -> Result<WorkerStatus> {
        worker_status(&self.connection, job)
    }

    /// Record that a preparation worker honoured the current pause/cancel
    /// request. Requires the worker's lease; reuses an existing acknowledgement.
    pub fn acknowledge_preparation_stop(
        &mut self,
        job: Uuid,
        worker: &crate::lease::LeaseToken,
    ) -> Result<Option<PreparationStopAck>> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        crate::lease::validate_job_worker_token(&tx, worker, job)?;
        let ack = preparation_ack(&tx, job, Some(generation(worker)), false)?;
        tx.commit().map_err(sql)?;
        Ok(ack)
    }

    /// Confirm an idle preparation stop: no worker holds the lease and no item
    /// has an unfinished start.
    pub fn acknowledge_idle_preparation_stop(
        &mut self,
        job: Uuid,
    ) -> Result<Option<PreparationStopAck>> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let items = self.preparation_job(job)?.job.item_ids.len();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if worker_status(&tx, job)?.lease == "active" {
            return Ok(None);
        }
        let started: u32 = tx
            .query_row(
                "SELECT COUNT(*) FROM preparation_events e WHERE job_id=?1 AND sequence=(SELECT MAX(sequence) FROM preparation_events WHERE job_id=e.job_id AND item_id=e.item_id) AND body LIKE '%\"state\":\"started\"%'",
                [job.to_string()],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if started > 0 || items == 0 {
            return Ok(None);
        }
        let ack = preparation_ack(&tx, job, None, true)?;
        tx.commit().map_err(sql)?;
        Ok(ack)
    }

    /// The acknowledgement of the newest control request, if any.
    pub fn preparation_stop_ack(&self, job: Uuid) -> Result<Option<PreparationStopAck>> {
        let Some(control) = self.preparation_control(job)? else {
            return Ok(None);
        };
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT digest,body FROM preparation_stop_acks WHERE job_id=?1 AND control_sequence=?2",
                params![job.to_string(), control.event.sequence],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        row.map(|(digest, body)| {
            let ack: PreparationStopAck =
                canonical::parse(&body).map_err(|_| "PREPARATION_STOP_ACK_CORRUPT")?;
            if canonical::digest("preparation-stop-ack", &ack).map_err(|e| e.to_string())? != digest
                || ack.job_id != job
                || ack.control_digest != control.digest
            {
                return Err("PREPARATION_STOP_ACK_CORRUPT".into());
            }
            Ok(ack)
        })
        .transpose()
    }

    /// Insert a tombstone after re-checking, inside the transaction, that no
    /// worker holds the job lease. Callers check terminal state beforehand.
    pub fn tombstone_job(&mut self, tombstone: &JobTombstone) -> Result<String> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let kind = match self.apply_job(tombstone.job_id) {
            Ok(definition) => match definition.job.mode {
                JobMode::Simulate => "simulate",
                _ => "apply",
            },
            Err(code) if code == "APPLY_JOB_NOT_FOUND" => {
                self.preparation_job(tombstone.job_id)?;
                "prepare"
            }
            Err(code) => return Err(code),
        };
        if tombstone.kind != kind || tombstone.schema_version != 1 {
            return Err("JOB_TOMBSTONE_INVALID".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if worker_status(&tx, tombstone.job_id)?.lease == "active" {
            return Err("JOB_DELETE_WORKER_ACTIVE".into());
        }
        let body = canonical::bytes(tombstone).map_err(|e| e.to_string())?;
        let digest = canonical::digest("job-tombstone", tombstone).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO job_tombstones(job_id,kind,digest,body) VALUES(?1,?2,?3,?4)",
            params![tombstone.job_id.to_string(), kind, digest, body],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                "JOB_ALREADY_TOMBSTONED".to_owned()
            }
            e => sql(e),
        })?;
        tx.commit().map_err(sql)?;
        Ok(digest)
    }

    pub fn job_tombstone(&self, job: Uuid) -> Result<Option<JobTombstone>> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT digest,body FROM job_tombstones WHERE job_id=?1",
                [job.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        row.map(|(digest, body)| {
            let tombstone: JobTombstone =
                canonical::parse(&body).map_err(|_| "JOB_TOMBSTONE_CORRUPT")?;
            if canonical::digest("job-tombstone", &tombstone).map_err(|e| e.to_string())? != digest
                || tombstone.job_id != job
            {
                return Err("JOB_TOMBSTONE_CORRUPT".into());
            }
            Ok(tombstone)
        })
        .transpose()
    }

    /// Every job kind in ID order. Tombstoned jobs are hidden unless requested.
    pub fn list_jobs(
        &self,
        after: Option<Uuid>,
        limit: u32,
        mode: Option<JobMode>,
        include_deleted: bool,
    ) -> Result<Vec<JobListing>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mode = mode.map(|m| match m {
            JobMode::Prepare => "prepare",
            JobMode::Simulate => "simulate",
            JobMode::Apply => "apply",
        });
        let mut statement = self
            .connection
            .prepare(
                "SELECT id,kind,digest,item_count,events,deleted FROM (
                   SELECT j.id AS id,'prepare' AS kind,j.digest AS digest,j.item_count AS item_count,
                     (SELECT COUNT(*) FROM preparation_events e WHERE e.job_id=j.id) AS events,
                     EXISTS(SELECT 1 FROM job_tombstones t WHERE t.job_id=j.id) AS deleted FROM preparation_jobs j
                   UNION ALL
                   SELECT a.id,a.mode,a.body_digest,a.item_count,
                     (SELECT COUNT(*) FROM apply_job_events e WHERE e.job_id=a.id),
                     EXISTS(SELECT 1 FROM job_tombstones t WHERE t.job_id=a.id) FROM apply_jobs a)
                 WHERE id>?1 AND (?2 IS NULL OR kind=?2) AND (?3 OR NOT deleted) ORDER BY id LIMIT ?4",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(
                params![
                    after.map(|id| id.to_string()).unwrap_or_default(),
                    mode,
                    include_deleted,
                    limit
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, u32>(3)?,
                        r.get::<_, u32>(4)?,
                        r.get::<_, bool>(5)?,
                    ))
                },
            )
            .map_err(sql)?;
        rows.map(|row| {
            let (id, kind, digest, item_count, event_count, tombstoned) = row.map_err(sql)?;
            Ok(JobListing {
                id: Uuid::parse_str(&id).map_err(|_| "JOB_LISTING_CORRUPT")?,
                kind,
                digest,
                item_count,
                event_count,
                tombstoned,
            })
        })
        .collect()
    }
}

fn preparation_ack(
    tx: &rusqlite::Connection,
    job: Uuid,
    lease_generation: Option<i64>,
    idle: bool,
) -> Result<Option<PreparationStopAck>> {
    use crate::preparation_control::ControlAction;
    let control: Option<(u32, String)> = tx
        .query_row(
            "SELECT sequence,digest FROM preparation_controls WHERE job_id=?1 ORDER BY sequence DESC LIMIT 1",
            [job.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((sequence, control_digest)) = control else {
        return Ok(None);
    };
    let body: Vec<u8> = tx
        .query_row(
            "SELECT body FROM preparation_controls WHERE job_id=?1 AND sequence=?2",
            params![job.to_string(), sequence],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let event: crate::preparation_control::ControlEvent =
        canonical::parse(&body).map_err(|_| "PREPARATION_CONTROL_CORRUPT")?;
    if event.action == ControlAction::Resume {
        return Ok(None);
    }
    let existing: Option<Vec<u8>> = tx
        .query_row(
            "SELECT body FROM preparation_stop_acks WHERE job_id=?1 AND control_sequence=?2",
            params![job.to_string(), sequence],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if let Some(body) = existing {
        return canonical::parse(&body)
            .map(Some)
            .map_err(|_| "PREPARATION_STOP_ACK_CORRUPT".into());
    }
    let ack = PreparationStopAck {
        schema_version: 1,
        job_id: job,
        control_sequence: sequence,
        control_digest,
        action: event.action,
        lease_generation,
        idle,
    };
    tx.execute(
        "INSERT INTO preparation_stop_acks(job_id,control_sequence,digest,body) VALUES(?1,?2,?3,?4)",
        params![
            job.to_string(),
            sequence,
            canonical::digest("preparation-stop-ack", &ack).map_err(|e| e.to_string())?,
            canonical::bytes(&ack).map_err(|e| e.to_string())?
        ],
    )
    .map_err(sql)?;
    Ok(Some(ack))
}

fn worker_status(connection: &rusqlite::Connection, job: Uuid) -> Result<WorkerStatus> {
    crate::lease::status(connection, &crate::lease::Resource::JobWorker(job))
}
