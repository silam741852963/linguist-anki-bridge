use linguist_core::{LearningDocument, canonical, records::*};
use linguist_store::{Store, preparation::*};
use serde_json::json;
use std::collections::BTreeMap;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lab-prepare-store-{}", uuid::Uuid::new_v4())))
    }
    fn open(&self) -> Store {
        Store::open(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn definition() -> PreparationDefinition {
    let values = BTreeMap::from([
        ("jobs.max_item_attempts".into(), json!(2)),
        ("input.max_file_mb".into(), json!(10)),
        ("selection.max_notes".into(), json!(5)),
        ("selection.order".into(), json!("input")),
    ]);
    PreparationDefinition {
        schema_version: 1,
        created_at: "fixture".into(),
        selection: SelectionReceipt {
            schema_version: 1,
            purpose: "english_vocab".into(),
            selector: SelectionInput::NoteIds(vec!["123".into(), "124".into()]),
            matched_note_ids: vec!["123".into(), "124".into()],
            selected_note_ids: vec!["123".into(), "124".into()],
            order: "input".into(),
            max_notes: 5,
            command_limit: None,
        },
        job: Job {
            id: uuid::Uuid::new_v4(),
            mode: JobMode::Prepare,
            settings: ResolvedSettings {
                semantic_fingerprint: String::new(),
                execution_fingerprint: String::new(),
                version: 2,
                fingerprint: canonical::digest("resolved-settings", &values).unwrap(),
                values,
                provenance: BTreeMap::new(),
                resource_hashes: BTreeMap::new(),
                secret_refs: BTreeMap::new(),
            },
            plan_refs: vec!["anki-note:123".into(), "anki-note:124".into()],
            item_ids: vec![uuid::Uuid::new_v4(), uuid::Uuid::new_v4()],
            pause_requested: false,
            cancel_requested: false,
        },
    }
}
fn capture(hash: &str) -> LearningDocument {
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    doc.target_language = "en".to_string().try_into().unwrap();
    let id = uuid::Uuid::new_v4();
    doc.sources.push(SourceRecord {
        id,
        kind: "anki_read_capture_v2".into(),
        location: "anki_note:123".into(),
        digest: hash.into(),
        text: None,
        fields: BTreeMap::new(),
        model_manifest: "original".into(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    doc.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id: id,
        digest: hash.into(),
        original_text: None,
        original_fields: BTreeMap::new(),
        asset_digests: vec![hash.into()],
    });
    doc
}

fn capture_items(store: &mut Store, definition: &PreparationDefinition) -> String {
    let hash = store.publish_asset(b"retained originals", 1000).unwrap();
    let mut head = None;
    for (item, note) in definition
        .job
        .item_ids
        .iter()
        .zip(&definition.selection.selected_note_ids)
    {
        let started = store
            .append_preparation_event(
                definition.job.id,
                *item,
                1,
                PreparationStage::Started,
                head.as_deref(),
            )
            .unwrap();
        let mut doc = capture(&hash);
        doc.id = uuid::Uuid::new_v4();
        doc.sources[0].location = format!("anki_note:{note}");
        let captured = store
            .append_preparation_event(
                definition.job.id,
                *item,
                1,
                PreparationStage::Captured {
                    document: Box::new(doc),
                },
                Some(&started.digest),
            )
            .unwrap();
        head = Some(captured.digest);
    }
    head.unwrap()
}

#[test]
fn captured_event_reads_reject_missing_asset_index_links() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    store.create_preparation_job(&definition).unwrap();
    capture_items(&mut store, &definition);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TRIGGER preparation_assets_no_delete")
        .unwrap();
    db.execute(
        "DELETE FROM preparation_event_assets WHERE job_id=?1 AND sequence=2",
        [definition.job.id.to_string()],
    )
    .unwrap();
    assert_eq!(
        store
            .preparation_events(definition.job.id, 0, 4)
            .unwrap_err(),
        "PREPARATION_ASSET_INDEX_CORRUPT"
    );
    assert_eq!(
        store
            .preparation_items(definition.job.id, 0, 2)
            .unwrap_err(),
        "PREPARATION_ASSET_INDEX_CORRUPT"
    );
}

#[test]
fn audit_pages_verify_chain_anchors_and_reject_rehashed_broken_links() {
    use linguist_store::preparation_control::ControlAction;
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    let pause = store
        .request_preparation_control(id, ControlAction::Pause)
        .unwrap();
    let resume = store
        .request_preparation_control(id, ControlAction::Resume)
        .unwrap();
    let mut cancel = store
        .request_preparation_control(id, ControlAction::Cancel)
        .unwrap();
    assert_eq!(
        store.preparation_controls(id, 1, 1).unwrap()[0].digest,
        resume.digest
    );
    assert_eq!(
        store.preparation_controls(id, 2, 1).unwrap()[0].digest,
        cancel.digest
    );
    assert!(store.preparation_controls(id, 100, 1).unwrap().is_empty());
    assert_eq!(
        store.preparation_controls(id, 0, 10).unwrap()[0].digest,
        pause.digest
    );
    let first = store
        .append_preparation_event(
            id,
            definition.job.item_ids[0],
            1,
            PreparationStage::Started,
            None,
        )
        .unwrap();
    let mut failed = store
        .append_preparation_event(
            id,
            definition.job.item_ids[0],
            1,
            PreparationStage::Failed {
                code: "SOURCE_READ_TIMEOUT".into(),
                retry_eligible: true,
            },
            Some(&first.digest),
        )
        .unwrap();
    let mut sequences = Vec::new();
    store
        .visit_preparation_events(id, 0, 10, |receipt| {
            sequences.push(receipt.event.sequence);
            Ok(())
        })
        .unwrap();
    assert_eq!(sequences, [1, 2]);
    assert!(store.preparation_events(id, 100, 1).unwrap().is_empty());
    cancel.event.parent_digest = None;
    failed.event.parent_digest = None;
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TRIGGER preparation_controls_no_update;DROP TRIGGER preparation_events_no_update;",
    )
    .unwrap();
    db.execute(
        "UPDATE preparation_controls SET digest=?1,body=?2 WHERE job_id=?3 AND sequence=3",
        rusqlite::params![
            canonical::digest("preparation-control", &cancel.event).unwrap(),
            canonical::bytes(&cancel.event).unwrap(),
            id.to_string()
        ],
    )
    .unwrap();
    db.execute(
        "UPDATE preparation_events SET digest=?1,body=?2 WHERE job_id=?3 AND sequence=2",
        rusqlite::params![
            canonical::digest("preparation-event", &failed.event).unwrap(),
            canonical::bytes(&failed.event).unwrap(),
            id.to_string()
        ],
    )
    .unwrap();
    assert_eq!(
        store.preparation_controls(id, 2, 1).unwrap_err(),
        "PREPARATION_CONTROL_CHAIN_CORRUPT"
    );
    assert!(store.preparation_controls(id, 0, 10).is_err());
    assert_eq!(
        store.preparation_events(id, 1, 1).unwrap_err(),
        "PREPARATION_EVENT_CORRUPT"
    );
    assert!(store.preparation_head(id).is_err());
    db.execute(
        "UPDATE preparation_controls SET digest='forged' WHERE job_id=?1 AND sequence=2",
        [id.to_string()],
    )
    .unwrap();
    assert_eq!(
        store.preparation_controls(id, 2, 1).unwrap_err(),
        "PREPARATION_CONTROL_CORRUPT"
    );
    db.execute(
        "UPDATE preparation_events SET body=zeroblob(10485761) WHERE job_id=?1 AND sequence=1",
        [id.to_string()],
    )
    .unwrap();
    assert_eq!(
        store.preparation_events(id, 1, 1).unwrap_err(),
        "PREPARATION_EVENT_CORRUPT"
    );
}

#[test]
fn controls_are_immutable_idempotent_and_gate_dispatch_without_losing_results() {
    use linguist_store::{lease::Resource, preparation_control::ControlAction};
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    let lease = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    let pause = store
        .request_preparation_control(id, ControlAction::Pause)
        .unwrap();
    assert_eq!(
        pause.digest,
        store
            .request_preparation_control(id, ControlAction::Pause)
            .unwrap()
            .digest
    );
    assert_eq!(
        store
            .append_preparation_event_with_lease(
                id,
                definition.job.item_ids[0],
                1,
                PreparationStage::Started,
                None,
                &lease
            )
            .unwrap_err(),
        "PREPARATION_CONTROL_BLOCKS_DISPATCH"
    );
    let resume = store
        .request_preparation_control(id, ControlAction::Resume)
        .unwrap();
    assert_eq!(
        resume.event.parent_digest.as_deref(),
        Some(pause.digest.as_str())
    );
    let started = store
        .append_preparation_event_with_lease(
            id,
            definition.job.item_ids[0],
            1,
            PreparationStage::Started,
            None,
            &lease,
        )
        .unwrap();
    let cancel = store
        .request_preparation_control(id, ControlAction::Cancel)
        .unwrap();
    assert_eq!(
        store
            .request_preparation_control(id, ControlAction::Resume)
            .unwrap_err(),
        "PREPARATION_CANCEL_IS_TERMINAL"
    );
    assert_eq!(
        store
            .append_preparation_event_with_lease(
                id,
                definition.job.item_ids[1],
                1,
                PreparationStage::Started,
                Some(&started.digest),
                &lease
            )
            .unwrap_err(),
        "PREPARATION_CONTROL_BLOCKS_DISPATCH"
    );
    store
        .append_preparation_event_with_lease(
            id,
            definition.job.item_ids[0],
            1,
            PreparationStage::Failed {
                code: "SOURCE_READ_TIMEOUT".into(),
                retry_eligible: true,
            },
            Some(&started.digest),
            &lease,
        )
        .unwrap();
    store.release_lease(&lease).unwrap();
    assert_eq!(store.preparation_job(id).unwrap(), definition);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE preparation_controls SET digest='changed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM preparation_controls", []).is_err());
    drop(db);
    drop(store);
    let store = Store::read_only(&f.0).unwrap();
    assert_eq!(
        store.preparation_control(id).unwrap().unwrap().digest,
        cancel.digest
    );
    assert_eq!(
        store.preparation_items(id, 0, 2).unwrap()[0].state,
        "failed"
    );
    assert_eq!(
        store.preparation_items(id, 0, 2).unwrap()[1].state,
        "pending"
    );
}

#[test]
fn schema_six_upgrade_preserves_queued_jobs_and_backs_up_before_controls() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    store.create_preparation_job(&definition).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE preparation_controls;PRAGMA user_version=6;")
        .unwrap();
    drop(db);
    assert!(Store::read_only(&f.0).is_err());
    let store = f.open();
    assert_eq!(
        store.preparation_job(definition.job.id).unwrap(),
        definition
    );
    assert!(
        store
            .preparation_control(definition.job.id)
            .unwrap()
            .is_none()
    );
    let backup = std::fs::read_dir(&f.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".schema-v6-")
        })
        .unwrap();
    let db = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        6
    );
    assert_eq!(
        db.pragma_query_value(None, "integrity_check", |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn complete_capture_plan_reopens_idempotently_and_preserves_review_children() {
    use linguist_store::lease::Resource;
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    let wrong = store
        .acquire_lease(&Resource::CollectionWriter(id), 60)
        .unwrap();
    assert_eq!(
        store
            .publish_preparation_plan(id, None, &wrong)
            .unwrap_err(),
        "LEASE_RESOURCE_CONFLICT"
    );
    store.release_lease(&wrong).unwrap();
    let lease = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    assert!(
        store
            .publish_preparation_plan(id, None, &lease)
            .unwrap()
            .is_none()
    );
    store.release_lease(&lease).unwrap();
    let head = capture_items(&mut store, &definition);
    // Publication can resume from retained checkpoints without a read client.
    drop(store);
    let mut store = f.open();
    let lease = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    assert_eq!(
        store
            .publish_preparation_plan(id, None, &lease)
            .unwrap_err(),
        "PREPARATION_HEAD_CONFLICT"
    );
    store
        .request_preparation_control(
            id,
            linguist_store::preparation_control::ControlAction::Pause,
        )
        .unwrap();
    assert_eq!(
        store
            .publish_preparation_plan(id, Some(&head), &lease)
            .unwrap_err(),
        "PREPARATION_CONTROL_BLOCKS_PUBLICATION"
    );
    assert!(store.list_revisions(10).unwrap().is_empty());
    assert!(
        store
            .preparation_items(id, 0, 10)
            .unwrap()
            .iter()
            .all(|item| item.state == "captured")
    );
    store
        .request_preparation_control(
            id,
            linguist_store::preparation_control::ControlAction::Resume,
        )
        .unwrap();
    let receipt = store
        .publish_preparation_plan(id, Some(&head), &lease)
        .unwrap()
        .unwrap();
    let mut plan = store.revision(id, 1).unwrap();
    assert_eq!(plan.selection.as_ref().unwrap(), &definition.selection);
    assert_eq!(plan.settings, definition.job.settings);
    assert_eq!(
        plan.documents
            .iter()
            .map(|d| d.sources[0].location.as_str())
            .collect::<Vec<_>>(),
        ["anki_note:123", "anki_note:124"]
    );
    plan.revision = 2;
    plan.parent_digest = Some(receipt.digest.clone());
    plan.documents[0]
        .issues
        .push(linguist_core::validation::Issue::new(
            "REVIEW_NOTE",
            linguist_core::validation::Severity::Review,
            None,
            "retained review",
        ));
    store.publish_revision(&plan).unwrap();
    let repeated = store
        .publish_preparation_plan(id, Some(&head), &lease)
        .unwrap()
        .unwrap();
    assert_eq!(repeated.digest, receipt.digest);
    assert_eq!(store.latest_revision(id).unwrap(), 2);
    assert_eq!(store.revision(id, 2).unwrap(), plan);
    store.release_lease(&lease).unwrap();
    assert!(
        store
            .publish_preparation_plan(id, Some(&head), &lease)
            .is_err()
    );
}

#[test]
fn matching_approval_projection_cannot_hide_conflicting_published_capture_evidence() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    let head = capture_items(&mut store, &definition);
    let documents: Vec<_> = store
        .preparation_events(id, 0, 100)
        .unwrap()
        .into_iter()
        .filter_map(|receipt| {
            if let PreparationStage::Captured { document } = receipt.event.stage {
                Some(*document)
            } else {
                None
            }
        })
        .collect();
    let sources: Vec<_> = documents.iter().flat_map(|d| &d.sources).collect();
    let mut plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id,
        revision: 1,
        parent_digest: None,
        settings: definition.job.settings,
        binding: None,
        source_digest: canonical::digest("source-capture", &sources).unwrap(),
        selection: Some(definition.selection),
        documents,
        rendered: vec![],
        review_decisions: vec![],
    };
    let digest = plan.approval_digest().unwrap();
    plan.documents[0]
        .issues
        .push(linguist_core::validation::Issue::new(
            "FORGED_REVIEW",
            linguist_core::validation::Severity::Review,
            None,
            "changed evidence",
        ));
    assert_eq!(plan.approval_digest().unwrap(), digest);
    store.publish_revision(&plan).unwrap();
    let lease = store
        .acquire_lease(&linguist_store::lease::Resource::JobWorker(id), 60)
        .unwrap();
    assert_eq!(
        store
            .publish_preparation_plan(id, Some(&head), &lease)
            .unwrap_err(),
        "PREPARATION_PLAN_CONFLICT"
    );
    store.release_lease(&lease).unwrap();
}

#[test]
fn batch_archive_limit_retains_completed_checkpoints_without_publishing_a_plan() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut definition = definition();
    definition
        .job
        .settings
        .values
        .insert("input.max_file_mb".into(), json!(1));
    definition.job.settings.fingerprint =
        canonical::digest("resolved-settings", &definition.job.settings.values).unwrap();
    let id = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    let mut head = None;
    for (index, (item, note)) in definition
        .job
        .item_ids
        .iter()
        .zip(&definition.selection.selected_note_ids)
        .enumerate()
    {
        let bytes = vec![index as u8; 600 * 1024];
        let digest = store.publish_asset(&bytes, 1024 * 1024).unwrap();
        let started = store
            .append_preparation_event(id, *item, 1, PreparationStage::Started, head.as_deref())
            .unwrap();
        let mut doc = capture(&digest);
        doc.id = uuid::Uuid::new_v4();
        doc.sources[0].location = format!("anki_note:{note}");
        let captured = store
            .append_preparation_event(
                id,
                *item,
                1,
                PreparationStage::Captured {
                    document: Box::new(doc),
                },
                Some(&started.digest),
            )
            .unwrap();
        head = Some(captured.digest);
    }
    let lease = store
        .acquire_lease(&linguist_store::lease::Resource::JobWorker(id), 60)
        .unwrap();
    assert_eq!(
        store
            .publish_preparation_plan(id, head.as_deref(), &lease)
            .unwrap_err(),
        "REVAMP_BATCH_ARCHIVE_LIMIT"
    );
    assert!(store.list_revisions(10).unwrap().is_empty());
    assert!(
        store
            .preparation_items(id, 0, 10)
            .unwrap()
            .iter()
            .all(|item| item.state == "captured")
    );
    store.release_lease(&lease).unwrap();
}

#[test]
fn worker_checkpoints_reject_wrong_expired_and_released_fencing_tokens() {
    use linguist_store::lease::Resource;
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    let item = definition.job.item_ids[0];
    store.create_preparation_job(&definition).unwrap();
    let wrong = store
        .acquire_lease(&Resource::JobWorker(uuid::Uuid::new_v4()), 60)
        .unwrap();
    assert_eq!(
        store
            .append_preparation_event_with_lease(
                id,
                item,
                1,
                PreparationStage::Started,
                None,
                &wrong
            )
            .unwrap_err(),
        "LEASE_RESOURCE_CONFLICT"
    );
    store.release_lease(&wrong).unwrap();
    let first = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    let started = store
        .append_preparation_event_with_lease(id, item, 1, PreparationStage::Started, None, &first)
        .unwrap();
    store.release_lease(&first).unwrap();
    let second = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    let failure = PreparationStage::Failed {
        code: "SOURCE_READ_TIMEOUT".into(),
        retry_eligible: true,
    };
    assert_eq!(
        store
            .append_preparation_event_with_lease(
                id,
                item,
                1,
                failure.clone(),
                Some(&started.digest),
                &first
            )
            .unwrap_err(),
        "LEASE_STALE_OR_EXPIRED"
    );
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute(
        "UPDATE leases SET expires_ms=0 WHERE resource=?1",
        [format!("job:{id}")],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        store
            .append_preparation_event_with_lease(
                id,
                item,
                1,
                failure.clone(),
                Some(&started.digest),
                &second
            )
            .unwrap_err(),
        "LEASE_STALE_OR_EXPIRED"
    );
    assert_eq!(store.preparation_events(id, 0, 10).unwrap().len(), 1);
    assert_eq!(
        store.preparation_items(id, 0, 1).unwrap()[0].state,
        "started"
    );
    store.renew_lease(&second, 60).unwrap();
    store
        .append_preparation_event_with_lease(id, item, 1, failure, Some(&started.digest), &second)
        .unwrap();
    assert_eq!(store.preparation_events(id, 0, 10).unwrap().len(), 2);
    store.release_lease(&second).unwrap();
}

#[test]
fn preparation_rejects_inconsistent_split_settings_fingerprints() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut definition = definition();
    let (semantic, execution) = setting_fingerprints(&definition.job.settings.values).unwrap();
    definition.job.settings.semantic_fingerprint = semantic;
    definition.job.settings.execution_fingerprint = execution;
    store.create_preparation_job(&definition).unwrap();

    let mut forged = definition.clone();
    forged.job.id = uuid::Uuid::new_v4();
    forged.job.settings.semantic_fingerprint = "forged".into();
    forged.job.settings.execution_fingerprint =
        setting_fingerprints(&forged.job.settings.values).unwrap().1;
    assert_eq!(
        store.create_preparation_job(&forged).unwrap_err(),
        "INVALID_PREPARATION_SETTINGS"
    );
}

#[test]
fn definitions_events_and_original_assets_survive_reopen_with_cas_conflicts() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let digest = store.create_preparation_job(&definition).unwrap();
    assert_eq!(store.create_preparation_job(&definition).unwrap(), digest);
    let mut changed = definition.clone();
    changed.created_at = "different".into();
    assert_eq!(
        store.create_preparation_job(&changed).unwrap_err(),
        "PREPARATION_JOB_CONFLICT"
    );
    let id = definition.job.id;
    let item = definition.job.item_ids[0];
    assert!(store.preparation_head(id).unwrap().is_none());
    let started = store
        .append_preparation_event(id, item, 1, PreparationStage::Started, None)
        .unwrap();
    assert_eq!(
        store.preparation_head(id).unwrap().unwrap().digest,
        started.digest
    );
    assert!(
        store
            .append_preparation_event(
                id,
                item,
                1,
                PreparationStage::Failed {
                    code: "SOURCE_IDENTITY_CONFLICT".into(),
                    retry_eligible: true,
                },
                Some(&started.digest)
            )
            .is_err()
    );
    let mut second = f.open();
    assert_eq!(
        second
            .append_preparation_event(
                id,
                definition.job.item_ids[1],
                1,
                PreparationStage::Started,
                None
            )
            .unwrap_err(),
        "PREPARATION_HEAD_CONFLICT"
    );
    let hash = canonical::asset_digest(b"original capture bytes");
    let stage = PreparationStage::Captured {
        document: Box::new(capture(&hash)),
    };
    assert!(
        store
            .append_preparation_event(id, item, 1, stage.clone(), Some(&started.digest))
            .is_err()
    );
    store
        .publish_asset(b"original capture bytes", 1000)
        .unwrap();
    let captured = store
        .append_preparation_event(id, item, 1, stage, Some(&started.digest))
        .unwrap();
    assert!(
        store
            .append_preparation_event(
                id,
                item,
                2,
                PreparationStage::Started,
                Some(&captured.digest)
            )
            .is_err()
    );
    drop(second);
    drop(store);
    let store = Store::read_only(&f.0).unwrap();
    assert_eq!(store.preparation_job(id).unwrap(), definition);
    let head = store.preparation_head(id).unwrap().unwrap();
    assert_eq!(head.event.sequence, 2);
    assert_eq!(head.digest, captured.digest);
    let events = store.preparation_events(id, 0, 1).unwrap();
    assert_eq!(events[0].digest, started.digest);
    assert_eq!(
        store.preparation_events(id, 1, 100).unwrap()[0].event,
        captured.event
    );
    assert_eq!(store.asset(&hash, 1000).unwrap(), b"original capture bytes");
    let summaries = store.list_preparation_jobs(None, 1).unwrap();
    assert_eq!(summaries[0].item_count, 2);
    assert_eq!(summaries[0].checkpoint_count, 2);
    assert!(store.list_preparation_jobs(Some(id), 1).unwrap().is_empty());
    let items = store.preparation_items(id, 0, 1).unwrap();
    assert_eq!(items[0].state, "captured");
    assert_eq!(items[0].checkpoint_digest.as_ref(), Some(&captured.digest));
    let pending = store.preparation_items(id, 1, 1).unwrap();
    assert_eq!(pending[0].state, "pending");
    assert_eq!(pending[0].input_ref, "anki-note:124");
}

#[test]
fn retries_are_explicit_classified_bounded_and_never_assume_a_worker_died() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    store.create_preparation_job(&definition).unwrap();
    let id = definition.job.id;
    let item = definition.job.item_ids[0];
    let started = store
        .append_preparation_event(id, item, 1, PreparationStage::Started, None)
        .unwrap();
    assert!(
        store
            .append_preparation_event(
                id,
                item,
                2,
                PreparationStage::Started,
                Some(&started.digest)
            )
            .is_err()
    );
    let failed = store
        .append_preparation_event(
            id,
            item,
            1,
            PreparationStage::Failed {
                code: "SOURCE_READ_TIMEOUT".into(),
                retry_eligible: true,
            },
            Some(&started.digest),
        )
        .unwrap();
    let retry = store
        .append_preparation_event(id, item, 2, PreparationStage::Started, Some(&failed.digest))
        .unwrap();
    let failed = store
        .append_preparation_event(
            id,
            item,
            2,
            PreparationStage::Failed {
                code: "SOURCE_READ_TIMEOUT".into(),
                retry_eligible: true,
            },
            Some(&retry.digest),
        )
        .unwrap();
    assert!(
        store
            .append_preparation_event(id, item, 3, PreparationStage::Started, Some(&failed.digest))
            .is_err()
    );
    let other = definition.job.item_ids[1];
    let started = store
        .append_preparation_event(
            id,
            other,
            1,
            PreparationStage::Started,
            Some(&failed.digest),
        )
        .unwrap();
    let failed = store
        .append_preparation_event(
            id,
            other,
            1,
            PreparationStage::Failed {
                code: "SOURCE_IDENTITY_CONFLICT".into(),
                retry_eligible: false,
            },
            Some(&started.digest),
        )
        .unwrap();
    assert!(
        store
            .append_preparation_event(
                id,
                other,
                2,
                PreparationStage::Started,
                Some(&failed.digest)
            )
            .is_err()
    );
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE preparation_events SET digest='changed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM preparation_jobs", []).is_err());
}

#[test]
fn equivalent_float_settings_are_idempotent_but_changed_settings_conflict() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut definition = definition();
    definition
        .job
        .settings
        .values
        .insert("audio.speed".into(), json!(1.0));
    definition.job.settings.fingerprint =
        canonical::digest("resolved-settings", &definition.job.settings.values).unwrap();
    let digest = store.create_preparation_job(&definition).unwrap();
    assert_eq!(store.create_preparation_job(&definition).unwrap(), digest);
    let reopened = store.preparation_job(definition.job.id).unwrap();
    assert_eq!(store.create_preparation_job(&reopened).unwrap(), digest);
    definition
        .job
        .settings
        .values
        .insert("audio.speed".into(), json!(1.5));
    definition.job.settings.fingerprint =
        canonical::digest("resolved-settings", &definition.job.settings.values).unwrap();
    assert_eq!(
        store.create_preparation_job(&definition).unwrap_err(),
        "PREPARATION_JOB_CONFLICT"
    );
}

#[test]
fn migration_from_schema_five_backs_up_and_preserves_old_state() {
    let f = Fixture::new();
    drop(f.open());
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE preparation_controls;DROP TABLE preparation_event_assets;DROP TABLE preparation_events;DROP TABLE preparation_jobs;PRAGMA user_version=5;").unwrap();
    drop(db);
    let mut store = f.open();
    let definition = definition();
    store.create_preparation_job(&definition).unwrap();
    drop(store);
    let backup = std::fs::read_dir(&f.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".schema-v5-")
        })
        .unwrap();
    let db = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        db.pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn interrupted_read_reconciliation_requires_fencing_and_keeps_attempt_limits() {
    use linguist_store::lease::Resource;
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let job = definition.job.id;
    let item = definition.job.item_ids[0];
    store.create_preparation_job(&definition).unwrap();
    let started = store
        .append_preparation_event(job, item, 1, PreparationStage::Started, None)
        .unwrap();
    assert!(
        store
            .append_preparation_event(
                job,
                item,
                1,
                PreparationStage::Interrupted {
                    actor: "operator".into()
                },
                Some(&started.digest)
            )
            .unwrap_err()
            .contains("LEASE_REQUIRED")
    );
    let lease = store.acquire_lease(&Resource::JobWorker(job), 60).unwrap();
    let first = store
        .append_preparation_event_with_lease(
            job,
            item,
            1,
            PreparationStage::Interrupted {
                actor: "operator".into(),
            },
            Some(&started.digest),
            &lease,
        )
        .unwrap();
    let items = store.preparation_items(job, 0, 10).unwrap();
    assert_eq!(items[0].state, "failed");
    assert!(items[0].retry_eligible);
    assert_eq!(items[0].attempt, 1);
    assert_eq!(items[1].state, "pending");
    let started = store
        .append_preparation_event_with_lease(
            job,
            item,
            2,
            PreparationStage::Started,
            Some(&first.digest),
            &lease,
        )
        .unwrap();
    let second = store
        .append_preparation_event_with_lease(
            job,
            item,
            2,
            PreparationStage::Interrupted {
                actor: "operator".into(),
            },
            Some(&started.digest),
            &lease,
        )
        .unwrap();
    assert!(!store.preparation_items(job, 0, 10).unwrap()[0].retry_eligible);
    assert!(
        store
            .append_preparation_event_with_lease(
                job,
                item,
                3,
                PreparationStage::Started,
                Some(&second.digest),
                &lease
            )
            .is_err()
    );
    store.release_lease(&lease).unwrap();
    let events = f.open().preparation_events(job, 0, 10).unwrap();
    assert_eq!(events.len(), 4);
    assert!(
        matches!(&events[3].event.stage, PreparationStage::Interrupted { actor } if actor == "operator")
    );
}

#[test]
fn enriched_checkpoints_keep_the_capture_first_and_only_add_provider_sources() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let id = definition.job.id;
    let item = definition.job.item_ids[0];
    store.create_preparation_job(&definition).unwrap();
    let hash = store.publish_asset(b"retained originals", 1000).unwrap();
    let started = store
        .append_preparation_event(id, item, 1, PreparationStage::Started, None)
        .unwrap();
    let provider = |kind: &str, location: &str| SourceRecord {
        id: uuid::Uuid::new_v4(),
        kind: kind.into(),
        location: location.into(),
        digest: hash.clone(),
        text: None,
        fields: BTreeMap::new(),
        model_manifest: String::new(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    };
    let append = |store: &mut Store, document: LearningDocument| {
        store.append_preparation_event(
            id,
            item,
            1,
            PreparationStage::Captured {
                document: Box::new(document),
            },
            Some(&started.digest),
        )
    };
    // A second Anki capture or a provider source ahead of the capture is refused.
    let mut twice = capture(&hash);
    twice
        .sources
        .push(provider("anki_read_capture_v2", "anki_note:124"));
    assert_eq!(
        append(&mut store, twice).unwrap_err(),
        "PREPARATION_CAPTURE_CONFLICT"
    );
    let mut reordered = capture(&hash);
    reordered
        .sources
        .insert(0, provider("wiktionary_definition_v0.8", "https://example.org"));
    assert_eq!(
        append(&mut store, reordered).unwrap_err(),
        "PREPARATION_CAPTURE_CONFLICT"
    );
    let mut enriched = capture(&hash);
    enriched
        .sources
        .push(provider("wiktionary_definition_v0.8", "https://example.org"));
    append(&mut store, enriched).unwrap();
}
