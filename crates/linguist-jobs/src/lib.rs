//! Native, durable batch-job storage.
//!
//! This repository only creates and opens its own schema. Legacy Python
//! databases are deliberately rejected untouched; N15 owns audited migration.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use linguist_core::{
    BatchArtifactReference, BatchItemContract, BatchItemState, BatchJobContract, BatchJobState,
    ClaimStage,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

const SCHEMA_VERSION: i64 = 1;
const MAX_PAGE_SIZE: usize = 1_000;
static ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchItemSeed {
    pub note_id: i64,
    pub word: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewJob {
    pub deck_key: String,
    pub deck_name: String,
    pub dry_run: bool,
    pub settings: BTreeMap<String, Value>,
    pub items: Vec<BatchItemSeed>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSummary {
    pub job: BatchJobContract,
    pub total: u64,
    pub counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemPage {
    pub items: Vec<BatchItemContract>,
    pub total: u64,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedItem {
    pub item: BatchItemContract,
    pub stage: ClaimStage,
}

/// Anki-facing restoration remains outside the SQLite repository. This keeps
/// deletion and retention operations provably unable to contact Anki.
pub trait BatchRollbackPort {
    fn restore_snapshot(&self, snapshot_id: &str) -> Result<(), String>;
}

pub trait BatchWorkerPort {
    fn process(
        &mut self,
        job: &BatchJobContract,
        item: &BatchItemContract,
    ) -> Result<Value, String>;
    fn commit(
        &mut self,
        job: &BatchJobContract,
        item: &BatchItemContract,
        artifact: &Value,
    ) -> Result<BatchCommitResult, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchCommitResult {
    pub snapshot_id: String,
    pub result_note_id: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerStep {
    Idle,
    Processed(i64),
    Committed(i64),
    Retrying(i64),
    Failed(i64),
    Completed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RollbackReport {
    pub reverted_item_ids: Vec<i64>,
    pub conflicts: Vec<i64>,
    pub failures: Vec<(i64, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditIssue {
    pub job_id: Option<String>,
    pub item_id: Option<i64>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyAudit {
    pub jobs: Vec<LegacyJob>,
    pub issues: Vec<AuditIssue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyJob {
    pub job: BatchJobContract,
    pub items: Vec<BatchItemContract>,
}

impl LegacyAudit {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationReport {
    pub backup: PathBuf,
    pub target: PathBuf,
    pub jobs: usize,
    pub items: usize,
    pub artifacts: usize,
}

/// Process-scoped exclusive lease. Dropping it releases the kernel lock.
pub struct RunnerLease {
    file: File,
}

impl std::fmt::Debug for RunnerLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunnerLease")
            .finish_non_exhaustive()
    }
}

impl Drop for RunnerLease {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: `file` is open for the lease lifetime and flock only uses its fd.
        unsafe {
            libc::flock(std::os::fd::AsRawFd::as_raw_fd(&self.file), libc::LOCK_UN);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationMetadata {
    pub version: i64,
    pub name: String,
    pub applied_at: String,
}

#[derive(Debug)]
pub enum JobRepositoryError {
    ExistingDatabase(PathBuf),
    InvalidPage,
    EmptyJob,
    DuplicateNoteId(i64),
    UnknownJob(String),
    UnknownItem(i64),
    InvalidArtifactPath,
    InvalidState(String),
    ActiveJob(String),
    LegacyAuditFailed(Vec<AuditIssue>),
    TargetExists(PathBuf),
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Corrupt(String),
}

impl std::fmt::Display for JobRepositoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExistingDatabase(path) => write!(
                formatter,
                "existing non-native database: {}",
                path.display()
            ),
            Self::InvalidPage => write!(formatter, "page limit must be greater than zero"),
            Self::EmptyJob => write!(formatter, "a batch job requires at least one item"),
            Self::DuplicateNoteId(note_id) => write!(formatter, "duplicate note id: {note_id}"),
            Self::UnknownJob(job_id) => write!(formatter, "unknown job: {job_id}"),
            Self::UnknownItem(item_id) => write!(formatter, "unknown item: {item_id}"),
            Self::InvalidArtifactPath => write!(formatter, "invalid artifact path"),
            Self::InvalidState(state) => write!(formatter, "invalid batch state: {state}"),
            Self::ActiveJob(job_id) => write!(formatter, "job still active: {job_id}"),
            Self::LegacyAuditFailed(issues) => write!(
                formatter,
                "legacy audit failed with {} issue(s)",
                issues.len()
            ),
            Self::TargetExists(path) => {
                write!(formatter, "migration target exists: {}", path.display())
            }
            Self::Io(error) => error.fmt(formatter),
            Self::Sql(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Corrupt(message) => write!(formatter, "corrupt native job database: {message}"),
        }
    }
}

impl std::error::Error for JobRepositoryError {}

impl From<std::io::Error> for JobRepositoryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for JobRepositoryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}
impl From<serde_json::Error> for JobRepositoryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug)]
pub struct JobRepository {
    database: PathBuf,
    artifact_root: PathBuf,
}

impl JobRepository {
    /// Open an existing native database or create a new one. A database that
    /// lacks this repository's metadata is never modified.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, JobRepositoryError> {
        let database = path.into();
        let artifact_root = database.with_file_name(format!(
            "{}_artifacts",
            database
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("jobs")
        ));
        let repository = Self {
            database,
            artifact_root,
        };
        if repository.database.exists() {
            repository.verify_native_database()?;
        } else {
            repository.initialize()?;
        }
        fs::create_dir_all(&repository.artifact_root)?;
        Ok(repository)
    }

    pub fn database_path(&self) -> &Path {
        &self.database
    }
    pub fn artifact_root(&self) -> &Path {
        &self.artifact_root
    }

    /// Audit a byte-for-byte copied legacy database through a read-only
    /// connection. The source database and its WAL are never opened writable.
    pub fn audit_legacy(source: &Path) -> Result<LegacyAudit, JobRepositoryError> {
        let copy =
            std::env::temp_dir().join(format!("linguist-jobs-audit-{}.sqlite3", new_id("copy")));
        fs::copy(source, &copy)?;
        copy_sqlite_sidecars(source, &copy)?;
        let result = audit_legacy_copy(&copy, &legacy_artifact_root(source));
        let _ = fs::remove_file(&copy);
        let _ = fs::remove_file(sqlite_sidecar(&copy, "-wal"));
        let _ = fs::remove_file(sqlite_sidecar(&copy, "-shm"));
        result
    }

    /// Explicit migration entry point. It copies the legacy database to a
    /// sibling backup before creating a separate native target database.
    pub fn migrate_legacy(
        source: &Path,
        target: &Path,
    ) -> Result<MigrationReport, JobRepositoryError> {
        if target.exists() {
            return Err(JobRepositoryError::TargetExists(target.into()));
        }
        let audit = Self::audit_legacy(source)?;
        if !audit.is_valid() {
            return Err(JobRepositoryError::LegacyAuditFailed(audit.issues));
        }
        let backup = backup_path(source);
        fs::copy(source, &backup)?;
        let repository = Self::open(target)?;
        let mut artifacts = 0;
        for legacy in &audit.jobs {
            artifacts += repository.import_legacy_job(legacy, &legacy_artifact_root(source))?;
        }
        Ok(MigrationReport {
            backup,
            target: target.into(),
            jobs: audit.jobs.len(),
            items: audit.jobs.iter().map(|job| job.items.len()).sum(),
            artifacts,
        })
    }

    /// Return `None` if another process owns the runner. The lock is owned by
    /// the OS, so a crash releases it without a stale-file recovery path.
    pub fn acquire_runner_lease(&self) -> Result<Option<RunnerLease>, JobRepositoryError> {
        let path = self.database.with_extension("sqlite3.runner.lock");
        let file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: `file` stays live in the returned lease and flock only
            // accesses the valid descriptor.
            let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if status != 0 {
                return Ok(None);
            }
        }
        Ok(Some(RunnerLease { file }))
    }

    pub fn recover_interrupted(&self) -> Result<(), JobRepositoryError> {
        let now = now();
        let connection = self.connect()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE batch_jobs SET status='paused', updated_at=?, last_error=? WHERE status IN ('running', 'pausing')",
            params![now.clone(), "Application stopped while this job was running; resume is safe."],
        )?;
        transaction.execute(
            "UPDATE batch_items SET status='pending', updated_at=? WHERE status='processing'",
            [now.clone()],
        )?;
        transaction.execute(
            "UPDATE batch_items SET status='processed', updated_at=? WHERE status='committing'",
            [now.clone()],
        )?;
        transaction.execute("UPDATE batch_jobs SET status='rollback_paused', updated_at=? WHERE status='rolling_back'", [now])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_job_state(
        &self,
        job_id: &str,
        state: BatchJobState,
        error: &str,
    ) -> Result<(), JobRepositoryError> {
        let now = now();
        let state_name = job_state_name(state);
        let started_at = (state == BatchJobState::Running).then_some(now.clone());
        let finished_at = matches!(
            state,
            BatchJobState::Completed
                | BatchJobState::Failed
                | BatchJobState::Cancelled
                | BatchJobState::RolledBack
                | BatchJobState::RollbackPartial
        )
        .then_some(now.clone());
        let connection = self.connect()?;
        let updated = connection.execute(
            "UPDATE batch_jobs SET status=?, updated_at=?, last_error=?,
                    started_at=COALESCE(started_at, ?),
                    finished_at=CASE WHEN ?='running' THEN NULL ELSE COALESCE(?, finished_at) END
             WHERE id=?",
            params![
                state_name,
                now,
                error,
                started_at,
                state_name,
                finished_at,
                job_id
            ],
        )?;
        if updated == 1 {
            Ok(())
        } else {
            Err(JobRepositoryError::UnknownJob(job_id.into()))
        }
    }

    /// Claim pending processing work or an already-processed commit. The
    /// immediate transaction means two runners cannot claim the same row.
    pub fn claim_next(&self, job_id: &str) -> Result<Option<ClaimedItem>, JobRepositoryError> {
        let now = now();
        let mut connection = self.connect()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let item = transaction.query_row(
            "SELECT id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id,
                    result_note_id, last_error, started_at, finished_at, updated_at
             FROM batch_items WHERE job_id=? AND status IN ('processed', 'pending')
                 AND (next_attempt_at IS NULL OR next_attempt_at<=?)
             ORDER BY CASE status WHEN 'processed' THEN 0 ELSE 1 END, ordinal LIMIT 1",
            params![job_id, now], row_item,
        ).optional()?;
        let Some(mut item) = item else {
            transaction.commit()?;
            return Ok(None);
        };
        let old_state = parse_item_state(&item.status)?;
        let stage = old_state
            .claim_stage()
            .ok_or_else(|| JobRepositoryError::InvalidState(item.status.clone()))?;
        let new_state = match stage {
            ClaimStage::Process => BatchItemState::Processing,
            ClaimStage::Commit => BatchItemState::Committing,
        };
        transaction.execute(
            "UPDATE batch_items SET status=?, started_at=COALESCE(started_at, ?), updated_at=? WHERE id=?",
            params![item_state_name(new_state), now, now, item.id],
        )?;
        transaction.execute(
            "UPDATE batch_jobs SET updated_at=? WHERE id=?",
            params![now, job_id],
        )?;
        transaction.commit()?;
        item.status = item_state_name(new_state).into();
        item.updated_at = Some(now);
        Ok(Some(ClaimedItem { item, stage }))
    }

    /// Run one durable stage. Processing always persists an artifact before a
    /// later call may contact Anki for commit.
    pub fn run_next<P: BatchWorkerPort>(
        &self,
        job_id: &str,
        port: &mut P,
        max_attempts: u32,
        backoff_seconds: u64,
    ) -> Result<WorkerStep, JobRepositoryError> {
        let job = self
            .job(job_id)?
            .ok_or_else(|| JobRepositoryError::UnknownJob(job_id.into()))?;
        if job.status != "running" {
            return Ok(WorkerStep::Idle);
        }
        let Some(claim) = self.claim_next(job_id)? else {
            return Ok(if self.finish_if_complete(job_id)? {
                WorkerStep::Completed
            } else {
                WorkerStep::Idle
            });
        };
        let item_id = claim.item.id.expect("native claimed items have ids");
        let result = match claim.stage {
            ClaimStage::Process => port.process(&job, &claim.item).and_then(|artifact| {
                self.replace_artifact(job_id, item_id, &artifact)
                    .map(|_| WorkerStep::Processed(item_id))
                    .map_err(|error| error.to_string())
            }),
            ClaimStage::Commit => {
                let artifact = claim
                    .item
                    .artifact
                    .as_ref()
                    .ok_or_else(|| "processed item has no artifact".to_owned())
                    .and_then(|reference| {
                        self.load_artifact(reference)
                            .map_err(|error| error.to_string())
                    });
                artifact.and_then(|artifact| {
                    port.commit(&job, &claim.item, &artifact)
                        .and_then(|receipt| {
                            self.complete_item(
                                item_id,
                                &receipt.snapshot_id,
                                receipt.result_note_id,
                            )
                            .map(|_| WorkerStep::Committed(item_id))
                            .map_err(|error| error.to_string())
                        })
                })
            }
        };
        match result {
            Ok(step) => Ok(step),
            Err(error) => {
                let retry = self.retry_or_fail(item_id, &error, max_attempts, backoff_seconds)?;
                Ok(if retry {
                    WorkerStep::Retrying(item_id)
                } else {
                    WorkerStep::Failed(item_id)
                })
            }
        }
    }

    pub fn retry_or_fail(
        &self,
        item_id: i64,
        error: &str,
        max_attempts: u32,
        backoff_seconds: u64,
    ) -> Result<bool, JobRepositoryError> {
        let connection = self.connect()?;
        let item = connection.query_row(
            "SELECT id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id,
                    result_note_id, last_error, started_at, finished_at, updated_at FROM batch_items WHERE id=?",
            [item_id], row_item,
        ).optional()?.ok_or(JobRepositoryError::UnknownItem(item_id))?;
        let attempts = item.attempts + 1;
        let retry = attempts < max_attempts.max(1);
        let resume = if item.artifact.is_some() {
            BatchItemState::Processed
        } else {
            BatchItemState::Pending
        };
        let next_attempt_at = retry.then(|| {
            future_timestamp(
                backoff_seconds.saturating_mul(2_u64.saturating_pow(attempts.saturating_sub(1))),
            )
        });
        connection.execute(
            "UPDATE batch_items SET status=?, attempts=?, next_attempt_at=?, last_error=?, finished_at=?, updated_at=? WHERE id=?",
            params![item_state_name(if retry { resume } else { BatchItemState::Failed }), attempts, next_attempt_at, error,
                if retry { None } else { Some(now()) }, now(), item_id],
        )?;
        Ok(retry)
    }

    pub fn cancel(&self, job_id: &str) -> Result<(), JobRepositoryError> {
        let now = now();
        let connection = self.connect()?;
        let transaction = connection.unchecked_transaction()?;
        let updated = transaction.execute(
            "UPDATE batch_jobs SET status='cancelled', updated_at=?, finished_at=? WHERE id=?",
            params![now, now, job_id],
        )?;
        if updated != 1 {
            return Err(JobRepositoryError::UnknownJob(job_id.into()));
        }
        transaction.execute("UPDATE batch_items SET status='skipped', finished_at=?, updated_at=? WHERE job_id=? AND status='pending'", params![now, now, job_id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_if_complete(&self, job_id: &str) -> Result<bool, JobRepositoryError> {
        let connection = self.connect()?;
        let remaining: u64 = connection.query_row(
            "SELECT COUNT(*) FROM batch_items WHERE job_id=? AND status NOT IN ('completed', 'reverted', 'skipped', 'failed')", [job_id], |row| row.get(0),
        )?;
        if remaining != 0 {
            return Ok(false);
        }
        self.set_job_state(job_id, BatchJobState::Completed, "")?;
        Ok(true)
    }

    pub fn complete_item(
        &self,
        item_id: i64,
        snapshot_id: &str,
        result_note_id: i64,
    ) -> Result<(), JobRepositoryError> {
        let now = now();
        let connection = self.connect()?;
        let changed = connection.execute(
            "UPDATE batch_items SET status='completed', snapshot_id=?, result_note_id=?, finished_at=?, updated_at=? WHERE id=?",
            params![snapshot_id, result_note_id, now, now, item_id],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(JobRepositoryError::UnknownItem(item_id))
        }
    }

    /// Restore completed rows in reverse commit order. A later completed job
    /// owning the same note blocks the older rollback before Anki is touched.
    pub fn rollback<P: BatchRollbackPort>(
        &self,
        job_id: &str,
        port: &P,
    ) -> Result<RollbackReport, JobRepositoryError> {
        let job = self
            .job(job_id)?
            .ok_or_else(|| JobRepositoryError::UnknownJob(job_id.into()))?;
        if job.dry_run {
            let connection = self.connect()?;
            let mut statement = connection.prepare(
                "SELECT id FROM batch_items WHERE job_id=? AND status='completed' ORDER BY ordinal DESC",
            )?;
            let ids = statement
                .query_map([job_id], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);
            connection.execute(
                "UPDATE batch_items SET status='reverted', updated_at=? WHERE job_id=? AND status='completed'",
                params![now(), job_id],
            )?;
            self.set_job_state(job_id, BatchJobState::RolledBack, "")?;
            return Ok(RollbackReport {
                reverted_item_ids: ids,
                ..RollbackReport::default()
            });
        }
        self.set_job_state(job_id, BatchJobState::RollingBack, "")?;
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id,
                    result_note_id, last_error, started_at, finished_at, updated_at
             FROM batch_items WHERE job_id=? AND status IN ('completed', 'rollback_failed') ORDER BY ordinal DESC",
        )?;
        let items = statement
            .query_map([job_id], row_item)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut report = RollbackReport::default();
        for item in items {
            let item_id = item.id.expect("native item ids are present");
            if self.later_completed_change_exists(job_id, item.note_id)? {
                self.mark_rollback_failure(item_id, "newer completed job owns this note")?;
                report.conflicts.push(item_id);
                continue;
            }
            let Some(snapshot_id) = item.snapshot_id.as_deref() else {
                self.mark_rollback_failure(item_id, "completed item has no snapshot")?;
                report
                    .failures
                    .push((item_id, "completed item has no snapshot".into()));
                continue;
            };
            match port.restore_snapshot(snapshot_id) {
                Ok(()) => {
                    let connection = self.connect()?;
                    connection.execute(
                        "UPDATE batch_items SET status='reverted', last_error='', finished_at=?, updated_at=? WHERE id=?",
                        params![now(), now(), item_id],
                    )?;
                    report.reverted_item_ids.push(item_id);
                }
                Err(error) => {
                    self.mark_rollback_failure(item_id, &error)?;
                    report.failures.push((item_id, error));
                }
            }
        }
        let final_state = if report.conflicts.is_empty() && report.failures.is_empty() {
            BatchJobState::RolledBack
        } else {
            BatchJobState::RollbackPartial
        };
        self.set_job_state(job_id, final_state, "")?;
        Ok(report)
    }

    /// Delete database rows and their artifacts only. Snapshots are owned by
    /// the separate snapshot repository and deliberately remain untouched.
    pub fn delete_job(&self, job_id: &str) -> Result<RollbackReport, JobRepositoryError> {
        let job = self
            .job(job_id)?
            .ok_or_else(|| JobRepositoryError::UnknownJob(job_id.into()))?;
        if matches!(job.status.as_str(), "running" | "pausing" | "rolling_back") {
            return Err(JobRepositoryError::ActiveJob(job_id.into()));
        }
        let connection = self.connect()?;
        let active: u64 = connection.query_row(
            "SELECT COUNT(*) FROM batch_items WHERE job_id=? AND status IN ('processing', 'committing')", [job_id], |row| row.get(0),
        )?;
        if active != 0 {
            return Err(JobRepositoryError::ActiveJob(job_id.into()));
        }
        let mut statement = connection.prepare("SELECT id FROM batch_items WHERE job_id=?")?;
        let item_ids = statement
            .query_map([job_id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let connection = self.connect()?;
        connection.execute("DELETE FROM batch_jobs WHERE id=?", [job_id])?;
        let directory = self.artifact_root.join(job_id);
        if directory.parent() == Some(self.artifact_root.as_path()) && directory.exists() {
            fs::remove_dir_all(directory)?;
        }
        Ok(RollbackReport {
            reverted_item_ids: item_ids,
            ..RollbackReport::default()
        })
    }

    fn mark_rollback_failure(&self, item_id: i64, error: &str) -> Result<(), JobRepositoryError> {
        let connection = self.connect()?;
        connection.execute(
            "UPDATE batch_items SET status='rollback_failed', last_error=?, updated_at=? WHERE id=?",
            params![error, now(), item_id],
        )?;
        Ok(())
    }

    fn later_completed_change_exists(
        &self,
        job_id: &str,
        note_id: i64,
    ) -> Result<bool, JobRepositoryError> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT 1 FROM batch_items newer JOIN batch_jobs newer_job ON newer_job.id=newer.job_id
             JOIN batch_jobs current_job ON current_job.id=?
             WHERE newer.note_id=? AND newer_job.created_at>current_job.created_at AND newer.status='completed' LIMIT 1",
            params![job_id, note_id], |_| Ok(()),
        ).optional().map(|value| value.is_some()).map_err(Into::into)
    }

    pub fn migration_metadata(&self) -> Result<Vec<MigrationMetadata>, JobRepositoryError> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version")?;
        statement
            .query_map([], |row| {
                Ok(MigrationMetadata {
                    version: row.get(0)?,
                    name: row.get(1)?,
                    applied_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn create_job(&self, job: NewJob) -> Result<String, JobRepositoryError> {
        if job.items.is_empty() {
            return Err(JobRepositoryError::EmptyJob);
        }
        let mut notes = BTreeSet::new();
        for item in &job.items {
            if !notes.insert(item.note_id) {
                return Err(JobRepositoryError::DuplicateNoteId(item.note_id));
            }
        }
        let id = new_id("batch");
        let now = now();
        let settings = serde_json::to_string(&job.settings)?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO batch_jobs (id, deck_key, deck_name, status, dry_run, settings_json, created_at, updated_at, last_error)
             VALUES (?, ?, ?, 'queued', ?, ?, ?, ?, '')",
            params![id, job.deck_key, job.deck_name, job.dry_run, settings, now, now],
        )?;
        for (ordinal, item) in job.items.iter().enumerate() {
            transaction.execute(
                "INSERT INTO batch_items (job_id, ordinal, note_id, word, status, updated_at)
                 VALUES (?, ?, ?, ?, 'pending', ?)",
                params![id, ordinal as i64, item.note_id, item.word, now],
            )?;
        }
        transaction.commit()?;
        Ok(id)
    }

    pub fn job(&self, job_id: &str) -> Result<Option<BatchJobContract>, JobRepositoryError> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT id, deck_key, deck_name, status, dry_run, settings_json, created_at, updated_at, started_at, finished_at, last_error
             FROM batch_jobs WHERE id=?",
            [job_id], row_job,
        ).optional().map_err(Into::into)
    }

    pub fn job_summaries(&self) -> Result<Vec<JobSummary>, JobRepositoryError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT j.id, j.deck_key, j.deck_name, j.status, j.dry_run, j.settings_json, j.created_at, j.updated_at,
                    j.started_at, j.finished_at, j.last_error, i.status, COUNT(i.id)
             FROM batch_jobs j LEFT JOIN batch_items i ON i.job_id=j.id
             GROUP BY j.id, i.status ORDER BY j.created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row_job(row)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, u64>(12)?,
            ))
        })?;
        let mut summaries = BTreeMap::<String, JobSummary>::new();
        for row in rows {
            let (job, status, count) = row?;
            let entry = summaries
                .entry(job.id.clone())
                .or_insert_with(|| JobSummary {
                    job: job.clone(),
                    total: 0,
                    counts: BTreeMap::new(),
                });
            entry.total += count;
            if let Some(status) = status {
                entry.counts.insert(status, count);
            }
        }
        Ok(summaries.into_values().collect())
    }

    pub fn item_page(
        &self,
        job_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<ItemPage, JobRepositoryError> {
        if limit == 0 {
            return Err(JobRepositoryError::InvalidPage);
        }
        if self.job(job_id)?.is_none() {
            return Err(JobRepositoryError::UnknownJob(job_id.into()));
        }
        let limit = limit.min(MAX_PAGE_SIZE);
        let connection = self.connect()?;
        let total = connection.query_row(
            "SELECT COUNT(*) FROM batch_items WHERE job_id=?",
            [job_id],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(
            "SELECT id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id,
                    result_note_id, last_error, started_at, finished_at, updated_at
             FROM batch_items WHERE job_id=? ORDER BY ordinal LIMIT ? OFFSET ?",
        )?;
        let items = statement
            .query_map(params![job_id, limit as i64, offset as i64], row_item)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ItemPage {
            items,
            total,
            offset,
            limit,
        })
    }

    /// Atomically replace an artifact, then point the item at the completed
    /// file in one SQLite transaction. Artifacts never live in SQLite blobs.
    pub fn replace_artifact(
        &self,
        job_id: &str,
        item_id: i64,
        artifact: &Value,
    ) -> Result<BatchArtifactReference, JobRepositoryError> {
        self.require_item(job_id, item_id)?;
        let directory = self.artifact_root.join(job_id);
        fs::create_dir_all(&directory)?;
        let final_path = directory.join(format!("{item_id}.json"));
        let temporary = directory.join(format!(
            ".{item_id}.{}.tmp",
            ID_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec(artifact)?;
        let write_result = (|| -> Result<(), JobRepositoryError> {
            let mut file = File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &final_path)?;
            sync_directory(&directory)?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let reference = BatchArtifactReference {
            reference: final_path.to_string_lossy().into_owned(),
            extensions: BTreeMap::new(),
        };
        let connection = self.connect()?;
        let updated = connection.execute(
            "UPDATE batch_items SET artifact_path=?, status='processed', last_error='', updated_at=? WHERE id=? AND job_id=?",
            params![reference.reference, now(), item_id, job_id],
        )?;
        if updated != 1 {
            return Err(JobRepositoryError::UnknownItem(item_id));
        }
        Ok(reference)
    }

    pub fn load_artifact(
        &self,
        artifact: &BatchArtifactReference,
    ) -> Result<Value, JobRepositoryError> {
        let path = PathBuf::from(&artifact.reference);
        if !path.starts_with(&self.artifact_root) {
            return Err(JobRepositoryError::InvalidArtifactPath);
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn require_item(&self, job_id: &str, item_id: i64) -> Result<(), JobRepositoryError> {
        let connection = self.connect()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM batch_items WHERE id=? AND job_id=?",
                params![item_id, job_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            Ok(())
        } else {
            Err(JobRepositoryError::UnknownItem(item_id))
        }
    }

    fn import_legacy_job(
        &self,
        legacy: &LegacyJob,
        legacy_artifacts: &Path,
    ) -> Result<usize, JobRepositoryError> {
        let job = &legacy.job;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO batch_jobs (id, deck_key, deck_name, status, dry_run, settings_json, created_at, updated_at, started_at, finished_at, last_error)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![job.id, job.deck_key, job.deck_name, job.status, job.dry_run, serde_json::to_string(&job.settings)?,
                job.created_at, job.updated_at, job.started_at, job.finished_at, job.last_error],
        )?;
        let mut copied = 0;
        for item in &legacy.items {
            let artifact = item
                .artifact
                .as_ref()
                .map(|source| {
                    let source_path = PathBuf::from(&source.reference);
                    let file_name = source_path
                        .file_name()
                        .ok_or(JobRepositoryError::InvalidArtifactPath)?;
                    let directory = self.artifact_root.join(&job.id);
                    fs::create_dir_all(&directory)?;
                    let target_path = directory.join(file_name);
                    copy_atomic(&source_path, &target_path)?;
                    copied += 1;
                    Ok::<BatchArtifactReference, JobRepositoryError>(BatchArtifactReference {
                        reference: target_path.to_string_lossy().into_owned(),
                        extensions: source.extensions.clone(),
                    })
                })
                .transpose()?;
            transaction.execute(
                "INSERT INTO batch_items (id, job_id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id, result_note_id, last_error, started_at, finished_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![item.id, job.id, item.ordinal, item.note_id, item.word, item.status, item.attempts,
                    item.next_attempt_at, artifact.as_ref().map(|artifact| &artifact.reference), item.snapshot_id,
                    item.result_note_id, item.last_error, item.started_at, item.finished_at, item.updated_at],
            )?;
        }
        transaction.commit()?;
        let _ = legacy_artifacts;
        Ok(copied)
    }

    fn initialize(&self) -> Result<(), JobRepositoryError> {
        if let Some(parent) = self.database.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.database)?;
        configure(&connection)?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL);
             CREATE TABLE batch_jobs (
                 id TEXT PRIMARY KEY, deck_key TEXT NOT NULL, deck_name TEXT NOT NULL, status TEXT NOT NULL,
                 dry_run INTEGER NOT NULL CHECK(dry_run IN (0, 1)), settings_json TEXT NOT NULL,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL, started_at TEXT, finished_at TEXT,
                 last_error TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE batch_items (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT NOT NULL REFERENCES batch_jobs(id) ON DELETE CASCADE,
                 ordinal INTEGER NOT NULL, note_id INTEGER NOT NULL, word TEXT NOT NULL, status TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0, next_attempt_at TEXT, artifact_path TEXT, snapshot_id TEXT,
                 result_note_id INTEGER, last_error TEXT NOT NULL DEFAULT '', started_at TEXT, finished_at TEXT,
                 updated_at TEXT NOT NULL, UNIQUE(job_id, note_id)
             );
             CREATE INDEX idx_batch_items_work ON batch_items(job_id, status, ordinal);
             CREATE INDEX idx_batch_items_note ON batch_items(note_id, status);
             COMMIT;",
        )?;
        connection.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, 'native_jobs_v1', ?)",
            params![SCHEMA_VERSION, now()],
        )?;
        Ok(())
    }

    fn verify_native_database(&self) -> Result<(), JobRepositoryError> {
        let connection =
            Connection::open_with_flags(&self.database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version = connection
            .query_row(
                "SELECT version FROM schema_migrations WHERE version=?",
                [SCHEMA_VERSION],
                |row| row.get::<_, i64>(0),
            )
            .optional();
        match version {
            Ok(Some(_)) => Ok(()),
            Ok(None) | Err(_) => Err(JobRepositoryError::ExistingDatabase(self.database.clone())),
        }
    }

    fn connect(&self) -> Result<Connection, JobRepositoryError> {
        let connection = Connection::open(&self.database)?;
        configure(&connection)?;
        Ok(connection)
    }
}

fn configure(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
}

fn row_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<BatchJobContract> {
    let settings: String = row.get(5)?;
    let settings = serde_json::from_str(&settings).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(BatchJobContract {
        id: row.get(0)?,
        deck_key: row.get(1)?,
        deck_name: row.get(2)?,
        status: row.get(3)?,
        dry_run: row.get(4)?,
        settings,
        created_at: Some(row.get(6)?),
        updated_at: Some(row.get(7)?),
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
        last_error: row.get(10)?,
        extensions: BTreeMap::new(),
    })
}

fn row_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<BatchItemContract> {
    let artifact_path: Option<String> = row.get(7)?;
    Ok(BatchItemContract {
        id: Some(row.get(0)?),
        ordinal: Some(row.get(1)?),
        note_id: row.get(2)?,
        word: row.get(3)?,
        status: row.get(4)?,
        attempts: row.get(5)?,
        next_attempt_at: row.get(6)?,
        artifact: artifact_path.map(|reference| BatchArtifactReference {
            reference,
            extensions: BTreeMap::new(),
        }),
        snapshot_id: row.get(8)?,
        result_note_id: row.get(9)?,
        last_error: row.get(10)?,
        started_at: row.get(11)?,
        finished_at: row.get(12)?,
        updated_at: row.get(13)?,
        extensions: BTreeMap::new(),
    })
}

fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        now(),
        ID_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn now() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}

fn future_timestamp(seconds: u64) -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(std::time::Duration::from_secs(seconds));
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}

fn job_state_name(state: BatchJobState) -> &'static str {
    match state {
        BatchJobState::Queued => "queued",
        BatchJobState::Running => "running",
        BatchJobState::Pausing => "pausing",
        BatchJobState::Paused => "paused",
        BatchJobState::Completed => "completed",
        BatchJobState::Failed => "failed",
        BatchJobState::Cancelled => "cancelled",
        BatchJobState::RollingBack => "rolling_back",
        BatchJobState::RollbackPaused => "rollback_paused",
        BatchJobState::RolledBack => "rolled_back",
        BatchJobState::RollbackPartial => "rollback_partial",
    }
}

fn item_state_name(state: BatchItemState) -> &'static str {
    match state {
        BatchItemState::Pending => "pending",
        BatchItemState::Processing => "processing",
        BatchItemState::Processed => "processed",
        BatchItemState::Committing => "committing",
        BatchItemState::Completed => "completed",
        BatchItemState::Failed => "failed",
        BatchItemState::Skipped => "skipped",
        BatchItemState::Reverted => "reverted",
        BatchItemState::RollbackFailed => "rollback_failed",
    }
}

fn parse_item_state(state: &str) -> Result<BatchItemState, JobRepositoryError> {
    match state {
        "pending" => Ok(BatchItemState::Pending),
        "processing" => Ok(BatchItemState::Processing),
        "processed" => Ok(BatchItemState::Processed),
        "committing" => Ok(BatchItemState::Committing),
        "completed" => Ok(BatchItemState::Completed),
        "failed" => Ok(BatchItemState::Failed),
        "skipped" => Ok(BatchItemState::Skipped),
        "reverted" => Ok(BatchItemState::Reverted),
        "rollback_failed" => Ok(BatchItemState::RollbackFailed),
        _ => Err(JobRepositoryError::InvalidState(state.into())),
    }
}

fn valid_job_state(state: &str) -> bool {
    matches!(
        state,
        "queued"
            | "running"
            | "pausing"
            | "paused"
            | "completed"
            | "failed"
            | "cancelled"
            | "rolling_back"
            | "rollback_paused"
            | "rolled_back"
            | "rollback_partial"
    )
}

fn audit_legacy_copy(copy: &Path, artifact_root: &Path) -> Result<LegacyAudit, JobRepositoryError> {
    let connection = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut audit = LegacyAudit::default();
    for table in ["batch_jobs", "batch_items"] {
        let exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
                [table],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            audit.issues.push(AuditIssue {
                job_id: None,
                item_id: None,
                message: format!("missing table {table}"),
            });
        }
    }
    if !audit.issues.is_empty() {
        return Ok(audit);
    }
    let mut jobs = connection.prepare(
        "SELECT id, deck_key, deck_name, status, dry_run, settings_json, created_at, updated_at, started_at, finished_at, last_error FROM batch_jobs ORDER BY created_at, id",
    )?;
    let jobs = jobs
        .query_map([], row_job)?
        .collect::<Result<Vec<_>, _>>()?;
    for job in jobs {
        let mut issues = Vec::new();
        if !valid_job_state(&job.status) {
            issues.push("unknown job status".into());
        }
        let mut statement = connection.prepare(
            "SELECT id, ordinal, note_id, word, status, attempts, next_attempt_at, artifact_path, snapshot_id, result_note_id, last_error, started_at, finished_at, updated_at FROM batch_items WHERE job_id=? ORDER BY ordinal",
        )?;
        let items = statement
            .query_map([&job.id], row_item)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut notes = BTreeSet::new();
        for item in &items {
            if !notes.insert(item.note_id) {
                audit.issues.push(AuditIssue {
                    job_id: Some(job.id.clone()),
                    item_id: item.id,
                    message: "duplicate note id".into(),
                });
            }
            if parse_item_state(&item.status).is_err() {
                audit.issues.push(AuditIssue {
                    job_id: Some(job.id.clone()),
                    item_id: item.id,
                    message: "unknown item status".into(),
                });
            }
            if let Some(artifact) = &item.artifact {
                let path = PathBuf::from(&artifact.reference);
                if !path.starts_with(artifact_root) || !path.is_file() {
                    audit.issues.push(AuditIssue {
                        job_id: Some(job.id.clone()),
                        item_id: item.id,
                        message: "artifact missing or outside legacy root".into(),
                    });
                } else if serde_json::from_slice::<Value>(&fs::read(path)?)
                    .ok()
                    .filter(Value::is_object)
                    .is_none()
                {
                    audit.issues.push(AuditIssue {
                        job_id: Some(job.id.clone()),
                        item_id: item.id,
                        message: "artifact is not a JSON object".into(),
                    });
                }
            }
        }
        for message in issues {
            audit.issues.push(AuditIssue {
                job_id: Some(job.id.clone()),
                item_id: None,
                message,
            });
        }
        audit.jobs.push(LegacyJob { job, items });
    }
    Ok(audit)
}

fn legacy_artifact_root(database: &Path) -> PathBuf {
    database.with_file_name(format!(
        "{}_artifacts",
        database
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("jobs")
    ))
}

fn backup_path(source: &Path) -> PathBuf {
    source.with_file_name(format!(
        "{}.pre-native-{}.bak",
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("jobs.sqlite3"),
        new_id("backup")
    ))
}

fn copy_atomic(source: &Path, target: &Path) -> Result<(), JobRepositoryError> {
    let temporary = target.with_file_name(format!(
        ".{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact")
    ));
    fs::copy(source, &temporary)?;
    File::open(&temporary)?.sync_all()?;
    fs::rename(&temporary, target)?;
    if let Some(parent) = target.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", database.display(), suffix))
}

fn copy_sqlite_sidecars(source: &Path, target: &Path) -> Result<(), JobRepositoryError> {
    for suffix in ["-wal", "-shm"] {
        let source = sqlite_sidecar(source, suffix);
        if source.exists() {
            fs::copy(source, sqlite_sidecar(target, suffix))?;
        }
    }
    Ok(())
}

fn sync_directory(directory: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process::Command};

    use serde_json::json;

    use super::*;

    fn temporary_path(name: &str) -> PathBuf {
        env::temp_dir()
            .join(format!("linguist-jobs-{name}-{}", new_id("test")))
            .join("jobs.sqlite3")
    }

    fn job(items: &[i64]) -> NewJob {
        NewJob {
            deck_key: "japanese_vocab".into(),
            deck_name: "Japanese".into(),
            dry_run: false,
            settings: BTreeMap::from([("max_attempts".into(), json!(3))]),
            items: items
                .iter()
                .map(|note_id| BatchItemSeed {
                    note_id: *note_id,
                    word: format!("word-{note_id}"),
                })
                .collect(),
        }
    }

    fn legacy_fixture(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = Connection::open(path).unwrap();
        connection.execute_batch(
            "CREATE TABLE batch_jobs (id TEXT PRIMARY KEY, deck_key TEXT NOT NULL, deck_name TEXT NOT NULL, status TEXT NOT NULL, dry_run INTEGER NOT NULL, settings_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, last_error TEXT NOT NULL);
             CREATE TABLE batch_items (id INTEGER PRIMARY KEY, job_id TEXT NOT NULL, ordinal INTEGER NOT NULL, note_id INTEGER NOT NULL, word TEXT NOT NULL, status TEXT NOT NULL, attempts INTEGER NOT NULL, next_attempt_at TEXT, artifact_path TEXT, snapshot_id TEXT, result_note_id INTEGER, last_error TEXT NOT NULL, started_at TEXT, finished_at TEXT, updated_at TEXT NOT NULL);"
        ).unwrap();
        let statuses = [
            ("queued", "pending"),
            ("paused", "pending"),
            ("failed", "failed"),
            ("completed", "completed"),
            ("running", "processing"),
        ];
        let artifacts = legacy_artifact_root(path);
        fs::create_dir_all(artifacts.join("completed")).unwrap();
        let artifact = artifacts.join("completed/4.json");
        fs::write(&artifact, br#"{"word":"done"}"#).unwrap();
        for (index, (job_status, item_status)) in statuses.into_iter().enumerate() {
            connection.execute(
                "INSERT INTO batch_jobs VALUES (?, 'japanese_vocab', 'Japanese', ?, 0, '{\"max_attempts\":3}', ?, ?, NULL, NULL, '')",
                params![job_status, job_status, format!("2026-01-0{}T00:00:00Z", index + 1), format!("2026-01-0{}T00:00:00Z", index + 1)],
            ).unwrap();
            let artifact_path =
                (job_status == "completed").then(|| artifact.to_string_lossy().into_owned());
            connection.execute(
                "INSERT INTO batch_items VALUES (?, ?, 0, ?, ?, ?, 0, NULL, ?, ?, ?, '', NULL, NULL, ?)",
                params![index as i64 + 1, job_status, index as i64 + 1, format!("word-{job_status}"), item_status, artifact_path,
                    (job_status == "completed").then_some("snapshot-4"), (job_status == "completed").then_some(4_i64), format!("2026-01-0{}T00:00:00Z", index + 1)],
            ).unwrap();
        }
    }

    #[test]
    fn creates_native_schema_with_wal_metadata_and_immutable_settings() {
        let path = temporary_path("create");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[1, 2])).unwrap();
        let stored = repository.job(&id).unwrap().unwrap();
        assert_eq!(stored.settings["max_attempts"], 3);
        assert_eq!(
            repository.migration_metadata().unwrap()[0].name,
            "native_jobs_v1"
        );
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn duplicate_note_ids_reject_the_entire_job() {
        let path = temporary_path("duplicates");
        let repository = JobRepository::open(&path).unwrap();
        assert!(matches!(
            repository.create_job(job(&[7, 7])),
            Err(JobRepositoryError::DuplicateNoteId(7))
        ));
        assert!(repository.job_summaries().unwrap().is_empty());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn pages_items_and_counts_without_loading_the_whole_job() {
        let path = temporary_path("paging");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository
            .create_job(job(&(0..2_005).collect::<Vec<_>>()))
            .unwrap();
        let page = repository.item_page(&id, 10_000, 1_000).unwrap();
        assert_eq!(page.limit, MAX_PAGE_SIZE);
        assert_eq!(page.total, 2_005);
        assert_eq!(page.items.len(), 1_000);
        assert_eq!(page.items[0].note_id, 1_000);
        assert_eq!(
            repository.job_summaries().unwrap()[0].counts["pending"],
            2_005
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn atomically_replaces_artifacts_and_keeps_item_reference_current() {
        let path = temporary_path("artifact");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[42])).unwrap();
        let item = repository.item_page(&id, 1, 0).unwrap().items.remove(0);
        let artifact = repository
            .replace_artifact(&id, item.id.unwrap(), &json!({"value": 1}))
            .unwrap();
        assert_eq!(
            repository.load_artifact(&artifact).unwrap(),
            json!({"value": 1})
        );
        let artifact = repository
            .replace_artifact(&id, item.id.unwrap(), &json!({"value": 2}))
            .unwrap();
        assert_eq!(
            repository.load_artifact(&artifact).unwrap(),
            json!({"value": 2})
        );
        assert_eq!(
            repository.item_page(&id, 1, 0).unwrap().items[0].status,
            "processed"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn existing_non_native_database_is_left_untouched() {
        let path = temporary_path("legacy");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"legacy bytes").unwrap();
        let before = fs::read(&path).unwrap();
        assert!(matches!(
            JobRepository::open(&path),
            Err(JobRepositoryError::ExistingDatabase(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn claims_are_atomic_and_processed_artifacts_resume_at_commit() {
        let path = temporary_path("claim");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[42])).unwrap();
        let first = repository.claim_next(&id).unwrap().unwrap();
        assert_eq!(first.stage, ClaimStage::Process);
        assert!(repository.claim_next(&id).unwrap().is_none());
        repository
            .replace_artifact(&id, first.item.id.unwrap(), &json!({"draft": true}))
            .unwrap();
        let retry = repository.claim_next(&id).unwrap().unwrap();
        assert_eq!(retry.stage, ClaimStage::Commit);
        assert!(
            repository
                .retry_or_fail(retry.item.id.unwrap(), "offline", 3, 60)
                .unwrap()
        );
        assert_eq!(
            repository.item_page(&id, 1, 0).unwrap().items[0].status,
            "processed"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn recovery_returns_work_to_safe_boundaries_and_lease_is_exclusive() {
        let path = temporary_path("recovery");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[1, 2])).unwrap();
        repository
            .set_job_state(&id, BatchJobState::Running, "")
            .unwrap();
        let processing = repository.claim_next(&id).unwrap().unwrap();
        repository
            .replace_artifact(
                &id,
                repository.item_page(&id, 2, 0).unwrap().items[1]
                    .id
                    .unwrap(),
                &json!({}),
            )
            .unwrap();
        let committing = repository.claim_next(&id).unwrap().unwrap();
        assert_eq!(processing.stage, ClaimStage::Process);
        assert_eq!(committing.stage, ClaimStage::Commit);
        let lease = repository.acquire_runner_lease().unwrap().unwrap();
        let child = Command::new(env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::runner_lease_child_probe")
            .env("LINGUIST_JOBS_LEASE_PROBE", &path)
            .status()
            .unwrap();
        assert!(child.success());
        repository.recover_interrupted().unwrap();
        assert_eq!(repository.job(&id).unwrap().unwrap().status, "paused");
        let states = repository
            .item_page(&id, 2, 0)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.status)
            .collect::<Vec<_>>();
        assert_eq!(states, vec!["pending", "processed"]);
        drop(lease);
        assert!(repository.acquire_runner_lease().unwrap().is_some());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn runner_lease_child_probe() {
        let Ok(path) = env::var("LINGUIST_JOBS_LEASE_PROBE") else {
            return;
        };
        let repository = JobRepository::open(path).unwrap();
        assert!(repository.acquire_runner_lease().unwrap().is_none());
    }

    #[derive(Default)]
    struct MockRollbackPort {
        restored: std::sync::Mutex<Vec<String>>,
        fail: Option<String>,
    }

    impl BatchRollbackPort for MockRollbackPort {
        fn restore_snapshot(&self, snapshot_id: &str) -> Result<(), String> {
            self.restored.lock().unwrap().push(snapshot_id.into());
            if self.fail.as_deref() == Some(snapshot_id) {
                Err("restore failed".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn rollback_reverses_commits_and_never_overwrites_newer_job() {
        let path = temporary_path("rollback");
        let repository = JobRepository::open(&path).unwrap();
        let older = repository.create_job(job(&[10, 11])).unwrap();
        let old_items = repository.item_page(&older, 2, 0).unwrap().items;
        repository
            .complete_item(old_items[0].id.unwrap(), "old-10", 10)
            .unwrap();
        repository
            .complete_item(old_items[1].id.unwrap(), "old-11", 11)
            .unwrap();
        let newer = repository.create_job(job(&[10])).unwrap();
        let newer_item = repository.item_page(&newer, 1, 0).unwrap().items.remove(0);
        repository
            .complete_item(newer_item.id.unwrap(), "new-10", 10)
            .unwrap();
        let port = MockRollbackPort::default();

        let report = repository.rollback(&older, &port).unwrap();
        assert_eq!(report.conflicts, vec![old_items[0].id.unwrap()]);
        assert_eq!(*port.restored.lock().unwrap(), vec!["old-11"]);
        assert_eq!(
            repository.job(&older).unwrap().unwrap().status,
            "rollback_partial"
        );
        assert_eq!(
            repository.item_page(&older, 2, 0).unwrap().items[0].status,
            "rollback_failed"
        );
        assert_eq!(
            repository.item_page(&newer, 1, 0).unwrap().items[0].status,
            "completed"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn rollback_failures_retry_and_deletion_removes_only_artifacts() {
        let path = temporary_path("retention");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[7])).unwrap();
        let item = repository.item_page(&id, 1, 0).unwrap().items.remove(0);
        let artifact = repository
            .replace_artifact(&id, item.id.unwrap(), &json!({"cached": true}))
            .unwrap();
        repository
            .complete_item(item.id.unwrap(), "snap-7", 7)
            .unwrap();
        let failing = MockRollbackPort {
            fail: Some("snap-7".into()),
            ..Default::default()
        };
        assert_eq!(
            repository.rollback(&id, &failing).unwrap().failures.len(),
            1
        );
        let success = MockRollbackPort::default();
        assert_eq!(
            repository
                .rollback(&id, &success)
                .unwrap()
                .reverted_item_ids,
            vec![item.id.unwrap()]
        );
        assert!(std::path::Path::new(&artifact.reference).exists());

        // delete_job accepts no Anki/snapshot port: this test proves retention
        // cannot call Anki while removing only its database/artifact scope.
        repository.delete_job(&id).unwrap();
        assert!(!std::path::Path::new(&artifact.reference).exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn audit_and_backup_first_migration_preserve_python_fixture_bytes() {
        let source = temporary_path("legacy-migrate");
        legacy_fixture(&source);
        let before = fs::read(&source).unwrap();
        let audit = JobRepository::audit_legacy(&source).unwrap();
        assert!(audit.is_valid());
        assert_eq!(audit.jobs.len(), 5);
        assert_eq!(fs::read(&source).unwrap(), before);

        let target = source.with_file_name("native.sqlite3");
        let report = JobRepository::migrate_legacy(&source, &target).unwrap();
        assert_eq!((report.jobs, report.items, report.artifacts), (5, 5, 1));
        assert_eq!(fs::read(&source).unwrap(), before);
        assert_eq!(fs::read(&report.backup).unwrap(), before);
        let migrated = JobRepository::open(&target).unwrap();
        let statuses = migrated
            .job_summaries()
            .unwrap()
            .into_iter()
            .map(|summary| summary.job.status)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            statuses,
            BTreeSet::from([
                "queued".into(),
                "paused".into(),
                "failed".into(),
                "completed".into(),
                "running".into(),
            ])
        );
        let completed = migrated
            .item_page("completed", 1, 0)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(
            migrated
                .load_artifact(completed.artifact.as_ref().unwrap())
                .unwrap(),
            json!({"word": "done"})
        );
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[derive(Default)]
    struct Worker {
        events: Vec<String>,
    }

    impl BatchWorkerPort for Worker {
        fn process(
            &mut self,
            _: &BatchJobContract,
            item: &BatchItemContract,
        ) -> Result<Value, String> {
            self.events.push(format!("process:{}", item.note_id));
            Ok(json!({"note_id":item.note_id,"document":{"expression":item.word}}))
        }

        fn commit(
            &mut self,
            _: &BatchJobContract,
            item: &BatchItemContract,
            artifact: &Value,
        ) -> Result<BatchCommitResult, String> {
            assert_eq!(artifact["note_id"], item.note_id);
            self.events.push(format!("commit:{}", item.note_id));
            Ok(BatchCommitResult {
                snapshot_id: format!("snapshot-{}", item.note_id),
                result_note_id: item.note_id,
            })
        }
    }

    #[test]
    fn worker_persists_processed_artifact_before_commit() {
        let path = temporary_path("worker");
        let repository = JobRepository::open(&path).unwrap();
        let id = repository.create_job(job(&[11])).unwrap();
        repository
            .set_job_state(&id, BatchJobState::Running, "")
            .unwrap();
        let mut worker = Worker::default();
        assert!(matches!(
            repository.run_next(&id, &mut worker, 3, 0).unwrap(),
            WorkerStep::Processed(_)
        ));
        let processed = repository.item_page(&id, 1, 0).unwrap().items.remove(0);
        assert_eq!(processed.status, "processed");
        assert!(processed.artifact.is_some());
        assert!(matches!(
            repository.run_next(&id, &mut worker, 3, 0).unwrap(),
            WorkerStep::Committed(_)
        ));
        assert_eq!(
            repository.run_next(&id, &mut worker, 3, 0).unwrap(),
            WorkerStep::Completed
        );
        assert_eq!(worker.events, ["process:11", "commit:11"]);
        let completed = repository.item_page(&id, 1, 0).unwrap().items.remove(0);
        assert_eq!(completed.snapshot_id.as_deref(), Some("snapshot-11"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
