//! Immutable local approvals, protected by exact revision/digest and latest-head checks.
use crate::{Result, Store, sql};
use linguist_core::{approval::ApprovalRequest, canonical, records::Approval};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
pub(crate) const SCHEMA_SQL:&str="
CREATE TABLE approvals(id TEXT PRIMARY KEY,plan_id TEXT NOT NULL,revision INTEGER NOT NULL,plan_digest TEXT NOT NULL,record_digest TEXT NOT NULL,body BLOB NOT NULL,FOREIGN KEY(plan_id,revision) REFERENCES revisions(id,revision));
CREATE INDEX approvals_plan ON approvals(plan_id,revision,id);
CREATE TRIGGER approvals_no_update BEFORE UPDATE ON approvals BEGIN SELECT RAISE(ABORT,'approvals are immutable'); END;
CREATE TRIGGER approvals_no_delete BEFORE DELETE ON approvals BEGIN SELECT RAISE(ABORT,'approval retention requires explicit migration'); END;
";
#[derive(Debug, Serialize)]
pub struct ApprovalReceipt {
    pub id: uuid::Uuid,
    pub record_digest: String,
    pub approval: Approval,
    pub apply_authorized: bool,
}
impl Store {
    pub fn approve_revision(
        &mut self,
        request: &ApprovalRequest,
    ) -> Result<Option<ApprovalReceipt>> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let plan = self.revision(request.plan_id, request.revision)?;
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "CLOCK_UNAVAILABLE")?
            .as_secs();
        let approval =
            linguist_core::approval::build(&plan, request, format!("unix-seconds:{seconds}"))
                .map_err(|e| e.to_string())?;
        if approval.item_ids.is_empty() {
            return Ok(None);
        }
        let record_digest =
            canonical::digest("approval-record", &approval).map_err(|e| e.to_string())?;
        let body = canonical::bytes(&approval).map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let (revision, digest): (u32, String) = tx
            .query_row(
                "SELECT revision,digest FROM revisions WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                [request.plan_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(sql)?;
        if revision != request.revision || digest != request.digest {
            return Err("APPROVAL_REVISION_CONFLICT: latest revision changed".into());
        }
        tx.execute("INSERT INTO approvals(id,plan_id,revision,plan_digest,record_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",params![id.to_string(),request.plan_id.to_string(),revision,digest,record_digest,body]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(Some(ApprovalReceipt {
            id,
            record_digest,
            approval,
            apply_authorized: false,
        }))
    }
    /// Every approval recorded for one exact revision, oldest first by ID order.
    pub fn approvals_for(
        &self,
        plan_id: uuid::Uuid,
        revision: u32,
    ) -> Result<Vec<ApprovalReceipt>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM approvals WHERE plan_id=?1 AND revision=?2 ORDER BY id")
            .map_err(sql)?;
        let ids = statement
            .query_map(params![plan_id.to_string(), revision], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql)?;
        ids.map(|id| {
            self.approval(uuid::Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "APPROVAL_CORRUPT")?)
        })
        .collect()
    }
    pub fn approval(&self, id: uuid::Uuid) -> Result<ApprovalReceipt> {
        let record: Option<(String, u32, String, String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT plan_id,revision,plan_digest,record_digest,body FROM approvals WHERE id=?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        let (plan_id, revision, digest, record_digest, body) =
            record.ok_or("APPROVAL_NOT_FOUND")?;
        let approval: Approval = canonical::parse(&body).map_err(|_| "APPROVAL_CORRUPT")?;
        if approval.plan_id.to_string() != plan_id
            || approval.revision != revision
            || approval.digest != digest
            || canonical::digest("approval-record", &approval).map_err(|_| "APPROVAL_CORRUPT")?
                != record_digest
        {
            return Err("APPROVAL_CORRUPT".into());
        }
        let plan = self.revision(approval.plan_id, revision)?;
        let request = ApprovalRequest {
            plan_id: approval.plan_id,
            revision,
            digest,
            item_ids: Some(approval.item_ids.clone()),
            actor: approval.actor.clone(),
            accepted_warnings: approval.accepted_warnings.clone(),
        };
        let expected =
            linguist_core::approval::build(&plan, &request, approval.approved_at.clone())
                .map_err(|_| "APPROVAL_CORRUPT")?;
        if expected != approval {
            return Err("APPROVAL_CORRUPT".into());
        }
        Ok(ApprovalReceipt {
            id,
            record_digest,
            approval,
            apply_authorized: false,
        })
    }
}
