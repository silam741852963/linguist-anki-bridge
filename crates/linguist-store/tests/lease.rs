use linguist_store::{lease::*, *};
use uuid::Uuid;
struct Fixture {
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("lab-lease-test-{}", Uuid::new_v4())),
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
#[test]
fn worker_identity_is_live_and_tokens_are_not_reentrant() {
    let f = Fixture::new();
    let mut a = f.store();
    let mut b = f.store();
    let resource = Resource::CollectionWriter(Uuid::new_v4());
    let token = a.acquire_lease(&resource, 60).unwrap();
    assert_eq!(
        ProcessIdentity::current().unwrap().liveness(),
        Liveness::Alive
    );
    b.validate_lease(&token).unwrap();
    assert!(b.acquire_lease(&resource, 60).is_err());
    a.release_lease(&token).unwrap();
    assert!(a.validate_lease(&token).is_err());
    let next = b.acquire_lease(&resource, 60).unwrap();
    assert!(a.renew_lease(&token, 60).is_err());
    b.validate_lease(&next).unwrap();
}
#[test]
fn expired_lease_cannot_be_reclaimed_while_owner_is_alive() {
    let f = Fixture::new();
    let mut a = f.store();
    let resource = Resource::JobWorker(Uuid::new_v4());
    let token = a.acquire_lease(&resource, 60).unwrap();
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute("UPDATE leases SET expires_ms=0", []).unwrap();
    assert!(a.validate_lease(&token).is_err());
    assert!(f.store().acquire_lease(&resource, 60).is_err());
    a.renew_lease(&token, 60).unwrap();
    a.validate_lease(&token).unwrap();
}
#[test]
fn expired_absent_worker_is_reclaimed_with_new_fencing_token() {
    let f = Fixture::new();
    let mut a = f.store();
    let resource = Resource::JobWorker(Uuid::new_v4());
    let old = a.acquire_lease(&resource, 60).unwrap();
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute("UPDATE leases SET expires_ms=0,pid=4294967295", [])
        .unwrap();
    let new = f.store().acquire_lease(&resource, 60).unwrap();
    assert!(a.validate_lease(&old).is_err());
    a.validate_lease(&new).unwrap();
}
#[test]
fn independent_job_and_writer_leases_do_not_contend() {
    let f = Fixture::new();
    let mut store = f.store();
    let id = Uuid::new_v4();
    let job = store.acquire_lease(&Resource::JobWorker(id), 60).unwrap();
    let writer = store
        .acquire_lease(&Resource::CollectionWriter(id), 60)
        .unwrap();
    store.validate_lease(&job).unwrap();
    store.validate_lease(&writer).unwrap();
}
