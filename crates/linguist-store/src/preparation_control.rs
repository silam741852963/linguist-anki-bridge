//! Append-only preparation control requests. A request is not proof a worker stopped.
use crate::{Result, Store, sql};
use linguist_core::canonical;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE preparation_controls(job_id TEXT NOT NULL REFERENCES preparation_jobs(id),sequence INTEGER NOT NULL CHECK(sequence>0),digest TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(job_id,sequence));
CREATE TRIGGER preparation_controls_no_update BEFORE UPDATE ON preparation_controls BEGIN SELECT RAISE(ABORT,'control history is immutable'); END;
CREATE TRIGGER preparation_controls_no_delete BEFORE DELETE ON preparation_controls BEGIN SELECT RAISE(ABORT,'control history requires retention migration'); END;
";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Pause,
    Resume,
    Cancel,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvent {
    pub schema_version: u16,
    pub job_id: Uuid,
    pub sequence: u32,
    pub parent_digest: Option<String>,
    pub action: ControlAction,
}
#[derive(Clone, Debug, Serialize)]
pub struct ControlReceipt {
    pub digest: String,
    pub event: ControlEvent,
}
fn read(connection: &rusqlite::Connection, job: Uuid) -> Result<Option<ControlReceipt>> {
    let row: Option<(u32, String, Option<Vec<u8>>)> = connection.query_row(
        "SELECT sequence,digest,CASE WHEN length(body)<=4096 THEN body ELSE NULL END FROM preparation_controls WHERE job_id=?1 ORDER BY sequence DESC LIMIT 1",
        [job.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(sql)?;
    row.map(|(sequence, digest, body)| {
        let body = body.ok_or("PREPARATION_CONTROL_CORRUPT")?;
        let event: ControlEvent =
            canonical::parse(&body).map_err(|_| "PREPARATION_CONTROL_CORRUPT")?;
        if event.schema_version != 1
            || event.job_id != job
            || event.sequence != sequence
            || sequence == 0
            || canonical::digest("preparation-control", &event)
                .map_err(|_| "PREPARATION_CONTROL_CORRUPT")?
                != digest
        {
            return Err("PREPARATION_CONTROL_CORRUPT".into());
        }
        Ok(ControlReceipt { digest, event })
    })
    .transpose()
}
pub(crate) fn stops_dispatch(connection: &rusqlite::Connection, job: Uuid) -> Result<bool> {
    Ok(read(connection, job)?.is_some_and(|receipt| receipt.event.action != ControlAction::Resume))
}
impl Store {
    /// Verify a bounded control-history page and its immediately preceding anchor.
    pub fn preparation_controls(
        &self,
        job: Uuid,
        after_sequence: u32,
        limit: u32,
    ) -> Result<Vec<ControlReceipt>> {
        if !(1..=1000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        self.preparation_job(job)?;
        let mut statement = self.connection.prepare(
            "SELECT sequence,digest,CASE WHEN length(body)<=4096 THEN body ELSE NULL END FROM preparation_controls WHERE job_id=?1 AND sequence>=?2 ORDER BY sequence LIMIT ?3",
        ).map_err(sql)?;
        let rows = statement
            .query_map(
                params![
                    job.to_string(),
                    after_sequence.max(1),
                    limit + u32::from(after_sequence > 0)
                ],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .map_err(sql)?;
        let mut sequence = after_sequence;
        let mut previous = None;
        let mut receipts = Vec::new();
        for row in rows {
            let (stored_sequence, digest, body) = row.map_err(sql)?;
            let body = body.ok_or("PREPARATION_CONTROL_CORRUPT")?;
            let event: ControlEvent =
                canonical::parse(&body).map_err(|_| "PREPARATION_CONTROL_CORRUPT")?;
            if event.schema_version != 1
                || event.job_id != job
                || event.sequence != stored_sequence
                || canonical::digest("preparation-control", &event)
                    .map_err(|_| "PREPARATION_CONTROL_CORRUPT")?
                    != digest
            {
                return Err("PREPARATION_CONTROL_CORRUPT".into());
            }
            if after_sequence > 0 && event.sequence == after_sequence {
                previous = Some(digest);
                continue;
            }
            if sequence.checked_add(1) != Some(event.sequence)
                || event.parent_digest != previous
                || (after_sequence > 0 && previous.is_none())
            {
                return Err("PREPARATION_CONTROL_CHAIN_CORRUPT".into());
            }
            sequence = event.sequence;
            previous = Some(digest.clone());
            receipts.push(ControlReceipt { digest, event });
        }
        Ok(receipts)
    }
    pub fn preparation_control(&self, job: Uuid) -> Result<Option<ControlReceipt>> {
        self.preparation_job(job)?;
        read(&self.connection, job)
    }
    pub fn request_preparation_control(
        &mut self,
        job: Uuid,
        action: ControlAction,
    ) -> Result<ControlReceipt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        self.preparation_job(job)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let previous = read(&tx, job)?;
        if let Some(previous) = &previous {
            if previous.event.action == action {
                return Ok(previous.clone());
            }
            if previous.event.action == ControlAction::Cancel {
                return Err("PREPARATION_CANCEL_IS_TERMINAL".into());
            }
        }
        let event = ControlEvent {
            schema_version: 1,
            job_id: job,
            sequence: previous
                .as_ref()
                .map_or(Some(1), |receipt| receipt.event.sequence.checked_add(1))
                .ok_or("PREPARATION_CONTROL_SEQUENCE_LIMIT")?,
            parent_digest: previous.map(|receipt| receipt.digest),
            action,
        };
        let digest = canonical::digest("preparation-control", &event).map_err(|e| e.to_string())?;
        let body = canonical::bytes(&event).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO preparation_controls(job_id,sequence,digest,body) VALUES(?1,?2,?3,?4)",
            params![job.to_string(), event.sequence, digest, body],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(ControlReceipt { digest, event })
    }
}
