use linguist_anki::native::inspect_native_manifest;
use linguist_core::records::*;
use linguist_store::Store;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};
use uuid::Uuid;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
}

struct Server {
    endpoint: String,
    thread: std::thread::JoinHandle<Vec<Value>>,
}
impl Server {
    fn new(results: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = vec![];
            for result in results {
                let deadline = Instant::now() + Duration::from_secs(5);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "expected read did not arrive");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("content-length:") {
                        length = line.split(':').nth(1).unwrap().trim().parse().unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                requests.push(serde_json::from_slice(&bytes).unwrap());
                let body = json!({"result":result,"error":null}).to_string();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let mut stream = stream;
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(body.as_bytes()).unwrap();
            }
            requests
        });
        Self { endpoint, thread }
    }
    fn finish(self) -> Vec<Value> {
        self.thread.join().unwrap()
    }
}

fn manifest(journal: &OperationJournal) -> Value {
    json!({
        "protocol":"lab-native-v1", "companion_version":"0.1.0",
        "bridge_id":journal.binding.bridge_id,
        "integration":{"anki_version":"fixture","anki_connect_source_digest":"d".repeat(64)},
        "collection_session":{
            "lineage_id":journal.binding.lineage_id,
            "session_epoch":journal.binding.session_epoch,
            "profile_fingerprint":journal.binding.profile_fingerprint,
            "path_fingerprint":journal.binding.path_fingerprint,
        },
        "actions":["labCapabilities","labOperationStatus"],
        "mutation_variants":[], "api_key_configured":false,
    })
}

fn pending_journal(endpoint: String) -> OperationJournal {
    OperationJournal {
        id: Uuid::new_v4(),
        group_id: None,
        approval_digest: format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        binding: CollectionBinding {
            endpoint,
            profile_fingerprint: "e".repeat(64),
            path_fingerprint: "f".repeat(64),
            bridge_id: Uuid::new_v4(),
            lineage_id: Uuid::new_v4(),
            session_epoch: Uuid::new_v4(),
            capability_digest: String::new(),
        },
        snapshot_id: Uuid::new_v4(),
        backup_id: Uuid::new_v4(),
        state: OperationState::Preflight,
        steps: vec![JournalStep {
            id: Uuid::new_v4(),
            action: "create_note".into(),
            payload_digest: "a".repeat(64),
            precondition_digest: "c".repeat(64),
            expected_post_digest: "d".repeat(64),
            state: StepState::RequestStarted,
            observed_digest: None,
        }],
        issues: vec![],
    }
}

fn status(journal: &OperationJournal) -> Value {
    json!({
        "lineage_id":journal.binding.lineage_id,
        "operation_id":journal.steps[0].id,
        "payload_digest":journal.steps[0].payload_digest,
        "approved_digest":journal.approval_digest,
        "session_epoch":journal.binding.session_epoch,
        "variant":"create_note", "state":"unknown", "reason":"worker_crash",
        "event_digest":"9".repeat(64), "needs_recovery":true,
        "dispatch_newly_authorized":false,
    })
}

fn save_pending(root: &std::path::Path, journal: &OperationJournal) {
    let mut store = Store::open(root).unwrap();
    let mut prepared = journal.clone();
    prepared.state = OperationState::Prepared;
    prepared.steps[0].state = StepState::IntentRecorded;
    let initial = store.append_journal(&prepared, None).unwrap();
    store.append_journal(journal, Some(&initial)).unwrap();
}

#[test]
fn doctor_bridge_inspects_declaration_without_enabling_writes() {
    let journal = pending_journal(String::new());
    let root = std::env::temp_dir().join(format!("lab-bridge-doctor-{}", Uuid::new_v4()));
    let server = Server::new(vec![json!("Fixture"), manifest(&journal), json!("Fixture")]);
    let out = cli()
        .arg("--set")
        .arg(format!("anki.endpoint={}", server.endpoint))
        .arg("--set")
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["doctor", "--bridge", "--offline"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["probe"], "native_bridge");
    assert_eq!(
        value["native_bridge"]["declaration"]["protocol"],
        "lab-native-v1"
    );
    assert_eq!(value["native_bridge"]["compatibility_verified"], false);
    assert_eq!(
        value["native_bridge"]["collection_identity_verified"],
        false
    );
    assert_eq!(value["native_bridge"]["collection_writes_enabled"], false);
    assert_eq!(value["release_gates"], "not_run");
    assert!(!root.exists());
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .map(|request| request["action"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["getActiveProfile", "labCapabilities", "getActiveProfile"]
    );
    let conflict = cli()
        .args(["doctor", "--bridge", "--ollama"])
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(2));
}

#[test]
fn live_recovery_reads_step_identity_but_does_not_verify_native_effect() {
    let root = std::env::temp_dir().join(format!("lab-native-status-cli-{}", Uuid::new_v4()));
    let mut journal = pending_journal(String::new());
    let mut declaration = manifest(&journal);
    let server = Server::new(vec![
        json!("Fixture"),
        declaration.clone(),
        json!("Fixture"),
        json!("Fixture"),
        status(&journal),
        json!("Fixture"),
    ]);
    journal.binding.endpoint = server.endpoint.clone();
    declaration = manifest(&journal);
    journal.binding.capability_digest = inspect_native_manifest(declaration)
        .unwrap()
        .manifest_digest;
    save_pending(&root, &journal);
    let out = cli()
        .arg("--set")
        .arg(format!("storage.state_dir={}", root.display()))
        .arg("--set")
        .arg(format!("anki.endpoint={}", server.endpoint))
        .args(["recover", "inspect"])
        .arg(journal.id.to_string())
        .arg("--live")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["native_status_checked"], true);
    assert_eq!(value["live_checked"], false);
    assert_eq!(value["reconciliation_available"], false);
    assert_eq!(value["live"]["journals"][0]["binding_conflicts"], json!([]));
    assert_eq!(
        value["live"]["journals"][0]["steps"][0]["intent_conflicts"],
        json!([])
    );
    assert_eq!(
        value["live"]["journals"][0]["steps"][0]["native_status"]["state"],
        "unknown"
    );
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .map(|value| value["action"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "getActiveProfile",
            "labCapabilities",
            "getActiveProfile",
            "getActiveProfile",
            "labOperationStatus",
            "getActiveProfile"
        ]
    );
    assert_eq!(
        requests[4]["params"]["operation_id"],
        journal.steps[0].id.to_string()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn foreign_bridge_is_reported_without_requesting_operation_status() {
    let root = std::env::temp_dir().join(format!("lab-native-foreign-cli-{}", Uuid::new_v4()));
    let mut journal = pending_journal(String::new());
    let mut declaration = manifest(&journal);
    declaration["bridge_id"] = json!(Uuid::new_v4());
    let server = Server::new(vec![json!("Fixture"), declaration, json!("Fixture")]);
    journal.binding.endpoint = server.endpoint.clone();
    journal.binding.capability_digest = "0".repeat(64);
    save_pending(&root, &journal);
    let out = cli()
        .arg("--set")
        .arg(format!("storage.state_dir={}", root.display()))
        .arg("--set")
        .arg(format!("anki.endpoint={}", server.endpoint))
        .args(["recover", "inspect"])
        .arg(journal.id.to_string())
        .arg("--live")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["journals"][0]["journal"]["id"],
        journal.id.to_string()
    );
    assert_eq!(
        value["live"]["journals"][0]["binding_conflicts"][0],
        "bridge_changed"
    );
    assert_eq!(
        value["live"]["journals"][0]["steps"][0]["read_error"],
        "ANKI_NATIVE_BRIDGE_CONFLICT"
    );
    assert_eq!(value["native_status_checked"], false);
    assert_eq!(server.finish().len(), 3);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_status_with_changed_payload_is_not_treated_as_matching_intent() {
    let root = std::env::temp_dir().join(format!("lab-native-drift-cli-{}", Uuid::new_v4()));
    let mut journal = pending_journal(String::new());
    let mut changed = status(&journal);
    changed["payload_digest"] = json!("1".repeat(64));
    let declaration = manifest(&journal);
    let server = Server::new(vec![
        json!("Fixture"),
        declaration.clone(),
        json!("Fixture"),
        json!("Fixture"),
        changed,
        json!("Fixture"),
    ]);
    journal.binding.endpoint = server.endpoint.clone();
    journal.binding.capability_digest = inspect_native_manifest(declaration)
        .unwrap()
        .manifest_digest;
    save_pending(&root, &journal);
    let out = cli()
        .arg("--set")
        .arg(format!("storage.state_dir={}", root.display()))
        .arg("--set")
        .arg(format!("anki.endpoint={}", server.endpoint))
        .args(["recover", "inspect"])
        .arg(journal.id.to_string())
        .arg("--live")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["live"]["journals"][0]["steps"][0]["intent_conflicts"],
        json!(["payload_changed"])
    );
    assert_eq!(value["reconciliation_available"], false);
    assert_eq!(server.finish().len(), 6);
    std::fs::remove_dir_all(root).unwrap();
}
