//! Append-only evidence. The store computes reports instead of accepting caller readiness.
use crate::{Result, Store, sql};
use linguist_core::{canonical, plan_validation::ValidationEvidence};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE validations(id TEXT PRIMARY KEY,plan_id TEXT NOT NULL,revision INTEGER NOT NULL,plan_digest TEXT NOT NULL,evidence_digest TEXT NOT NULL,body BLOB NOT NULL,FOREIGN KEY(plan_id,revision) REFERENCES revisions(id,revision));
CREATE INDEX validations_plan ON validations(plan_id,revision,id);
CREATE TRIGGER validations_no_update BEFORE UPDATE ON validations BEGIN SELECT RAISE(ABORT,'validation evidence is immutable'); END;
CREATE TRIGGER validations_no_delete BEFORE DELETE ON validations BEGIN SELECT RAISE(ABORT,'validation retention requires explicit migration'); END;
";
#[derive(Debug, Serialize)]
pub struct ValidationReceipt {
    pub evidence_digest: String,
    pub evidence: ValidationEvidence,
}
impl Store {
    pub fn validate_revision(
        &mut self,
        id: uuid::Uuid,
        revision: u32,
    ) -> Result<ValidationReceipt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let plan = self.revision(id, revision)?;
        let evidence = linguist_core::plan_validation::inspect(&plan).map_err(|e| e.to_string())?;
        let evidence_digest =
            canonical::digest("plan-validation", &evidence).map_err(|e| e.to_string())?;
        let body = canonical::bytes(&evidence).map_err(|e| e.to_string())?;
        self.connection.execute("INSERT INTO validations(id,plan_id,revision,plan_digest,evidence_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",params![evidence.id.to_string(),id.to_string(),revision,evidence.plan_digest,evidence_digest,body]).map_err(sql)?;
        Ok(ValidationReceipt {
            evidence_digest,
            evidence,
        })
    }
    pub fn validation_evidence(&self, id: uuid::Uuid) -> Result<ValidationReceipt> {
        let record: Option<(String,u32,String,String,Vec<u8>)> = self.connection.query_row(
            "SELECT plan_id,revision,plan_digest,evidence_digest,body FROM validations WHERE id=?1",[id.to_string()],
            |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional().map_err(sql)?;
        let (plan_id, revision, plan_digest, evidence_digest, body) =
            record.ok_or("VALIDATION_NOT_FOUND")?;
        let evidence: ValidationEvidence =
            canonical::parse(&body).map_err(|_| "VALIDATION_CORRUPT")?;
        if evidence.id != id
            || evidence.plan_id.to_string() != plan_id
            || evidence.revision != revision
            || evidence.plan_digest != plan_digest
            || canonical::digest("plan-validation", &evidence).map_err(|_| "VALIDATION_CORRUPT")?
                != evidence_digest
        {
            return Err("VALIDATION_CORRUPT".into());
        }
        let plan = self.revision(evidence.plan_id, revision)?;
        let mut expected =
            linguist_core::plan_validation::inspect(&plan).map_err(|_| "VALIDATION_CORRUPT")?;
        expected.id = id;
        if expected != evidence {
            return Err("VALIDATION_CORRUPT".into());
        }
        Ok(ValidationReceipt {
            evidence_digest,
            evidence,
        })
    }
}
