use linguist_core::records::*;
use linguist_store::{journal::*, *};
use uuid::Uuid;
struct Fixture {
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("lab-journal-test-{}", Uuid::new_v4())),
        }
    }
    fn store(&self) -> Store {
        Store::open(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn journal() -> OperationJournal {
    OperationJournal {
        id: Uuid::new_v4(),
        group_id: None,
        approval_digest: "approved".into(),
        binding: CollectionBinding {
            endpoint: "http://127.0.0.1:8765".into(),
            profile_fingerprint: "profile".into(),
            path_fingerprint: "path".into(),
            bridge_id: Uuid::new_v4(),
            lineage_id: Uuid::new_v4(),
            session_epoch: Uuid::new_v4(),
            capability_digest: "capability".into(),
        },
        snapshot_id: Uuid::new_v4(),
        backup_id: Uuid::new_v4(),
        state: OperationState::Prepared,
        steps: vec![JournalStep {
            id: Uuid::new_v4(),
            action: "backup-export".into(),
            payload_digest: "payload".into(),
            precondition_digest: "before".into(),
            expected_post_digest: "after".into(),
            state: StepState::IntentRecorded,
            observed_digest: None,
        }],
        issues: vec![],
    }
}
fn start(store: &mut Store) -> JournalVersion {
    let mut j = journal();
    let before = store.append_journal(&j, None).unwrap();
    j.state = OperationState::Preflight;
    j.steps[0].state = StepState::RequestStarted;
    store.append_journal(&j, Some(&before)).unwrap()
}
#[test]
fn started_request_survives_restart_and_is_pending_without_blind_rewind() {
    let f = Fixture::new();
    let mut store = f.store();
    let started = start(&mut store);
    drop(store);
    let mut store = f.store();
    let current = store.journal(started.journal.id).unwrap();
    assert_eq!(current.sequence, 2);
    assert!(current.pending_recovery);
    assert_eq!(store.pending_journals(10).unwrap().len(), 1);
    let mut rewind = current.journal.clone();
    rewind.steps[0].state = StepState::IntentRecorded;
    assert!(store.append_journal(&rewind, Some(&current)).is_err());
}
#[test]
fn unknown_needs_recovery_and_cannot_be_redispatched() {
    let f = Fixture::new();
    let mut store = f.store();
    let before = start(&mut store);
    let mut unknown = before.journal.clone();
    unknown.steps[0].state = StepState::Unknown;
    assert!(store.append_journal(&unknown, Some(&before)).is_err());
    unknown.state = OperationState::NeedsRecovery;
    let unknown = store.append_journal(&unknown, Some(&before)).unwrap();
    let mut retry = unknown.journal.clone();
    retry.steps[0].state = StepState::RequestStarted;
    assert!(store.append_journal(&retry, Some(&unknown)).is_err());
    drop(store);
    assert!(
        f.store()
            .journal(unknown.journal.id)
            .unwrap()
            .pending_recovery
    );
}
#[test]
fn stale_versions_and_forged_prior_objects_are_rejected() {
    let f = Fixture::new();
    let mut a = f.store();
    let mut b = f.store();
    let mut j = journal();
    let first = a.append_journal(&j, None).unwrap();
    j.state = OperationState::Preflight;
    let second = a.append_journal(&j, Some(&first)).unwrap();
    assert!(b.append_journal(&j, Some(&first)).is_err());
    let mut forged = second;
    forged.journal.steps[0].payload_digest = "forged".into();
    assert!(b.append_journal(&j, Some(&forged)).is_err());
}
#[test]
fn sent_effect_never_becomes_failed_before_write_or_unverified_commit() {
    let f = Fixture::new();
    let mut store = f.store();
    let before = start(&mut store);
    let mut j = before.journal.clone();
    j.state = OperationState::FailedBeforeWrite;
    assert!(store.append_journal(&j, Some(&before)).is_err());
    j.state = OperationState::Committed;
    assert!(store.append_journal(&j, Some(&before)).is_err());
}
#[test]
fn exact_verified_post_digest_is_required_and_intents_cannot_change() {
    let f = Fixture::new();
    let mut store = f.store();
    let before = start(&mut store);
    let mut j = before.journal.clone();
    j.steps[0].state = StepState::ObservedSuccess;
    j.steps[0].observed_digest = Some("wrong".into());
    let before = store.append_journal(&j, Some(&before)).unwrap();
    j.steps[0].state = StepState::Verified;
    assert!(store.append_journal(&j, Some(&before)).is_err());
    j.steps[0].observed_digest = Some("after".into());
    let verified = store.append_journal(&j, Some(&before)).unwrap();
    j.steps[0].payload_digest = "changed".into();
    assert!(store.append_journal(&j, Some(&verified)).is_err());
}
#[test]
fn historical_events_cannot_be_updated_or_deleted() {
    let f = Fixture::new();
    let mut store = f.store();
    start(&mut store);
    let connection = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    assert!(
        connection
            .execute("DELETE FROM journal_events", [])
            .is_err()
    );
    assert!(
        connection
            .execute("UPDATE journal_events SET state='committed'", [])
            .is_err()
    );
}

#[test]
fn journal_read_rejects_state_index_that_disagrees_with_body() {
    let f = Fixture::new();
    let mut store = f.store();
    let current = start(&mut store);
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TRIGGER journal_events_no_update")
        .unwrap();
    db.execute(
        "UPDATE journal_events SET state='committed' WHERE operation=?1 AND sequence=2",
        [current.journal.id.to_string()],
    )
    .unwrap();
    assert_eq!(
        store.journal(current.journal.id).unwrap_err(),
        "JOURNAL_CORRUPT"
    );
}
#[test]
fn schema_one_migration_has_verified_private_backup() {
    let f = Fixture::new();
    drop(f.store());
    let db = f.root.join("state.sqlite3");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("DROP TABLE preparation_controls;DROP TABLE preparation_event_assets;DROP TABLE preparation_events;DROP TABLE preparation_jobs;DROP TABLE approvals;DROP TABLE validations;DROP TABLE journal_heads;DROP TABLE journal_events;DROP TABLE leases;PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    drop(f.store());
    let backup = std::fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".schema-v1-")
                && p.extension().is_some_and(|e| e == "sqlite3")
        })
        .unwrap();
    let connection = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}
#[test]
fn crash_boundary_request_started_survives_process_exit() {
    const VARIABLE: &str = "LAB_TEST_CRASH_JOURNAL_ROOT";
    if let Ok(root) = std::env::var(VARIABLE) {
        let mut store = Store::open(std::path::Path::new(&root)).unwrap();
        start(&mut store);
        std::process::exit(23);
    }
    let f = Fixture::new();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_boundary_request_started_survives_process_exit",
            "--nocapture",
        ])
        .env(VARIABLE, &f.root)
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(23));
    let store = f.store();
    let pending = store.pending_journals(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].journal.steps[0].state, StepState::RequestStarted);
    assert!(pending[0].pending_recovery);
}
#[test]
fn pending_step_blocks_starting_any_later_effect() {
    let f = Fixture::new();
    let mut store = f.store();
    let before = start(&mut store);
    let mut j = before.journal.clone();
    let mut step = j.steps[0].clone();
    step.id = Uuid::new_v4();
    step.state = StepState::IntentRecorded;
    j.steps.push(step);
    let before = store.append_journal(&j, Some(&before)).unwrap();
    j.steps[1].state = StepState::RequestStarted;
    assert!(store.append_journal(&j, Some(&before)).is_err());
    assert_eq!(store.pending_journal_count().unwrap(), 1);
}
