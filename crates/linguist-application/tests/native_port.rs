//! `NativePort` protocol mapping over a scripted `lab-native-v1` endpoint.
//! The real companion and Anki are exercised by the disposable desktop
//! scenarios; this covers the fail-closed gate and status mapping.
use linguist_application::{
    apply::{ApplyPort, Effect, MutationRequest, NativeStatus, OwnerToken},
    backup::{CheckpointExporter, ExportRequest, PortFailure},
    native_port::{NativeDeadlines, NativePort},
};
use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

const KEY: &str = "native-port-test-key";
const AC_DIGEST: &str = "629566e8eea59f3d67abf1b2339d5c0c621b2d894139e8335db030d022582873";

type Handler = Box<dyn FnMut(&str, &Value) -> Value + Send>;

struct Companion {
    endpoint: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl Companion {
    fn start(mut handler: Handler) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("content-length:") {
                        length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let action = request["action"].as_str().unwrap().to_owned();
                seen.lock().unwrap().push(action.clone());
                let reply = if request["key"] != KEY {
                    json!({"result": null, "error": "valid api key must be provided"})
                } else {
                    match action.as_str() {
                        "getActiveProfile" => json!({"result": "Disposable", "error": null}),
                        _ => handler(&action, &request["params"]),
                    }
                };
                let text = reply.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        Self { endpoint, log }
    }
    fn actions(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn ok(value: Value) -> Value {
    json!({"result": value, "error": null})
}

fn sha(text: &str) -> String {
    linguist_core::canonical::asset_digest(text.as_bytes())
}

fn manifest(anki_version: &str, variants: bool) -> Value {
    json!({
        "protocol": "lab-native-v1", "companion_version": "0.1.0",
        "bridge_id": "c17625b0-7a88-4aab-a8a5-c1d993c72a00",
        "integration": {"anki_version": anki_version, "anki_connect_source_digest": AC_DIGEST},
        "collection_session": {
            "lineage_id": "44cb6177-4c34-483f-9149-5e67512ce303",
            "session_epoch": "6e5cfa2f-2224-4710-b8d2-5741f763c8f7",
            "profile_fingerprint": sha("lab-profile-v1\0Disposable"),
            "path_fingerprint": "b".repeat(64),
        },
        "actions": ["labCapabilities", "labBegin", "labInspect", "labMutate",
                    "labOperationStatus", "labRebind", "labEnd"],
        "mutation_variants": if variants { json!(["install_model", "export_checkpoint",
            "store_media", "create_note", "update_note", "restore_note",
            "delete_unstudied_created_note"]) } else { json!([]) },
        "api_key_configured": variants,
    })
}

fn settings(endpoint: &str, key_env: Option<&str>) -> linguist_config::Effective {
    let mut options = ResolveOptions::default();
    options
        .flags
        .insert("anki.endpoint".into(), json!(endpoint));
    if let Some(name) = key_env {
        options.flags.insert("anki.api_key_env".into(), json!(name));
    }
    resolve(&Registry::builtin(), &ConfigFile::default(), &options).unwrap()
}

fn client(endpoint: &str) -> linguist_anki::Client {
    let environment = BTreeMap::from([("LAB_TEST_KEY".to_owned(), KEY.to_owned())]);
    linguist_anki::Client::from_settings(&settings(endpoint, Some("LAB_TEST_KEY")), &environment)
        .unwrap()
}

fn deadlines(operation: u64) -> NativeDeadlines {
    NativeDeadlines {
        operation: Duration::from_millis(operation),
        export: Duration::from_millis(operation),
        poll: Duration::from_millis(10),
    }
}

fn status(operation: &str, state: &str, reason: Value, receipt: Value, variant: &str) -> Value {
    json!({
        "lineage_id": "44cb6177-4c34-483f-9149-5e67512ce303", "operation_id": operation,
        "payload_digest": "a".repeat(64),
        "approved_digest": format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        "session_epoch": "6e5cfa2f-2224-4710-b8d2-5741f763c8f7", "variant": variant,
        "state": state, "reason": reason, "receipt": receipt, "event_digest": "c".repeat(64),
        "needs_recovery": matches!(state, "running" | "unknown"),
        "dispatch_newly_authorized": false,
    })
}

#[test]
fn unverified_builds_and_missing_credentials_keep_writes_unavailable() {
    let companion = Companion::start(Box::new(|action, _| match action {
        "labCapabilities" => ok(manifest("25.10.0", true)),
        other => panic!("unexpected {other}"),
    }));
    let state = std::env::temp_dir();
    let error = NativePort::connect(
        &client(&companion.endpoint),
        &state,
        deadlines(100),
        1 << 20,
    )
    .err()
    .unwrap();
    assert!(error.starts_with("CAPABILITY_UNAVAILABLE"), "{error}");
    assert!(error.contains("not a verified"), "{error}");
    // Without an API key nothing is requested at all.
    let keyless = linguist_anki::Client::from_settings(
        &settings(&companion.endpoint, None),
        &BTreeMap::new(),
    )
    .unwrap();
    let before = companion.actions().len();
    let error = NativePort::connect(&keyless, &state, deadlines(100), 1 << 20)
        .err()
        .unwrap();
    assert!(error.contains("anki.api_key_env"), "{error}");
    assert_eq!(companion.actions().len(), before);
    // A verified build without variants (no session/key on the companion) also refuses.
    let companion = Companion::start(Box::new(|action, _| match action {
        "labCapabilities" => ok(manifest("25.09.2", false)),
        other => panic!("unexpected {other}"),
    }));
    let error = NativePort::connect(
        &client(&companion.endpoint),
        &state,
        deadlines(100),
        1 << 20,
    )
    .err()
    .unwrap();
    assert!(error.contains("no mutation variants"), "{error}");
}

fn delete_request(operation: Uuid, owner: &OwnerToken, port: &NativePort) -> MutationRequest {
    let effect = Effect::DeleteUnstudiedCreatedNote {
        note_id: 7,
        expected_pre_digest: format!("lab-jcs-v1:lab-apply-precondition-v1:{}", "d".repeat(64)),
    };
    MutationRequest {
        operation_id: operation,
        parent_operation_id: Uuid::new_v4(),
        approval_digest: format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        binding: port.binding(),
        owner: owner.clone(),
        payload_digest: effect.payload_digest().unwrap(),
        effect,
    }
}

#[test]
fn mutations_poll_to_terminal_and_map_timeouts_and_refusals() {
    let polls = Arc::new(Mutex::new(0));
    let counter = polls.clone();
    let submitted = Arc::new(Mutex::new(Vec::<Value>::new()));
    let record = submitted.clone();
    let lookup = submitted.clone();
    let companion = Companion::start(Box::new(move |action, params| match action {
        "labCapabilities" => ok(manifest("25.09.2", true)),
        "labBegin" => ok(
            json!({"owner_token": "11111111-1111-4111-8111-111111111111",
                                "fence": 3, "staging_dir": "/nonexistent"}),
        ),
        "labEnd" => ok(json!(true)),
        "labMutate" => {
            record.lock().unwrap().push(params.clone());
            let operation = params["operation_id"].as_str().unwrap();
            let mut reply = status(
                operation,
                "queued",
                Value::Null,
                Value::Null,
                "delete_unstudied_created_note",
            );
            reply["payload_digest"] = json!(linguist_core::canonical::asset_digest(
                params["payload"].as_str().unwrap().as_bytes()
            ));
            if operation.starts_with("00000000") {
                return json!({"result": null, "error": "BRIDGE_OPERATION_PENDING"});
            }
            ok(reply)
        }
        "labOperationStatus" => {
            let operation = params["operation_id"].as_str().unwrap();
            let mut count = counter.lock().unwrap();
            *count += 1;
            let state = if operation.starts_with("ffffffff") || *count < 3 {
                "running"
            } else {
                "verified"
            };
            let receipt = if state == "verified" {
                json!({"note_id": 7, "removed": true})
            } else {
                Value::Null
            };
            let mut reply = status(
                operation,
                state,
                Value::Null,
                receipt,
                "delete_unstudied_created_note",
            );
            let payloads = lookup.lock().unwrap();
            let sent = payloads
                .iter()
                .find(|p| p["operation_id"] == operation)
                .unwrap();
            reply["payload_digest"] = json!(linguist_core::canonical::asset_digest(
                sent["payload"].as_str().unwrap().as_bytes()
            ));
            ok(reply)
        }
        other => panic!("unexpected {other}"),
    }));
    let client = client(&companion.endpoint);
    let mut port =
        NativePort::connect(&client, &std::env::temp_dir(), deadlines(400), 1 << 20).unwrap();
    let binding = port.execution_binding().unwrap();
    let owner = port
        .begin(&binding, &format!("lab-jcs-v1:plan:{}", "b".repeat(64)))
        .unwrap();
    assert_eq!(owner.fence, 3);
    let operation = Uuid::new_v4();
    let request = delete_request(operation, &owner, &port);
    assert_eq!(port.mutate(&request).unwrap(), NativeStatus::Verified);
    // The exact canonical wire envelope is what the companion hashes.
    let sent: Value =
        serde_json::from_str(submitted.lock().unwrap()[0]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(sent["variant"], "delete_unstudied_created_note");
    assert_eq!(sent["body"]["note_id"], 7);
    // A row that never leaves `running` before the deadline is unknown.
    let stuck = Uuid::parse_str("ffffffff-0000-4000-8000-000000000000").unwrap();
    match port.mutate(&delete_request(stuck, &owner, &port)) {
        Err(PortFailure::Unknown(reason)) => assert!(reason.contains("ANKI_NATIVE_TIMEOUT")),
        other => panic!("{other:?}"),
    }
    // A companion refusal of the request itself queued nothing.
    let refused = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
    match port.mutate(&delete_request(refused, &owner, &port)) {
        Err(PortFailure::Rejected(reason)) => assert!(reason.contains("BRIDGE_OPERATION_PENDING")),
        other => panic!("{other:?}"),
    }
    // A stale owner never reaches the companion.
    let stale = OwnerToken {
        token: Uuid::new_v4(),
        fence: 2,
    };
    let before = companion.actions().len();
    assert!(matches!(
        port.mutate(&delete_request(Uuid::new_v4(), &stale, &port)),
        Err(PortFailure::Rejected(_))
    ));
    assert_eq!(companion.actions().len(), before);
}

#[test]
fn export_copies_only_a_package_matching_the_receipt() {
    let directory = std::env::temp_dir().join(format!("lab-native-export-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let exported = directory.join("companion.colpkg");
    std::fs::write(&exported, b"package bytes").unwrap();
    let lie = Arc::new(Mutex::new(false));
    let lying = lie.clone();
    let path = exported.display().to_string();
    let companion = Companion::start(Box::new(move |action, params| match action {
        "labCapabilities" => ok(manifest("25.09.2", true)),
        "labBegin" => {
            assert!(
                params["approved_digest"]
                    .as_str()
                    .unwrap()
                    .starts_with("lab-jcs-v1:checkpoint:")
            );
            ok(
                json!({"owner_token": "11111111-1111-4111-8111-111111111111",
                      "fence": 4, "staging_dir": "/nonexistent"}),
            )
        }
        "labEnd" => ok(json!(true)),
        "labMutate" => {
            let operation = params["operation_id"].as_str().unwrap();
            let size = if *lying.lock().unwrap() { 999 } else { 13 };
            let mut reply = status(
                operation,
                "verified",
                Value::Null,
                json!({"path": path, "size_bytes": size,
                       "sha256": linguist_core::canonical::asset_digest(b"package bytes")}),
                "export_checkpoint",
            );
            reply["approved_digest"] = params["approved_digest"].clone();
            ok(reply)
        }
        other => panic!("unexpected {other}"),
    }));
    let client = client(&companion.endpoint);
    let mut port = NativePort::connect(&client, &directory, deadlines(400), 1 << 20).unwrap();
    let binding = port.binding();
    let request = |destination| ExportRequest {
        operation_id: Uuid::new_v4(),
        binding: binding.clone(),
        destination,
        include_media: true,
        include_scheduling: true,
    };
    *lie.lock().unwrap() = true;
    let first = directory.join("first.colpkg");
    assert!(matches!(
        port.export_checkpoint(&request(first.clone())),
        Err(PortFailure::Rejected(code)) if code == "CHECKPOINT_EXPORT_CLAIM_MISMATCH"
    ));
    *lie.lock().unwrap() = false;
    let second = directory.join("second.colpkg");
    let claim = port.export_checkpoint(&request(second.clone())).unwrap();
    assert_eq!(claim.path, second);
    assert_eq!(std::fs::read(&second).unwrap(), b"package bytes");
    assert!(
        !exported.exists(),
        "companion copy removed after the verified copy"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}
