//! Collection revamp campaigns (WP-24): an immutable definition, one mutable
//! row per batch (phase plus JSON working data, compare-and-set on a version),
//! the campaign's status, an append-only event history and the checkpoints the
//! campaign created. Storing a record never calls Anki or authorizes a write;
//! every effect is carried out by the jobs and plans the batch data names.
use crate::{Result, Store, sql};
use linguist_core::canonical;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS campaigns(
 id TEXT PRIMARY KEY, created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 definition_digest TEXT NOT NULL, definition BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS campaign_state(
 campaign TEXT PRIMARY KEY REFERENCES campaigns(id), status TEXT NOT NULL,
 code TEXT, detail TEXT, version INTEGER NOT NULL CHECK(version>0), updated_ms INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS campaign_batches(
 campaign TEXT NOT NULL REFERENCES campaigns(id), batch INTEGER NOT NULL CHECK(batch>0),
 cohort TEXT NOT NULL, phase TEXT NOT NULL, version INTEGER NOT NULL CHECK(version>0),
 data BLOB NOT NULL, updated_ms INTEGER NOT NULL, PRIMARY KEY(campaign,batch));
CREATE TABLE IF NOT EXISTS campaign_events(
 campaign TEXT NOT NULL REFERENCES campaigns(id), sequence INTEGER NOT NULL CHECK(sequence>0),
 at_ms INTEGER NOT NULL, batch INTEGER, kind TEXT NOT NULL, body BLOB NOT NULL,
 PRIMARY KEY(campaign,sequence));
CREATE INDEX IF NOT EXISTS campaign_events_kind ON campaign_events(campaign,kind,at_ms);
CREATE TABLE IF NOT EXISTS campaign_checkpoints(
 campaign TEXT NOT NULL REFERENCES campaigns(id), checkpoint TEXT NOT NULL, batch INTEGER NOT NULL,
 path TEXT NOT NULL, created_ms INTEGER NOT NULL, pruned_ms INTEGER, PRIMARY KEY(campaign,checkpoint));
CREATE TABLE IF NOT EXISTS merge_deletions(
 note_id TEXT PRIMARY KEY, survivor TEXT NOT NULL, plan TEXT NOT NULL, operation_id TEXT NOT NULL,
 approval_digest TEXT NOT NULL, expected_pre_digest TEXT NOT NULL, survivor_model TEXT NOT NULL,
 evidence_asset TEXT NOT NULL, state TEXT NOT NULL, restore_operation TEXT,
 created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS merge_deletions_no_delete BEFORE DELETE ON merge_deletions BEGIN SELECT RAISE(ABORT,'merge deletions are retained'); END;
CREATE TRIGGER IF NOT EXISTS campaigns_no_update BEFORE UPDATE ON campaigns BEGIN SELECT RAISE(ABORT,'campaign definitions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS campaigns_no_delete BEFORE DELETE ON campaigns BEGIN SELECT RAISE(ABORT,'campaign definitions are retained'); END;
CREATE TRIGGER IF NOT EXISTS campaign_events_no_update BEFORE UPDATE ON campaign_events BEGIN SELECT RAISE(ABORT,'campaign events are immutable'); END;
CREATE TRIGGER IF NOT EXISTS campaign_events_no_delete BEFORE DELETE ON campaign_events BEGIN SELECT RAISE(ABORT,'campaign events are retained'); END;
CREATE TRIGGER IF NOT EXISTS campaign_batches_no_delete BEFORE DELETE ON campaign_batches BEGIN SELECT RAISE(ABORT,'campaign batches are retained'); END;
";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CampaignRecord {
    pub id: Uuid,
    pub created_ms: u64,
    pub definition_digest: String,
    pub definition: serde_json::Value,
    pub status: String,
    pub code: Option<String>,
    pub detail: Option<String>,
    pub version: u64,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BatchRecord {
    pub batch: u32,
    pub cohort: String,
    pub phase: String,
    pub version: u64,
    pub data: serde_json::Value,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CampaignEvent {
    pub sequence: u64,
    pub at_ms: u64,
    pub batch: Option<u32>,
    pub kind: String,
    pub body: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CampaignCheckpoint {
    pub checkpoint: String,
    pub batch: u32,
    pub path: String,
    pub created_ms: u64,
    pub pruned_ms: Option<u64>,
}

/// One batch as frozen at creation.
pub struct NewBatch {
    pub batch: u32,
    pub cohort: String,
    pub phase: String,
    pub data: serde_json::Value,
}

fn json(bytes: Vec<u8>) -> Result<serde_json::Value> {
    serde_json::from_slice(&bytes).map_err(|_| "CAMPAIGN_RECORD_CORRUPT".into())
}

fn encode(value: &serde_json::Value) -> Result<Vec<u8>> {
    canonical::bytes(value).map_err(|e| e.to_string())
}

fn append(
    tx: &rusqlite::Transaction<'_>,
    campaign: Uuid,
    batch: Option<u32>,
    kind: &str,
    body: &serde_json::Value,
    now: u64,
) -> Result<u64> {
    let next: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM campaign_events WHERE campaign=?1",
            [campaign.to_string()],
            |r| r.get(0),
        )
        .map_err(sql)?;
    tx.execute(
        "INSERT INTO campaign_events(campaign,sequence,at_ms,batch,kind,body) VALUES(?1,?2,?3,?4,?5,?6)",
        params![campaign.to_string(), next, now as i64, batch, kind, encode(body)?],
    )
    .map_err(sql)?;
    Ok(next as u64)
}

impl Store {
    fn writable_check(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err("STORE_READ_ONLY".into())
        }
    }

    /// Store an immutable campaign definition with its frozen batches.
    pub fn create_campaign(
        &mut self,
        id: Uuid,
        definition: &serde_json::Value,
        batches: &[NewBatch],
        now: u64,
    ) -> Result<String> {
        self.writable_check()?;
        if id.is_nil() || batches.is_empty() {
            return Err("CAMPAIGN_INVALID".into());
        }
        let body = encode(definition)?;
        let digest = canonical::asset_digest(&body);
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute(
            "INSERT INTO campaigns(id,created_ms,definition_digest,definition) VALUES(?1,?2,?3,?4)",
            params![id.to_string(), now as i64, digest, body],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                "CAMPAIGN_EXISTS".to_owned()
            }
            e => sql(e),
        })?;
        tx.execute(
            "INSERT INTO campaign_state(campaign,status,code,detail,version,updated_ms) VALUES(?1,'running',NULL,NULL,1,?2)",
            params![id.to_string(), now as i64],
        )
        .map_err(sql)?;
        for batch in batches {
            tx.execute(
                "INSERT INTO campaign_batches(campaign,batch,cohort,phase,version,data,updated_ms) VALUES(?1,?2,?3,?4,1,?5,?6)",
                params![id.to_string(), batch.batch, batch.cohort, batch.phase, encode(&batch.data)?, now as i64],
            )
            .map_err(sql)?;
        }
        append(
            &tx,
            id,
            None,
            "created",
            &serde_json::json!({"definition_digest": digest, "batches": batches.len()}),
            now,
        )?;
        tx.commit().map_err(sql)?;
        Ok(digest)
    }

    pub fn campaign(&self, id: Uuid) -> Result<CampaignRecord> {
        type Row = (
            i64,
            String,
            Vec<u8>,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
        );
        let row: Option<Row> = self
            .connection
            .query_row(
                "SELECT c.created_ms,c.definition_digest,c.definition,s.status,s.code,s.detail,s.version,s.updated_ms FROM campaigns c JOIN campaign_state s ON s.campaign=c.id WHERE c.id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
            )
            .optional()
            .map_err(sql)?;
        let (created, digest, body, status, code, detail, version, updated) =
            row.ok_or("CAMPAIGN_NOT_FOUND")?;
        if canonical::asset_digest(&body) != digest {
            return Err("CAMPAIGN_RECORD_CORRUPT".into());
        }
        Ok(CampaignRecord {
            id,
            created_ms: created as u64,
            definition_digest: digest,
            definition: json(body)?,
            status,
            code,
            detail,
            version: version as u64,
            updated_ms: updated as u64,
        })
    }

    /// Campaign IDs, newest first.
    pub fn campaign_ids(&self) -> Result<Vec<Uuid>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM campaigns ORDER BY created_ms DESC, rowid DESC")
            .map_err(sql)?;
        statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .map(|r| {
                Uuid::parse_str(&r.map_err(sql)?).map_err(|_| "CAMPAIGN_RECORD_CORRUPT".into())
            })
            .collect()
    }

    pub fn campaign_batches(&self, id: Uuid) -> Result<Vec<BatchRecord>> {
        let mut statement = self
            .connection
            .prepare("SELECT batch,cohort,phase,version,data,updated_ms FROM campaign_batches WHERE campaign=?1 ORDER BY batch")
            .map_err(sql)?;
        let rows = statement
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|row| {
            let (batch, cohort, phase, version, data, updated) = row.map_err(sql)?;
            Ok(BatchRecord {
                batch,
                cohort,
                phase,
                version: version as u64,
                data: json(data)?,
                updated_ms: updated as u64,
            })
        })
        .collect()
    }

    pub fn campaign_batch(&self, id: Uuid, batch: u32) -> Result<BatchRecord> {
        self.campaign_batches(id)?
            .into_iter()
            .find(|b| b.batch == batch)
            .ok_or("CAMPAIGN_BATCH_NOT_FOUND".into())
    }

    /// Compare-and-set one batch: phase and data replace the row only when
    /// its version is still `expected_version`; the event is appended in the
    /// same transaction. Returns the new version.
    #[allow(clippy::too_many_arguments)]
    pub fn update_campaign_batch(
        &mut self,
        id: Uuid,
        batch: u32,
        expected_version: u64,
        phase: &str,
        data: &serde_json::Value,
        kind: &str,
        event: &serde_json::Value,
        now: u64,
    ) -> Result<u64> {
        self.writable_check()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let changed = tx
            .execute(
                "UPDATE campaign_batches SET phase=?4,data=?5,version=version+1,updated_ms=?6 WHERE campaign=?1 AND batch=?2 AND version=?3",
                params![id.to_string(), batch, expected_version as i64, phase, encode(data)?, now as i64],
            )
            .map_err(sql)?;
        if changed != 1 {
            return Err("CAMPAIGN_BATCH_CONFLICT".into());
        }
        append(&tx, id, Some(batch), kind, event, now)?;
        tx.commit().map_err(sql)?;
        Ok(expected_version + 1)
    }

    /// Split one batch (compare-and-set on its version): the row keeps
    /// `keep` and a new batch with the next free number takes `moved`, in
    /// the same cohort, both in `phase`. Returns the new batch number.
    #[allow(clippy::too_many_arguments)]
    pub fn split_campaign_batch(
        &mut self,
        id: Uuid,
        batch: u32,
        expected_version: u64,
        phase: &str,
        keep: &serde_json::Value,
        moved: &serde_json::Value,
        event: &serde_json::Value,
        now: u64,
    ) -> Result<u32> {
        self.move_to_new_batch(
            id,
            batch,
            expected_version,
            (phase, keep),
            (phase, moved),
            "split",
            event,
            now,
        )
    }

    /// Atomically update one batch and create the next batch from part of
    /// it, each with its own phase (`split`, or `requeued` notes deferred
    /// in a finished batch going round again).
    #[allow(clippy::too_many_arguments)]
    pub fn move_to_new_batch(
        &mut self,
        id: Uuid,
        batch: u32,
        expected_version: u64,
        (phase, keep): (&str, &serde_json::Value),
        (moved_phase, moved): (&str, &serde_json::Value),
        kind: &str,
        event: &serde_json::Value,
        now: u64,
    ) -> Result<u32> {
        self.writable_check()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let cohort: String = tx
            .query_row(
                "SELECT cohort FROM campaign_batches WHERE campaign=?1 AND batch=?2 AND version=?3",
                params![id.to_string(), batch, expected_version as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?
            .ok_or("CAMPAIGN_BATCH_CONFLICT")?;
        tx.execute(
            "UPDATE campaign_batches SET phase=?4,data=?5,version=version+1,updated_ms=?6 WHERE campaign=?1 AND batch=?2 AND version=?3",
            params![id.to_string(), batch, expected_version as i64, phase, encode(keep)?, now as i64],
        )
        .map_err(sql)?;
        let next: u32 = tx
            .query_row(
                "SELECT MAX(batch)+1 FROM campaign_batches WHERE campaign=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(sql)?;
        tx.execute(
            "INSERT INTO campaign_batches(campaign,batch,cohort,phase,version,data,updated_ms) VALUES(?1,?2,?3,?4,1,?5,?6)",
            params![id.to_string(), next, cohort, moved_phase, encode(moved)?, now as i64],
        )
        .map_err(sql)?;
        append(
            &tx,
            id,
            Some(batch),
            kind,
            &serde_json::json!({"new_batch": next, "detail": event}),
            now,
        )?;
        tx.commit().map_err(sql)?;
        Ok(next)
    }

    /// Compare-and-set the campaign status (running, paused, cancelled, closed).
    #[allow(clippy::too_many_arguments)]
    pub fn set_campaign_status(
        &mut self,
        id: Uuid,
        expected_version: u64,
        status: &str,
        code: Option<&str>,
        detail: Option<&str>,
        kind: &str,
        event: &serde_json::Value,
        now: u64,
    ) -> Result<u64> {
        self.writable_check()?;
        if !matches!(status, "running" | "paused" | "cancelled" | "closed") {
            return Err("CAMPAIGN_STATUS_INVALID".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let changed = tx
            .execute(
                "UPDATE campaign_state SET status=?3,code=?4,detail=?5,version=version+1,updated_ms=?6 WHERE campaign=?1 AND version=?2",
                params![id.to_string(), expected_version as i64, status, code, detail, now as i64],
            )
            .map_err(sql)?;
        if changed != 1 {
            return Err("CAMPAIGN_STATUS_CONFLICT".into());
        }
        append(&tx, id, None, kind, event, now)?;
        tx.commit().map_err(sql)?;
        Ok(expected_version + 1)
    }

    pub fn append_campaign_event(
        &mut self,
        id: Uuid,
        batch: Option<u32>,
        kind: &str,
        body: &serde_json::Value,
        now: u64,
    ) -> Result<u64> {
        self.writable_check()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM campaigns WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        exists.ok_or("CAMPAIGN_NOT_FOUND")?;
        let sequence = append(&tx, id, batch, kind, body, now)?;
        tx.commit().map_err(sql)?;
        Ok(sequence)
    }

    pub fn campaign_events(&self, id: Uuid, after: u64, limit: u32) -> Result<Vec<CampaignEvent>> {
        let mut statement = self
            .connection
            .prepare("SELECT sequence,at_ms,batch,kind,body FROM campaign_events WHERE campaign=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")
            .map_err(sql)?;
        let rows = statement
            .query_map(params![id.to_string(), after as i64, limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<u32>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })
            .map_err(sql)?;
        rows.map(|row| {
            let (sequence, at, batch, kind, body) = row.map_err(sql)?;
            Ok(CampaignEvent {
                sequence: sequence as u64,
                at_ms: at as u64,
                batch,
                kind,
                body: json(body)?,
            })
        })
        .collect()
    }

    /// The newest event of one kind (for example the last `tuned` limits).
    pub fn last_campaign_event(&self, id: Uuid, kind: &str) -> Result<Option<CampaignEvent>> {
        let row = self
            .connection
            .query_row(
                "SELECT sequence,at_ms,batch,kind,body FROM campaign_events WHERE campaign=?1 AND kind=?2 ORDER BY sequence DESC LIMIT 1",
                params![id.to_string(), kind],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<u32>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        row.map(|(sequence, at, batch, kind, body)| {
            Ok(CampaignEvent {
                sequence: sequence as u64,
                at_ms: at as u64,
                batch,
                kind,
                body: json(body)?,
            })
        })
        .transpose()
    }

    /// Events of one kind since `since_ms` (for example agent runs today).
    pub fn count_campaign_events(&self, id: Uuid, kind: &str, since_ms: u64) -> Result<u64> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM campaign_events WHERE campaign=?1 AND kind=?2 AND at_ms>=?3",
                params![id.to_string(), kind, since_ms as i64],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(sql)
    }

    /// Record a checkpoint this campaign created; recording twice is a no-op.
    pub fn record_campaign_checkpoint(
        &mut self,
        id: Uuid,
        checkpoint: &str,
        batch: u32,
        path: &str,
        now: u64,
    ) -> Result<()> {
        self.writable_check()?;
        self.connection
            .execute(
                "INSERT INTO campaign_checkpoints(campaign,checkpoint,batch,path,created_ms,pruned_ms) VALUES(?1,?2,?3,?4,?5,NULL) ON CONFLICT(campaign,checkpoint) DO NOTHING",
                params![id.to_string(), checkpoint, batch, path, now as i64],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub fn campaign_checkpoints(&self, id: Uuid) -> Result<Vec<CampaignCheckpoint>> {
        let mut statement = self
            .connection
            .prepare("SELECT checkpoint,batch,path,created_ms,pruned_ms FROM campaign_checkpoints WHERE campaign=?1 ORDER BY created_ms, rowid")
            .map_err(sql)?;
        let rows = statement
            .query_map([id.to_string()], |r| {
                Ok(CampaignCheckpoint {
                    checkpoint: r.get(0)?,
                    batch: r.get(1)?,
                    path: r.get(2)?,
                    created_ms: r.get::<_, i64>(3)? as u64,
                    pruned_ms: r.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                })
            })
            .map_err(sql)?;
        rows.map(|r| r.map_err(sql)).collect()
    }

    /// Mark a campaign checkpoint pruned after its file was removed.
    pub fn mark_campaign_checkpoint_pruned(
        &mut self,
        id: Uuid,
        checkpoint: &str,
        now: u64,
    ) -> Result<()> {
        self.writable_check()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let changed = tx
            .execute(
                "UPDATE campaign_checkpoints SET pruned_ms=?3 WHERE campaign=?1 AND checkpoint=?2 AND pruned_ms IS NULL",
                params![id.to_string(), checkpoint, now as i64],
            )
            .map_err(sql)?;
        if changed != 1 {
            return Err("CAMPAIGN_CHECKPOINT_NOT_FOUND".into());
        }
        append(
            &tx,
            id,
            None,
            "checkpoint_pruned",
            &serde_json::json!({"checkpoint": checkpoint}),
            now,
        )?;
        tx.commit().map_err(sql)?;
        Ok(())
    }
}

/// WP-24 duplicate merge: one deleted (or to-be-deleted) merged note. The
/// operation ID and the pre-delete evidence are recorded before the delete
/// is sent, so a crash is resolved from the companion ledger, and rollback
/// recreates the note from the evidence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MergeDeletion {
    pub note_id: String,
    pub survivor: String,
    pub plan: Uuid,
    pub operation_id: Uuid,
    pub approval_digest: String,
    pub expected_pre_digest: String,
    pub survivor_model: String,
    pub evidence_asset: String,
    /// intent, sent, deleted, restore_sent, restored.
    pub state: String,
    pub restore_operation: Option<Uuid>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

impl Store {
    pub fn record_merge_deletion(&mut self, record: &MergeDeletion) -> Result<()> {
        self.writable_check()?;
        self.connection
            .execute(
                "INSERT INTO merge_deletions(note_id,survivor,plan,operation_id,approval_digest,expected_pre_digest,survivor_model,evidence_asset,state,restore_operation,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![record.note_id, record.survivor, record.plan.to_string(), record.operation_id.to_string(),
                    record.approval_digest, record.expected_pre_digest, record.survivor_model, record.evidence_asset,
                    record.state, record.restore_operation.map(|u| u.to_string()), record.created_ms as i64, record.updated_ms as i64],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    "MERGE_DELETION_EXISTS".to_owned()
                }
                e => sql(e),
            })?;
        Ok(())
    }

    pub fn merge_deletion(&self, note_id: &str) -> Result<Option<MergeDeletion>> {
        self.connection
            .query_row(
                "SELECT note_id,survivor,plan,operation_id,approval_digest,expected_pre_digest,survivor_model,evidence_asset,state,restore_operation,created_ms,updated_ms FROM merge_deletions WHERE note_id=?1",
                [note_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?,
                        r.get::<_, String>(8)?, r.get::<_, Option<String>>(9)?, r.get::<_, i64>(10)?, r.get::<_, i64>(11)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?
            .map(|(note_id, survivor, plan, op, approval, pre, model, asset, state, restore, created, updated)| {
                let uuid = |s: &str| Uuid::parse_str(s).map_err(|_| "MERGE_DELETION_CORRUPT".to_owned());
                Ok(MergeDeletion {
                    note_id,
                    survivor,
                    plan: uuid(&plan)?,
                    operation_id: uuid(&op)?,
                    approval_digest: approval,
                    expected_pre_digest: pre,
                    survivor_model: model,
                    evidence_asset: asset,
                    state,
                    restore_operation: restore.as_deref().map(uuid).transpose()?,
                    created_ms: created as u64,
                    updated_ms: updated as u64,
                })
            })
            .transpose()
    }

    /// Compare-and-set a merge deletion's state.
    pub fn set_merge_deletion_state(
        &mut self,
        note_id: &str,
        expected: &str,
        state: &str,
        restore_operation: Option<Uuid>,
        now: u64,
    ) -> Result<()> {
        self.writable_check()?;
        let changed = self
            .connection
            .execute(
                "UPDATE merge_deletions SET state=?3,restore_operation=COALESCE(?4,restore_operation),updated_ms=?5 WHERE note_id=?1 AND state=?2",
                params![note_id, expected, state, restore_operation.map(|u| u.to_string()), now as i64],
            )
            .map_err(sql)?;
        if changed != 1 {
            return Err("MERGE_DELETION_STATE_CONFLICT".into());
        }
        Ok(())
    }
}
