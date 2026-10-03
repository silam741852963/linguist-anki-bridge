use linguist_core::records::{BackupReceipt, CollectionBinding};
use linguist_store::{
    Store,
    checkpoint::{CheckpointRecord, ModelOperationRecord},
};
use uuid::Uuid;

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Fixture, Store) {
    let root = std::env::temp_dir().join(format!("lab-store-checkpoint-{}", Uuid::new_v4()));
    let store = Store::open(&root).unwrap();
    (Fixture(root), store)
}
fn record(created_ms: u64) -> CheckpointRecord {
    CheckpointRecord {
        receipt: BackupReceipt {
            id: Uuid::new_v4(),
            binding: CollectionBinding {
                endpoint: "http://127.0.0.1:8765".into(),
                profile_fingerprint: "a".repeat(64),
                path_fingerprint: "b".repeat(64),
                bridge_id: Uuid::new_v4(),
                lineage_id: Uuid::new_v4(),
                session_epoch: Uuid::new_v4(),
                capability_digest: "c".repeat(64),
            },
            path: "/tmp/a.colpkg".into(),
            checksum: "d".repeat(64),
            scope_digest: "scope".into(),
            includes_scheduling: true,
            includes_media: true,
            includes_schema: true,
            verification_digest: "verified".into(),
            restoration_evidence: Some("restored".into()),
        },
        operation_id: Uuid::new_v4(),
        created_ms,
        scope: "collection".into(),
        size_bytes: 10,
        evidence: serde_json::json!({"schema_version":1}),
    }
}

#[test]
fn receipts_are_create_new_immutable_and_listed_newest_first() {
    let (f, mut store) = fixture();
    let old = record(1);
    let new = record(2);
    store.publish_checkpoint(&old).unwrap();
    store.publish_checkpoint(&new).unwrap();
    assert_eq!(
        store.publish_checkpoint(&old).unwrap_err(),
        "CHECKPOINT_RECEIPT_CONFLICT"
    );
    let listed = store.list_checkpoints(None, None, 10).unwrap();
    assert_eq!(listed, vec![new.clone(), old.clone()]);
    assert_eq!(
        store.list_checkpoints(None, Some(2), 10).unwrap(),
        vec![new.clone()]
    );
    assert!(
        store
            .list_checkpoints(Some("affected"), None, 10)
            .unwrap()
            .is_empty()
    );
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE checkpoint_receipts SET created_ms=9", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM checkpoint_receipts", []).is_err());
}

#[test]
fn unverified_or_unrestored_receipts_are_rejected() {
    let (_f, mut store) = fixture();
    for change in [
        |r: &mut CheckpointRecord| r.receipt.restoration_evidence = None,
        |r: &mut CheckpointRecord| r.receipt.checksum = "short".into(),
        |r: &mut CheckpointRecord| r.receipt.path = "relative.colpkg".into(),
        |r: &mut CheckpointRecord| r.scope = "affected".into(),
        |r: &mut CheckpointRecord| r.size_bytes = 0,
    ] {
        let mut bad = record(1);
        change(&mut bad);
        assert_eq!(
            store.publish_checkpoint(&bad).unwrap_err(),
            "CHECKPOINT_RECORD_INVALID"
        );
    }
}

#[test]
fn verifications_append_and_model_operations_index_by_name() {
    let (_f, mut store) = fixture();
    let receipt = record(1);
    store.publish_checkpoint(&receipt).unwrap();
    let id = receipt.receipt.id;
    assert_eq!(
        store
            .append_checkpoint_verification(id, 5, &serde_json::json!({"passed":true}))
            .unwrap()
            .sequence,
        1
    );
    assert_eq!(
        store
            .append_checkpoint_verification(id, 6, &serde_json::json!({"passed":true}))
            .unwrap()
            .sequence,
        2
    );
    assert!(
        store
            .append_checkpoint_verification(Uuid::new_v4(), 6, &serde_json::json!({}))
            .is_err()
    );
    assert_eq!(store.checkpoint_verifications(id).unwrap().len(), 2);
    let operation = ModelOperationRecord {
        operation_id: Uuid::new_v4(),
        model_name: "Linguist Grammar v2".into(),
        manifest_digest: "e".repeat(64),
        created_ms: 1,
        evidence: serde_json::json!({"schema_version":1}),
    };
    store.publish_model_operation(&operation).unwrap();
    assert_eq!(
        store.publish_model_operation(&operation).unwrap_err(),
        "MODEL_OPERATION_CONFLICT"
    );
    assert_eq!(
        store.model_operations_named("Linguist Grammar v2").unwrap(),
        vec![operation]
    );
    assert!(store.model_operations_named("Other").unwrap().is_empty());
    let db = rusqlite::Connection::open(_f.0.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE model_operations SET model_name='x'", [])
            .is_err()
    );
    assert!(
        db.execute("DELETE FROM checkpoint_verifications", [])
            .is_err()
    );
}
