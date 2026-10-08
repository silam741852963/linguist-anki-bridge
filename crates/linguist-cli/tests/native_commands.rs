//! RI-03 `plans bind` and the native write gate at the command level, over a
//! scripted `lab-native-v1` endpoint (the real companion runs in the
//! disposable desktop scenarios).
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

const KEY: &str = "native-command-test-key";
const AC_DIGEST: &str = "629566e8eea59f3d67abf1b2339d5c0c621b2d894139e8335db030d022582873";

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fake_companion(anki_version: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
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
            let result = match action.as_str() {
                _ if request["key"] != KEY => Value::Null,
                "getActiveProfile" => json!("Disposable"),
                "labCapabilities" => json!({
                    "protocol": "lab-native-v1", "companion_version": "0.3.0",
                    "bridge_id": "c17625b0-7a88-4aab-a8a5-c1d993c72a00",
                    "integration": {"anki_version": anki_version,
                                    "anki_connect_source_digest": AC_DIGEST},
                    "collection_session": {
                        "lineage_id": "44cb6177-4c34-483f-9149-5e67512ce303",
                        "session_epoch": "6e5cfa2f-2224-4710-b8d2-5741f763c8f7",
                        "profile_fingerprint": linguist_core::canonical::asset_digest(
                            "lab-profile-v1\0Disposable".as_bytes()),
                        "path_fingerprint": "b".repeat(64),
                    },
                    "actions": ["labCapabilities", "labBegin", "labInspect", "labMutate",
                                "labOperationStatus", "labRebind", "labEnd"],
                    "mutation_variants": ["install_model", "export_checkpoint", "store_media",
                        "create_note", "update_note", "restore_note",
                        "delete_unstudied_created_note"],
                    "api_key_configured": true,
                }),
                other => panic!("unexpected {other}"),
            };
            let text = json!({"result": result, "error": if result.is_null() {
                json!("valid api key must be provided") } else { Value::Null }})
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (endpoint, log)
}

fn cli(f: &Fixture, endpoint: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.env_clear()
        .env("HOME", f.0.join("home"))
        .env("NATIVE_TEST_KEY", KEY)
        .args(["--output", "json"])
        .args([
            "--set",
            &format!("storage.state_dir={}", f.0.join("state").display()),
        ])
        .args(["--set", &format!("anki.endpoint={endpoint}")])
        .args(["--set", "anki.api_key_env=NATIVE_TEST_KEY"]);
    c
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn plans_bind_records_the_verified_binding_in_a_new_revision() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-native-bind-{}", Uuid::new_v4())));
    std::fs::create_dir_all(f.0.join("home")).unwrap();
    let (endpoint, log) = fake_companion("25.09.2");
    let added = cli(&f, &endpoint)
        .args([
            "--offline",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "llm.enabled=false",
        ])
        .args([
            "--set",
            "images.search_when_missing=false",
            "--set",
            "kanji.enabled=false",
        ])
        .args([
            "--purpose",
            "japanese_vocab",
            "vocab",
            "add",
            "--expression",
            "食べる",
        ])
        .args([
            "--meaning",
            "to eat",
            "--sense-key",
            "eat",
            "--target-language",
            "ja",
        ])
        .args(["--reading", "たべる"])
        .output()
        .unwrap();
    assert!(added.status.success(), "{added:?}");
    let added = json_of(&added.stdout);
    let plan = added["plan_id"].as_str().unwrap().to_owned();
    let digest = added["digest"].as_str().unwrap().to_owned();
    assert!(
        log.lock().unwrap().is_empty(),
        "offline preparation contacts nothing"
    );
    // A stale digest is refused before any request.
    let stale = cli(&f, &endpoint)
        .args([
            "plans",
            "bind",
            &plan,
            "--digest",
            &format!("lab-jcs-v1:plan:{}", "0".repeat(64)),
        ])
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(5), "{stale:?}");
    assert!(log.lock().unwrap().is_empty());
    let bound = cli(&f, &endpoint)
        .args(["plans", "bind", &plan, "--digest", &digest])
        .output()
        .unwrap();
    assert!(bound.status.success(), "{bound:?}");
    let bound = json_of(&bound.stdout);
    assert_eq!(bound["revision"], 2);
    assert_eq!(bound["noop"], false);
    assert_eq!(
        bound["binding"]["lineage_id"],
        "44cb6177-4c34-483f-9149-5e67512ce303"
    );
    assert_eq!(bound["binding"]["endpoint"], endpoint);
    let digest2 = bound["digest"].as_str().unwrap().to_owned();
    // Binding again is a no-op for the same collection.
    let again = cli(&f, &endpoint)
        .args(["plans", "bind", &plan, "--digest", &digest2])
        .output()
        .unwrap();
    assert_eq!(json_of(&again.stdout)["noop"], true);
    // The bound revision no longer reports a weak binding.
    let preview = cli(&f, &endpoint)
        .args(["apply", &plan, "--revision", "2"])
        .output()
        .unwrap();
    let preview = json_of(&preview.stdout);
    assert!(
        !preview.to_string().contains("APPLY_BINDING_WEAK"),
        "{preview}"
    );
    let store = linguist_store::Store::read_only(&f.0.join("state")).unwrap();
    let revision = store.revision(Uuid::parse_str(&plan).unwrap(), 2).unwrap();
    assert_eq!(revision.parent_digest.as_deref(), Some(digest.as_str()));
}

#[test]
fn an_unverified_companion_cannot_bind_or_write() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-native-gate-{}", Uuid::new_v4())));
    std::fs::create_dir_all(f.0.join("home")).unwrap();
    let (endpoint, log) = fake_companion("25.10.0");
    let plan = Uuid::new_v4().to_string();
    for args in [
        vec!["plans", "bind", plan.as_str(), "--digest", "x"],
        vec!["backup", "create", "--scope", "collection", "--apply"],
        vec!["models", "install", "japanese_vocab", "--apply"],
    ] {
        let out = cli(&f, &endpoint).args(&args).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_ne!(out.status.code(), Some(0), "{args:?}: {stderr}");
        if args[0] != "plans" {
            assert_eq!(out.status.code(), Some(3), "{args:?}: {stderr}");
            assert!(
                stderr.contains("not a verified lab-native-v1 build"),
                "{stderr}"
            );
        }
    }
    let actions = log.lock().unwrap().clone();
    assert!(
        actions
            .iter()
            .all(|a| matches!(a.as_str(), "getActiveProfile" | "labCapabilities")),
        "{actions:?}"
    );
    assert!(!f.0.join("state").join("leases").exists());
}
