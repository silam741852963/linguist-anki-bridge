//! Immutable apply-operation intents and append-only session rebinding decisions.
//! Storing either record never calls Anki, dispatches an effect or authorizes a write.
use crate::{Result, Store, sql};
use linguist_core::{canonical, records::ResumeBindingDecision};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS apply_operations(
 operation TEXT PRIMARY KEY, plan_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
 item_id TEXT NOT NULL, created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS apply_operations_item ON apply_operations(plan_id,item_id,created_ms);
CREATE TABLE IF NOT EXISTS apply_binding_decisions(
 operation TEXT NOT NULL REFERENCES apply_operations(operation), sequence INTEGER NOT NULL CHECK(sequence>0),
 body_digest TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(operation,sequence));
CREATE TRIGGER IF NOT EXISTS apply_operations_no_update BEFORE UPDATE ON apply_operations BEGIN SELECT RAISE(ABORT,'apply operations are immutable'); END;
CREATE TRIGGER IF NOT EXISTS apply_operations_no_delete BEFORE DELETE ON apply_operations BEGIN SELECT RAISE(ABORT,'apply operation retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS apply_binding_decisions_no_update BEFORE UPDATE ON apply_binding_decisions BEGIN SELECT RAISE(ABORT,'binding decisions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS apply_binding_decisions_no_delete BEFORE DELETE ON apply_binding_decisions BEGIN SELECT RAISE(ABORT,'binding decision retention requires explicit migration'); END;
";

/// Frozen intent for one plan item. `intent` holds the application's desired
/// post-state, exact effect payloads and fresh pre-state evidence; it is written
/// before the journal and is never updated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyOperationRecord {
    pub operation_id: Uuid,
    pub plan_id: Uuid,
    pub revision: u32,
    pub item_id: Uuid,
    pub approval_id: Uuid,
    pub created_ms: u64,
    pub intent: serde_json::Value,
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8], digest: &str) -> Result<T> {
    if canonical::asset_digest(body) != digest {
        return Err("APPLY_OPERATION_CORRUPT".into());
    }
    canonical::parse(body).map_err(|_| "APPLY_OPERATION_CORRUPT".into())
}

impl Store {
    /// Create-new publication; an existing operation ID is a conflict.
    pub fn publish_apply_operation(&mut self, record: &ApplyOperationRecord) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if record.operation_id.is_nil()
            || record.plan_id.is_nil()
            || record.item_id.is_nil()
            || record.approval_id.is_nil()
            || record.revision == 0
            || !record.intent.is_object()
        {
            return Err("APPLY_OPERATION_INVALID".into());
        }
        // The approval must exist and cover this exact revision and item.
        let approval = self.approval(record.approval_id)?.approval;
        if approval.plan_id != record.plan_id
            || approval.revision != record.revision
            || !approval.item_ids.contains(&record.item_id)
        {
            return Err("APPLY_OPERATION_APPROVAL_CONFLICT".into());
        }
        let body = canonical::bytes(record).map_err(|e| e.to_string())?;
        let created = i64::try_from(record.created_ms).map_err(|_| "APPLY_OPERATION_INVALID")?;
        self.connection
            .execute(
                "INSERT INTO apply_operations(operation,plan_id,revision,item_id,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    record.operation_id.to_string(),
                    record.plan_id.to_string(),
                    record.revision,
                    record.item_id.to_string(),
                    created,
                    canonical::asset_digest(&body),
                    body
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    "APPLY_OPERATION_CONFLICT".to_owned()
                }
                e => sql(e),
            })?;
        Ok(())
    }

    pub fn apply_operation(&self, operation_id: Uuid) -> Result<ApplyOperationRecord> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM apply_operations WHERE operation=?1",
                [operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("APPLY_OPERATION_NOT_FOUND")?;
        let record: ApplyOperationRecord = decode(&body, &digest)?;
        if record.operation_id != operation_id {
            return Err("APPLY_OPERATION_CORRUPT".into());
        }
        Ok(record)
    }

    /// Every recorded operation for one plan item, oldest first.
    pub fn apply_operations_for_item(
        &self,
        plan_id: Uuid,
        item_id: Uuid,
    ) -> Result<Vec<ApplyOperationRecord>> {
        let mut statement = self
            .connection
            .prepare("SELECT operation FROM apply_operations WHERE plan_id=?1 AND item_id=?2 ORDER BY created_ms,operation")
            .map_err(sql)?;
        let ids = statement
            .query_map([plan_id.to_string(), item_id.to_string()], |r| {
                r.get::<_, String>(0)
            })
            .map_err(sql)?;
        ids.map(|id| {
            self.apply_operation(
                Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "APPLY_OPERATION_CORRUPT")?,
            )
        })
        .collect()
    }

    /// Append one explicit session-rebinding decision. The decision must start
    /// from the operation's current execution binding (the journal binding or
    /// the previous decision's new binding).
    pub fn append_binding_decision(&mut self, decision: &ResumeBindingDecision) -> Result<u32> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        decision.validate().map_err(|e| e.to_string())?;
        self.apply_operation(decision.operation_id)?;
        let journal = self.journal(decision.operation_id)?.journal;
        let previous = self.binding_decisions(decision.operation_id)?;
        let current = previous
            .last()
            .map(|d| &d.new_binding)
            .unwrap_or(&journal.binding);
        if decision.approval_digest != journal.approval_digest || &decision.old_binding != current {
            return Err("BINDING_DECISION_CONFLICT".into());
        }
        let sequence = u32::try_from(previous.len() + 1).map_err(|_| "BINDING_DECISION_INVALID")?;
        let body = canonical::bytes(decision).map_err(|e| e.to_string())?;
        self.connection
            .execute(
                "INSERT INTO apply_binding_decisions(operation,sequence,body_digest,body) VALUES(?1,?2,?3,?4)",
                params![
                    decision.operation_id.to_string(),
                    sequence,
                    canonical::asset_digest(&body),
                    body
                ],
            )
            .map_err(sql)?;
        Ok(sequence)
    }

    pub fn binding_decisions(&self, operation_id: Uuid) -> Result<Vec<ResumeBindingDecision>> {
        let mut statement = self
            .connection
            .prepare("SELECT sequence,body_digest,body FROM apply_binding_decisions WHERE operation=?1 ORDER BY sequence")
            .map_err(sql)?;
        let rows = statement
            .query_map([operation_id.to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for (index, row) in rows.enumerate() {
            let (sequence, digest, body) = row.map_err(sql)?;
            let decision: ResumeBindingDecision = decode(&body, &digest)?;
            if sequence as usize != index + 1
                || decision.operation_id != operation_id
                || decision.validate().is_err()
            {
                return Err("APPLY_OPERATION_CORRUPT".into());
            }
            out.push(decision);
        }
        Ok(out)
    }
}
