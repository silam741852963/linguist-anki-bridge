//! Generation-engine certifications (RI-06). A certification is immutable
//! evidence that one exact local engine identity (model digest, `/api/show`
//! manifest, transport profile and generation settings) passed the
//! parameter-support and input-preservation probes. It never authorizes a
//! different identity; a changed model, engine version or setting needs a new
//! certification.
use crate::{Result, Store, sql};
use linguist_core::canonical;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS engine_certifications(
 id TEXT PRIMARY KEY, identity_digest TEXT NOT NULL, engine_version TEXT NOT NULL,
 passed INTEGER NOT NULL CHECK(passed IN (0,1)), created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS engine_certifications_identity ON engine_certifications(identity_digest, created_ms);
CREATE TRIGGER IF NOT EXISTS engine_certifications_no_update BEFORE UPDATE ON engine_certifications BEGIN SELECT RAISE(ABORT,'engine certifications are immutable'); END;
CREATE TRIGGER IF NOT EXISTS engine_certifications_no_delete BEFORE DELETE ON engine_certifications BEGIN SELECT RAISE(ABORT,'engine certifications are retained'); END;
";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EngineCertificationRecord {
    pub id: Uuid,
    pub identity_digest: String,
    pub engine_version: String,
    pub passed: bool,
    pub created_ms: u64,
    pub body_digest: String,
    pub body: serde_json::Value,
}

impl Store {
    /// Record one certification run, passed or failed.
    pub fn record_engine_certification(
        &mut self,
        identity_digest: &str,
        engine_version: &str,
        passed: bool,
        created_ms: u64,
        body: &serde_json::Value,
    ) -> Result<EngineCertificationRecord> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if identity_digest.trim().is_empty() || engine_version.trim().is_empty() {
            return Err("ENGINE_CERTIFICATION_INVALID".into());
        }
        let bytes = canonical::bytes(body).map_err(|e| e.to_string())?;
        let body_digest = canonical::asset_digest(&bytes);
        let id = Uuid::new_v4();
        self.connection
            .execute(
                "INSERT INTO engine_certifications(id,identity_digest,engine_version,passed,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![id.to_string(), identity_digest, engine_version, passed as i64, created_ms as i64, body_digest, bytes],
            )
            .map_err(sql)?;
        Ok(EngineCertificationRecord {
            id,
            identity_digest: identity_digest.into(),
            engine_version: engine_version.into(),
            passed,
            created_ms,
            body_digest,
            body: body.clone(),
        })
    }

    /// The newest certification run for this identity, passed or not. A
    /// later failed run supersedes an earlier pass.
    pub fn latest_engine_certification(
        &self,
        identity_digest: &str,
    ) -> Result<Option<EngineCertificationRecord>> {
        let row: Option<(String, String, i64, i64, String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT id,engine_version,passed,created_ms,body_digest,body FROM engine_certifications WHERE identity_digest=?1 ORDER BY created_ms DESC, rowid DESC LIMIT 1",
                [identity_digest],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()
            .map_err(sql)?;
        row.map(
            |(id, engine_version, passed, created_ms, body_digest, body)| {
                if canonical::asset_digest(&body) != body_digest {
                    return Err("ENGINE_CERTIFICATION_CORRUPT".into());
                }
                Ok(EngineCertificationRecord {
                    id: Uuid::parse_str(&id).map_err(|_| "ENGINE_CERTIFICATION_CORRUPT")?,
                    identity_digest: identity_digest.into(),
                    engine_version,
                    passed: passed == 1,
                    created_ms: created_ms as u64,
                    body_digest,
                    body: serde_json::from_slice(&body)
                        .map_err(|_| "ENGINE_CERTIFICATION_CORRUPT")?,
                })
            },
        )
        .transpose()
    }
}
