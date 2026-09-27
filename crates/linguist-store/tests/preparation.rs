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
        fields: BTreeMap::new(),
        model_manifest: "original".into(),
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    doc.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id: id,
        digest: hash.into(),
        original_fields: BTreeMap::new(),
        asset_digests: vec![hash.into()],
    });
    doc
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
    let started = store
        .append_preparation_event(id, item, 1, PreparationStage::Started, None)
        .unwrap();
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
    db.execute_batch("DROP TABLE preparation_event_assets;DROP TABLE preparation_events;DROP TABLE preparation_jobs;PRAGMA user_version=5;").unwrap();
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
