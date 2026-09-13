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
}
