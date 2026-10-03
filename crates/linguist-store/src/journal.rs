//! Durable append-only journal history. Dispatch belongs to the application layer.
use crate::{Result, Store, sql};
use linguist_core::{
    canonical,
    records::{OperationJournal, OperationState as O, StepState as S},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
pub(crate) const SCHEMA_SQL:&str="
CREATE TABLE journal_events(operation TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>0),state TEXT NOT NULL,pending INTEGER NOT NULL CHECK(pending IN(0,1)),body_digest TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(operation,sequence));
CREATE TABLE journal_heads(operation TEXT PRIMARY KEY,sequence INTEGER NOT NULL,FOREIGN KEY(operation,sequence) REFERENCES journal_events(operation,sequence));
CREATE INDEX journal_pending ON journal_events(pending,operation,sequence);
CREATE TRIGGER journal_events_no_update BEFORE UPDATE ON journal_events BEGIN SELECT RAISE(ABORT,'journal events are immutable'); END;
CREATE TRIGGER journal_events_no_delete BEFORE DELETE ON journal_events BEGIN SELECT RAISE(ABORT,'journal retention requires explicit migration'); END;
";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalVersion {
    pub sequence: u32,
    pub digest: String,
    pub pending_recovery: bool,
    pub journal: OperationJournal,
}
fn pending(j: &OperationJournal) -> bool {
    if matches!(j.state, O::Committed | O::FailedBeforeWrite) {
        return false;
    }
    matches!(j.state, O::Mutating | O::Verifying | O::NeedsRecovery)
        || j.steps.iter().any(|s| {
            matches!(
                s.state,
                S::RequestStarted | S::Unknown | S::ObservedSuccess | S::ObservedFailure
            )
        })
}
fn validate(j: &OperationJournal) -> Result<()> {
    if j.approval_digest.is_empty()
        || j.id.is_nil()
        || j.backup_id.is_nil()
        || j.snapshot_id.is_nil()
    {
        return Err("INVALID_JOURNAL_IDENTITY".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for step in &j.steps {
        if !ids.insert(step.id)
            || step.id.is_nil()
            || step.action.trim().is_empty()
            || [
                &step.payload_digest,
                &step.precondition_digest,
                &step.expected_post_digest,
            ]
            .iter()
            .any(|s| s.is_empty())
        {
            return Err("INVALID_JOURNAL_STEP".into());
        }
        if step.state == S::Verified
            && step.observed_digest.as_ref() != Some(&step.expected_post_digest)
        {
            return Err(
                "STEP_NOT_VERIFIED: observed digest does not match expected post-state".into(),
            );
        }
        if step.state == S::IntentRecorded && step.observed_digest.is_some() {
            return Err("INTENT_CANNOT_HAVE_OBSERVED_RESULT".into());
        }
    }
    if matches!(j.state, O::Compensated | O::Restored) {
        return Err(
            "JOURNAL_FINALIZATION_UNAVAILABLE: compensation/restore receipts are not implemented"
                .into(),
        );
    }
    // Commit requires every recorded effect to be verified against its expected post-state.
    if j.state == O::Committed
        && (j.steps.is_empty() || j.steps.iter().any(|s| s.state != S::Verified))
    {
        return Err("COMMIT_REQUIRES_VERIFIED_STEPS".into());
    }
    // An explicit observed failure is recorded only when the caller has evidence of no
    // collection effect; an absent receipt is unknown, never a failure.
    if j.state == O::FailedBeforeWrite
        && j.steps
            .iter()
            .any(|s| !matches!(s.state, S::IntentRecorded | S::ObservedFailure))
    {
        return Err("SENT_EFFECT_CANNOT_FAIL_BEFORE_WRITE".into());
    }
    Ok(())
}
fn transition(old: &OperationJournal, new: &OperationJournal) -> Result<()> {
    validate(new)?;
    if old.id != new.id
        || old.group_id != new.group_id
        || old.approval_digest != new.approval_digest
        || old.binding != new.binding
        || old.snapshot_id != new.snapshot_id
        || old.backup_id != new.backup_id
    {
        return Err(
            "JOURNAL_IDENTITY_CONFLICT: rebind requires an explicit verified decision".into(),
        );
    }
    let allowed = old.state == new.state
        || matches!(
            (old.state, new.state),
            (
                O::Prepared,
                O::Preflight | O::FailedBeforeWrite | O::NeedsRecovery
            ) | (
                O::Preflight,
                O::Checkpointed | O::FailedBeforeWrite | O::NeedsRecovery
            ) | (
                O::Checkpointed,
                O::Mutating | O::FailedBeforeWrite | O::NeedsRecovery
            ) | (
                O::Mutating,
                O::Verifying | O::NeedsRecovery | O::FailedBeforeWrite
            ) | (O::Verifying, O::NeedsRecovery | O::Committed)
                | (
                    O::NeedsRecovery,
                    O::Mutating | O::Verifying | O::FailedBeforeWrite
                )
        );
    if !allowed
        || matches!(
            old.state,
            O::FailedBeforeWrite | O::Committed | O::Compensated | O::Restored
        )
    {
        return Err("INVALID_OPERATION_TRANSITION".into());
    }
    for (index, step) in new.steps.iter().enumerate() {
        if step.state == S::RequestStarted
            && old
                .steps
                .get(index)
                .is_some_and(|s| s.state == S::IntentRecorded)
            && (old.steps[..index].iter().any(|s| s.state != S::Verified)
                || old
                    .steps
                    .iter()
                    .any(|s| matches!(s.state, S::RequestStarted | S::Unknown)))
        {
            return Err("UNRESOLVED_STEP_BLOCKS_DISPATCH".into());
        }
    }
    if new.steps.len() < old.steps.len() {
        return Err("JOURNAL_STEP_REMOVAL".into());
    }
    for (a, b) in old.steps.iter().zip(&new.steps) {
        if a.id != b.id
            || a.action != b.action
            || a.payload_digest != b.payload_digest
            || a.precondition_digest != b.precondition_digest
            || a.expected_post_digest != b.expected_post_digest
        {
            return Err("JOURNAL_INTENT_CONFLICT".into());
        }
        let allowed = a.state == b.state
            || matches!(
                (a.state, b.state),
                (S::IntentRecorded, S::RequestStarted)
                    | (
                        S::RequestStarted,
                        S::ObservedSuccess | S::ObservedFailure | S::Unknown
                    )
                    | (S::ObservedSuccess, S::Verified)
                    | (S::Unknown, S::Verified)
                    // Reconciliation may prove that an unknown request had no effect.
                    | (S::Unknown, S::ObservedFailure)
            );
        if !allowed {
            return Err(
                "INVALID_STEP_TRANSITION: sent/unknown effects cannot be blindly retried".into(),
            );
        }
        if a.state == S::Verified && a != b {
            return Err("VERIFIED_STEP_IMMUTABLE".into());
        }
        if a.state == b.state && a.observed_digest != b.observed_digest {
            return Err("OBSERVATION_IMMUTABLE: use a validated transition".into());
        }
    }
    if new.steps[old.steps.len()..]
        .iter()
        .any(|s| s.state != S::IntentRecorded)
    {
        return Err("NEW_STEP_REQUIRES_DURABLE_INTENT".into());
    }
    if new.steps.iter().any(|s| s.state == S::Unknown) && new.state != O::NeedsRecovery {
        return Err("UNKNOWN_EFFECT_REQUIRES_RECOVERY".into());
    }
    Ok(())
}
impl Store {
    pub fn journal(&self, id: uuid::Uuid) -> Result<JournalVersion> {
        let row:Option<(u32,String,Vec<u8>,bool,String)>=self.connection.query_row("SELECT e.sequence,e.body_digest,e.body,e.pending,e.state FROM journal_events e JOIN journal_heads h ON h.operation=e.operation AND h.sequence=e.sequence WHERE e.operation=?1",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sql)?;
        let (sequence, digest, body, indexed_pending, indexed_state) =
            row.ok_or("JOURNAL_NOT_FOUND")?;
        if canonical::asset_digest(&body) != digest {
            return Err("JOURNAL_CORRUPT".into());
        }
        let journal: OperationJournal = canonical::parse(&body).map_err(|_| "JOURNAL_CORRUPT")?;
        if journal.id != id
            || pending(&journal) != indexed_pending
            || validate(&journal).is_err()
            || serde_json::to_value(journal.state)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .as_deref()
                != Some(indexed_state.as_str())
        {
            return Err("JOURNAL_CORRUPT".into());
        }
        Ok(JournalVersion {
            sequence,
            digest,
            pending_recovery: indexed_pending,
            journal,
        })
    }
    /// Initial identity/intents are immutable. Later updates compare the exact previous version.
    pub fn append_journal(
        &mut self,
        journal: &OperationJournal,
        expected: Option<&JournalVersion>,
    ) -> Result<JournalVersion> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        validate(journal)?;
        match expected {
            None => {
                if journal.state != O::Prepared
                    || journal.steps.iter().any(|s| s.state != S::IntentRecorded)
                {
                    return Err("NEW_JOURNAL_REQUIRES_PREPARED_INTENTS".into());
                }
            }
            Some(version) => transition(&version.journal, journal)?,
        }
        let body = canonical::bytes(journal).map_err(|e| e.to_string())?;
        let digest = canonical::asset_digest(&body);
        let sequence = expected
            .map(|v| v.sequence.checked_add(1))
            .unwrap_or(Some(1))
            .ok_or("JOURNAL_SEQUENCE_EXHAUSTED")?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let actual:Option<(u32,String)>=tx.query_row("SELECT e.sequence,e.body_digest FROM journal_events e JOIN journal_heads h ON h.operation=e.operation AND h.sequence=e.sequence WHERE e.operation=?1",[journal.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
        if actual != expected.map(|v| (v.sequence, v.digest.clone())) {
            return Err("JOURNAL_VERSION_CONFLICT".into());
        }
        // The supplied prior object must itself match its recorded digest, not just the head label.
        if let Some(version) = expected
            && canonical::asset_digest(
                &canonical::bytes(&version.journal).map_err(|e| e.to_string())?,
            ) != version.digest
        {
            return Err("JOURNAL_VERSION_CONFLICT".into());
        }
        let state = serde_json::to_value(journal.state)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        tx.execute("INSERT INTO journal_events(operation,sequence,state,pending,body_digest,body) VALUES(?1,?2,?3,?4,?5,?6)",params![journal.id.to_string(),sequence,state,pending(journal),digest,body]).map_err(sql)?;
        tx.execute("INSERT INTO journal_heads(operation,sequence) VALUES(?1,?2) ON CONFLICT(operation) DO UPDATE SET sequence=excluded.sequence",params![journal.id.to_string(),sequence]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(JournalVersion {
            sequence,
            digest,
            pending_recovery: pending(journal),
            journal: journal.clone(),
        })
    }
    pub fn pending_journal_count(&self) -> Result<u64> {
        let count:i64=self.connection.query_row("SELECT COUNT(*) FROM journal_events e JOIN journal_heads h ON h.operation=e.operation AND h.sequence=e.sequence WHERE e.pending=1",[],|r|r.get(0)).map_err(sql)?;
        u64::try_from(count).map_err(|_| "JOURNAL_INDEX_CORRUPT".into())
    }
    pub fn pending_journals(&self, limit: u32) -> Result<Vec<JournalVersion>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mut stmt=self.connection.prepare("SELECT e.operation FROM journal_events e JOIN journal_heads h ON h.operation=e.operation AND h.sequence=e.sequence WHERE e.pending=1 ORDER BY e.operation LIMIT ?1").map_err(sql)?;
        let ids = stmt
            .query_map([limit], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        ids.map(|id| {
            self.journal(uuid::Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "JOURNAL_CORRUPT")?)
        })
        .collect()
    }
}
