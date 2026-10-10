use linguist_application::{
    backup::{
        CheckpointExporter, CheckpointRequest, DependentScope, ExportClaim, ExportRequest,
        PortFailure, ScopePreference, create_checkpoint, plan_coverage, require_checkpoint,
        summarize, verify_registered,
    },
    checkpoint::{CoverageRequirement, PackageLimits, ScopeCard, ScopeManifest, ScopeMedia},
    model_install::{
        InstallRequest, ModelInstallRequest, ModelPort, ObservedModel, OperationEvidence, install,
        manifest_digest, reconcile,
    },
};
use linguist_core::{
    model::{self, ManagedModel},
    records::{CollectionBinding, OperationState, StepState},
};
use linguist_store::{
    Store,
    lease::{LeaseToken, Resource},
};
use prost::Message;
use sha1::Digest as _;
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, PartialEq, Message)]
struct MediaEntries {
    #[prost(message, repeated, tag = "1")]
    entries: Vec<MediaEntry>,
}
#[derive(Clone, PartialEq, Message)]
struct MediaEntry {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    size: u32,
    #[prost(bytes, tag = "3")]
    sha1: Vec<u8>,
}

const MEDIA: &[u8] = b"disposable voice bytes";

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-checkpoint-writes-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("out")).unwrap();
        std::fs::create_dir_all(root.join("restore")).unwrap();
        std::fs::create_dir_all(root.join("scratch")).unwrap();
        Self { root }
    }
    fn store(&self) -> Store {
        Store::open(&self.root.join("state")).unwrap()
    }
    fn output(&self, name: &str) -> PathBuf {
        self.root.join("out").join(name)
    }
    fn limits(&self) -> PackageLimits {
        PackageLimits {
            max_package_bytes: 4 * 1024 * 1024,
            max_collection_bytes: 4 * 1024 * 1024,
            max_media_bytes: 1024 * 1024,
            max_media_map_bytes: 1024 * 1024,
            max_entries: 100,
            timeout: Duration::from_secs(10),
            scratch_dir: self.root.join("scratch"),
        }
    }
    fn request(&self, name: &str, group: Option<Uuid>) -> CheckpointRequest {
        CheckpointRequest {
            binding: binding(),
            scope: scope(),
            preference: ScopePreference::Affected,
            output: self.output(name),
            group_id: group,
            protected_manifest_digest: "protected-v1".into(),
            restore_target: self.root.join("restore"),
            limits: self.limits(),
            now_ms: 1_000_000,
        }
    }
    fn empty(&self, dir: &str) -> bool {
        std::fs::read_dir(self.root.join(dir)).unwrap().count() == 0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn lease(store: &mut Store) -> LeaseToken {
    store
        .acquire_lease(&Resource::CollectionWriter(Uuid::from_u128(7)), 300)
        .unwrap()
}

fn binding() -> CollectionBinding {
    CollectionBinding {
        endpoint: "http://127.0.0.1:8765".into(),
        profile_fingerprint: "a".repeat(64),
        path_fingerprint: "b".repeat(64),
        bridge_id: Uuid::from_u128(1),
        lineage_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(3),
        capability_digest: "c".repeat(64),
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn scope() -> ScopeManifest {
    ScopeManifest {
        schema_version: 1,
        requirement: CoverageRequirement {
            scheduling: true,
            media: true,
            schema: true,
        },
        note_ids: vec![10],
        cards: vec![ScopeCard {
            card_id: 20,
            note_id: 10,
            reps: 1,
            review_count: 1,
        }],
        model_ids: vec![1],
        media: vec![ScopeMedia {
            name: "voice.ogg".into(),
            sha1: sha1_hex(MEDIA),
        }],
    }
}

#[derive(Clone, Copy)]
struct Shape {
    revlog: bool,
    card: bool,
    media: bool,
}
const FULL: Shape = Shape {
    revlog: true,
    card: true,
    media: true,
};

fn collection(shape: Shape) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-collection-{}", Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY, ver INTEGER, models TEXT); INSERT INTO col VALUES (1,18,'{}'); CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT); INSERT INTO notes VALUES (10,1,'word'); CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,ord INTEGER,queue INTEGER,due INTEGER,ivl INTEGER,factor INTEGER,reps INTEGER,lapses INTEGER); CREATE TABLE revlog(id INTEGER PRIMARY KEY,cid INTEGER,ease INTEGER,ivl INTEGER,lastIvl INTEGER); CREATE TABLE graves(usn INTEGER,oid INTEGER,type INTEGER); CREATE TABLE notetypes(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO notetypes VALUES (1,'Basic');").unwrap();
    if shape.card {
        connection
            .execute_batch("INSERT INTO cards VALUES (20,10,1,0,2,5,3,2500,1,0);")
            .unwrap();
    }
    if shape.revlog {
        connection
            .execute_batch("INSERT INTO revlog VALUES (30,20,3,3,0);")
            .unwrap();
    }
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

fn package(shape: Shape) -> Vec<u8> {
    let map = if shape.media {
        MediaEntries {
            entries: vec![MediaEntry {
                name: "voice.ogg".into(),
                size: MEDIA.len() as u32,
                sha1: sha1::Sha1::digest(MEDIA).to_vec(),
            }],
        }
        .encode_to_vec()
    } else {
        vec![]
    };
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entry = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    entry("meta", &[8, 3]);
    entry(
        "collection.anki21b",
        &zstd::encode_all(collection(shape).as_slice(), 0).unwrap(),
    );
    entry("collection.anki2", &collection(shape));
    entry("media", &zstd::encode_all(map.as_slice(), 0).unwrap());
    if shape.media {
        entry("0", &zstd::encode_all(MEDIA, 0).unwrap());
    }
    zip.finish().unwrap().into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes))
}

enum Behavior {
    Write(Shape),
    FalseSuccess,
    Truncated,
    WrongSize,
    OtherPath,
    Rejected,
    Unknown,
}

struct Exporter {
    behavior: Behavior,
    calls: usize,
}
impl CheckpointExporter for Exporter {
    fn export_checkpoint(&mut self, request: &ExportRequest) -> Result<ExportClaim, PortFailure> {
        self.calls += 1;
        assert!(!request.destination.exists());
        assert!(request.include_media && request.include_scheduling);
        let write = |bytes: &[u8]| {
            std::fs::write(&request.destination, bytes).unwrap();
            ExportClaim {
                path: request.destination.clone(),
                size_bytes: bytes.len() as u64,
                sha256: sha256(bytes),
            }
        };
        match self.behavior {
            Behavior::Write(shape) => Ok(write(&package(shape))),
            Behavior::FalseSuccess => {
                let bytes = package(FULL);
                Ok(ExportClaim {
                    path: request.destination.clone(),
                    size_bytes: bytes.len() as u64,
                    sha256: sha256(&bytes),
                })
            }
            Behavior::Truncated => {
                let bytes = package(FULL);
                Ok(write(&bytes[..bytes.len() / 2]))
            }
            Behavior::WrongSize => {
                let mut claim = write(&package(FULL));
                claim.size_bytes += 1;
                Ok(claim)
            }
            Behavior::OtherPath => {
                let mut claim = write(&package(FULL));
                claim.path = request.destination.with_extension("other");
                Ok(claim)
            }
            Behavior::Rejected => Err(PortFailure::Rejected("preflight_rejected".into())),
            Behavior::Unknown => Err(PortFailure::Unknown("transport_ambiguous".into())),
        }
    }
}

fn exporter(behavior: Behavior) -> Exporter {
    Exporter { behavior, calls: 0 }
}

#[test]
fn verified_checkpoint_has_receipt_restoration_and_committed_journal() {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let mut port = exporter(Behavior::Write(FULL));
    let outcome = create_checkpoint(&mut store, &token, &mut port, f.request("a.colpkg", None))
        .map_err(|e| e.code)
        .unwrap();
    assert_eq!(port.calls, 1);
    assert!(outcome.coverage.escalated);
    assert_eq!(
        outcome.coverage.escalation_reason,
        Some("schema_action_requires_collection_package")
    );
    let receipt = &outcome.record.receipt;
    assert!(Path::new(&receipt.path).is_file());
    assert_eq!(
        receipt.checksum,
        sha256(&std::fs::read(&receipt.path).unwrap())
    );
    assert!(receipt.includes_scheduling && receipt.includes_media && receipt.includes_schema);
    assert!(receipt.restoration_evidence.is_some());
    let restoration = &outcome.record.evidence["restoration"];
    assert_eq!(restoration["passed"], true);
    assert_eq!(restoration["anki_importer_used"], false);
    assert_eq!(restoration["scope"]["reviews_verified"], 1);
    assert_eq!(restoration["restored_media_files"], 1);
    // Only the final artifact remains; restoration and scratch files are removed.
    assert_eq!(std::fs::read_dir(f.root.join("out")).unwrap().count(), 1);
    assert!(f.empty("restore") && f.empty("scratch"));
    let journal = store.journal(outcome.journal_id).unwrap();
    assert_eq!(journal.journal.state, OperationState::Committed);
    assert!(!journal.pending_recovery);
    assert_eq!(journal.journal.steps[0].state, StepState::Verified);
    let stored = store.checkpoint(receipt.id).unwrap();
    assert_eq!(&stored, &outcome.record);
    let summary = summarize(&store, &stored).unwrap();
    assert!(summary.checkpoint_eligible && summary.restoration_tested);
    let listed = store
        .list_checkpoints(Some("collection"), Some(999_999), 10)
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert!(
        store
            .list_checkpoints(None, Some(1_000_001), 10)
            .unwrap()
            .is_empty()
    );
    let report = verify_registered(
        &mut store,
        receipt.id,
        f.limits(),
        Some(&f.root.join("restore")),
        1_000_500,
    )
    .unwrap();
    assert!(report.verified && report.restoration_tested);
    assert_eq!(store.checkpoint_verifications(receipt.id).unwrap().len(), 1);
}

fn assert_blocked(behavior: Behavior, expected: &str, state: OperationState) {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let mut port = exporter(behavior);
    let failure =
        create_checkpoint(&mut store, &token, &mut port, f.request("a.colpkg", None)).unwrap_err();
    assert!(
        failure.code.starts_with(expected),
        "{} != {expected}",
        failure.code
    );
    assert_eq!(failure.journal_state, Some(state));
    let journal = store.journal(failure.journal_id.unwrap()).unwrap();
    assert_eq!(journal.journal.state, state);
    assert_eq!(
        journal.pending_recovery,
        state == OperationState::NeedsRecovery
    );
    assert!(store.list_checkpoints(None, None, 10).unwrap().is_empty());
    assert!(!f.output("a.colpkg").exists());
    assert!(f.empty("restore") && f.empty("scratch"));
    if state == OperationState::FailedBeforeWrite {
        assert!(f.empty("out"), "temporary artifact must be removed");
    }
}

#[test]
fn false_api_success_without_file_is_not_a_checkpoint() {
    assert_blocked(
        Behavior::FalseSuccess,
        "CHECKPOINT_ARTIFACT_MISSING",
        OperationState::FailedBeforeWrite,
    );
}

#[test]
fn truncated_package_fails_verification() {
    assert_blocked(
        Behavior::Truncated,
        "CHECKPOINT_ZIP_INVALID",
        OperationState::FailedBeforeWrite,
    );
}

#[test]
fn claim_mismatch_and_foreign_path_fail() {
    assert_blocked(
        Behavior::WrongSize,
        "CHECKPOINT_EXPORT_CLAIM_MISMATCH",
        OperationState::FailedBeforeWrite,
    );
    assert_blocked(
        Behavior::OtherPath,
        "CHECKPOINT_EXPORT_PATH_MISMATCH",
        OperationState::FailedBeforeWrite,
    );
}

#[test]
fn missing_media_scheduling_or_card_fails_scope() {
    assert_blocked(
        Behavior::Write(Shape {
            media: false,
            ..FULL
        }),
        "CHECKPOINT_SCOPE_MEDIA_MISSING",
        OperationState::FailedBeforeWrite,
    );
    assert_blocked(
        Behavior::Write(Shape {
            revlog: false,
            ..FULL
        }),
        "CHECKPOINT_SCOPE_SCHEDULING_MISSING",
        OperationState::FailedBeforeWrite,
    );
    assert_blocked(
        Behavior::Write(Shape {
            card: false,
            revlog: false,
            ..FULL
        }),
        "CHECKPOINT_SCOPE_CARD_MISSING",
        OperationState::FailedBeforeWrite,
    );
}

#[test]
fn rejected_and_unknown_exports_are_journaled_differently() {
    assert_blocked(
        Behavior::Rejected,
        "CHECKPOINT_EXPORT_REJECTED",
        OperationState::FailedBeforeWrite,
    );
    assert_blocked(
        Behavior::Unknown,
        "CHECKPOINT_EXPORT_UNKNOWN",
        OperationState::NeedsRecovery,
    );
}

#[test]
fn invalid_requests_fail_before_journal_or_export() {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let mut port = exporter(Behavior::Write(FULL));
    std::fs::write(f.output("exists.colpkg"), b"x").unwrap();
    for request in [
        f.request("exists.colpkg", None),
        f.request("not-a-package.zip", None),
        CheckpointRequest {
            output: PathBuf::from("relative.colpkg"),
            ..f.request("a.colpkg", None)
        },
        CheckpointRequest {
            scope: ScopeManifest {
                note_ids: vec![11, 10],
                ..scope()
            },
            ..f.request("a.colpkg", None)
        },
        CheckpointRequest {
            protected_manifest_digest: " ".into(),
            ..f.request("a.colpkg", None)
        },
    ] {
        let failure = create_checkpoint(&mut store, &token, &mut port, request).unwrap_err();
        assert!(failure.journal_id.is_none(), "{}", failure.code);
    }
    assert_eq!(port.calls, 0);
    assert_eq!(store.pending_journal_count().unwrap(), 0);
}

#[test]
fn coverage_always_escalates_to_collection_package() {
    let mut content = scope();
    content.requirement.schema = false;
    let plan = plan_coverage(ScopePreference::Affected, &content);
    assert_eq!(plan.package_scope, "collection");
    assert_eq!(
        plan.escalation_reason,
        Some("affected_scope_package_unavailable")
    );
    assert!(!plan.requirement.schema);
    let plan = plan_coverage(ScopePreference::Collection, &scope());
    assert!(!plan.escalated);
    assert!(plan.requirement.scheduling && plan.requirement.media && plan.requirement.schema);
}

fn dependent<'a>(
    binding: &'a CollectionBinding,
    notes: &'a [i64],
    group: Option<Uuid>,
    digest: &'a str,
    age: u64,
) -> DependentScope<'a> {
    DependentScope {
        binding,
        requirement: CoverageRequirement {
            scheduling: true,
            media: true,
            schema: false,
        },
        note_ids: notes,
        card_ids: &[20],
        model_ids: &[],
        media_names: &[],
        group_id: group,
        protected_manifest_digest: digest,
        now_ms: 1_030_000,
        reuse_max_age_seconds: age,
        max_package_bytes: 4 * 1024 * 1024,
    }
}

#[test]
fn gate_checks_binding_scope_reuse_and_artifact_bytes() {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let group = Uuid::new_v4();
    let outcome = create_checkpoint(
        &mut store,
        &token,
        &mut exporter(Behavior::Write(FULL)),
        f.request("a.colpkg", Some(group)),
    )
    .map_err(|e| e.code)
    .unwrap();
    let id = outcome.record.receipt.id;
    let binding = binding();
    let ok =
        require_checkpoint(&store, id, &dependent(&binding, &[10], Some(group), "x", 0)).unwrap();
    assert_eq!(ok.reuse_reason, None);
    let mut other = binding.clone();
    other.session_epoch = Uuid::new_v4();
    let check = |b: &CollectionBinding, notes: &[i64], g: Option<Uuid>, d: &str, age: u64| {
        require_checkpoint(&store, id, &dependent(b, notes, g, d, age))
    };
    assert_eq!(
        check(&other, &[10], Some(group), "x", 0).unwrap_err(),
        "CHECKPOINT_BINDING_MISMATCH"
    );
    assert_eq!(
        check(&binding, &[11], Some(group), "x", 0).unwrap_err(),
        "CHECKPOINT_SCOPE_INSUFFICIENT"
    );
    // Cross-group reuse needs configured age and the same protected manifest.
    let other_group = Some(Uuid::new_v4());
    assert_eq!(
        check(&binding, &[10], other_group, "protected-v1", 0).unwrap_err(),
        "CHECKPOINT_REUSE_REJECTED"
    );
    assert_eq!(
        check(&binding, &[10], other_group, "protected-v1", 10).unwrap_err(),
        "CHECKPOINT_REUSE_REJECTED"
    );
    assert_eq!(
        check(&binding, &[10], other_group, "changed", 60).unwrap_err(),
        "CHECKPOINT_REUSE_REJECTED"
    );
    assert!(
        check(&binding, &[10], other_group, "protected-v1", 60)
            .unwrap()
            .reuse_reason
            .is_some()
    );
    let path = PathBuf::from(&outcome.record.receipt.path);
    let bytes = std::fs::read(&path).unwrap();
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    std::fs::write(&path, &changed).unwrap();
    assert_eq!(
        check(&binding, &[10], Some(group), "x", 0).unwrap_err(),
        "CHECKPOINT_ARTIFACT_CHANGED"
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        check(&binding, &[10], Some(group), "x", 0).unwrap_err(),
        "CHECKPOINT_ARTIFACT_MISSING"
    );
    assert_eq!(
        require_checkpoint(
            &store,
            Uuid::new_v4(),
            &dependent(&binding, &[10], None, "x", 0)
        )
        .unwrap_err(),
        "CHECKPOINT_NOT_FOUND"
    );
}

#[test]
fn content_only_checkpoint_cannot_authorize_schema_action() {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let mut request = f.request("a.colpkg", None);
    request.scope.requirement.schema = false;
    request.scope.model_ids.clear();
    let outcome = create_checkpoint(
        &mut store,
        &token,
        &mut exporter(Behavior::Write(FULL)),
        request,
    )
    .map_err(|e| e.code)
    .unwrap();
    // A collection package still records the schema tables it carries.
    assert!(outcome.record.receipt.includes_schema);
    let binding = binding();
    let mut scope = dependent(&binding, &[], None, "protected-v1", 60);
    scope.model_ids = &[1];
    assert_eq!(
        require_checkpoint(&store, outcome.record.receipt.id, &scope).unwrap_err(),
        "CHECKPOINT_SCOPE_INSUFFICIENT"
    );
}

// ---------- model installation ----------

#[derive(Clone, Copy, PartialEq)]
enum Install {
    Exact,
    Partial,
    Lost { created: bool },
    Rejected,
}

struct Models {
    existing: Vec<ObservedModel>,
    behavior: Install,
    installs: usize,
    evidence: OperationEvidence,
}
fn observed(model: &ManagedModel, id: i64) -> ObservedModel {
    let mut templates = model.templates.clone();
    templates.sort_by_key(|t| t.ordinal);
    ObservedModel {
        id,
        name: model.name.clone(),
        fields: model.fields.clone(),
        templates,
        css: model.css.clone(),
    }
}
impl ModelPort for Models {
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>, String> {
        Ok(self
            .existing
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect())
    }
    fn install_model(&mut self, request: &ModelInstallRequest) -> Result<i64, PortFailure> {
        self.installs += 1;
        assert!(request.expected_absent);
        assert_eq!(
            request.manifest_digest,
            manifest_digest(&request.manifest).unwrap()
        );
        match self.behavior {
            Install::Exact => {
                self.existing.push(observed(&request.manifest, 500));
                Ok(500)
            }
            Install::Partial => {
                let mut partial = observed(&request.manifest, 501);
                partial.css.clear();
                self.existing.push(partial);
                Ok(501)
            }
            Install::Lost { created } => {
                if created {
                    self.existing.push(observed(&request.manifest, 502));
                }
                Err(PortFailure::Unknown("response lost".into()))
            }
            Install::Rejected => Err(PortFailure::Rejected("preflight_rejected".into())),
        }
    }
    fn operation_evidence(&mut self, _: Uuid) -> Result<OperationEvidence, String> {
        Ok(self.evidence.clone())
    }
}

fn models(existing: Vec<ObservedModel>, behavior: Install) -> Models {
    Models {
        existing,
        behavior,
        installs: 0,
        evidence: OperationEvidence::Absent,
    }
}

struct Installed {
    f: Fixture,
    store: Store,
    token: LeaseToken,
    checkpoint: Uuid,
}
fn with_checkpoint() -> Installed {
    let f = Fixture::new();
    let mut store = f.store();
    let token = lease(&mut store);
    let checkpoint = create_checkpoint(
        &mut store,
        &token,
        &mut exporter(Behavior::Write(FULL)),
        f.request("a.colpkg", None),
    )
    .map_err(|e| e.code)
    .unwrap()
    .record
    .receipt
    .id;
    Installed {
        f,
        store,
        token,
        checkpoint,
    }
}
fn request(checkpoint: Uuid) -> InstallRequest<'static> {
    InstallRequest {
        binding: binding(),
        target: model::grammar(),
        checkpoint_id: checkpoint,
        group_id: None,
        protected_manifest_digest: "protected-v1",
        reuse_max_age_seconds: 600,
        max_package_bytes: 4 * 1024 * 1024,
        now_ms: 1_010_000,
    }
}

#[test]
fn checkpoint_failure_guarantees_zero_model_mutation() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Exact);
    let missing = install(&mut s.store, &s.token, &mut port, request(Uuid::new_v4())).unwrap_err();
    assert_eq!(missing, "CHECKPOINT_NOT_FOUND");
    let mut wrong = request(s.checkpoint);
    wrong.binding.session_epoch = Uuid::new_v4();
    assert_eq!(
        install(&mut s.store, &s.token, &mut port, wrong).unwrap_err(),
        "CHECKPOINT_BINDING_MISMATCH"
    );
    let receipt = s.store.checkpoint(s.checkpoint).unwrap().receipt;
    std::fs::remove_file(&receipt.path).unwrap();
    assert_eq!(
        install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap_err(),
        "CHECKPOINT_ARTIFACT_MISSING"
    );
    assert_eq!(port.installs, 0);
    assert!(
        s.store
            .model_operations_named(&model::grammar().name)
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
    drop(s.f);
}

#[test]
fn exact_model_is_reused_and_same_name_different_manifest_blocks() {
    let mut s = with_checkpoint();
    let target = model::grammar();
    let mut port = models(vec![observed(&target, 42)], Install::Exact);
    let reused = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!((reused.action, reused.model_id), ("reused", Some(42)));
    let mut different = observed(&target, 43);
    different.fields.swap(0, 1);
    different.css.push_str("/* user */");
    let mut port = models(vec![different], Install::Exact);
    let error = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap_err();
    assert!(error.starts_with("MODEL_NAME_COLLISION"), "{error}");
    assert!(error.contains("fields") && error.contains("css"));
    // Grammar v4 has one template: a changed front is the template difference.
    let mut reordered = observed(&target, 44);
    reordered.templates[0].front.push_str("<!-- user -->");
    let mut port2 = models(vec![reordered], Install::Exact);
    assert!(
        install(&mut s.store, &s.token, &mut port2, request(s.checkpoint))
            .unwrap_err()
            .contains("templates")
    );
    let mut port3 = models(
        vec![observed(&target, 1), observed(&target, 2)],
        Install::Exact,
    );
    assert_eq!(
        install(&mut s.store, &s.token, &mut port3, request(s.checkpoint)).unwrap_err(),
        "MODEL_NAME_AMBIGUOUS"
    );
    assert_eq!(port.installs + port2.installs + port3.installs, 0);
    assert_eq!(port.existing.len(), 1, "shared model is never deleted");
}

#[test]
fn created_model_is_journaled_before_call_and_verified() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Exact);
    let created = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!((created.action, created.model_id), ("created", Some(500)));
    assert_eq!(created.journal_state, Some(OperationState::Committed));
    let operation = created.operation_id.unwrap();
    let journal = s.store.journal(operation).unwrap().journal;
    assert_eq!(journal.backup_id, s.checkpoint);
    assert_eq!(journal.steps[0].state, StepState::Verified);
    let record = s.store.model_operation(operation).unwrap();
    assert_eq!(record.evidence["observed_same_name"], serde_json::json!([]));
    // A second request now reuses the verified model.
    let again = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!(again.action, "reused");
    assert_eq!(port.installs, 1);
}

#[test]
fn partial_model_needs_recovery_and_blocks_new_attempts() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Partial);
    let partial = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!(partial.action, "needs_recovery");
    assert!(partial.needs_recovery);
    assert!(
        partial
            .issues
            .iter()
            .any(|i| i.code == "MODEL_PARTIAL_REQUIRES_RECOVERY")
    );
    let operation = partial.operation_id.unwrap();
    assert!(s.store.journal(operation).unwrap().pending_recovery);
    let blocked = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap_err();
    assert!(blocked.starts_with("MODEL_INSTALL_RECOVERY_REQUIRED"));
    // Reconciliation cannot adopt a model whose manifest differs.
    port.evidence = OperationEvidence::Created {
        model_id: 501,
        manifest_digest: manifest_digest(&model::grammar()).unwrap(),
    };
    let still = reconcile(&mut s.store, &s.token, &mut port, operation).unwrap();
    assert_eq!(still.action, "needs_recovery");
    assert_eq!(port.installs, 1);
    assert_eq!(port.existing.len(), 1, "partial model is not deleted");
}

#[test]
fn lost_response_reconciles_only_with_operation_evidence() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Lost { created: true });
    let lost = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!(lost.journal_state, Some(OperationState::NeedsRecovery));
    let operation = lost.operation_id.unwrap();
    let journal = s.store.journal(operation).unwrap();
    assert_eq!(journal.journal.steps[0].state, StepState::Unknown);
    // Exact name and manifest without companion evidence are insufficient.
    let name_only = reconcile(&mut s.store, &s.token, &mut port, operation).unwrap();
    assert_eq!(name_only.action, "needs_recovery");
    assert!(
        name_only
            .issues
            .iter()
            .any(|i| i.code == "MODEL_RECONCILE_EVIDENCE_INSUFFICIENT")
    );
    port.evidence = OperationEvidence::Created {
        model_id: 999,
        manifest_digest: manifest_digest(&model::grammar()).unwrap(),
    };
    assert_eq!(
        reconcile(&mut s.store, &s.token, &mut port, operation)
            .unwrap()
            .action,
        "needs_recovery"
    );
    port.evidence = OperationEvidence::Created {
        model_id: 502,
        manifest_digest: manifest_digest(&model::grammar()).unwrap(),
    };
    let adopted = reconcile(&mut s.store, &s.token, &mut port, operation).unwrap();
    assert_eq!((adopted.action, adopted.model_id), ("created", Some(502)));
    assert_eq!(adopted.journal_state, Some(OperationState::Committed));
    assert!(!s.store.journal(operation).unwrap().pending_recovery);
    assert_eq!(port.installs, 1, "reconciliation never re-sends");
}

#[test]
fn lost_response_without_effect_closes_with_evidence_and_allows_retry() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Lost { created: false });
    let lost = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    let operation = lost.operation_id.unwrap();
    // Absent name without evidence stays unknown.
    assert_eq!(
        reconcile(&mut s.store, &s.token, &mut port, operation)
            .unwrap()
            .action,
        "needs_recovery"
    );
    port.evidence = OperationEvidence::FailedBeforeWrite;
    let closed = reconcile(&mut s.store, &s.token, &mut port, operation).unwrap();
    assert_eq!(closed.action, "not_created");
    assert_eq!(
        closed.journal_state,
        Some(OperationState::FailedBeforeWrite)
    );
    port.behavior = Install::Exact;
    let created = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap();
    assert_eq!(created.action, "created");
    assert_eq!(port.installs, 2);
}

#[test]
fn rejected_install_without_effect_fails_before_write() {
    let mut s = with_checkpoint();
    let mut port = models(vec![], Install::Rejected);
    let error = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap_err();
    assert!(error.starts_with("MODEL_INSTALL_REJECTED"));
    let operation = s
        .store
        .model_operations_named(&model::grammar().name)
        .unwrap()[0]
        .operation_id;
    let journal = s.store.journal(operation).unwrap();
    assert_eq!(journal.journal.state, OperationState::FailedBeforeWrite);
    assert!(!journal.pending_recovery);
}

#[test]
fn crash_after_request_started_reconciles_as_unknown() {
    let mut s = with_checkpoint();
    // A port whose install never returns normally models a crash after dispatch.
    struct Crash;
    impl ModelPort for Crash {
        fn models_named(&mut self, _: &str) -> Result<Vec<ObservedModel>, String> {
            Ok(vec![])
        }
        fn install_model(&mut self, _: &ModelInstallRequest) -> Result<i64, PortFailure> {
            panic!("process died after request_started")
        }
        fn operation_evidence(&mut self, _: Uuid) -> Result<OperationEvidence, String> {
            Ok(OperationEvidence::Absent)
        }
    }
    let root = s.f.root.join("state");
    let checkpoint = s.checkpoint;
    let token = s.token.clone();
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut store = Store::open(&root).unwrap();
        let _ = install(&mut store, &token, &mut Crash, request(checkpoint));
    }));
    assert!(crashed.is_err());
    let operation = s
        .store
        .model_operations_named(&model::grammar().name)
        .unwrap()[0]
        .operation_id;
    let journal = s.store.journal(operation).unwrap();
    assert_eq!(journal.journal.state, OperationState::Mutating);
    assert_eq!(journal.journal.steps[0].state, StepState::RequestStarted);
    assert!(journal.pending_recovery);
    let mut port = models(vec![], Install::Exact);
    let blocked = install(&mut s.store, &s.token, &mut port, request(s.checkpoint)).unwrap_err();
    assert!(blocked.starts_with("MODEL_INSTALL_RECOVERY_REQUIRED"));
    let unknown = reconcile(&mut s.store, &s.token, &mut port, operation).unwrap();
    assert_eq!(unknown.journal_state, Some(OperationState::NeedsRecovery));
    assert_eq!(
        s.store.journal(operation).unwrap().journal.steps[0].state,
        StepState::Unknown
    );
    assert_eq!(port.installs, 0);
}
