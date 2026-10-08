use linguist_store::Store;
use serde_json::json;

#[test]
fn engine_certifications_are_immutable_and_the_newest_run_wins() {
    let root = std::env::temp_dir().join(format!("lab-engine-cert-{}", uuid::Uuid::new_v4()));
    let mut store = Store::open(&root).unwrap();
    assert!(store.latest_engine_certification("id-a").unwrap().is_none());
    let pass = store
        .record_engine_certification("id-a", "0.12.0", true, 10, &json!({"probes":[1]}))
        .unwrap();
    assert_eq!(
        store.latest_engine_certification("id-a").unwrap().unwrap(),
        pass
    );
    assert!(store.latest_engine_certification("id-b").unwrap().is_none());
    let fail = store
        .record_engine_certification("id-a", "0.12.0", false, 20, &json!({"probes":[]}))
        .unwrap();
    // A later failed run supersedes the earlier pass.
    assert!(
        !store
            .latest_engine_certification("id-a")
            .unwrap()
            .unwrap()
            .passed
    );
    assert_eq!(
        store
            .latest_engine_certification("id-a")
            .unwrap()
            .unwrap()
            .id,
        fail.id
    );
    assert!(
        store
            .record_engine_certification(" ", "v", true, 1, &json!({}))
            .is_err()
    );
    drop(store);
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE engine_certifications SET passed=1", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM engine_certifications", []).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
