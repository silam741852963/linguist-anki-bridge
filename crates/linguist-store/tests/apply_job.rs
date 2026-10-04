//! Schema 12: worker stop acknowledgements, tombstones and their guards.
use linguist_core::{canonical, records::*};
use linguist_store::{
    Store,
    apply_job::JobTombstone,
    lease::Resource,
    preparation::{PreparationDefinition, PreparationStage},
    preparation_control::ControlAction,
};
use serde_json::json;
use std::collections::BTreeMap;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lab-job-store-{}", uuid::Uuid::new_v4())))
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
            selector: SelectionInput::NoteIds(vec!["123".into()]),
            matched_note_ids: vec!["123".into()],
            selected_note_ids: vec!["123".into()],
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
            plan_refs: vec!["anki-note:123".into()],
            item_ids: vec![uuid::Uuid::new_v4()],
            pause_requested: false,
            cancel_requested: false,
        },
    }
}

fn tombstone(job: uuid::Uuid) -> JobTombstone {
    JobTombstone {
        schema_version: 1,
        job_id: job,
        kind: "prepare".into(),
        definition_digest: "d".into(),
        final_state: "cancelled".into(),
        summary: BTreeMap::new(),
        retained_operations: vec![],
        retained_plan: None,
        created_ms: 1,
    }
}

#[test]
fn schema_eleven_upgrade_adds_job_tables_and_keeps_existing_jobs() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    store.create_preparation_job(&definition).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TABLE job_tombstones; DROP TABLE preparation_stop_acks; DROP TABLE apply_job_events; DROP TABLE apply_jobs; PRAGMA user_version=11;",
    )
    .unwrap();
    drop(db);
    // Read-only commands never upgrade.
    assert!(Store::read_only(&f.0).is_err());
    let store = Store::open_existing(&f.0).unwrap();
    assert_eq!(
        store.preparation_job(definition.job.id).unwrap(),
        definition
    );
    assert!(store.job_tombstone(definition.job.id).unwrap().is_none());
    let listed = store.list_jobs(None, 10, None, false).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, "prepare");
    assert!(std::fs::read_dir(&f.0).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".schema-v11-")
    }));
}

#[test]
fn stop_acknowledgements_need_the_worker_lease_or_an_idle_job() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let job = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    // No control request: nothing to acknowledge.
    let worker = store.acquire_lease(&Resource::JobWorker(job), 60).unwrap();
    assert!(
        store
            .acknowledge_preparation_stop(job, &worker)
            .unwrap()
            .is_none()
    );
    store
        .request_preparation_control(job, ControlAction::Pause)
        .unwrap();
    // A live lease holder blocks the idle confirmation.
    assert!(
        store
            .acknowledge_idle_preparation_stop(job)
            .unwrap()
            .is_none()
    );
    let ack = store
        .acknowledge_preparation_stop(job, &worker)
        .unwrap()
        .unwrap();
    assert!(!ack.idle);
    assert_eq!(ack.lease_generation, Some(worker.generation()));
    // Repeats reuse the same acknowledgement.
    assert_eq!(
        store
            .acknowledge_preparation_stop(job, &worker)
            .unwrap()
            .unwrap(),
        ack
    );
    assert_eq!(store.preparation_stop_ack(job).unwrap().unwrap(), ack);
    store.release_lease(&worker).unwrap();
    // A new request is not covered by the older acknowledgement.
    store
        .request_preparation_control(job, ControlAction::Resume)
        .unwrap();
    store
        .request_preparation_control(job, ControlAction::Cancel)
        .unwrap();
    assert!(store.preparation_stop_ack(job).unwrap().is_none());
    // An unfinished start blocks the idle confirmation. Cancel blocks fenced
    // worker starts, so the interrupted start is written through the unfenced path.
    store
        .append_preparation_event(
            job,
            definition.job.item_ids[0],
            1,
            PreparationStage::Started,
            None,
        )
        .unwrap();
    assert!(
        store
            .acknowledge_idle_preparation_stop(job)
            .unwrap()
            .is_none()
    );
}

#[test]
fn tombstones_block_controls_events_and_runs_and_are_permanent() {
    let f = Fixture::new();
    let mut store = f.open();
    let definition = definition();
    let job = definition.job.id;
    store.create_preparation_job(&definition).unwrap();
    // A held lease blocks tombstoning inside the transaction.
    let worker = store.acquire_lease(&Resource::JobWorker(job), 60).unwrap();
    assert_eq!(
        store.tombstone_job(&tombstone(job)).unwrap_err(),
        "JOB_DELETE_WORKER_ACTIVE"
    );
    store.release_lease(&worker).unwrap();
    let mut wrong = tombstone(job);
    wrong.kind = "apply".into();
    assert_eq!(
        store.tombstone_job(&wrong).unwrap_err(),
        "JOB_TOMBSTONE_INVALID"
    );
    store.tombstone_job(&tombstone(job)).unwrap();
    assert_eq!(
        store.tombstone_job(&tombstone(job)).unwrap_err(),
        "JOB_ALREADY_TOMBSTONED"
    );
    assert_eq!(
        store
            .request_preparation_control(job, ControlAction::Pause)
            .unwrap_err(),
        "JOB_TOMBSTONED"
    );
    assert_eq!(
        store
            .append_preparation_event(
                job,
                definition.job.item_ids[0],
                1,
                PreparationStage::Started,
                None
            )
            .unwrap_err(),
        "JOB_TOMBSTONED"
    );
    // The definition is retained and the tombstone cannot be removed.
    assert_eq!(store.preparation_job(job).unwrap(), definition);
    drop(store);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    assert!(db.execute("DELETE FROM job_tombstones", []).is_err());
    assert!(
        db.execute("UPDATE job_tombstones SET kind='apply'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM preparation_jobs", []).is_err());
}
