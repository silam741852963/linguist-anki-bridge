//! Immutable verified-checkpoint receipts, later verification evidence and model
//! operation records. Storing a receipt never calls Anki or authorizes a write.
use crate::{Result, Store, sql};
use linguist_core::{canonical, records::BackupReceipt};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS checkpoint_receipts(
 id TEXT PRIMARY KEY, operation TEXT NOT NULL UNIQUE, created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 scope TEXT NOT NULL, scope_digest TEXT NOT NULL, body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS checkpoint_receipts_created ON checkpoint_receipts(created_ms,id);
CREATE TABLE IF NOT EXISTS checkpoint_verifications(
 receipt TEXT NOT NULL REFERENCES checkpoint_receipts(id), sequence INTEGER NOT NULL CHECK(sequence>0),
 created_ms INTEGER NOT NULL CHECK(created_ms>=0), body_digest TEXT NOT NULL, body BLOB NOT NULL,
 PRIMARY KEY(receipt,sequence));
CREATE TABLE IF NOT EXISTS model_operations(
 operation TEXT PRIMARY KEY, model_name TEXT NOT NULL, manifest_digest TEXT NOT NULL,
 created_ms INTEGER NOT NULL CHECK(created_ms>=0), body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS model_operations_name ON model_operations(model_name,created_ms);
CREATE TRIGGER IF NOT EXISTS checkpoint_receipts_no_update BEFORE UPDATE ON checkpoint_receipts BEGIN SELECT RAISE(ABORT,'checkpoint receipts are immutable'); END;
CREATE TRIGGER IF NOT EXISTS checkpoint_receipts_no_delete BEFORE DELETE ON checkpoint_receipts BEGIN SELECT RAISE(ABORT,'checkpoint retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS checkpoint_verifications_no_update BEFORE UPDATE ON checkpoint_verifications BEGIN SELECT RAISE(ABORT,'checkpoint verifications are immutable'); END;
CREATE TRIGGER IF NOT EXISTS checkpoint_verifications_no_delete BEFORE DELETE ON checkpoint_verifications BEGIN SELECT RAISE(ABORT,'checkpoint verification retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS model_operations_no_update BEFORE UPDATE ON model_operations BEGIN SELECT RAISE(ABORT,'model operations are immutable'); END;
CREATE TRIGGER IF NOT EXISTS model_operations_no_delete BEFORE DELETE ON model_operations BEGIN SELECT RAISE(ABORT,'model operation retention requires explicit migration'); END;
";

/// Receipt plus the evidence it summarizes. `evidence` holds the application's
/// scope manifest, package inspection, scope report and restoration test.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRecord {
    pub receipt: BackupReceipt,
    pub operation_id: Uuid,
    pub created_ms: u64,
    pub scope: String,
    pub size_bytes: u64,
    pub evidence: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointVerification {
    pub receipt_id: Uuid,
    pub sequence: u32,
    pub created_ms: u64,
    pub evidence: serde_json::Value,
}

/// Pre-effect evidence for one model installation operation. It is the journal's
/// snapshot for model operations; it never stores note content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOperationRecord {
    pub operation_id: Uuid,
    pub model_name: String,
    pub manifest_digest: String,
    pub created_ms: u64,
    pub evidence: serde_json::Value,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate(record: &CheckpointRecord) -> Result<()> {
    let receipt = &record.receipt;
    if receipt.id.is_nil()
        || record.operation_id.is_nil()
        || !std::path::Path::new(&receipt.path).is_absolute()
        || !hex64(&receipt.checksum)
        || receipt.scope_digest.is_empty()
        || receipt.verification_digest.is_empty()
        || receipt
            .restoration_evidence
            .as_deref()
            .is_none_or(str::is_empty)
        || record.size_bytes == 0
        || !matches!(record.scope.as_str(), "collection")
        || !record.evidence.is_object()
    {
        return Err("CHECKPOINT_RECORD_INVALID".into());
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8], digest: &str) -> Result<T> {
    if canonical::asset_digest(body) != digest {
        return Err("CHECKPOINT_RECORD_CORRUPT".into());
    }
    canonical::parse(body).map_err(|_| "CHECKPOINT_RECORD_CORRUPT".into())
}

impl Store {
    /// Create-new publication; an existing receipt ID or operation is a conflict.
    pub fn publish_checkpoint(&mut self, record: &CheckpointRecord) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        validate(record)?;
        let body = canonical::bytes(record).map_err(|e| e.to_string())?;
        let digest = canonical::asset_digest(&body);
        let created = i64::try_from(record.created_ms).map_err(|_| "CHECKPOINT_RECORD_INVALID")?;
        self.connection
            .execute(
                "INSERT INTO checkpoint_receipts(id,operation,created_ms,scope,scope_digest,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    record.receipt.id.to_string(),
                    record.operation_id.to_string(),
                    created,
                    record.scope,
                    record.receipt.scope_digest,
                    digest,
                    body
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    "CHECKPOINT_RECEIPT_CONFLICT".to_owned()
                }
                e => sql(e),
            })?;
        Ok(())
    }

    pub fn checkpoint(&self, id: Uuid) -> Result<CheckpointRecord> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM checkpoint_receipts WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("CHECKPOINT_NOT_FOUND")?;
        let record: CheckpointRecord = decode(&body, &digest)?;
        if record.receipt.id != id || validate(&record).is_err() {
            return Err("CHECKPOINT_RECORD_CORRUPT".into());
        }
        Ok(record)
    }

    /// Newest first, bounded. Filters apply to indexed columns only.
    pub fn list_checkpoints(
        &self,
        scope: Option<&str>,
        since_ms: Option<u64>,
        limit: u32,
    ) -> Result<Vec<CheckpointRecord>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let since = i64::try_from(since_ms.unwrap_or(0)).map_err(|_| "INVALID_SINCE")?;
        let mut statement = self
            .connection
            .prepare("SELECT id FROM checkpoint_receipts WHERE created_ms>=?1 AND (?2 IS NULL OR scope=?2) ORDER BY created_ms DESC,id LIMIT ?3")
            .map_err(sql)?;
        let ids = statement
            .query_map(params![since, scope, limit], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        ids.map(|id| {
            self.checkpoint(
                Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?,
            )
        })
        .collect()
    }

    /// Append-only later verification (for example a disposable restoration test).
    pub fn append_checkpoint_verification(
        &mut self,
        receipt_id: Uuid,
        created_ms: u64,
        evidence: &serde_json::Value,
    ) -> Result<CheckpointVerification> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        self.checkpoint(receipt_id)?;
        if !evidence.is_object() {
            return Err("CHECKPOINT_VERIFICATION_INVALID".into());
        }
        let created = i64::try_from(created_ms).map_err(|_| "CHECKPOINT_VERIFICATION_INVALID")?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let last: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM checkpoint_verifications WHERE receipt=?1",
                [receipt_id.to_string()],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let sequence = u32::try_from(last + 1).map_err(|_| "CHECKPOINT_VERIFICATION_EXHAUSTED")?;
        let verification = CheckpointVerification {
            receipt_id,
            sequence,
            created_ms,
            evidence: evidence.clone(),
        };
        let body = canonical::bytes(&verification).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO checkpoint_verifications(receipt,sequence,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5)",
            params![receipt_id.to_string(), sequence, created, canonical::asset_digest(&body), body],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(verification)
    }

    pub fn checkpoint_verifications(
        &self,
        receipt_id: Uuid,
    ) -> Result<Vec<CheckpointVerification>> {
        let mut statement = self
            .connection
            .prepare("SELECT body_digest,body FROM checkpoint_verifications WHERE receipt=?1 ORDER BY sequence")
            .map_err(sql)?;
        let rows = statement
            .query_map([receipt_id.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(sql)?;
        rows.map(|row| {
            let (digest, body) = row.map_err(sql)?;
            let verification: CheckpointVerification = decode(&body, &digest)?;
            if verification.receipt_id != receipt_id {
                return Err("CHECKPOINT_RECORD_CORRUPT".into());
            }
            Ok(verification)
        })
        .collect()
    }

    pub fn publish_model_operation(&mut self, record: &ModelOperationRecord) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if record.operation_id.is_nil()
            || record.model_name.trim().is_empty()
            || !hex64(&record.manifest_digest)
            || !record.evidence.is_object()
        {
            return Err("MODEL_OPERATION_INVALID".into());
        }
        let body = canonical::bytes(record).map_err(|e| e.to_string())?;
        let created = i64::try_from(record.created_ms).map_err(|_| "MODEL_OPERATION_INVALID")?;
        self.connection
            .execute(
                "INSERT INTO model_operations(operation,model_name,manifest_digest,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    record.operation_id.to_string(),
                    record.model_name,
                    record.manifest_digest,
                    created,
                    canonical::asset_digest(&body),
                    body
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    "MODEL_OPERATION_CONFLICT".to_owned()
                }
                e => sql(e),
            })?;
        Ok(())
    }

    pub fn model_operation(&self, operation_id: Uuid) -> Result<ModelOperationRecord> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM model_operations WHERE operation=?1",
                [operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("MODEL_OPERATION_NOT_FOUND")?;
        let record: ModelOperationRecord = decode(&body, &digest)?;
        if record.operation_id != operation_id {
            return Err("CHECKPOINT_RECORD_CORRUPT".into());
        }
        Ok(record)
    }

    /// Every recorded operation for an exact model name, oldest first.
    pub fn model_operations_named(&self, name: &str) -> Result<Vec<ModelOperationRecord>> {
        let mut statement = self
            .connection
            .prepare("SELECT operation FROM model_operations WHERE model_name=?1 ORDER BY created_ms,operation")
            .map_err(sql)?;
        let ids = statement
            .query_map([name], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        ids.map(|id| {
            self.model_operation(
                Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?,
            )
        })
        .collect()
    }
}
