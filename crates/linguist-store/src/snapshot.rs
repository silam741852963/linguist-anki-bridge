//! Immutable original snapshots and separately recorded native after-state evidence.
use crate::{Result, Store, sql};
use linguist_core::{
    canonical,
    records::{NativeOperationReceipt, NativeReceiptState, Snapshot, StepState},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS snapshots(
 id TEXT PRIMARY KEY, operation TEXT NOT NULL UNIQUE, before_digest TEXT NOT NULL,
 body_digest TEXT NOT NULL, body BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS snapshot_assets(
 snapshot TEXT NOT NULL REFERENCES snapshots(id), digest TEXT NOT NULL REFERENCES assets(digest),
 PRIMARY KEY(snapshot,digest));
CREATE TABLE IF NOT EXISTS snapshot_after(
 snapshot TEXT PRIMARY KEY REFERENCES snapshots(id), operation TEXT NOT NULL,
 observed_digest TEXT NOT NULL, receipt_digest TEXT NOT NULL, body BLOB NOT NULL,
 actual_asset TEXT NOT NULL REFERENCES assets(digest),
 evidence_asset TEXT NOT NULL REFERENCES assets(digest));
CREATE TRIGGER IF NOT EXISTS snapshots_no_update BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT,'snapshots are immutable'); END;
CREATE TRIGGER IF NOT EXISTS snapshots_no_delete BEFORE DELETE ON snapshots BEGIN SELECT RAISE(ABORT,'snapshot retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS snapshot_assets_no_update BEFORE UPDATE ON snapshot_assets BEGIN SELECT RAISE(ABORT,'snapshot assets are immutable'); END;
CREATE TRIGGER IF NOT EXISTS snapshot_assets_no_delete BEFORE DELETE ON snapshot_assets BEGIN SELECT RAISE(ABORT,'snapshot asset retention requires explicit migration'); END;
CREATE TRIGGER IF NOT EXISTS snapshot_after_no_update BEFORE UPDATE ON snapshot_after BEGIN SELECT RAISE(ABORT,'snapshot after-state is immutable'); END;
CREATE TRIGGER IF NOT EXISTS snapshot_after_no_delete BEFORE DELETE ON snapshot_after BEGIN SELECT RAISE(ABORT,'snapshot after-state retention requires explicit migration'); END;
";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub snapshot: Snapshot,
    pub after: Option<NativeOperationReceipt>,
    pub post_state_status: String,
}

fn original_digest(snapshot: &Snapshot) -> Result<String> {
    canonical::digest(
        "snapshot-original",
        &(&snapshot.originals, &snapshot.archives, &snapshot.media),
    )
    .map_err(|e| e.to_string())
}

fn referenced_assets(snapshot: &Snapshot) -> std::collections::BTreeSet<String> {
    snapshot
        .archives
        .iter()
        .flat_map(|archive| {
            std::iter::once(archive.digest.clone()).chain(archive.asset_digests.clone())
        })
        .chain(snapshot.media.iter().map(|asset| asset.digest.clone()))
        .collect()
}

fn originals_linked(snapshot: &Snapshot) -> bool {
    let mut originals = std::collections::BTreeSet::new();
    let mut archives = std::collections::BTreeSet::new();
    let mut archived_sources = std::collections::BTreeSet::new();
    snapshot.originals.iter().all(|source| {
        !source.id.is_nil()
            && originals.insert(source.id)
            && snapshot.archives.iter().any(|archive| {
                archive.source_id == source.id
                    && archive.digest == source.digest
                    && archive.asset_digests.contains(&source.digest)
            })
    }) && snapshot.archives.iter().all(|archive| {
        !archive.id.is_nil()
            && archives.insert(archive.id)
            && archived_sources.insert(archive.source_id)
            && originals.contains(&archive.source_id)
    })
}

impl Store {
    /// Bounded read index; every returned record is fully revalidated.
    pub fn list_snapshots(&self, limit: u32) -> Result<Vec<SnapshotRecord>> {
        self.snapshot_page(None, limit).map(|(records, _)| records)
    }

    /// Keyset page; the cursor identifies the last scanned snapshot, including
    /// records later excluded by CLI filters.
    pub fn snapshot_page(
        &self,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<(Vec<SnapshotRecord>, Option<Uuid>)> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mut stmt = self
            .connection
            .prepare("SELECT id FROM snapshots WHERE id>?1 ORDER BY id LIMIT ?2")
            .map_err(sql)?;
        let ids = stmt
            .query_map(
                params![
                    after.map(|id| id.to_string()).unwrap_or_default(),
                    limit + 1
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(sql)?;
        let mut ids: Vec<Uuid> = ids
            .map(|id| Uuid::parse_str(&id.map_err(sql)?).map_err(|_| "SNAPSHOT_CORRUPT".into()))
            .collect::<Result<_>>()?;
        let has_more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next = if has_more { ids.last().copied() } else { None };
        let records = ids
            .into_iter()
            .map(|id| self.snapshot(id))
            .collect::<Result<_>>()?;
        Ok((records, next))
    }

    /// Publish originals before a native effect. The after-state is never part of this body.
    pub fn publish_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if snapshot.id.is_nil()
            || snapshot.operation_id.is_nil()
            || snapshot.before_digest != original_digest(snapshot)?
            || !originals_linked(snapshot)
        {
            return Err("SNAPSHOT_ORIGINAL_INVALID".into());
        }
        let assets = referenced_assets(snapshot);
        for digest in &assets {
            self.asset(digest, 100 * 1024 * 1024)?;
        }
        for media in &snapshot.media {
            if self.asset(&media.digest, media.size_bytes)?.len() as u64 != media.size_bytes {
                return Err("ASSET_MANIFEST_SIZE_MISMATCH".into());
            }
        }
        let body = canonical::bytes(snapshot).map_err(|e| e.to_string())?;
        let body_digest = canonical::asset_digest(&body);
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute(
            "INSERT INTO snapshots(id,operation,before_digest,body_digest,body) VALUES(?1,?2,?3,?4,?5)",
            params![snapshot.id.to_string(), snapshot.operation_id.to_string(), snapshot.before_digest, body_digest, body],
        ).map_err(sql)?;
        for digest in assets {
            tx.execute(
                "INSERT INTO snapshot_assets(snapshot,digest) VALUES(?1,?2)",
                params![snapshot.id.to_string(), digest],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(())
    }

    /// Retain one verified companion read-back. This storage receipt does not authorize
    /// journal finalization; the application must still reconcile the native effect.
    pub fn append_snapshot_after(
        &mut self,
        id: Uuid,
        receipt: &NativeOperationReceipt,
    ) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        receipt.validate().map_err(|e| e.to_string())?;
        if receipt.state != NativeReceiptState::Verified {
            return Err("SNAPSHOT_AFTER_NOT_VERIFIED".into());
        }
        let original = self.snapshot(id)?;
        let journal = self.journal(original.snapshot.operation_id)?;
        let step = journal
            .journal
            .steps
            .last()
            .ok_or("SNAPSHOT_JOURNAL_STEP_MISSING")?;
        if journal.journal.snapshot_id != id
            || step.state == StepState::IntentRecorded
            || receipt.operation_id != step.id
            || receipt.lineage_id != journal.journal.binding.lineage_id
            || !self.session_epoch_bound(&journal.journal, receipt.session_epoch)?
            || receipt.approved_digest != journal.journal.approval_digest
            || receipt.payload_digest != step.payload_digest
            || receipt
                .readback
                .as_ref()
                .is_none_or(|readback| readback.observed_state_digest != step.expected_post_digest)
        {
            return Err("SNAPSHOT_RECEIPT_CONFLICT".into());
        }
        self.asset(&receipt.evidence_digest, 100 * 1024 * 1024)?;
        let observed = &receipt
            .readback
            .as_ref()
            .ok_or("SNAPSHOT_AFTER_NOT_VERIFIED")?
            .observed_state_digest;
        self.asset(observed, 100 * 1024 * 1024)?;
        let body = canonical::bytes(receipt).map_err(|e| e.to_string())?;
        let body_digest = canonical::asset_digest(&body);
        self.connection.execute(
            "INSERT INTO snapshot_after(snapshot,operation,observed_digest,receipt_digest,body,actual_asset,evidence_asset) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id.to_string(), receipt.operation_id.to_string(), observed, body_digest, body, observed, receipt.evidence_digest],
        ).map_err(sql)?;
        Ok(())
    }

    /// The journal's own epoch, or one adopted by a recorded rebinding decision.
    fn session_epoch_bound(
        &self,
        journal: &linguist_core::records::OperationJournal,
        epoch: Uuid,
    ) -> Result<bool> {
        if journal.binding.session_epoch == epoch {
            return Ok(true);
        }
        Ok(self
            .binding_decisions(journal.id)?
            .iter()
            .any(|decision| decision.new_binding.session_epoch == epoch))
    }

    pub fn snapshot(&self, id: Uuid) -> Result<SnapshotRecord> {
        let row: Option<(String, String, String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT operation,before_digest,body_digest,body FROM snapshots WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(sql)?;
        let (operation, before, body_digest, body) = row.ok_or("SNAPSHOT_NOT_FOUND")?;
        if canonical::asset_digest(&body) != body_digest {
            return Err("SNAPSHOT_CORRUPT".into());
        }
        let snapshot: Snapshot = canonical::parse(&body).map_err(|_| "SNAPSHOT_CORRUPT")?;
        if snapshot.id != id
            || snapshot.operation_id.to_string() != operation
            || snapshot.before_digest != before
            || original_digest(&snapshot)? != before
            || !originals_linked(&snapshot)
        {
            return Err("SNAPSHOT_CORRUPT".into());
        }
        let assets = referenced_assets(&snapshot);
        let mut stmt = self
            .connection
            .prepare("SELECT digest FROM snapshot_assets WHERE snapshot=?1 ORDER BY digest")
            .map_err(sql)?;
        let indexed: std::collections::BTreeSet<String> = stmt
            .query_map([id.to_string()], |r| r.get(0))
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        if assets != indexed {
            return Err("SNAPSHOT_ASSET_INDEX_CORRUPT".into());
        }
        for digest in &assets {
            self.asset(digest, 100 * 1024 * 1024)?;
        }
        for media in &snapshot.media {
            if self.asset(&media.digest, media.size_bytes)?.len() as u64 != media.size_bytes {
                return Err("ASSET_MANIFEST_SIZE_MISMATCH".into());
            }
        }
        let after_row: Option<(String,String,String,Vec<u8>,String,String)> = self.connection.query_row(
            "SELECT operation,observed_digest,receipt_digest,body,actual_asset,evidence_asset FROM snapshot_after WHERE snapshot=?1",
            [id.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
        ).optional().map_err(sql)?;
        let after = if let Some((operation, observed, digest, body, actual_asset, evidence_asset)) =
            after_row
        {
            if canonical::asset_digest(&body) != digest {
                return Err("SNAPSHOT_AFTER_CORRUPT".into());
            }
            let receipt: NativeOperationReceipt =
                canonical::parse(&body).map_err(|_| "SNAPSHOT_AFTER_CORRUPT")?;
            let journal = self.journal(snapshot.operation_id)?;
            let step = journal
                .journal
                .steps
                .last()
                .ok_or("SNAPSHOT_JOURNAL_STEP_MISSING")?;
            if receipt.validate().is_err()
                || receipt.state != NativeReceiptState::Verified
                || receipt.operation_id.to_string() != operation
                || journal.journal.snapshot_id != id
                || step.state == StepState::IntentRecorded
                || receipt.operation_id != step.id
                || receipt.lineage_id != journal.journal.binding.lineage_id
                || !self.session_epoch_bound(&journal.journal, receipt.session_epoch)?
                || receipt.approved_digest != journal.journal.approval_digest
                || receipt.payload_digest != step.payload_digest
                || observed != step.expected_post_digest
                || receipt
                    .readback
                    .as_ref()
                    .is_none_or(|r| r.observed_state_digest != observed)
                || receipt.evidence_digest != evidence_asset
                || actual_asset != observed
            {
                return Err("SNAPSHOT_AFTER_CORRUPT".into());
            }
            self.asset(&actual_asset, 100 * 1024 * 1024)?;
            self.asset(&evidence_asset, 100 * 1024 * 1024)?;
            Some(receipt)
        } else {
            None
        };
        let post_state_status = if after.is_some() {
            "receipt_recorded"
        } else {
            "unknown"
        }
        .into();
        Ok(SnapshotRecord {
            snapshot,
            after,
            post_state_status,
        })
    }
}
