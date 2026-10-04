//! Imported legacy batch-job records (OP-47). They are read-only history:
//! nothing here can dispatch work, and the original database bytes are kept as
//! an asset so the import can be re-audited.
use crate::{Result, Store, sql};
use linguist_core::canonical;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS legacy_job_imports(
 id TEXT PRIMARY KEY, source_sha256 TEXT NOT NULL UNIQUE REFERENCES assets(digest),
 imported_ms INTEGER NOT NULL CHECK(imported_ms>=0), report_digest TEXT NOT NULL, report BLOB NOT NULL);
CREATE TRIGGER IF NOT EXISTS legacy_job_imports_no_update BEFORE UPDATE ON legacy_job_imports BEGIN SELECT RAISE(ABORT,'legacy job imports are immutable'); END;
CREATE TRIGGER IF NOT EXISTS legacy_job_imports_no_delete BEFORE DELETE ON legacy_job_imports BEGIN SELECT RAISE(ABORT,'legacy job imports are retained'); END;
";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LegacyImport {
    pub id: Uuid,
    pub source_sha256: String,
    pub imported_ms: u64,
    pub report_digest: String,
    pub report: serde_json::Value,
}

impl Store {
    /// Retain the original legacy database bytes and the translated report.
    /// Importing the same bytes again returns the existing record unchanged.
    pub fn record_legacy_job_import(
        &mut self,
        source: &[u8],
        report: &serde_json::Value,
        imported_ms: u64,
    ) -> Result<(LegacyImport, bool)> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let digest = canonical::asset_digest(source);
        if let Some(existing) = self.legacy_job_import_by_source(&digest)? {
            return Ok((existing, false));
        }
        self.publish_asset(source, 100 * 1024 * 1024)?;
        let body = canonical::bytes(report).map_err(|e| e.to_string())?;
        let report_digest = canonical::asset_digest(&body);
        let id = Uuid::new_v4();
        self.connection
            .execute(
                "INSERT INTO legacy_job_imports(id,source_sha256,imported_ms,report_digest,report) VALUES(?1,?2,?3,?4,?5)",
                params![id.to_string(), digest, imported_ms as i64, report_digest, body],
            )
            .map_err(sql)?;
        Ok((
            LegacyImport {
                id,
                source_sha256: digest,
                imported_ms,
                report_digest,
                report: report.clone(),
            },
            true,
        ))
    }

    fn legacy_job_import_by_source(&self, digest: &str) -> Result<Option<LegacyImport>> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM legacy_job_imports WHERE source_sha256=?1",
                [digest],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        id.map(|id| {
            self.legacy_job_import(Uuid::parse_str(&id).map_err(|_| "LEGACY_IMPORT_CORRUPT")?)
        })
        .transpose()
    }

    /// One imported record; the stored report digest is verified on read.
    pub fn legacy_job_import(&self, id: Uuid) -> Result<LegacyImport> {
        let row: Option<(String, i64, String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT source_sha256,imported_ms,report_digest,report FROM legacy_job_imports WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(sql)?;
        let (source_sha256, imported_ms, report_digest, body) =
            row.ok_or("LEGACY_IMPORT_NOT_FOUND")?;
        if canonical::asset_digest(&body) != report_digest {
            return Err("LEGACY_IMPORT_CORRUPT".into());
        }
        Ok(LegacyImport {
            id,
            source_sha256,
            imported_ms: imported_ms as u64,
            report_digest,
            report: serde_json::from_slice(&body).map_err(|_| "LEGACY_IMPORT_CORRUPT")?,
        })
    }

    /// Imported records, newest first, as (id, source digest, imported_ms).
    pub fn legacy_job_imports(&self, limit: u32) -> Result<Vec<(Uuid, String, u64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT id,source_sha256,imported_ms FROM legacy_job_imports ORDER BY imported_ms DESC,id LIMIT ?1")
            .map_err(sql)?;
        statement
            .query_map([limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(sql)?
            .map(|row| {
                let (id, digest, ms) = row.map_err(sql)?;
                Ok((
                    Uuid::parse_str(&id).map_err(|_| "LEGACY_IMPORT_CORRUPT")?,
                    digest,
                    ms as u64,
                ))
            })
            .collect()
    }
}
