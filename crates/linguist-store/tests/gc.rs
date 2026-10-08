//! ALG-GC: mark-and-sweep of store assets with protected history, active
//! readers and unresolved recovery as roots or blockers.
use linguist_core::canonical;
use linguist_store::{
    Store,
    gc::{FileKind, ORPHAN_GRACE_MS, PrunePolicy},
    lease::Resource,
};
use serde_json::json;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lab-gc-{}", uuid::Uuid::new_v4())))
    }
    fn store(&self) -> Store {
        Store::open(&self.0).unwrap()
    }
    fn age(&self, digest: &str, days: u64) {
        let file = std::fs::File::options()
            .write(true)
            .open(self.0.join("assets").join(digest))
            .unwrap();
        file.set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400 + 7200),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn policy(retention_days: u64) -> PrunePolicy {
    PrunePolicy {
        retention_ms: retention_days * 86_400_000,
        unreferenced_budget_bytes: None,
        now_ms: now_ms(),
    }
}

/// Root `referenced` through an immutable legacy-import report row.
fn root_through_report(store: &mut Store, referenced: &[&str]) -> String {
    let source = format!("legacy-db-{}", uuid::Uuid::new_v4());
    store
        .record_legacy_job_import(source.as_bytes(), &json!({"assets": referenced}), 1)
        .unwrap();
    canonical::asset_digest(source.as_bytes())
}

#[test]
fn reachable_and_transitively_referenced_assets_survive_and_orphans_are_tombstoned() {
    let f = Fixture::new();
    let mut store = f.store();
    let leaf = store.publish_asset(b"leaf media bytes", 1024).unwrap();
    let parent_body = format!("{{\"media\":\"{leaf}\"}}");
    let parent = store.publish_asset(parent_body.as_bytes(), 1024).unwrap();
    let orphan = store.publish_asset(b"orphaned evidence", 1024).unwrap();
    let young = store.publish_asset(b"young orphan", 1024).unwrap();
    let source = root_through_report(&mut store, &[&parent]);
    for digest in [&leaf, &parent, &orphan, &source] {
        f.age(digest, 40);
    }
    // A stray file (crash after link, before index) and an interrupted temp write.
    let stray_bytes = b"stray bytes";
    let stray = canonical::asset_digest(stray_bytes);
    std::fs::write(f.0.join("assets").join(&stray), stray_bytes).unwrap();
    f.age(&stray, 40);
    let temp = format!(".{}.tmp", uuid::Uuid::new_v4());
    std::fs::write(f.0.join("assets").join(&temp), b"partial").unwrap();
    f.age(&temp, 1);
    std::fs::write(f.0.join("assets").join("README"), b"not ours").unwrap();

    let census = store.asset_census().unwrap();
    let state = |name: &str| {
        census
            .files
            .iter()
            .find(|a| a.name == name)
            .unwrap()
            .clone()
    };
    assert!(state(&parent).reachable && state(&leaf).reachable && state(&source).reachable);
    assert!(!state(&orphan).reachable && !state(&young).reachable);
    assert_eq!(state(&stray).kind, FileKind::Stray);
    assert_eq!(state(&temp).kind, FileKind::Temporary);
    assert_eq!(state("README").kind, FileKind::Unmanaged);
    assert!(census.roots.contains_key("legacy_job_imports"));
    assert!(census.blockers().is_empty());

    let preview = store.prune_assets(&policy(30), false).unwrap();
    let mut names: Vec<_> = preview.candidates.iter().map(|c| c.name.clone()).collect();
    names.sort();
    let mut expected = vec![orphan.clone(), stray.clone(), temp.clone()];
    expected.sort();
    assert_eq!(
        names, expected,
        "young, reachable and unmanaged files are kept"
    );
    assert!(
        f.0.join("assets").join(&orphan).exists(),
        "preview deletes nothing"
    );

    let receipt = store.prune_assets(&policy(30), true).unwrap();
    assert!(receipt.executed && receipt.unlink_failures.is_empty());
    assert_eq!(receipt.unlinked.len(), 3);
    for name in &expected {
        assert!(!f.0.join("assets").join(name).exists());
    }
    for kept in [&leaf, &parent, &young, &source] {
        store.asset(kept, 1024).unwrap();
    }
    assert!(f.0.join("assets/README").exists());
    assert_eq!(store.asset(&orphan, 1024).unwrap_err(), "ASSET_NOT_FOUND");
    let runs = store.gc_runs(5).unwrap();
    assert_eq!(runs[0]["state"], "completed");
    assert_eq!(runs[0]["files"], 3);
    // Republishing pruned bytes works: tombstones never block new evidence.
    assert_eq!(
        store.publish_asset(b"orphaned evidence", 1024).unwrap(),
        orphan
    );
}

#[test]
fn retention_and_grace_protect_recent_unreferenced_assets() {
    let f = Fixture::new();
    let mut store = f.store();
    let fresh = store.publish_asset(b"fresh", 1024).unwrap();
    let preview = store.prune_assets(&policy(0), false).unwrap();
    assert!(
        preview.candidates.is_empty(),
        "the orphan grace protects in-flight publications"
    );
    f.age(&fresh, 0);
    const { assert!(ORPHAN_GRACE_MS <= 7200 * 1000) };
    let preview = store.prune_assets(&policy(0), false).unwrap();
    assert_eq!(preview.candidates.len(), 1);
    let preview = store.prune_assets(&policy(30), false).unwrap();
    assert!(
        preview.candidates.is_empty(),
        "retention days apply beyond the grace"
    );
    // A budget prunes the oldest unreferenced files first, never reachable ones.
    let older = store.publish_asset(b"older orphan", 1024).unwrap();
    f.age(&older, 5);
    f.age(&fresh, 1);
    let budget = PrunePolicy {
        retention_ms: 30 * 86_400_000,
        unreferenced_budget_bytes: Some(5),
        now_ms: now_ms(),
    };
    let preview = store.prune_assets(&budget, false).unwrap();
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(preview.candidates[0].name, older);
}

#[test]
fn active_readers_block_pruning() {
    let f = Fixture::new();
    let mut store = f.store();
    let orphan = store.publish_asset(b"orphan during read", 1024).unwrap();
    f.age(&orphan, 40);
    let mut reader = f.store();
    let token = reader
        .acquire_lease(&Resource::JobWorker(uuid::Uuid::new_v4()), 60)
        .unwrap();
    let preview = store.prune_assets(&policy(30), false).unwrap();
    assert!(preview.blockers[0].starts_with("GC_BLOCKED_BY_ACTIVE_LEASE"));
    let error = store.prune_assets(&policy(30), true).unwrap_err();
    assert!(error.starts_with("GC_BLOCKED_BY_ACTIVE_LEASE"), "{error}");
    assert!(f.0.join("assets").join(&orphan).exists());
    reader.release_lease(&token).unwrap();
    let receipt = store.prune_assets(&policy(30), true).unwrap();
    assert_eq!(receipt.unlinked, vec![orphan]);
}

#[test]
fn unresolved_recovery_blocks_pruning() {
    let f = Fixture::new();
    let mut store = f.store();
    let orphan = store
        .publish_asset(b"orphan during recovery", 1024)
        .unwrap();
    f.age(&orphan, 40);
    drop(store);
    // A pending journal head (an unknown native outcome) is a permanent root set.
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch(
        "INSERT INTO journal_events(operation,sequence,state,pending,body_digest,body) VALUES('op',1,'needs_recovery',1,'d',x'00');
         INSERT INTO journal_heads(operation,sequence) VALUES('op',1);",
    )
    .unwrap();
    drop(db);
    let mut store = f.store();
    let error = store.prune_assets(&policy(30), true).unwrap_err();
    assert!(error.starts_with("GC_BLOCKED_BY_RECOVERY"), "{error}");
    assert!(f.0.join("assets").join(&orphan).exists());
}

#[test]
fn interrupted_prune_is_finished_from_its_tombstones() {
    let f = Fixture::new();
    let mut store = f.store();
    let orphan = store.publish_asset(b"interrupted", 1024).unwrap();
    f.age(&orphan, 40);
    drop(store);
    // Simulate a crash after commit but before unlink: tombstone + trash file.
    let run = uuid::Uuid::new_v4().to_string();
    let trash = f.0.join("assets/.gc-trash").join(&run);
    std::fs::create_dir_all(&trash).unwrap();
    std::fs::rename(f.0.join("assets").join(&orphan), trash.join(&orphan)).unwrap();
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO gc_runs(id,created_ms,policy,state) VALUES(?1,1,x'7b7d','tombstoned')",
        [&run],
    )
    .unwrap();
    db.execute(
        "INSERT INTO gc_tombstones(run,name,kind,size,modified_ms,state) VALUES(?1,?2,'asset',11,1,'tombstoned')",
        [&run, &orphan],
    )
    .unwrap();
    db.execute("DELETE FROM assets WHERE digest=?1", [&orphan])
        .unwrap();
    assert!(
        db.execute("DELETE FROM gc_tombstones", []).is_err(),
        "tombstones are retained"
    );
    drop(db);
    let mut store = f.store();
    assert_eq!(
        store.asset_census().unwrap().interrupted_runs,
        vec![run.clone()]
    );
    let receipt = store.prune_assets(&policy(30), true).unwrap();
    assert_eq!(receipt.finished_interrupted_runs, vec![run]);
    assert!(!trash.join(&orphan).exists());
    assert_eq!(store.gc_runs(5).unwrap()[0]["state"], "completed");
}

#[test]
fn schema_twelve_upgrades_to_thirteen_with_gc_and_import_tables() {
    let f = Fixture::new();
    drop(f.store());
    let db = rusqlite::Connection::open(f.0.join("state.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TRIGGER gc_tombstones_no_delete; DROP TRIGGER gc_runs_no_delete; DROP TABLE gc_tombstones; DROP TABLE gc_runs;
         DROP TRIGGER legacy_job_imports_no_update; DROP TRIGGER legacy_job_imports_no_delete; DROP TABLE legacy_job_imports;
         PRAGMA user_version=12;",
    )
    .unwrap();
    drop(db);
    let mut store = f.store();
    assert!(store.gc_runs(1).unwrap().is_empty());
    let (record, created) = store
        .record_legacy_job_import(b"db", &json!({"x":1}), 5)
        .unwrap();
    assert!(created);
    let (again, created) = store
        .record_legacy_job_import(b"db", &json!({"x":2}), 6)
        .unwrap();
    assert!(!created, "importing the same bytes is idempotent");
    assert_eq!(again, record);
    assert_eq!(
        store.legacy_job_import(record.id).unwrap().report,
        json!({"x":1})
    );
    let version: i64 = rusqlite::Connection::open(f.0.join("state.sqlite3"))
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 14);
    assert!(std::fs::read_dir(&f.0).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".schema-v12-")
    }));
}
