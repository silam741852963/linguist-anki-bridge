use linguist_store::{Store, campaign::NewBatch, lease::Resource};
use serde_json::json;
use uuid::Uuid;

fn root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("lab-campaign-{name}-{}", Uuid::new_v4()))
}

fn batches() -> Vec<NewBatch> {
    (1..=2)
        .map(|b| NewBatch {
            batch: b,
            cohort: "ja_plain".into(),
            phase: "selected".into(),
            data: json!({"notes": [format!("{b}00")]}),
        })
        .collect()
}

#[test]
fn campaign_definition_is_immutable_and_batches_move_by_compare_and_set() {
    let root = root("cas");
    let mut store = Store::open(&root).unwrap();
    let id = Uuid::new_v4();
    let digest = store
        .create_campaign(id, &json!({"purpose": "japanese_vocab"}), &batches(), 10)
        .unwrap();
    assert_eq!(
        store
            .create_campaign(id, &json!({}), &batches(), 11)
            .unwrap_err(),
        "CAMPAIGN_EXISTS"
    );
    let record = store.campaign(id).unwrap();
    assert_eq!(record.definition_digest, digest);
    assert_eq!(record.status, "running");
    assert_eq!(store.campaign_ids().unwrap(), vec![id]);
    let batch = store.campaign_batch(id, 1).unwrap();
    assert_eq!((batch.phase.as_str(), batch.version), ("selected", 1));
    let version = store
        .update_campaign_batch(
            id,
            1,
            1,
            "preparing",
            &json!({"notes": ["100"], "prepare_job": "j"}),
            "phase",
            &json!({"to": "preparing"}),
            20,
        )
        .unwrap();
    assert_eq!(version, 2);
    // A stale writer (same expected version) is refused and changes nothing.
    assert_eq!(
        store
            .update_campaign_batch(id, 1, 1, "failed", &json!({}), "phase", &json!({}), 21)
            .unwrap_err(),
        "CAMPAIGN_BATCH_CONFLICT"
    );
    let batch = store.campaign_batch(id, 1).unwrap();
    assert_eq!(batch.phase, "preparing");
    assert_eq!(batch.data["prepare_job"], "j");
    let v = store
        .set_campaign_status(
            id,
            1,
            "paused",
            Some("CAMPAIGN_PAUSED_VERIFY"),
            Some("card lost"),
            "paused",
            &json!({}),
            30,
        )
        .unwrap();
    assert_eq!(v, 2);
    assert!(
        store
            .set_campaign_status(id, 1, "running", None, None, "resumed", &json!({}), 31)
            .is_err()
    );
    assert!(
        store
            .set_campaign_status(id, 2, "bogus", None, None, "x", &json!({}), 31)
            .is_err()
    );
    store
        .append_campaign_event(id, None, "agent_run", &json!({}), 40)
        .unwrap();
    store
        .append_campaign_event(id, None, "agent_run", &json!({}), 50)
        .unwrap();
    assert_eq!(store.count_campaign_events(id, "agent_run", 45).unwrap(), 1);
    let events = store.campaign_events(id, 0, 100).unwrap();
    assert_eq!(
        events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
        ["created", "phase", "paused", "agent_run", "agent_run"]
    );
    assert_eq!(events[1].batch, Some(1));
    // History rows cannot be rewritten or removed.
    let conn = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert!(conn.execute("DELETE FROM campaign_events", []).is_err());
    assert!(
        conn.execute("UPDATE campaigns SET created_ms=0", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM campaign_batches", []).is_err());
}

#[test]
fn campaign_checkpoints_are_recorded_once_and_pruned_once() {
    let mut store = Store::open(&root("checkpoints")).unwrap();
    let id = Uuid::new_v4();
    store
        .create_campaign(id, &json!({}), &batches(), 1)
        .unwrap();
    store
        .record_campaign_checkpoint(id, "c1", 1, "/x/c1.colpkg", 2)
        .unwrap();
    store
        .record_campaign_checkpoint(id, "c1", 1, "/x/c1.colpkg", 3)
        .unwrap();
    assert_eq!(store.campaign_checkpoints(id).unwrap().len(), 1);
    store.mark_campaign_checkpoint_pruned(id, "c1", 4).unwrap();
    assert_eq!(
        store
            .mark_campaign_checkpoint_pruned(id, "c1", 5)
            .unwrap_err(),
        "CAMPAIGN_CHECKPOINT_NOT_FOUND"
    );
    assert_eq!(
        store.campaign_checkpoints(id).unwrap()[0].pruned_ms,
        Some(4)
    );
}

#[test]
fn one_campaign_tick_holds_the_campaign_lease() {
    let mut store = Store::open(&root("lease")).unwrap();
    let id = Uuid::new_v4();
    let token = store.acquire_lease(&Resource::Campaign(id), 60).unwrap();
    assert_eq!(
        store
            .acquire_lease(&Resource::Campaign(id), 60)
            .unwrap_err(),
        "LEASE_HELD_OR_OWNER_UNVERIFIED"
    );
    // Other campaigns and job leases are separate resources.
    store
        .acquire_lease(&Resource::Campaign(Uuid::new_v4()), 60)
        .unwrap();
    store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    store.release_lease(&token).unwrap();
    store.acquire_lease(&Resource::Campaign(id), 60).unwrap();
}

#[test]
fn a_schema_14_store_gains_campaign_tables_on_open() {
    let root = root("migrate");
    drop(Store::open(&root).unwrap());
    let conn = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    conn.execute_batch("DROP TRIGGER merge_deletions_no_delete; DROP TABLE merge_deletions; DROP TABLE campaign_checkpoints; DROP TRIGGER campaign_events_no_delete; DROP TABLE campaign_events; DROP TRIGGER campaign_batches_no_delete; DROP TABLE campaign_batches; DROP TABLE campaign_state; DROP TRIGGER campaigns_no_delete; DROP TRIGGER campaigns_no_update; DROP TABLE campaigns; PRAGMA user_version=14;").unwrap();
    drop(conn);
    assert!(
        Store::read_only(&root).is_err(),
        "old binaries' schema is refused read-only"
    );
    let mut store = Store::open_existing(&root).unwrap();
    store
        .create_campaign(Uuid::new_v4(), &json!({}), &batches(), 1)
        .unwrap();
    assert!(std::fs::read_dir(&root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".schema-v14-")
    }));
}
