use linguist_application::{
    backup::{
        CheckpointExporter, CheckpointRequest, ExportClaim, ExportRequest, PortFailure,
        ScopePreference, create_checkpoint,
    },
    checkpoint::{CoverageRequirement, PackageLimits, ScopeCard, ScopeManifest},
};
use linguist_core::records::CollectionBinding;
use prost::Message;
use std::{io::Write, path::PathBuf, process::Command};
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

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("lab-backup-cli-{}", Uuid::new_v4()));
    for dir in ["out", "scratch", "restore"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    Fixture(root)
}

fn cli(f: &Fixture) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.args(["--output", "json"]);
    c.env_clear();
    c.env("HOME", "/tmp/lab-command-tests-no-config");
    c.args([
        "--set",
        &format!("storage.state_dir={}", f.0.join("state").display()),
        "--set",
        &format!(
            "backup.verify_scratch_dir={}",
            f.0.join("scratch").display()
        ),
    ]);
    c
}

fn collection() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-cli-collection-{}", Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY, ver INTEGER, models TEXT); INSERT INTO col VALUES (1,18,'{}'); CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT); INSERT INTO notes VALUES (10,1,'word'); CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,ord INTEGER,queue INTEGER,due INTEGER,ivl INTEGER,factor INTEGER,reps INTEGER,lapses INTEGER); INSERT INTO cards VALUES (20,10,1,0,2,5,3,2500,1,0); CREATE TABLE revlog(id INTEGER PRIMARY KEY,cid INTEGER,ease INTEGER,ivl INTEGER,lastIvl INTEGER); INSERT INTO revlog VALUES (30,20,3,3,0); CREATE TABLE graves(usn INTEGER,oid INTEGER,type INTEGER); CREATE TABLE notetypes(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO notetypes VALUES (1,'Basic');").unwrap();
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

fn package() -> Vec<u8> {
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
        &zstd::encode_all(collection().as_slice(), 0).unwrap(),
    );
    entry("collection.anki2", &collection());
    let map = MediaEntries { entries: vec![] }.encode_to_vec();
    entry("media", &zstd::encode_all(map.as_slice(), 0).unwrap());
    zip.finish().unwrap().into_inner()
}

fn scope() -> ScopeManifest {
    ScopeManifest {
        schema_version: 1,
        requirement: CoverageRequirement {
            scheduling: true,
            media: false,
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
        media: vec![],
    }
}

struct Writes;
impl CheckpointExporter for Writes {
    fn export_checkpoint(&mut self, request: &ExportRequest) -> Result<ExportClaim, PortFailure> {
        let bytes = package();
        std::fs::write(&request.destination, &bytes).unwrap();
        Ok(ExportClaim {
            path: request.destination.clone(),
            size_bytes: bytes.len() as u64,
            sha256: format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)),
        })
    }
}

fn stored_checkpoint(f: &Fixture) -> (Uuid, PathBuf) {
    let mut store = linguist_store::Store::open(&f.0.join("state")).unwrap();
    let token = store
        .acquire_lease(
            &linguist_store::lease::Resource::CollectionWriter(Uuid::from_u128(9)),
            60,
        )
        .unwrap();
    let outcome = create_checkpoint(
        &mut store,
        &token,
        &mut Writes,
        CheckpointRequest {
            binding: CollectionBinding {
                endpoint: "http://127.0.0.1:8765".into(),
                profile_fingerprint: "a".repeat(64),
                path_fingerprint: "b".repeat(64),
                bridge_id: Uuid::from_u128(1),
                lineage_id: Uuid::from_u128(2),
                session_epoch: Uuid::from_u128(3),
                capability_digest: "c".repeat(64),
            },
            scope: scope(),
            preference: ScopePreference::Collection,
            output: f.0.join("out/a.colpkg"),
            group_id: None,
            protected_manifest_digest: "protected".into(),
            restore_target: f.0.join("restore"),
            limits: PackageLimits {
                max_package_bytes: 1 << 22,
                max_collection_bytes: 1 << 22,
                max_media_bytes: 1 << 20,
                max_media_map_bytes: 1 << 20,
                max_entries: 100,
                timeout: std::time::Duration::from_secs(10),
                scratch_dir: f.0.join("scratch"),
            },
            now_ms: 1_767_225_600_000, // 2026-01-01T00:00:00Z
        },
    )
    .map_err(|e| e.code)
    .unwrap();
    store.release_lease(&token).unwrap();
    let receipt = outcome.record.receipt;
    (receipt.id, PathBuf::from(receipt.path))
}

fn json(out: &std::process::Output) -> serde_json::Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn create_previews_coverage_and_apply_fails_before_any_effect() {
    let f = fixture();
    let manifest = f.0.join("scope.json");
    std::fs::write(&manifest, serde_json::to_vec(&scope()).unwrap()).unwrap();
    let output = f.0.join("out/new.colpkg");
    let preview = json(
        &cli(&f)
            .args(["backup", "create", "--scope", "affected", "--output"])
            .arg(&output)
            .arg("--scope-manifest")
            .arg(&manifest)
            .output()
            .unwrap(),
    );
    assert_eq!(preview["mode"], "preview");
    assert_eq!(preview["coverage"]["package_scope"], "collection");
    assert_eq!(
        preview["coverage"]["escalation_reason"],
        "schema_action_requires_collection_package"
    );
    assert_eq!(preview["coverage"]["requirement"]["media"], true);
    assert_eq!(preview["scope_entries"]["cards"], 1);
    assert_eq!(preview["native_export_available"], false);
    assert_eq!(preview["apply_eligible"], false);
    let apply = cli(&f)
        .args(["backup", "create", "--scope", "collection", "--output"])
        .arg(&output)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(apply.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&apply.stderr).contains("CAPABILITY_UNAVAILABLE"));
    assert!(!output.exists());
    assert!(!f.0.join("state").exists(), "no state, lease or journal");
    for (args, code) in [
        (
            vec!["--scope", "deck"],
            "CHECKPOINT_SCOPE_PREFERENCE_INVALID",
        ),
        (
            vec!["--scope", "collection", "--output", "relative.colpkg"],
            "CHECKPOINT_OUTPUT_MUST_BE_ABSOLUTE",
        ),
    ] {
        let mut command = cli(&f);
        command.args(["backup", "create"]).args(&args);
        if !args.contains(&"--output") {
            command.arg("--output").arg(&output);
        }
        let out = command.output().unwrap();
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(code),
            "{out:?}"
        );
    }
    let mut bad = scope();
    bad.note_ids = vec![11];
    std::fs::write(&manifest, serde_json::to_vec(&bad).unwrap()).unwrap();
    let out = cli(&f)
        .args(["backup", "create", "--scope", "collection", "--output"])
        .arg(&output)
        .arg("--scope-manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("CHECKPOINT_SCOPE_INVALID"));
}

#[test]
fn list_reports_receipts_and_filters_by_time_and_scope() {
    let f = fixture();
    let empty = json(&cli(&f).args(["backup", "list"]).output().unwrap());
    assert_eq!(empty["checkpoints"], serde_json::json!([]));
    assert!(!f.0.join("state").exists());
    let (id, path) = stored_checkpoint(&f);
    let listed = json(&cli(&f).args(["backup", "list"]).output().unwrap());
    let entry = &listed["checkpoints"][0];
    assert_eq!(entry["id"], id.to_string());
    assert_eq!(entry["path"], path.to_string_lossy().as_ref());
    assert_eq!(entry["checkpoint_eligible"], true);
    assert_eq!(entry["includes_schema"], true);
    for (args, count) in [
        (vec!["--since", "2025-12-31"], 1),
        (vec!["--since", "2026-01-01T00:00:01Z"], 0),
        (vec!["--scope", "affected"], 0),
        (vec!["--scope", "collection"], 1),
    ] {
        let value = json(
            &cli(&f)
                .args(["backup", "list"])
                .args(&args)
                .output()
                .unwrap(),
        );
        assert_eq!(
            value["checkpoints"].as_array().unwrap().len(),
            count,
            "{args:?}"
        );
    }
    for bad in [
        vec!["--since", "2026-02-30"],
        vec!["--since", "2026-01-01T00:00:00+09:00"],
        vec!["--scope", "deck"],
    ] {
        let out = cli(&f)
            .args(["backup", "list"])
            .args(&bad)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{bad:?}");
    }
}

#[test]
fn verify_registered_receipt_detects_missing_and_changed_artifacts() {
    let f = fixture();
    let (id, path) = stored_checkpoint(&f);
    let id = id.to_string();
    let ok = json(&cli(&f).args(["backup", "verify", &id]).output().unwrap());
    assert_eq!(ok["verification"]["verified"], true);
    assert_eq!(ok["verification"]["restoration_tested"], false);
    let restored = json(
        &cli(&f)
            .args(["backup", "verify", &id, "--restore-test-target"])
            .arg(f.0.join("restore"))
            .output()
            .unwrap(),
    );
    assert_eq!(restored["verification"]["restoration_tested"], true);
    assert_eq!(
        restored["verification"]["evidence"]["restoration"]["scope"]["reviews_verified"],
        1
    );
    assert_eq!(std::fs::read_dir(f.0.join("restore")).unwrap().count(), 0);
    let mut bytes = std::fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    let changed = cli(&f).args(["backup", "verify", &id]).output().unwrap();
    assert!(String::from_utf8_lossy(&changed.stderr).contains("CHECKPOINT_ARTIFACT_CHANGED"));
    std::fs::remove_file(&path).unwrap();
    let missing = cli(&f).args(["backup", "verify", &id]).output().unwrap();
    assert!(String::from_utf8_lossy(&missing.stderr).contains("CHECKPOINT_ARTIFACT_MISSING"));
    let unknown = cli(&f)
        .args(["backup", "verify", &Uuid::new_v4().to_string()])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("CHECKPOINT_NOT_FOUND"));
}

#[test]
fn verify_unregistered_file_checks_scope_and_restores_but_is_never_eligible() {
    let f = fixture();
    let file = f.0.join("out/file.colpkg");
    std::fs::write(&file, package()).unwrap();
    let manifest = f.0.join("scope.json");
    std::fs::write(&manifest, serde_json::to_vec(&scope()).unwrap()).unwrap();
    let report = json(
        &cli(&f)
            .args(["backup", "verify"])
            .arg(&file)
            .arg("--scope-manifest")
            .arg(&manifest)
            .arg("--restore-test-target")
            .arg(f.0.join("restore"))
            .output()
            .unwrap(),
    );
    assert_eq!(report["registered"], false);
    assert_eq!(report["checkpoint_eligible"], false);
    assert_eq!(report["inspection"]["collection_scope_verified"], true);
    assert_eq!(report["restoration"]["passed"], true);
    assert!(!f.0.join("state").exists());
    let mut missing = scope();
    missing.cards[0].review_count = 2;
    std::fs::write(&manifest, serde_json::to_vec(&missing).unwrap()).unwrap();
    let out = cli(&f)
        .args(["backup", "verify"])
        .arg(&file)
        .arg("--scope-manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("CHECKPOINT_SCOPE_SCHEDULING_MISSING"));
    let truncated = f.0.join("out/truncated.colpkg");
    let bytes = package();
    std::fs::write(&truncated, &bytes[..bytes.len() - 10]).unwrap();
    let out = cli(&f)
        .args(["backup", "verify"])
        .arg(&truncated)
        .output()
        .unwrap();
    assert!(!out.status.success());
}
