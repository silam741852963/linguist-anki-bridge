//! WP-24 duplicate merge.
//!
//! A duplicate group is merged into its survivor (the note with the most
//! repetitions). The survivor is revamped like any note; each merged note's
//! fields, tags and media are archived on the survivor's plan item, so
//! nothing it held is lost from the record. After the survivor is applied
//! and verified, each merged note is deleted through the companion
//! (`delete_merged_note`), bound to its unchanged content. Its complete
//! pre-delete evidence (note, cards with their schedule, review rows) is
//! stored first, so `restore_merged` can recreate it with the same IDs.
use crate::apply::{ApplyPort, Effect, MutationRequest, NativeStatus};
use linguist_core::{
    canonical,
    records::{PlanRevision, SourceArchive, SourceRecord},
};
use linguist_store::{Store, campaign::MergeDeletion};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const SOURCE_PREFIX: &str = "merged_anki_note:";

/// One merged note as read before the merge: its `inspect_note` evidence and
/// the bytes of the media it references (when the companion could read them).
pub struct MergedNote {
    pub evidence: Value,
    pub media: Vec<(String, Vec<u8>)>,
}

fn note_fields(evidence: &Value) -> BTreeMap<String, String> {
    evidence["note"]["fields"]
        .as_object()
        .map(|f| {
            f.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// Archive every merged note on `item` and publish the child revision.
pub fn add_merged_notes(
    store: &mut Store,
    base: &PlanRevision,
    digest: &str,
    item: Uuid,
    notes: &[MergedNote],
    max_asset_bytes: u64,
    now_seconds: u64,
) -> Result<PlanRevision, String> {
    if base.approval_digest().map_err(|e| e.to_string())? != digest
        || store.latest_revision(base.id)? != base.revision
    {
        return Err("MERGE_BASE_CONFLICT".into());
    }
    let mut child = base.clone();
    let document = child
        .documents
        .iter_mut()
        .find(|d| d.id == item)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    for merged in notes {
        let note_id = merged.evidence["note_id"]
            .as_str()
            .ok_or("MERGE_EVIDENCE_INVALID")?
            .to_owned();
        let location = format!("{SOURCE_PREFIX}{note_id}");
        if document.sources.iter().any(|s| s.location == location) {
            continue;
        }
        let survivor = document
            .sources
            .iter()
            .find_map(|s| s.location.strip_prefix("anki_note:"))
            .ok_or("MERGE_SURVIVOR_NOT_A_NOTE")?;
        if survivor == note_id {
            return Err("MERGE_SURVIVOR_IS_MERGED".into());
        }
        let record = json!({"schema": "linguist-merged-note-v1", "evidence": merged.evidence});
        let bytes = canonical::bytes(&record).map_err(|e| e.to_string())?;
        let record_digest = store.publish_asset(&bytes, max_asset_bytes.max(bytes.len() as u64))?;
        let mut assets = vec![record_digest.clone()];
        for (_, data) in &merged.media {
            assets.push(store.publish_asset(data, max_asset_bytes)?);
        }
        let fields = note_fields(&merged.evidence);
        let source_id = Uuid::new_v4();
        document.sources.push(SourceRecord {
            id: source_id,
            kind: "merged_note".into(),
            location,
            digest: record_digest.clone(),
            text: None,
            fields: fields.clone(),
            model_manifest: "linguist-merged-note-v1".into(),
            template_manifest: None,
            captured_at_unix_seconds: Some(now_seconds.max(1)),
            tags: merged.evidence["note"]["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect(),
            cards: vec![],
            media_refs: vec![],
        });
        document.archives.push(SourceArchive {
            id: Uuid::new_v4(),
            source_id,
            digest: record_digest,
            original_text: None,
            original_fields: fields,
            asset_digests: assets,
        });
    }
    document.issues.retain(|i| i.stage != "validation");
    let issues = linguist_core::validation::validate(document);
    document.issues = issues;
    child.revision = base.revision + 1;
    child.parent_digest = Some(digest.to_owned());
    child.rendered.clear();
    for document in &child.documents {
        let empty = BTreeMap::new();
        let fields = document
            .sources
            .first()
            .map(|s| &s.fields)
            .unwrap_or(&empty);
        if let Ok(rendered) = linguist_core::render::render(document, fields) {
            child.rendered.push(rendered);
        }
    }
    store.publish_revision(&child)?;
    Ok(child)
}

/// The merged notes a plan records for its survivor items.
pub fn merged_in(plan: &PlanRevision) -> Vec<(Uuid, String)> {
    plan.documents
        .iter()
        .flat_map(|d| {
            d.sources.iter().filter_map(move |s| {
                s.location
                    .strip_prefix(SOURCE_PREFIX)
                    .map(|n| (d.id, n.to_owned()))
            })
        })
        .collect()
}

fn decision_digest(record: &Value) -> Result<String, String> {
    canonical::digest("lab-merge-decision-v1", record).map_err(|e| e.to_string())
}

/// Send one recorded merge effect, or settle it from the ledger after a crash.
fn drive(
    port: &mut dyn ApplyPort,
    operation: Uuid,
    approval: &str,
    effect: Effect,
    sent: bool,
) -> Result<bool, String> {
    if sent {
        match port.status(operation)? {
            NativeStatus::Verified => return Ok(true),
            NativeStatus::Absent | NativeStatus::FailedBeforeWrite { .. } => {}
            NativeStatus::Unknown { reason } => {
                return Err(format!(
                    "MERGE_OUTCOME_UNKNOWN: operation {operation}: {reason}"
                ));
            }
            NativeStatus::Queued | NativeStatus::Running => {
                return Err(format!(
                    "MERGE_OPERATION_IN_PROGRESS: operation {operation}"
                ));
            }
        }
    }
    let binding = port.execution_binding()?;
    let owner = port.begin(&binding, approval)?;
    let request = MutationRequest {
        operation_id: operation,
        parent_operation_id: operation,
        approval_digest: approval.to_owned(),
        binding,
        owner: owner.clone(),
        payload_digest: effect.payload_digest()?,
        effect,
    };
    let result = port.mutate(&request);
    let _ = port.end(&owner);
    match result {
        Ok(NativeStatus::Verified) => Ok(true),
        Ok(NativeStatus::FailedBeforeWrite { reason })
        | Err(crate::backup::PortFailure::Rejected(reason)) => {
            Err(format!("MERGE_REFUSED: {reason}"))
        }
        Ok(other) => Err(format!(
            "MERGE_OUTCOME_UNKNOWN: operation {operation}: {other:?}"
        )),
        Err(crate::backup::PortFailure::Unknown(reason)) => Err(format!(
            "MERGE_OUTCOME_UNKNOWN: operation {operation}: {reason}"
        )),
    }
}

/// Delete one merged note after its survivor was applied. `evidence` is the
/// note's current `inspect_note` evidence (read by the caller).
pub fn delete_merged(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    plan: &PlanRevision,
    note_id: &str,
    survivor: &str,
    evidence: Option<&Value>,
    now_ms: u64,
) -> Result<Value, String> {
    let note: i64 = note_id.parse().map_err(|_| "MERGE_NOTE_ID_INVALID")?;
    let survivor_id: i64 = survivor.parse().map_err(|_| "MERGE_NOTE_ID_INVALID")?;
    let record = match store.merge_deletion(note_id)? {
        Some(record) if record.state == "deleted" || record.state == "restored" => {
            return Ok(json!({"note_id": note_id, "state": record.state, "noop": true}));
        }
        Some(record) => record,
        None => {
            if !merged_in(plan).iter().any(|(doc, n)| {
                n == note_id
                    && plan.documents.iter().any(|d| {
                        d.id == *doc
                            && d.sources
                                .iter()
                                .any(|s| s.location == format!("anki_note:{survivor}"))
                    })
            }) {
                return Err(
                    "MERGE_NOT_RECORDED: the plan does not merge this note into the survivor"
                        .into(),
                );
            }
            let evidence = evidence.ok_or("MERGE_NOTE_MISSING: the merged note does not exist")?;
            // The note must still hold what the plan archived.
            let archived = plan
                .documents
                .iter()
                .flat_map(|d| &d.sources)
                .find(|s| s.location == format!("{SOURCE_PREFIX}{note_id}"))
                .ok_or("MERGE_NOT_RECORDED")?;
            if note_fields(evidence) != archived.fields {
                return Err(
                    "MERGE_NOTE_CHANGED: the merged note changed since the merge was recorded"
                        .into(),
                );
            }
            let observed = port.note(note)?.ok_or("MERGE_NOTE_MISSING")?;
            let survivor_note = port.note(survivor_id)?.ok_or("MERGE_SURVIVOR_MISSING")?;
            // Only after the survivor was applied: it is on the plan's target type.
            let target = plan
                .documents
                .iter()
                .find(|d| {
                    d.sources
                        .iter()
                        .any(|s| s.location == format!("anki_note:{survivor}"))
                })
                .and_then(|d| plan.rendered.iter().find(|r| r.document_id == d.id))
                .map(|r| r.model.name.clone())
                .ok_or("MERGE_SURVIVOR_NOT_RENDERED")?;
            if survivor_note.model_name != target {
                return Err(format!(
                    "MERGE_SURVIVOR_NOT_APPLIED: survivor {survivor} is {} (expected {target})",
                    survivor_note.model_name
                ));
            }
            let expected_pre_digest = crate::apply::content_digest(&observed)?;
            let bytes = canonical::bytes(evidence).map_err(|e| e.to_string())?;
            let evidence_asset = store.publish_asset(&bytes, (bytes.len() as u64).max(1))?;
            let decision = json!({"note_id": note_id, "survivor": survivor, "plan": plan.id,
                "expected_pre_digest": expected_pre_digest, "evidence_asset": evidence_asset});
            let record = MergeDeletion {
                note_id: note_id.into(),
                survivor: survivor.into(),
                plan: plan.id,
                operation_id: Uuid::new_v4(),
                approval_digest: decision_digest(&decision)?,
                expected_pre_digest,
                survivor_model: target,
                evidence_asset,
                state: "intent".into(),
                restore_operation: None,
                created_ms: now_ms,
                updated_ms: now_ms,
            };
            store.record_merge_deletion(&record)?;
            record
        }
    };
    let sent = record.state == "sent";
    if !sent {
        store.set_merge_deletion_state(note_id, "intent", "sent", None, now_ms)?;
    }
    let effect = Effect::DeleteMergedNote {
        note_id: note,
        expected_pre_digest: record.expected_pre_digest.clone(),
        survivor_note_id: survivor_id,
        survivor_model_name: record.survivor_model.clone(),
    };
    match drive(
        port,
        record.operation_id,
        &record.approval_digest,
        effect,
        sent,
    ) {
        Ok(_) => {
            store.set_merge_deletion_state(note_id, "sent", "deleted", None, now_ms)?;
            Ok(
                json!({"note_id": note_id, "survivor": survivor, "state": "deleted",
                "operation_id": record.operation_id, "evidence_asset": record.evidence_asset}),
            )
        }
        Err(error) if error.starts_with("MERGE_REFUSED") => {
            store.set_merge_deletion_state(note_id, "sent", "intent", None, now_ms)?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

/// Recreate a deleted merged note from its stored evidence.
pub fn restore_merged(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    note_id: &str,
    max_asset_bytes: u64,
    now_ms: u64,
) -> Result<Value, String> {
    let record = store
        .merge_deletion(note_id)?
        .ok_or("MERGE_DELETION_NOT_FOUND")?;
    let sent = match record.state.as_str() {
        "restored" => return Ok(json!({"note_id": note_id, "state": "restored", "noop": true})),
        "deleted" => false,
        "restore_sent" => true,
        other => return Err(format!("MERGE_NOT_DELETED: state {other}")),
    };
    let evidence: Value = canonical::parse(&store.asset(&record.evidence_asset, max_asset_bytes)?)
        .map_err(|_| "MERGE_EVIDENCE_CORRUPT")?;
    let int = |v: &Value| -> Result<i64, String> {
        match v {
            Value::String(s) => s.parse().map_err(|_| "MERGE_EVIDENCE_CORRUPT".into()),
            Value::Number(n) => n.as_i64().ok_or_else(|| "MERGE_EVIDENCE_CORRUPT".into()),
            _ => Err("MERGE_EVIDENCE_CORRUPT".into()),
        }
    };
    let fields: Vec<Value> = evidence["model"]["fields"]
        .as_array()
        .ok_or("MERGE_EVIDENCE_CORRUPT")?
        .iter()
        .map(|name| evidence["note"]["fields"][name.as_str().unwrap_or_default()].clone())
        .collect();
    let mut cards = Vec::new();
    let mut reviews = Vec::new();
    for card in evidence["cards"]
        .as_array()
        .ok_or("MERGE_EVIDENCE_CORRUPT")?
    {
        cards.push(json!({
            "id": int(&card["id"])?, "ordinal": card["ordinal"], "deck_id": int(&card["deck_id"])?,
            "original_deck_id": int(&card["original_deck_id"])?, "type": card["type"],
            "queue": card["queue"], "due": card["due"], "interval": card["interval"],
            "ease_factor": card["ease_factor"], "repetitions": card["repetitions"],
            "lapses": card["lapses"], "remaining_steps": card["remaining_steps"],
            "original_due": card["original_due"], "flags": card["flags"],
            "custom_data": card["custom_data"].as_str().unwrap_or_default(),
        }));
        for review in card["reviews"].as_array().into_iter().flatten() {
            reviews.push(review.clone());
        }
    }
    let effect = Effect::RecreateDeletedNote {
        note: json!({"id": int(&evidence["note_id"])?, "guid": evidence["note"]["guid"],
            "model_id": int(&evidence["note"]["model_id"])?, "fields": fields,
            "tags": evidence["note"]["tags"]}),
        cards,
        reviews,
        expected_absent: true,
    };
    let operation = match record.restore_operation {
        Some(operation) if sent => operation,
        _ => {
            let operation = Uuid::new_v4();
            store.set_merge_deletion_state(
                note_id,
                "deleted",
                "restore_sent",
                Some(operation),
                now_ms,
            )?;
            operation
        }
    };
    let approval = decision_digest(
        &json!({"restore": note_id, "evidence_asset": record.evidence_asset,
        "operation": operation}),
    )?;
    match drive(port, operation, &approval, effect, sent) {
        Ok(_) => {
            store.set_merge_deletion_state(note_id, "restore_sent", "restored", None, now_ms)?;
            Ok(json!({"note_id": note_id, "state": "restored", "operation_id": operation}))
        }
        Err(error) if error.starts_with("MERGE_REFUSED") => {
            store.set_merge_deletion_state(note_id, "restore_sent", "deleted", None, now_ms)?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}
