use linguist_core::{canonical, records::*};
use linguist_store::Store;
use uuid::Uuid;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lab-snapshot-test-{}", Uuid::new_v4())))
    }
    fn store(&self) -> Store {
        Store::open(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot() -> Snapshot {
    let mut s = Snapshot {
        id: Uuid::new_v4(),
        operation_id: Uuid::new_v4(),
        originals: vec![],
        archives: vec![],
        media: vec![],
        before_digest: String::new(),
    };
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    s
}

fn source(id: Uuid, digest: String) -> SourceRecord {
    SourceRecord {
        id,
        kind: "anki_read_capture_v2".into(),
        location: "anki_note:123".into(),
        digest,
        text: None,
        fields: Default::default(),
        model_manifest: "model".into(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    }
}

fn journal_for(s: &Snapshot) -> OperationJournal {
    OperationJournal {
        id: s.operation_id,
        group_id: None,
        approval_digest: format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        binding: CollectionBinding {
            endpoint: "http://127.0.0.1:8765".into(),
            profile_fingerprint: "p".into(),
            path_fingerprint: "q".into(),
            bridge_id: Uuid::new_v4(),
            lineage_id: Uuid::new_v4(),
            session_epoch: Uuid::new_v4(),
            capability_digest: "capability".into(),
        },
        snapshot_id: s.id,
        backup_id: Uuid::new_v4(),
        state: OperationState::Prepared,
        steps: vec![JournalStep {
            id: Uuid::new_v4(),
            action: "create_note".into(),
            payload_digest: "a".repeat(64),
            precondition_digest: s.before_digest.clone(),
            expected_post_digest: canonical::asset_digest(b"{\"notes\":[]}"),
            state: StepState::IntentRecorded,
            observed_digest: None,
        }],
        issues: vec![],
    }
}

#[test]
fn originals_and_after_state_are_separate_immutable_records() {
    let f = Fixture::new();
    let mut store = f.store();
    let s = snapshot();
    store.publish_snapshot(&s).unwrap();
    assert!(store.publish_snapshot(&s).is_err());
    assert_eq!(store.list_snapshots(1).unwrap().len(), 1);
    assert!(store.list_snapshots(0).is_err());
    let mut journal = journal_for(&s);
    let prepared = store.append_journal(&journal, None).unwrap();
    journal.state = OperationState::Preflight;
    journal.steps[0].state = StepState::RequestStarted;
    store.append_journal(&journal, Some(&prepared)).unwrap();
    let evidence = b"native readback evidence";
    let evidence_digest = store.publish_asset(evidence, 1024).unwrap();
    let actual_digest = store.publish_asset(b"{\"notes\":[]}", 1024).unwrap();
    let receipt = NativeOperationReceipt {
        schema_version: 1,
        lineage_id: journal.binding.lineage_id,
        operation_id: journal.steps[0].id,
        session_epoch: journal.binding.session_epoch,
        payload_digest: journal.steps[0].payload_digest.clone(),
        approved_digest: journal.approval_digest.clone(),
        state: NativeReceiptState::Verified,
        readback: Some(NativeReadback {
            observed_state_digest: actual_digest.clone(),
            note_ids: vec![],
            card_ids: vec![],
            history_digest: None,
            manifest_digests: vec![],
        }),
        evidence_digest: evidence_digest.clone(),
    };
    let mut queued = receipt.clone();
    queued.state = NativeReceiptState::Queued;
    queued.readback = None;
    assert!(store.append_snapshot_after(s.id, &queued).is_err());
    let mut foreign = receipt.clone();
    foreign.operation_id = Uuid::new_v4();
    assert!(store.append_snapshot_after(s.id, &foreign).is_err());
    let mut wrong_approval = receipt.clone();
    wrong_approval.approved_digest = format!("lab-jcs-v1:plan:{}", "f".repeat(64));
    assert_eq!(
        store
            .append_snapshot_after(s.id, &wrong_approval)
            .unwrap_err(),
        "SNAPSHOT_RECEIPT_CONFLICT"
    );
    let mut wrong_after = receipt.clone();
    wrong_after.readback.as_mut().unwrap().observed_state_digest = "d".repeat(64);
    assert_eq!(
        store.append_snapshot_after(s.id, &wrong_after).unwrap_err(),
        "SNAPSHOT_RECEIPT_CONFLICT"
    );
    store.append_snapshot_after(s.id, &receipt).unwrap();
    assert!(store.append_snapshot_after(s.id, &receipt).is_err());
    drop(store);
    let record = f.store().snapshot(s.id).unwrap();
    assert_eq!(record.snapshot, s);
    assert_eq!(record.after, Some(receipt));
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    assert!(db.execute("DELETE FROM snapshots", []).is_err());
    assert!(
        db.execute("UPDATE snapshot_after SET observed_digest='fake'", [])
            .is_err()
    );
    drop(db);
    std::fs::write(f.0.join("assets").join(&evidence_digest), b"changed").unwrap();
    assert_eq!(f.store().snapshot(s.id).unwrap_err(), "ASSET_CORRUPT");
    std::fs::write(f.0.join("assets").join(evidence_digest), evidence).unwrap();
    std::fs::write(f.0.join("assets").join(actual_digest), b"changed too").unwrap();
    assert_eq!(f.store().snapshot(s.id).unwrap_err(), "ASSET_CORRUPT");
}

#[test]
fn failed_index_insert_rolls_back_snapshot_body() {
    let f = Fixture::new();
    let mut store = f.store();
    let digest = store.publish_asset(b"original", 1024).unwrap();
    let mut s = snapshot();
    let source_id = Uuid::new_v4();
    s.originals.push(source(source_id, digest.clone()));
    s.archives.push(SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: None,
        original_fields: Default::default(),
        asset_digests: vec![digest],
    });
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_snapshot_index BEFORE INSERT ON snapshot_assets BEGIN SELECT RAISE(ABORT,'injected write failure'); END;").unwrap();
    assert!(store.publish_snapshot(&s).is_err());
    drop(store);
    assert_eq!(f.store().snapshot(s.id).unwrap_err(), "SNAPSHOT_NOT_FOUND");
}

#[test]
fn snapshot_keyset_page_resumes_without_repeating_a_record() {
    let f = Fixture::new();
    let mut store = f.store();
    let a = snapshot();
    let b = snapshot();
    store.publish_snapshot(&a).unwrap();
    store.publish_snapshot(&b).unwrap();
    let (first, cursor) = store.snapshot_page(None, 1).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(cursor, Some(first[0].snapshot.id));
    let (second, end) = store.snapshot_page(cursor, 1).unwrap();
    assert_eq!(second.len(), 1);
    assert_ne!(first[0].snapshot.id, second[0].snapshot.id);
    assert_eq!(end, None);
}

#[test]
fn snapshot_rejects_unlinked_or_duplicate_original_archives() {
    let f = Fixture::new();
    let mut store = f.store();
    let digest = store.publish_asset(b"original", 1024).unwrap();
    let mut s = snapshot();
    let source_id = Uuid::new_v4();
    s.originals.push(source(source_id, digest.clone()));
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    assert_eq!(
        store.publish_snapshot(&s).unwrap_err(),
        "SNAPSHOT_ORIGINAL_INVALID"
    );
    s.archives.push(SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: None,
        original_fields: Default::default(),
        asset_digests: vec![digest.clone()],
    });
    let mut duplicate = s.archives[0].clone();
    duplicate.id = Uuid::new_v4();
    s.archives.push(duplicate);
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    assert_eq!(
        store.publish_snapshot(&s).unwrap_err(),
        "SNAPSHOT_ORIGINAL_INVALID"
    );
    s.archives.pop();
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    store.publish_snapshot(&s).unwrap();
}

#[test]
fn snapshot_requires_original_assets_and_contract_rejects_embedded_after_claim() {
    let f = Fixture::new();
    let mut store = f.store();
    let mut s = snapshot();
    let source_id = Uuid::new_v4();
    s.originals.push(source(source_id, "d".repeat(64)));
    s.archives.push(SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: "d".repeat(64),
        original_text: None,
        original_fields: Default::default(),
        asset_digests: vec![],
    });
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    assert!(store.publish_snapshot(&s).is_err());
    assert_eq!(
        store.publish_asset(b"original", 1024).unwrap(),
        canonical::asset_digest(b"original")
    );
    let digest = canonical::asset_digest(b"original");
    s.archives[0].digest = digest.clone();
    s.archives[0].asset_digests.push(digest.clone());
    s.originals[0].digest = digest;
    s.before_digest =
        canonical::digest("snapshot-original", &(&s.originals, &s.archives, &s.media)).unwrap();
    let mut forged = serde_json::to_value(&s).unwrap();
    forged["verified_after_digest"] = serde_json::json!("claimed");
    assert!(canonical::parse::<Snapshot>(&serde_json::to_vec(&forged).unwrap()).is_err());
    store.publish_snapshot(&s).unwrap();
    drop(store);
    assert_eq!(f.store().snapshot(s.id).unwrap().snapshot, s);
}

#[test]
fn schema_seven_upgrade_preserves_state_with_private_backup() {
    let f = Fixture::new();
    let s = snapshot();
    let mut store = f.store();
    store.publish_snapshot(&s).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE snapshot_after; DROP TABLE snapshot_assets; DROP TABLE snapshots; PRAGMA user_version=7;").unwrap();
    drop(db);
    let _ = f.store();
    let backup = std::fs::read_dir(&f.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".schema-v7-")
        })
        .unwrap();
    let db = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        db.pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(f.store().snapshot(s.id).unwrap_err(), "SNAPSHOT_NOT_FOUND");
}

#[cfg(target_os = "linux")]
#[test]
fn asset_file_size_limit_failure_never_publishes_metadata() {
    const VARIABLE: &str = "LAB_TEST_ASSET_LIMIT_ROOT";
    let bytes = vec![b'x'; 4096];
    if let Ok(root) = std::env::var(VARIABLE) {
        let mut store = Store::open(std::path::Path::new(&root)).unwrap();
        // Set the limit only after SQLite initialization, in this isolated child.
        let limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        unsafe {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        }
        assert!(store.publish_asset(&bytes, 8192).is_err());
        std::process::exit(0);
    }
    let f = Fixture::new();
    drop(f.store());
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "asset_file_size_limit_failure_never_publishes_metadata",
        ])
        .env(VARIABLE, &f.0)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let digest = canonical::asset_digest(&bytes);
    assert!(f.store().asset(&digest, 8192).is_err());
    assert!(!f.0.join("assets").join(digest).exists());
}

#[test]
fn crash_between_asset_file_and_metadata_leaves_recoverable_orphan() {
    const VARIABLE: &str = "LAB_TEST_ASSET_ORPHAN_ROOT";
    let bytes = b"original bytes before metadata";
    if let Ok(root) = std::env::var(VARIABLE) {
        let root = std::path::Path::new(&root);
        let mut store = Store::open(root).unwrap();
        std::fs::write(root.join("ready"), b"ready").unwrap();
        while !root.join("go").exists() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        store.publish_asset(bytes, 1024).unwrap();
        return;
    }
    let f = Fixture::new();
    drop(f.store());
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_between_asset_file_and_metadata_leaves_recoverable_orphan",
        ])
        .env(VARIABLE, &f.0)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !f.0.join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(f.0.join("ready").exists(), "child did not open store");
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    std::fs::write(f.0.join("go"), b"go").unwrap();
    let digest = canonical::asset_digest(bytes);
    let path = f.0.join("assets").join(&digest);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(path.exists(), "child did not publish asset bytes");
    child.kill().unwrap();
    child.wait().unwrap();
    db.execute_batch("ROLLBACK").unwrap();
    let mut store = f.store();
    assert_eq!(store.asset(&digest, 1024).unwrap_err(), "ASSET_NOT_FOUND");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(store.publish_asset(bytes, 1024).unwrap(), digest);
    drop(store);
    assert_eq!(f.store().asset(&digest, 1024).unwrap(), bytes);
}
