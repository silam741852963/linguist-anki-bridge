//! `jobs migrate --legacy` (OP-47): translate the legacy Python/desktop batch
//! job database into read-only history.
//!
//! The source is copied (with its WAL file, when present) into a private
//! scratch directory and opened read-only there, so the original files are
//! never opened for writing. Item outcomes that may have reached Anki
//! (`committing`, `rollback_failed`) require review and are never assumed
//! absent. Imported jobs are paused/read-only: nothing can resume them.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

pub const REPORT_SCHEMA: u32 = 1;
const SOURCE_LIMIT: u64 = 100 * 1024 * 1024;
const ITEM_LIMIT: usize = 200_000;

type JobRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
);
type ItemRow = (
    i64,
    i64,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
    String,
);

pub struct LegacySource {
    pub database: Vec<u8>,
    pub wal: Option<Vec<u8>>,
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|_| "LEGACY_JOBS_IO")?;
    let meta = file.metadata().map_err(|_| "LEGACY_JOBS_IO")?;
    if !meta.is_file() {
        return Err("LEGACY_JOBS_NOT_REGULAR_FILE".into());
    }
    if meta.len() > SOURCE_LIMIT {
        return Err(
            "LEGACY_JOBS_TOO_LARGE: legacy job databases over 100 MiB are not imported".into(),
        );
    }
    let mut bytes = Vec::new();
    file.take(SOURCE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "LEGACY_JOBS_IO")?;
    Ok(bytes)
}

/// Read the legacy database (and `-wal` file) without writing next to it.
pub fn read_source(path: &Path) -> Result<LegacySource, String> {
    let database = read_bounded(path)?;
    if !database.starts_with(b"SQLite format 3\0") {
        return Err("LEGACY_JOBS_NOT_SQLITE".into());
    }
    let mut wal_path = path.as_os_str().to_owned();
    wal_path.push("-wal");
    let wal = match std::fs::symlink_metadata(&wal_path) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => Some(read_bounded(Path::new(&wal_path))?),
        _ => None,
    };
    Ok(LegacySource { database, wal })
}

fn job_state(status: &str) -> (&'static str, bool) {
    match status {
        "queued" => ("paused", false),
        "running" | "pausing" | "paused" => ("paused", false),
        "completed" => ("completed", false),
        "failed" => ("failed", false),
        "cancelled" => ("cancelled", false),
        "rolled_back" => ("rolled_back", false),
        "rolling_back" | "rollback_paused" | "rollback_partial" => ("rollback_incomplete", true),
        _ => ("unsupported", true),
    }
}

fn item_state(status: &str) -> (&'static str, &'static str, bool) {
    match status {
        "pending" => ("pending", "LEGACY_ITEM_NEVER_STARTED", false),
        // Legacy recovery returned processing to pending: no collection write.
        "processing" => ("pending", "LEGACY_ITEM_INTERRUPTED_BEFORE_COMMIT", false),
        "processed" => (
            "prepared_not_applied",
            "LEGACY_ITEM_ARTIFACT_NOT_IMPORTED",
            false,
        ),
        "committing" => ("requires_review", "LEGACY_COMMIT_OUTCOME_UNKNOWN", true),
        "completed" => ("applied_historical", "LEGACY_ITEM_APPLIED", false),
        "failed" => ("failed", "LEGACY_ITEM_FAILED", false),
        "skipped" => ("skipped", "LEGACY_ITEM_SKIPPED", false),
        "reverted" => ("reverted", "LEGACY_ITEM_REVERTED", false),
        "rollback_failed" => ("requires_review", "LEGACY_ROLLBACK_OUTCOME_UNKNOWN", true),
        _ => ("unsupported", "LEGACY_ITEM_STATE_UNKNOWN", true),
    }
}

/// Translate a copy of the legacy database into an import report.
pub fn translate(source: &LegacySource, scratch_parent: &Path) -> Result<Value, String> {
    let scratch = linguist_provider::process::TempDir::create_in(scratch_parent)
        .map_err(|_| "LEGACY_JOBS_SCRATCH_IO")?;
    let copy = scratch.0.join("legacy.sqlite3");
    linguist_provider::process::write_private(&copy, &source.database)
        .map_err(|_| "LEGACY_JOBS_SCRATCH_IO")?;
    if let Some(wal) = &source.wal {
        linguist_provider::process::write_private(&scratch.0.join("legacy.sqlite3-wal"), wal)
            .map_err(|_| "LEGACY_JOBS_SCRATCH_IO")?;
    }
    let db = rusqlite::Connection::open_with_flags(
        &copy,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "LEGACY_JOBS_NOT_SQLITE")?;
    db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "LEGACY_JOBS_NOT_SQLITE")?;
    let check: String = db
        .pragma_query_value(None, "quick_check", |r| r.get(0))
        .map_err(|_| "LEGACY_JOBS_NOT_SQLITE")?;
    if check != "ok" {
        return Err("LEGACY_JOBS_INTEGRITY_FAILED".into());
    }
    let has = |table: &str, columns: &[&str]| -> Result<bool, String> {
        let mut statement = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| e.to_string())?;
        let present: Vec<String> = statement
            .query_map([], |r| r.get(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        Ok(columns.iter().all(|c| present.iter().any(|p| p == c)))
    };
    let job_columns = [
        "id",
        "deck_key",
        "deck_name",
        "status",
        "dry_run",
        "settings_json",
        "created_at",
        "updated_at",
        "started_at",
        "finished_at",
        "last_error",
    ];
    let item_columns = [
        "job_id",
        "ordinal",
        "note_id",
        "word",
        "status",
        "attempts",
        "artifact_path",
        "snapshot_id",
        "result_note_id",
        "last_error",
    ];
    if !has("batch_jobs", &job_columns)? || !has("batch_items", &item_columns)? {
        return Err("LEGACY_JOBS_SCHEMA_UNSUPPORTED: batch_jobs/batch_items with the legacy columns are required".into());
    }
    let migrations: Vec<Value> = if has("schema_migrations", &["version", "name"])? {
        let mut statement = db
            .prepare("SELECT version,name FROM schema_migrations ORDER BY version")
            .map_err(|e| e.to_string())?;
        statement
            .query_map([], |r| {
                Ok(json!({"version": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?}))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let mut statement = db
        .prepare("SELECT id,deck_key,deck_name,status,dry_run,settings_json,created_at,updated_at,started_at,finished_at,last_error FROM batch_jobs ORDER BY created_at,id")
        .map_err(|e| e.to_string())?;
    let jobs: Vec<JobRow> = statement
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
            ))
        })
        .map_err(|e| format!("LEGACY_JOBS_ROW_INVALID: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("LEGACY_JOBS_ROW_INVALID: {e}"))?;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut review = Vec::new();
    let mut unsupported = Vec::new();
    let mut translated = Vec::new();
    let mut total_items = 0usize;
    for (
        id,
        deck_key,
        deck_name,
        status,
        dry_run,
        settings_json,
        created_at,
        updated_at,
        started_at,
        finished_at,
        last_error,
    ) in jobs
    {
        let (state, job_review) = job_state(&status);
        let settings: Value = serde_json::from_str(&settings_json).unwrap_or_else(|_| {
            unsupported.push(json!({"job": id, "code": "LEGACY_JOB_SETTINGS_INVALID"}));
            Value::Null
        });
        let mut items_statement = db
            .prepare("SELECT ordinal,note_id,word,status,attempts,artifact_path,snapshot_id,result_note_id,last_error FROM batch_items WHERE job_id=?1 ORDER BY ordinal,id")
            .map_err(|e| e.to_string())?;
        let rows: Vec<ItemRow> = items_statement
            .query_map([&id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            })
            .map_err(|e| format!("LEGACY_JOBS_ROW_INVALID: {e}"))?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("LEGACY_JOBS_ROW_INVALID: {e}"))?;
        total_items += rows.len();
        if total_items > ITEM_LIMIT {
            return Err("LEGACY_JOBS_TOO_MANY_ITEMS".into());
        }
        let mut items = Vec::new();
        for (
            ordinal,
            note_id,
            word,
            item_status,
            attempts,
            artifact,
            snapshot,
            result_note,
            error,
        ) in rows
        {
            let (v2, code, needs_review) = item_state(&item_status);
            *counts.entry(format!("item:{v2}")).or_insert(0) += 1;
            let item = json!({
                "ordinal": ordinal,
                "note_id": note_id.to_string(),
                "word": word,
                "legacy_status": item_status,
                "state": v2,
                "code": code,
                "attempts": attempts,
                "legacy_snapshot_id": snapshot,
                "result_note_id": result_note.map(|n| n.to_string()),
                "legacy_artifact_path": artifact,
                "artifact_imported": false,
                "last_error": error,
            });
            if needs_review {
                review.push(json!({
                    "job": id,
                    "ordinal": ordinal,
                    "note_id": note_id.to_string(),
                    "code": code,
                    "detail": if code == "LEGACY_COMMIT_OUTCOME_UNKNOWN" {
                        "the legacy commit may have reached Anki; inspect the note and its legacy snapshot before any new write"
                    } else {
                        "outcome unknown; review the note before any new write"
                    },
                    "dry_run_job": dry_run != 0,
                }));
            }
            if v2 == "unsupported" {
                unsupported.push(json!({"job": id, "ordinal": ordinal, "code": code, "legacy_status": item_status}));
            }
            items.push(item);
        }
        *counts.entry(format!("job:{state}")).or_insert(0) += 1;
        if state == "unsupported" {
            unsupported.push(
                json!({"job": id, "code": "LEGACY_JOB_STATE_UNKNOWN", "legacy_status": status}),
            );
        }
        translated.push(json!({
            "legacy_id": id,
            "deck_key": deck_key,
            "deck_name": deck_name,
            "legacy_status": status,
            "state": state,
            "requires_review": job_review || items.iter().any(|i| i["state"] == "requires_review"),
            "runnable": false,
            "read_only_reason": "imported legacy history: settings, collection identity and approval are not validated, and legacy artifacts are not imported",
            "legacy_dry_run": dry_run != 0,
            "legacy_settings": settings,
            "created_at": created_at,
            "updated_at": updated_at,
            "started_at": started_at,
            "finished_at": finished_at,
            "last_error": last_error,
            "items": items,
        }));
    }
    Ok(json!({
        "schema_version": REPORT_SCHEMA,
        "source": {
            "database_sha256": linguist_core::canonical::asset_digest(&source.database),
            "database_bytes": source.database.len(),
            "wal_sha256": source.wal.as_deref().map(linguist_core::canonical::asset_digest),
            "wal_bytes": source.wal.as_ref().map(Vec::len),
            "schema_migrations": migrations,
        },
        "jobs": translated,
        "counts": counts,
        "requires_review": review,
        "unsupported": unsupported,
        "dispatches": false,
        "resumable": false,
        "next": "inspect requires_review items in Anki; new work uses jobs create from a reviewed plan",
    }))
}
