//! Immutable restore intents and receipts, and grammar split executions with
//! their append-only child attempts. Storing any record never calls Anki,
//! dispatches an effect or authorizes a write.
use crate::{Result, Store, sql};
use linguist_core::{
    canonical,
    records::{OperationState, StepState},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS restore_operations(
 operation TEXT PRIMARY KEY, target_operation TEXT NOT NULL, target_snapshot TEXT NOT NULL REFERENCES snapshots(id),
 created_ms INTEGER NOT NULL CHECK(created_ms>=0), body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS restore_operations_target ON restore_operations(target_operation,created_ms);
CREATE TABLE IF NOT EXISTS restore_receipts(
 operation TEXT PRIMARY KEY REFERENCES restore_operations(operation), target_operation TEXT NOT NULL,
 observed_asset TEXT NOT NULL REFERENCES assets(digest), evidence_asset TEXT NOT NULL REFERENCES assets(digest),
 body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS restore_receipts_target ON restore_receipts(target_operation);
CREATE TABLE IF NOT EXISTS split_executions(
 execution TEXT PRIMARY KEY, plan_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
 grammar_group TEXT NOT NULL, created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 body_digest TEXT NOT NULL, body BLOB NOT NULL, UNIQUE(plan_id,revision,grammar_group));
CREATE TABLE IF NOT EXISTS split_attempts(
 execution TEXT NOT NULL REFERENCES split_executions(execution), document TEXT NOT NULL,
 sequence INTEGER NOT NULL CHECK(sequence>0), operation TEXT NOT NULL UNIQUE,
 PRIMARY KEY(execution,document,sequence));
CREATE TRIGGER IF NOT EXISTS restore_operations_no_update BEFORE UPDATE ON restore_operations BEGIN SELECT RAISE(ABORT,'restore operations are immutable'); END;
CREATE TRIGGER IF NOT EXISTS restore_operations_no_delete BEFORE DELETE ON restore_operations BEGIN SELECT RAISE(ABORT,'restore operation retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS restore_receipts_no_update BEFORE UPDATE ON restore_receipts BEGIN SELECT RAISE(ABORT,'restore receipts are immutable'); END;
CREATE TRIGGER IF NOT EXISTS restore_receipts_no_delete BEFORE DELETE ON restore_receipts BEGIN SELECT RAISE(ABORT,'restore receipt retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS split_executions_no_update BEFORE UPDATE ON split_executions BEGIN SELECT RAISE(ABORT,'split executions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS split_executions_no_delete BEFORE DELETE ON split_executions BEGIN SELECT RAISE(ABORT,'split execution retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS split_attempts_no_update BEFORE UPDATE ON split_attempts BEGIN SELECT RAISE(ABORT,'split attempts are immutable'); END;
CREATE TRIGGER IF NOT EXISTS split_attempts_no_delete BEFORE DELETE ON split_attempts BEGIN SELECT RAISE(ABORT,'split attempt retention requires explicit migration'); END;
";

/// Frozen restore intent: the observed-state-bound decision and the exact
/// reverse effects. It is written before the restore journal and never updated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreOperationRecord {
    pub operation_id: Uuid,
    /// The apply operation whose snapshot is restored.
    pub target_operation: Uuid,
    pub target_snapshot: Uuid,
    pub created_ms: u64,
    pub decision: serde_json::Value,
    pub intent: serde_json::Value,
}

/// Verified restore result, saved before the restore journal commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreReceipt {
    pub schema_version: u16,
    pub restore_operation: Uuid,
    pub target_operation: Uuid,
    pub target_snapshot: Uuid,
    pub lineage_id: Uuid,
    pub session_epoch: Uuid,
    /// Asset holding the canonical observed projection of the restored state.
    pub observed_state_digest: String,
    /// Asset holding the raw read-back evidence.
    pub evidence_digest: String,
    pub note_id: Option<i64>,
    pub deleted_note_ids: Vec<i64>,
    pub kept_card_ids: Vec<i64>,
    pub removed_card_ids: Vec<i64>,
    pub history_digest: String,
    pub restored_media: Vec<String>,
}

/// Stable identities of one grammar split group execution, allocated before
/// any write. The source snapshot is taken once and linked to every child.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitExecutionRecord {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub revision: u32,
    pub grammar_group: Uuid,
    pub anchor_document: Uuid,
    /// Non-anchor units in reviewed order; they are created first.
    pub children: Vec<Uuid>,
    pub source_snapshot: Uuid,
    pub created_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitAttempt {
    pub document: Uuid,
    pub sequence: u32,
    pub operation: Uuid,
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8], digest: &str, code: &str) -> Result<T> {
    if canonical::asset_digest(body) != digest {
        return Err(code.into());
    }
    canonical::parse(body).map_err(|_| code.into())
}

fn constraint(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| match e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            code.to_owned()
        }
        e => sql(e),
    }
}

fn raw_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Store {
    /// Create-new publication of a restore intent for an existing snapshot.
    pub fn publish_restore_operation(&mut self, record: &RestoreOperationRecord) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if record.operation_id.is_nil()
            || record.target_operation.is_nil()
            || record.operation_id == record.target_operation
            || !record.decision.is_object()
            || !record.intent.is_object()
        {
            return Err("RESTORE_OPERATION_INVALID".into());
        }
        let snapshot = self.snapshot(record.target_snapshot)?;
        if snapshot.snapshot.operation_id != record.target_operation {
            return Err("RESTORE_OPERATION_INVALID".into());
        }
        let body = canonical::bytes(record).map_err(|e| e.to_string())?;
        let created = i64::try_from(record.created_ms).map_err(|_| "RESTORE_OPERATION_INVALID")?;
        self.connection
            .execute(
                "INSERT INTO restore_operations(operation,target_operation,target_snapshot,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    record.operation_id.to_string(),
                    record.target_operation.to_string(),
                    record.target_snapshot.to_string(),
                    created,
                    canonical::asset_digest(&body),
                    body
                ],
            )
            .map_err(constraint("RESTORE_OPERATION_CONFLICT"))?;
        Ok(())
    }

    pub fn restore_operation(&self, operation_id: Uuid) -> Result<RestoreOperationRecord> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM restore_operations WHERE operation=?1",
                [operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("RESTORE_OPERATION_NOT_FOUND")?;
        let record: RestoreOperationRecord = decode(&body, &digest, "RESTORE_OPERATION_CORRUPT")?;
        if record.operation_id != operation_id {
            return Err("RESTORE_OPERATION_CORRUPT".into());
        }
        Ok(record)
    }

    /// Every restore intent recorded for one apply operation, oldest first.
    pub fn restore_operations_for(&self, target: Uuid) -> Result<Vec<RestoreOperationRecord>> {
        let mut statement = self
            .connection
            .prepare("SELECT operation FROM restore_operations WHERE target_operation=?1 ORDER BY created_ms,operation")
            .map_err(sql)?;
        let ids: Vec<String> = statement
            .query_map([target.to_string()], |r| r.get(0))
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        ids.iter()
            .map(|id| {
                self.restore_operation(
                    Uuid::parse_str(id).map_err(|_| "RESTORE_OPERATION_CORRUPT")?,
                )
            })
            .collect()
    }

    /// Retain a verified restore result. The restore journal must have every
    /// step verified, and the observed digest must equal the last step's
    /// expected post-state. This does not commit the journal.
    pub fn append_restore_receipt(&mut self, receipt: &RestoreReceipt) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        self.check_restore_receipt(receipt)?;
        let body = canonical::bytes(receipt).map_err(|e| e.to_string())?;
        self.connection
            .execute(
                "INSERT INTO restore_receipts(operation,target_operation,observed_asset,evidence_asset,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    receipt.restore_operation.to_string(),
                    receipt.target_operation.to_string(),
                    receipt.observed_state_digest,
                    receipt.evidence_digest,
                    canonical::asset_digest(&body),
                    body
                ],
            )
            .map_err(constraint("RESTORE_RECEIPT_CONFLICT"))?;
        Ok(())
    }

    fn check_restore_receipt(&self, receipt: &RestoreReceipt) -> Result<()> {
        let record = self.restore_operation(receipt.restore_operation)?;
        let journal = self.journal(receipt.restore_operation)?.journal;
        let last = journal.steps.last().ok_or("RESTORE_RECEIPT_CONFLICT")?;
        if receipt.schema_version != 1
            || receipt.target_operation != record.target_operation
            || receipt.target_snapshot != record.target_snapshot
            || receipt.lineage_id != journal.binding.lineage_id
            || receipt.session_epoch != journal.binding.session_epoch
            || !raw_digest(&receipt.observed_state_digest)
            || !raw_digest(&receipt.evidence_digest)
            || journal.steps.iter().any(|s| s.state != StepState::Verified)
            || last.expected_post_digest != receipt.observed_state_digest
        {
            return Err("RESTORE_RECEIPT_CONFLICT".into());
        }
        self.asset(&receipt.observed_state_digest, 100 * 1024 * 1024)?;
        self.asset(&receipt.evidence_digest, 100 * 1024 * 1024)?;
        Ok(())
    }

    pub fn restore_receipt(&self, operation: Uuid) -> Result<Option<RestoreReceipt>> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM restore_receipts WHERE operation=?1",
                [operation.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((digest, body)) = row else {
            return Ok(None);
        };
        let receipt: RestoreReceipt = decode(&body, &digest, "RESTORE_RECEIPT_CORRUPT")?;
        if receipt.restore_operation != operation {
            return Err("RESTORE_RECEIPT_CORRUPT".into());
        }
        self.check_restore_receipt(&receipt)
            .map_err(|_| "RESTORE_RECEIPT_CORRUPT")?;
        Ok(Some(receipt))
    }

    /// True when a committed restore journal with a verified receipt names
    /// this apply operation as its target.
    pub(crate) fn has_committed_restore(tx: &rusqlite::Connection, target: Uuid) -> Result<bool> {
        let mut statement = tx
            .prepare("SELECT r.operation FROM restore_receipts r WHERE r.target_operation=?1")
            .map_err(sql)?;
        let ids: Vec<String> = statement
            .query_map([target.to_string()], |r| r.get(0))
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        for id in ids {
            let state: Option<String> = tx
                .query_row(
                    "SELECT e.state FROM journal_events e JOIN journal_heads h ON h.operation=e.operation AND h.sequence=e.sequence WHERE e.operation=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if state.as_deref() == Some("committed") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Create-new split execution with exactly one first attempt per unit,
    /// in one transaction, before any collection write.
    pub fn publish_split_execution(
        &mut self,
        record: &SplitExecutionRecord,
        first_attempts: &[(Uuid, Uuid)],
    ) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let mut units: Vec<Uuid> = record.children.clone();
        units.push(record.anchor_document);
        let distinct: std::collections::BTreeSet<_> = units.iter().collect();
        let operations: std::collections::BTreeSet<_> =
            first_attempts.iter().map(|(_, op)| op).collect();
        if record.execution_id.is_nil()
            || record.children.is_empty()
            || distinct.len() != units.len()
            || first_attempts.len() != units.len()
            || operations.len() != units.len()
            || first_attempts
                .iter()
                .any(|(doc, op)| op.is_nil() || !distinct.contains(doc))
        {
            return Err("SPLIT_EXECUTION_INVALID".into());
        }
        let plan = self.revision(record.plan_id, record.revision)?;
        let group = plan
            .grammar_groups
            .iter()
            .find(|g| g.id == record.grammar_group)
            .ok_or("SPLIT_EXECUTION_INVALID")?;
        let group_units: std::collections::BTreeSet<_> = group.units.iter().collect();
        if group.anchor_document != record.anchor_document || group_units != distinct {
            return Err("SPLIT_EXECUTION_INVALID".into());
        }
        if self.snapshot(record.source_snapshot)?.snapshot.operation_id != record.execution_id {
            return Err("SPLIT_EXECUTION_INVALID".into());
        }
        let body = canonical::bytes(record).map_err(|e| e.to_string())?;
        let created = i64::try_from(record.created_ms).map_err(|_| "SPLIT_EXECUTION_INVALID")?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute(
            "INSERT INTO split_executions(execution,plan_id,revision,grammar_group,created_ms,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                record.execution_id.to_string(),
                record.plan_id.to_string(),
                record.revision,
                record.grammar_group.to_string(),
                created,
                canonical::asset_digest(&body),
                body
            ],
        )
        .map_err(constraint("SPLIT_EXECUTION_CONFLICT"))?;
        for (document, operation) in first_attempts {
            tx.execute(
                "INSERT INTO split_attempts(execution,document,sequence,operation) VALUES(?1,?2,1,?3)",
                params![
                    record.execution_id.to_string(),
                    document.to_string(),
                    operation.to_string()
                ],
            )
            .map_err(constraint("SPLIT_EXECUTION_CONFLICT"))?;
        }
        tx.commit().map_err(sql)?;
        Ok(())
    }

    pub fn split_execution(&self, execution: Uuid) -> Result<SplitExecutionRecord> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT body_digest,body FROM split_executions WHERE execution=?1",
                [execution.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("SPLIT_EXECUTION_NOT_FOUND")?;
        let record: SplitExecutionRecord = decode(&body, &digest, "SPLIT_EXECUTION_CORRUPT")?;
        if record.execution_id != execution {
            return Err("SPLIT_EXECUTION_CORRUPT".into());
        }
        Ok(record)
    }

    pub fn split_execution_for(
        &self,
        plan_id: Uuid,
        revision: u32,
        grammar_group: Uuid,
    ) -> Result<Option<SplitExecutionRecord>> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT execution FROM split_executions WHERE plan_id=?1 AND revision=?2 AND grammar_group=?3",
                params![plan_id.to_string(), revision, grammar_group.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        id.map(|id| {
            self.split_execution(Uuid::parse_str(&id).map_err(|_| "SPLIT_EXECUTION_CORRUPT")?)
        })
        .transpose()
    }

    /// Every attempt of every unit, ordered by document then sequence.
    pub fn split_attempts(&self, execution: Uuid) -> Result<Vec<SplitAttempt>> {
        let mut statement = self
            .connection
            .prepare("SELECT document,sequence,operation FROM split_attempts WHERE execution=?1 ORDER BY document,sequence")
            .map_err(sql)?;
        let rows = statement
            .query_map([execution.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (document, sequence, operation) = row.map_err(sql)?;
            out.push(SplitAttempt {
                document: Uuid::parse_str(&document).map_err(|_| "SPLIT_EXECUTION_CORRUPT")?,
                sequence,
                operation: Uuid::parse_str(&operation).map_err(|_| "SPLIT_EXECUTION_CORRUPT")?,
            });
        }
        Ok(out)
    }

    /// Append a new attempt for one unit. The caller must already have proven
    /// that the previous attempt had no unresolved collection effect.
    pub fn append_split_attempt(
        &mut self,
        execution: Uuid,
        document: Uuid,
    ) -> Result<SplitAttempt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let record = self.split_execution(execution)?;
        if record.anchor_document != document && !record.children.contains(&document) {
            return Err("SPLIT_ATTEMPT_INVALID".into());
        }
        let last = self
            .split_attempts(execution)?
            .into_iter()
            .filter(|a| a.document == document)
            .map(|a| a.sequence)
            .max()
            .ok_or("SPLIT_EXECUTION_CORRUPT")?;
        let attempt = SplitAttempt {
            document,
            sequence: last.checked_add(1).ok_or("SPLIT_ATTEMPT_INVALID")?,
            operation: Uuid::new_v4(),
        };
        self.connection
            .execute(
                "INSERT INTO split_attempts(execution,document,sequence,operation) VALUES(?1,?2,?3,?4)",
                params![
                    execution.to_string(),
                    document.to_string(),
                    attempt.sequence,
                    attempt.operation.to_string()
                ],
            )
            .map_err(constraint("SPLIT_ATTEMPT_CONFLICT"))?;
        Ok(attempt)
    }

    /// Journals whose `group_id` names this group, ordered by operation ID.
    /// This scans the bounded journal head index.
    pub fn group_journals(
        &self,
        group: Uuid,
        limit: u32,
    ) -> Result<Vec<crate::journal::JournalVersion>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mut statement = self
            .connection
            .prepare("SELECT operation FROM journal_heads ORDER BY operation")
            .map_err(sql)?;
        let ids: Vec<String> = statement
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        let mut out = Vec::new();
        for id in ids {
            let version = self.journal(Uuid::parse_str(&id).map_err(|_| "JOURNAL_CORRUPT")?)?;
            if version.journal.group_id == Some(group) {
                if out.len() == limit as usize {
                    return Err("GROUP_JOURNAL_LIMIT".into());
                }
                out.push(version);
            }
        }
        Ok(out)
    }
}

/// Operation states a restore may finalize to.
pub fn restorable_state(state: OperationState) -> bool {
    matches!(
        state,
        OperationState::Committed | OperationState::NeedsRecovery
    )
}
