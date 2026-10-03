//! Immutable read-preparation definitions and asset-first, CAS-ordered checkpoints.
//! No worker liveness, collection mutation or automatic retry is inferred here.
use crate::{Result, Store, sql};
use linguist_core::{
    LearningDocument, canonical,
    records::{Job, JobMode, SelectionReceipt},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE preparation_jobs(id TEXT PRIMARY KEY,digest TEXT NOT NULL,item_count INTEGER NOT NULL,body BLOB NOT NULL);
CREATE TABLE preparation_events(job_id TEXT NOT NULL REFERENCES preparation_jobs(id),sequence INTEGER NOT NULL,item_id TEXT NOT NULL,digest TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(job_id,sequence));
CREATE INDEX preparation_events_item ON preparation_events(job_id,item_id,sequence);
CREATE TABLE preparation_event_assets(job_id TEXT NOT NULL,sequence INTEGER NOT NULL,digest TEXT NOT NULL REFERENCES assets(digest),PRIMARY KEY(job_id,sequence,digest),FOREIGN KEY(job_id,sequence) REFERENCES preparation_events(job_id,sequence));
CREATE TRIGGER preparation_jobs_no_update BEFORE UPDATE ON preparation_jobs BEGIN SELECT RAISE(ABORT,'preparation definitions are immutable'); END;
CREATE TRIGGER preparation_jobs_no_delete BEFORE DELETE ON preparation_jobs BEGIN SELECT RAISE(ABORT,'preparation retention requires explicit migration'); END;
CREATE TRIGGER preparation_events_no_update BEFORE UPDATE ON preparation_events BEGIN SELECT RAISE(ABORT,'preparation events are immutable'); END;
CREATE TRIGGER preparation_events_no_delete BEFORE DELETE ON preparation_events BEGIN SELECT RAISE(ABORT,'preparation retention requires explicit migration'); END;
CREATE TRIGGER preparation_assets_no_update BEFORE UPDATE ON preparation_event_assets BEGIN SELECT RAISE(ABORT,'preparation references are immutable'); END;
CREATE TRIGGER preparation_assets_no_delete BEFORE DELETE ON preparation_event_assets BEGIN SELECT RAISE(ABORT,'preparation retention requires explicit migration'); END;
";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationDefinition {
    pub schema_version: u16,
    pub job: Job,
    pub selection: SelectionReceipt,
    pub created_at: String,
}
impl PreparationDefinition {
    pub fn validate(&self) -> Result<()> {
        validate_definition(self)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparationStage {
    Started,
    Captured { document: Box<LearningDocument> },
    Failed { code: String, retry_eligible: bool },
    Interrupted { actor: String },
}
fn retry_code(code: &str) -> bool {
    matches!(
        code,
        "SOURCE_READ_TIMEOUT"
            | "SOURCE_READ_CONNECTION_FAILED"
            | "SOURCE_READ_RATE_LIMITED"
            | "SOURCE_READ_UNAVAILABLE"
    )
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationEvent {
    pub schema_version: u16,
    pub job_id: Uuid,
    pub sequence: u32,
    pub parent_digest: Option<String>,
    pub item_id: Uuid,
    pub attempt: u16,
    pub stage: PreparationStage,
}
#[derive(Debug, Serialize)]
pub struct PreparationReceipt {
    pub digest: String,
    pub event: PreparationEvent,
}
#[derive(Debug, Serialize)]
pub struct PreparationSummary {
    pub id: Uuid,
    pub digest: String,
    pub item_count: u32,
    pub checkpoint_count: u32,
}
#[derive(Debug, Serialize)]
pub struct PreparationItemSummary {
    pub index: u32,
    pub item_id: Uuid,
    pub input_ref: String,
    pub state: String,
    pub attempt: u16,
    pub checkpoint_sequence: Option<u32>,
    pub checkpoint_digest: Option<String>,
    pub document_id: Option<Uuid>,
    pub error_code: Option<String>,
    pub retry_eligible: bool,
}

fn validate_definition(definition: &PreparationDefinition) -> Result<()> {
    let job = &definition.job;
    definition
        .selection
        .validate_inputs(&job.settings)
        .map_err(|_| "INVALID_PREPARATION_SELECTION")?;
    if job.plan_refs
        != definition
            .selection
            .selected_note_ids
            .iter()
            .map(|id| format!("anki-note:{id}"))
            .collect::<Vec<_>>()
    {
        return Err("INVALID_PREPARATION_SELECTION".into());
    }
    if definition.schema_version != 1
        || job.id.is_nil()
        || job.mode != JobMode::Prepare
        || job.pause_requested
        || job.cancel_requested
        || job.item_ids.is_empty()
        || job.item_ids.len() > 100000
        || job.plan_refs.len() != job.item_ids.len()
        || job.item_ids.iter().any(Uuid::is_nil)
        || job
            .item_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != job.item_ids.len()
        || definition.created_at.is_empty()
        || definition.created_at.len() > 80
        || definition.created_at.chars().any(char::is_control)
    {
        return Err("INVALID_PREPARATION_DEFINITION".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for reference in &job.plan_refs {
        let id = reference
            .strip_prefix("anki-note:")
            .ok_or("PREPARATION_INPUT_UNSUPPORTED")?;
        let number: u64 = id.parse().map_err(|_| "INVALID_PREPARATION_INPUT")?;
        if number == 0 || number > 9007199254740991 || number.to_string() != id || !ids.insert(id) {
            return Err("INVALID_PREPARATION_INPUT".into());
        }
    }
    let (semantic_fingerprint, execution_fingerprint) =
        linguist_core::records::setting_fingerprints(&job.settings.values)
            .map_err(|e| e.to_string())?;
    let split_valid = (job.settings.semantic_fingerprint.is_empty()
        && job.settings.execution_fingerprint.is_empty())
        || (job.settings.semantic_fingerprint == semantic_fingerprint
            && job.settings.execution_fingerprint == execution_fingerprint);
    if job.settings.version != 2
        || !split_valid
        || canonical::digest("resolved-settings", &job.settings.values)
            .map_err(|e| e.to_string())?
            != job.settings.fingerprint
        || !matches!(
            job.settings
                .values
                .get("jobs.max_item_attempts")
                .and_then(serde_json::Value::as_u64),
            Some(1..=10)
        )
        || !matches!(
            job.settings
                .values
                .get("input.max_file_mb")
                .and_then(serde_json::Value::as_u64),
            Some(1..=100)
        )
    {
        return Err("INVALID_PREPARATION_SETTINGS".into());
    }
    Ok(())
}

impl Store {
    fn verify_preparation_event_assets(&self, event: &PreparationEvent) -> Result<()> {
        let expected = if let PreparationStage::Captured { document } = &event.stage {
            self.verify_document_assets(std::slice::from_ref(document.as_ref()))?
        } else {
            std::collections::BTreeSet::new()
        };
        let mut statement = self.connection.prepare(
            "SELECT digest FROM preparation_event_assets WHERE job_id=?1 AND sequence=?2 ORDER BY digest",
        ).map_err(sql)?;
        let indexed: std::collections::BTreeSet<String> = statement
            .query_map(params![event.job_id.to_string(), event.sequence], |r| {
                r.get(0)
            })
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        if expected != indexed {
            return Err("PREPARATION_ASSET_INDEX_CORRUPT".into());
        }
        Ok(())
    }

    /// Publish the complete immutable capture batch once, retaining the frozen selection.
    /// The job UUID identifies revision one; later review revisions remain untouched.
    pub fn publish_preparation_plan(
        &mut self,
        id: Uuid,
        expected_head: Option<&str>,
        lease: &crate::lease::LeaseToken,
    ) -> Result<Option<crate::RevisionSummary>> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        self.validate_job_worker_lease(lease, id)?;
        let definition = self.preparation_job(id)?;
        let head: Option<String> = self.connection.query_row(
            "SELECT digest FROM preparation_events WHERE job_id=?1 ORDER BY sequence DESC LIMIT 1",
            [id.to_string()], |row| row.get(0),
        ).optional().map_err(sql)?;
        if head.as_deref() != expected_head {
            return Err("PREPARATION_HEAD_CONFLICT".into());
        }
        let limit = definition.job.settings.values["input.max_file_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        let mut document_bytes = 0u64;
        let mut documents = Vec::with_capacity(definition.job.item_ids.len());
        for item in &definition.job.item_ids {
            let row: Option<(u32, String, Vec<u8>)> = self.connection.query_row(
                "SELECT sequence,digest,body FROM preparation_events WHERE job_id=?1 AND item_id=?2 ORDER BY sequence DESC LIMIT 1",
                params![id.to_string(), item.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional().map_err(sql)?;
            let Some((sequence, digest, body)) = row else {
                return Ok(None);
            };
            let event: PreparationEvent =
                canonical::parse(&body).map_err(|_| "PREPARATION_EVENT_CORRUPT")?;
            if event.schema_version != 1
                || event.job_id != id
                || event.item_id != *item
                || event.sequence != sequence
                || canonical::digest("preparation-event", &event)
                    .map_err(|_| "PREPARATION_EVENT_CORRUPT")?
                    != digest
            {
                return Err("PREPARATION_EVENT_CORRUPT".into());
            }
            self.verify_preparation_event_assets(&event)?;
            match event.stage {
                PreparationStage::Captured { document } => {
                    document_bytes = document_bytes
                        .checked_add(
                            canonical::bytes(&document)
                                .map_err(|e| e.to_string())?
                                .len() as u64,
                        )
                        .ok_or("PREPARATION_PLAN_SIZE_LIMIT")?;
                    if document_bytes > limit {
                        return Err("PREPARATION_PLAN_SIZE_LIMIT".into());
                    }
                    documents.push(*document);
                }
                _ => return Ok(None),
            }
        }
        let assets = self.verify_document_assets(&documents)?;
        let mut total = 0u64;
        for asset in assets {
            total = total
                .checked_add(self.asset(&asset, limit)?.len() as u64)
                .ok_or("REVAMP_BATCH_ARCHIVE_LIMIT")?;
            if total > limit {
                return Err("REVAMP_BATCH_ARCHIVE_LIMIT".into());
            }
        }
        let sources: Vec<_> = documents
            .iter()
            .flat_map(|document| &document.sources)
            .collect();
        let source_digest =
            canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
        let plan = linguist_core::records::PlanRevision {
            grammar_groups: vec![],
            schema_version: 2,
            id,
            revision: 1,
            parent_digest: None,
            settings: definition.job.settings,
            binding: None,
            source_digest,
            selection: Some(definition.selection),
            documents,
            rendered: vec![],
            review_decisions: vec![],
        };
        let body = canonical::bytes(&plan).map_err(|e| e.to_string())?;
        if body.len() as u64 > limit {
            return Err("PREPARATION_PLAN_SIZE_LIMIT".into());
        }
        let exists: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM revisions WHERE id=?1 AND revision=1)",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(sql)?;
        self.validate_job_worker_lease(lease, id)?;
        let digest = if exists {
            // Compare complete bytes, including evidence omitted from approval projections.
            let retained = self.revision(id, 1)?;
            if canonical::bytes(&retained).map_err(|e| e.to_string())? != body {
                return Err("PREPARATION_PLAN_CONFLICT".into());
            }
            retained.approval_digest().map_err(|e| e.to_string())?
        } else {
            self.publish_revision_inner(&plan, Some(lease))?
        };
        Ok(Some(crate::RevisionSummary {
            id,
            revision: 1,
            digest,
        }))
    }
    pub fn list_preparation_jobs(
        &self,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<PreparationSummary>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mut statement = self.connection.prepare("SELECT j.id,j.digest,j.item_count,(SELECT COUNT(*) FROM preparation_events e WHERE e.job_id=j.id) FROM preparation_jobs j WHERE j.id>?1 ORDER BY j.id LIMIT ?2").map_err(sql)?;
        let rows = statement
            .query_map(
                params![after.map(|id| id.to_string()).unwrap_or_default(), limit],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, u32>(2)?,
                        r.get::<_, u32>(3)?,
                    ))
                },
            )
            .map_err(sql)?;
        rows.map(|row| {
            let (id, digest, item_count, checkpoint_count) = row.map_err(sql)?;
            let id = Uuid::parse_str(&id).map_err(|_| "PREPARATION_JOB_CORRUPT")?;
            let definition = self.preparation_job(id)?;
            if item_count as usize != definition.job.item_ids.len()
                || canonical::digest("preparation-definition", &definition)
                    .map_err(|e| e.to_string())?
                    != digest
            {
                return Err("PREPARATION_JOB_CORRUPT".into());
            }
            Ok(PreparationSummary {
                id,
                digest,
                item_count,
                checkpoint_count,
            })
        })
        .collect()
    }
    pub fn preparation_items(
        &self,
        id: Uuid,
        after_index: u32,
        limit: u32,
    ) -> Result<Vec<PreparationItemSummary>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let definition = self.preparation_job(id)?;
        let mut summaries = Vec::new();
        for (index, item_id) in definition
            .job
            .item_ids
            .iter()
            .enumerate()
            .skip(after_index as usize)
            .take(limit as usize)
        {
            let row: Option<(u32,String,Vec<u8>)> = self.connection.query_row("SELECT sequence,digest,body FROM preparation_events WHERE job_id=?1 AND item_id=?2 ORDER BY sequence DESC LIMIT 1", params![id.to_string(),item_id.to_string()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sql)?;
            let mut summary = PreparationItemSummary {
                index: index as u32,
                item_id: *item_id,
                input_ref: definition.job.plan_refs[index].clone(),
                state: "pending".into(),
                attempt: 0,
                checkpoint_sequence: None,
                checkpoint_digest: None,
                document_id: None,
                error_code: None,
                retry_eligible: false,
            };
            if let Some((sequence, digest, body)) = row {
                let event: PreparationEvent =
                    canonical::parse(&body).map_err(|_| "PREPARATION_EVENT_CORRUPT")?;
                if event.schema_version != 1
                    || event.job_id != id
                    || event.item_id != *item_id
                    || event.sequence != sequence
                    || canonical::digest("preparation-event", &event).map_err(|e| e.to_string())?
                        != digest
                {
                    return Err("PREPARATION_EVENT_CORRUPT".into());
                }
                summary.attempt = event.attempt;
                summary.checkpoint_sequence = Some(sequence);
                summary.checkpoint_digest = Some(digest);
                self.verify_preparation_event_assets(&event)?;
                match event.stage {
                    PreparationStage::Interrupted { .. } => {
                        summary.state = "failed".into();
                        summary.error_code = Some("SOURCE_READ_INTERRUPTED".into());
                        summary.retry_eligible = u64::from(event.attempt)
                            < definition.job.settings.values["jobs.max_item_attempts"]
                                .as_u64()
                                .unwrap();
                    }
                    PreparationStage::Started => summary.state = "started".into(),
                    PreparationStage::Captured { document } => {
                        self.verify_document_assets(std::slice::from_ref(document.as_ref()))?;
                        summary.state = "captured".into();
                        summary.document_id = Some(document.id);
                    }
                    PreparationStage::Failed {
                        code,
                        retry_eligible,
                    } => {
                        summary.state = "failed".into();
                        summary.error_code = Some(code);
                        summary.retry_eligible = retry_eligible
                            && u64::from(event.attempt)
                                < definition.job.settings.values["jobs.max_item_attempts"]
                                    .as_u64()
                                    .unwrap();
                    }
                }
            }
            summaries.push(summary);
        }
        Ok(summaries)
    }
    pub fn create_preparation_job(&mut self, definition: &PreparationDefinition) -> Result<String> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        validate_definition(definition)?;
        let digest =
            canonical::digest("preparation-definition", definition).map_err(|e| e.to_string())?;
        let body = canonical::bytes(definition).map_err(|e| e.to_string())?;
        if body.len() > 100 * 1024 * 1024 {
            return Err("PREPARATION_DEFINITION_LIMIT".into());
        }
        self.connection
            .execute(
                "INSERT OR IGNORE INTO preparation_jobs(id,digest,item_count,body) VALUES(?1,?2,?3,?4)",
                params![
                    definition.job.id.to_string(),
                    digest,
                    definition.job.item_ids.len() as u32,
                    body
                ],
            )
            .map_err(sql)?;
        if canonical::digest(
            "preparation-definition",
            &self.preparation_job(definition.job.id)?,
        )
        .map_err(|e| e.to_string())?
            != digest
        {
            return Err("PREPARATION_JOB_CONFLICT".into());
        }
        Ok(digest)
    }
    pub fn preparation_job(&self, id: Uuid) -> Result<PreparationDefinition> {
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT digest,body FROM preparation_jobs WHERE id=?1 AND length(body)<=104857600",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let (digest, body) = row.ok_or("PREPARATION_JOB_NOT_FOUND_OR_OVERSIZED")?;
        let definition: PreparationDefinition =
            canonical::parse(&body).map_err(|_| "PREPARATION_JOB_CORRUPT")?;
        validate_definition(&definition).map_err(|_| "PREPARATION_JOB_CORRUPT")?;
        if definition.job.id != id
            || canonical::digest("preparation-definition", &definition)
                .map_err(|e| e.to_string())?
                != digest
        {
            return Err("PREPARATION_JOB_CORRUPT".into());
        }
        Ok(definition)
    }
    pub fn append_preparation_event(
        &mut self,
        job_id: Uuid,
        item_id: Uuid,
        attempt: u16,
        stage: PreparationStage,
        expected_head: Option<&str>,
    ) -> Result<PreparationReceipt> {
        self.append_preparation_event_inner(job_id, item_id, attempt, stage, expected_head, None)
    }
    /// Worker progress requires matching live fencing inside the checkpoint transaction.
    pub fn append_preparation_event_with_lease(
        &mut self,
        job_id: Uuid,
        item_id: Uuid,
        attempt: u16,
        stage: PreparationStage,
        expected_head: Option<&str>,
        worker: &crate::lease::LeaseToken,
    ) -> Result<PreparationReceipt> {
        self.append_preparation_event_inner(
            job_id,
            item_id,
            attempt,
            stage,
            expected_head,
            Some(worker),
        )
    }
    fn append_preparation_event_inner(
        &mut self,
        job_id: Uuid,
        item_id: Uuid,
        attempt: u16,
        stage: PreparationStage,
        expected_head: Option<&str>,
        worker: Option<&crate::lease::LeaseToken>,
    ) -> Result<PreparationReceipt> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if let Some(worker) = worker {
            self.validate_job_worker_lease(worker, job_id)?;
        }
        let definition = self.preparation_job(job_id)?;
        let position = definition
            .job
            .item_ids
            .iter()
            .position(|id| *id == item_id)
            .ok_or("PREPARATION_ITEM_NOT_FOUND")?;
        let prior: Option<(String, Vec<u8>)> = self.connection.query_row("SELECT digest,body FROM preparation_events WHERE job_id=?1 AND item_id=?2 ORDER BY sequence DESC LIMIT 1", params![job_id.to_string(),item_id.to_string()], |r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
        let prior: Option<PreparationEvent> = prior
            .as_ref()
            .map(|(digest, bytes)| {
                let event: PreparationEvent =
                    canonical::parse(bytes).map_err(|_| "PREPARATION_EVENT_CORRUPT")?;
                if event.schema_version != 1
                    || event.job_id != job_id
                    || event.item_id != item_id
                    || canonical::digest("preparation-event", &event)
                        .map_err(|_| "PREPARATION_EVENT_CORRUPT")?
                        != *digest
                {
                    return Err("PREPARATION_EVENT_CORRUPT");
                }
                Ok(event)
            })
            .transpose()?;
        let max_attempts = definition.job.settings.values["jobs.max_item_attempts"]
            .as_u64()
            .unwrap();
        let allowed = match (&prior, &stage) {
            (None, PreparationStage::Started) => attempt == 1,
            (Some(previous), PreparationStage::Started) => {
                matches!(
                    previous.stage,
                    PreparationStage::Failed {
                        retry_eligible: true,
                        ..
                    } | PreparationStage::Interrupted { .. }
                ) && u32::from(attempt) == u32::from(previous.attempt) + 1
            }
            (
                Some(previous),
                PreparationStage::Captured { .. }
                | PreparationStage::Failed { .. }
                | PreparationStage::Interrupted { .. },
            ) => previous.stage == PreparationStage::Started && previous.attempt == attempt,
            _ => false,
        };
        if !allowed || u64::from(attempt) > max_attempts {
            return Err("PREPARATION_TRANSITION_INVALID".into());
        }
        if let PreparationStage::Interrupted { actor } = &stage {
            if worker.is_none() {
                return Err("PREPARATION_RECOVERY_LEASE_REQUIRED".into());
            }
            if actor.trim().is_empty()
                || actor.chars().count() > 200
                || actor.chars().any(char::is_control)
            {
                return Err("PREPARATION_RECOVERY_ACTOR_INVALID".into());
            }
        }
        let assets = if let PreparationStage::Captured { document } = &stage {
            let expected =
                definition.job.plan_refs[position].replacen("anki-note:", "anki_note:", 1);
            if document.schema_version != 2
                || document.id.is_nil()
                || document.sources.len() != 1
                || document.sources[0].kind != "anki_read_capture_v2"
                || document.sources[0].location != expected
                || document.target_language.as_str().split('-').next()
                    != Some(if definition.selection.purpose.starts_with("japanese_") {
                        "ja"
                    } else {
                        "en"
                    })
                || (definition.selection.purpose.ends_with("_vocab")
                    != matches!(
                        document.content,
                        linguist_core::LearningContent::Vocabulary(_)
                    ))
                || !document.archives.iter().any(|a| {
                    a.source_id == document.sources[0].id
                        && a.digest == document.sources[0].digest
                        && a.original_fields == document.sources[0].fields
                        && a.asset_digests.contains(&a.digest)
                })
            {
                return Err("PREPARATION_CAPTURE_CONFLICT".into());
            }
            self.verify_document_assets(std::slice::from_ref(document.as_ref()))?
        } else {
            std::collections::BTreeSet::new()
        };
        if let PreparationStage::Failed {
            code,
            retry_eligible,
        } = &stage
            && (code.is_empty()
                || code.len() > 100
                || (*retry_eligible && !retry_code(code))
                || !code
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_'))
        {
            return Err("PREPARATION_ERROR_CODE_INVALID".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if let Some(worker) = worker {
            crate::lease::validate_job_worker_token(&tx, worker, job_id)?;
            if stage == PreparationStage::Started
                && crate::preparation_control::stops_dispatch(&tx, job_id)?
            {
                return Err("PREPARATION_CONTROL_BLOCKS_DISPATCH".into());
            }
        }
        let head: Option<(u32,String)> = tx.query_row("SELECT sequence,digest FROM preparation_events WHERE job_id=?1 ORDER BY sequence DESC LIMIT 1", [job_id.to_string()], |r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
        if head.as_ref().map(|h| h.1.as_str()) != expected_head {
            return Err("PREPARATION_HEAD_CONFLICT".into());
        }
        let sequence = head
            .as_ref()
            .map_or(Some(1), |h| h.0.checked_add(1))
            .ok_or("PREPARATION_SEQUENCE_LIMIT")?;
        let event = PreparationEvent {
            schema_version: 1,
            job_id,
            sequence,
            parent_digest: head.map(|h| h.1),
            item_id,
            attempt,
            stage,
        };
        let body = canonical::bytes(&event).map_err(|e| e.to_string())?;
        if body.len() as u64
            > definition.job.settings.values["input.max_file_mb"]
                .as_u64()
                .unwrap()
                * 1024
                * 1024
        {
            return Err("PREPARATION_CHECKPOINT_LIMIT".into());
        }
        let digest = canonical::digest("preparation-event", &event).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO preparation_events(job_id,sequence,item_id,digest,body) VALUES(?1,?2,?3,?4,?5)", params![job_id.to_string(),sequence,item_id.to_string(),digest,body]).map_err(sql)?;
        for asset in assets {
            tx.execute(
                "INSERT INTO preparation_event_assets(job_id,sequence,digest) VALUES(?1,?2,?3)",
                params![job_id.to_string(), sequence, asset],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(PreparationReceipt { digest, event })
    }
    /// Verify the newest checkpoint without materializing the full capture history.
    pub fn preparation_head(&self, id: Uuid) -> Result<Option<PreparationReceipt>> {
        self.preparation_job(id)?;
        let sequence: Option<u32> = self
            .connection
            .query_row(
                "SELECT MAX(sequence) FROM preparation_events WHERE job_id=?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(sql)?;
        match sequence {
            None => Ok(None),
            Some(0) => Err("PREPARATION_EVENT_CORRUPT".into()),
            Some(sequence) => self
                .preparation_events(id, sequence - 1, 1)?
                .into_iter()
                .next()
                .map(Some)
                .ok_or_else(|| "PREPARATION_EVENT_CORRUPT".into()),
        }
    }
    pub fn preparation_events(
        &self,
        id: Uuid,
        after_sequence: u32,
        limit: u32,
    ) -> Result<Vec<PreparationReceipt>> {
        let mut receipts = Vec::new();
        self.visit_preparation_events(id, after_sequence, limit, |receipt| {
            receipts.push(receipt);
            Ok(())
        })?;
        Ok(receipts)
    }
    /// Stream verified checkpoints without retaining captured document bodies for the whole page.
    pub fn visit_preparation_events(
        &self,
        id: Uuid,
        after_sequence: u32,
        limit: u32,
        mut visitor: impl FnMut(PreparationReceipt) -> Result<()>,
    ) -> Result<()> {
        if !(1..=1000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let definition = self.preparation_job(id)?;
        let max_bytes = definition.job.settings.values["input.max_file_mb"]
            .as_i64()
            .unwrap()
            * 1024
            * 1024;
        let mut statement = self.connection.prepare("SELECT sequence,digest,CASE WHEN length(body)<=?4 THEN body ELSE NULL END FROM preparation_events WHERE job_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(sql)?;
        let rows = statement
            .query_map(
                params![id.to_string(), after_sequence, limit, max_bytes],
                |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .map_err(sql)?;
        let mut previous_sequence = after_sequence;
        let mut previous_digest: Option<String> = if after_sequence == 0 {
            None
        } else {
            let anchor: Option<(String, Option<Vec<u8>>)> = self.connection.query_row(
                "SELECT digest,CASE WHEN length(body)<=?3 THEN body ELSE NULL END FROM preparation_events WHERE job_id=?1 AND sequence=?2",
                params![id.to_string(), after_sequence, max_bytes], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional().map_err(sql)?;
            anchor
                .map(|(digest, body)| {
                    let body = body.ok_or("PREPARATION_EVENT_CORRUPT")?;
                    let event: PreparationEvent =
                        canonical::parse(&body).map_err(|_| "PREPARATION_EVENT_CORRUPT")?;
                    if event.job_id != id
                        || event.schema_version != 1
                        || event.sequence != after_sequence
                        || canonical::digest("preparation-event", &event)
                            .map_err(|_| "PREPARATION_EVENT_CORRUPT")?
                            != digest
                    {
                        return Err("PREPARATION_EVENT_CORRUPT");
                    }
                    Ok(digest)
                })
                .transpose()?
        };
        for row in rows {
            let (sequence, digest, body) = row.map_err(sql)?;
            let body = body.ok_or("PREPARATION_EVENT_CORRUPT")?;
            let event: PreparationEvent =
                canonical::parse(&body).map_err(|_| "PREPARATION_EVENT_CORRUPT")?;
            if event.schema_version != 1
                || event.job_id != id
                || event.sequence != sequence
                || previous_sequence.checked_add(1) != Some(sequence)
                || event.parent_digest != previous_digest
                || (after_sequence > 0 && previous_digest.is_none())
                || canonical::digest("preparation-event", &event).map_err(|e| e.to_string())?
                    != digest
            {
                return Err("PREPARATION_EVENT_CORRUPT".into());
            }
            self.verify_preparation_event_assets(&event)?;
            previous_sequence = sequence;
            previous_digest = Some(digest.clone());
            visitor(PreparationReceipt { digest, event })?;
        }
        Ok(())
    }
}
