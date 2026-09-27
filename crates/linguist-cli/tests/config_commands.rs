use std::process::Command;
fn cli() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.env_clear();
    c.env("HOME", "/tmp/lab-command-tests-no-config");
    c
}
#[test]
fn help_and_defaults_do_not_read_bad_configuration_or_create_state() {
    let out = cli()
        .args(["--config", "/does/not/exist", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = cli()
        .args([
            "--config",
            "/does/not/exist",
            "config",
            "show",
            "--defaults",
            "llm.model",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["values"]["llm.model"], "gemma4:12b");
}
#[test]
fn structured_errors_stay_on_stderr_and_config_errors_exit_two() {
    let out = cli()
        .args(["--set", "jobs.heartbeat_seconds=30", "config", "validate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("CROSS_FIELD_CONSTRAINT")
    );
}
#[test]
fn effective_purpose_and_typed_flags_are_visible_with_provenance() {
    let out = cli()
        .args([
            "--purpose",
            "japanese_grammar",
            "--set",
            "learning.explanation_language=en",
            "config",
            "show",
            "learning",
            "--provenance",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["values"]["learning.explanation_language"], "en");
    assert_eq!(value["provenance"]["learning.explanation_language"], "flag");
}
#[test]
fn unknown_settings_and_duplicate_flags_cannot_be_ignored() {
    for args in [
        vec!["--set", "made.up=secret", "config", "validate"],
        vec![
            "--set",
            "llm.model=a",
            "--set",
            "llm.model=b",
            "config",
            "validate",
        ],
    ] {
        let out = cli().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("secret"));
    }
}
#[test]
fn document_tools_consume_registered_input_limits() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/v2/fixtures/vocabulary.json");
    let out = cli()
        .args(["--set", "input.max_record_chars=1", "document", "validate"])
        .arg(fixture)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("INPUT_RECORD_TOO_LARGE"));
}
#[test]
fn missing_state_plan_listing_is_read_only() {
    let root = std::env::temp_dir().join(format!("lab-list-test-{}", uuid::Uuid::new_v4()));
    let out = cli()
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["plans", "list"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["state_exists"], false);
    assert!(!root.exists());
}
#[test]
fn config_edit_commands_backup_and_reset_requires_execute() {
    let root = std::env::temp_dir().join(format!("lab-edit-cli-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    let out = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "set", "llm.temperature", "0.6"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["executed"], true);
    assert!(std::path::Path::new(value["backup"].as_str().unwrap()).exists());
    let before = std::fs::read(&path).unwrap();
    let out = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "reset", "llm.temperature"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let out = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "reset", "llm.temperature", "--execute"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_ne!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn recovery_inspection_never_initializes_state_and_live_read_is_explicitly_unavailable() {
    let root =
        std::env::temp_dir().join(format!("lab-recovery-read-test-{}", uuid::Uuid::new_v4()));
    let out = cli()
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["recover", "inspect", "--pending"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["state_exists"], false);
    assert!(!root.exists());
    let out = cli()
        .args(["recover", "inspect", "--pending", "--live"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
}
#[test]
fn note_selector_conflicts_and_invalid_ids_fail_before_anki_requests() {
    for args in [
        vec!["notes", "list", "--deck", "A", "--query", "x"],
        vec!["notes", "list"],
        vec!["notes", "show", "0123"],
        vec!["notes", "list", "--query", "x", "--limit", "0"],
        vec!["--purpose", "english_vocab", "vocab", "revamp"],
        vec![
            "--purpose",
            "english_vocab",
            "vocab",
            "revamp",
            "--note-id",
            "123",
            "--query",
            "x",
        ],
        vec![
            "--purpose",
            "japanese_grammar",
            "grammar",
            "revamp",
            "--deck",
            "A",
            "--query",
            "x",
        ],
    ] {
        let out = cli().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{:?}", out);
        assert!(out.stdout.is_empty());
    }
}
#[test]
fn local_doctor_and_builtin_models_need_no_anki_service() {
    for args in [vec!["doctor", "--local"], vec!["models", "builtin"]] {
        let out = cli().args(args).output().unwrap();
        assert!(out.status.success(), "{:?}", out);
        assert!(out.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap();
    }
}

#[test]
fn plan_diff_loads_exact_immutable_revisions_and_rejects_live_checks() {
    use linguist_core::records::{PlanRevision, ResolvedSettings};
    use std::collections::BTreeMap;
    let root = std::env::temp_dir().join(format!("lab-diff-cli-{}", uuid::Uuid::new_v4()));
    let mut store = linguist_store::Store::open(&root).unwrap();
    let mut plan = PlanRevision {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "first".into(),
        documents: vec![],
        rendered: vec![],
        review_decisions: vec![],
    };
    let parent = store.publish_revision(&plan).unwrap();
    plan.revision = 2;
    plan.parent_digest = Some(parent.clone());
    plan.source_digest = "second".into();
    store.publish_revision(&plan).unwrap();
    drop(store);
    let setting = format!("storage.state_dir={}", root.display());
    let id = plan.id.to_string();
    let out = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "diff",
            &id,
            "--from-revision",
            "1",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["from_revision"], 1);
    assert_eq!(value["to_revision"], 2);
    assert_eq!(value["from_digest"], parent);
    assert_eq!(value["to_digest"], plan.approval_digest().unwrap());
    assert!(
        value["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "/source_digest"
                && v["before"] == "first"
                && v["after"] == "second")
    );
    let out = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "diff",
            &id,
            "--from-revision",
            "1",
            "--live",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(store.list_revisions(10).unwrap().len(), 2);
    assert_eq!(store.revision(plan.id, 2).unwrap(), plan);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn plan_edit_cli_creates_child_and_rejects_stale_base() {
    use linguist_core::{LearningDocument, records::*};
    use std::collections::BTreeMap;
    let root = std::env::temp_dir().join(format!("lab-edit-cli-{}", uuid::Uuid::new_v4()));
    let doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    let plan = PlanRevision {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "fixture".into(),
        rendered: vec![linguist_core::render::render(&doc, &BTreeMap::new()).unwrap()],
        documents: vec![doc],
        review_decisions: vec![],
    };
    let mut store = linguist_store::Store::open(&root).unwrap();
    let digest = store.publish_revision(&plan).unwrap();
    drop(store);
    let patch = root.join("patch.json");
    std::fs::write(&patch, serde_json::to_vec(&serde_json::json!({"schema_version":2,"base_digest":digest,"items":[{"document_id":plan.documents[0].id,"personal_notes":{"intent":"set","value":"authored association"}}]})).unwrap()).unwrap();
    let setting = format!("storage.state_dir={}", root.display());
    let id = plan.id.to_string();
    let args = [
        "--set",
        &setting,
        "plans",
        "edit",
        &id,
        "--base-revision",
        "1",
        "--patch",
        patch.to_str().unwrap(),
    ];
    let out = cli().args(args).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["revision"], 2);
    assert_eq!(value["ready"], true);
    let out = cli().args(args).output().unwrap();
    assert_eq!(out.status.code(), Some(5));
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(store.revision(plan.id, 1).unwrap(), plan);
    assert_eq!(
        store.revision(plan.id, 2).unwrap().documents[0].personal_notes,
        "authored association"
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authored_add_cli_creates_plan_without_anki_and_blocks_requested_generation() {
    let root = std::env::temp_dir().join(format!("lab-add-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let input = root.join("input.json");
    std::fs::write(&input,r#"{"schema_version":2,"kind":"vocabulary","target_language":"en","body":{"expression":"eat","meaning":"consume food","sense_key":"food"}}"#).unwrap();
    let setting = format!("storage.state_dir={}/state", root.display());
    let out = cli()
        .args([
            "--set",
            &setting,
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "vocab",
            "add",
            "--document",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["ready"], true);
    assert_eq!(value["apply_eligible"], false);
    assert_eq!(value["duplicate_check_performed"], false);
    let store = linguist_store::Store::read_only(&root.join("state")).unwrap();
    let id = uuid::Uuid::parse_str(value["plan_id"].as_str().unwrap()).unwrap();
    assert_eq!(store.revision(id, 1).unwrap().rendered.len(), 1);
    let original = store.revision(id, 1).unwrap();
    let id_text = id.to_string();
    let validation = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "validate",
            &id_text,
            "--revision",
            "1",
        ])
        .output()
        .unwrap();
    assert!(validation.status.success(), "{validation:?}");
    let validation: serde_json::Value = serde_json::from_slice(&validation.stdout).unwrap();
    assert_eq!(validation["evidence"]["content_ready"], true);
    assert_eq!(validation["evidence"]["apply_eligible"], false);
    assert_eq!(validation["evidence"]["plan_digest"], value["digest"]);
    let evidence_id =
        uuid::Uuid::parse_str(validation["evidence"]["id"].as_str().unwrap()).unwrap();
    assert!(
        store
            .validation_evidence(evidence_id)
            .unwrap()
            .evidence
            .content_ready
    );
    assert_eq!(store.revision(id, 1).unwrap(), original);
    let digest = value["digest"].as_str().unwrap();
    let approved = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "approve",
            &id_text,
            "--revision",
            "1",
            "--digest",
            digest,
            "--actor",
            "fixture reviewer",
        ])
        .output()
        .unwrap();
    assert!(approved.status.success(), "{approved:?}");
    let approved: serde_json::Value = serde_json::from_slice(&approved.stdout).unwrap();
    assert_eq!(approved["receipt"]["apply_authorized"], false);
    assert_eq!(approved["receipt"]["approval"]["digest"], digest);
    let approval_id = uuid::Uuid::parse_str(approved["receipt"]["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        store.approval(approval_id).unwrap().approval.actor,
        "fixture reviewer"
    );
    let conflict = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "approve",
            &id_text,
            "--revision",
            "1",
            "--digest",
            "wrong",
            "--actor",
            "fixture reviewer",
        ])
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(5));
    let output = root.join("bundle.json");
    let exported = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "export",
            &id_text,
            "--output",
            output.to_str().unwrap(),
            "--include-private-archives",
        ])
        .output()
        .unwrap();
    assert!(exported.status.success(), "{exported:?}");
    let exported: serde_json::Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(
        exported["checksum"],
        linguist_core::canonical::asset_digest(&std::fs::read(&output).unwrap())
    );
    let conflict = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "export",
            &id_text,
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(5));
    let v1 = cli()
        .args([
            "--set",
            &setting,
            "plans",
            "export",
            &id_text,
            "--output",
            output.to_str().unwrap(),
            "--format",
            "v1",
        ])
        .output()
        .unwrap();
    assert_eq!(v1.status.code(), Some(2));
    let live = cli()
        .args(["--set", &setting, "plans", "validate", &id_text, "--live"])
        .output()
        .unwrap();
    assert_eq!(live.status.code(), Some(3));
    let out = cli()
        .args([
            "--set",
            &setting,
            "vocab",
            "add",
            "--document",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(store.list_revisions(10).unwrap().len(), 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn completions_are_generated_without_config_state_or_services() {
    let root = std::env::temp_dir().join(format!("lab-completions-{}", uuid::Uuid::new_v4()));
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let out = cli()
            .env("HOME", &root)
            .env("LAB_CONFIG", root.join("missing.toml"))
            .env("LAB_ANKI_ENDPOINT", "this is deliberately invalid")
            .args([
                "--config",
                "/does/not/exist",
                "--set",
                "made.up=private-sentinel",
                "completions",
                shell,
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{shell}: {out:?}");
        assert!(out.stderr.is_empty());
        let script = String::from_utf8(out.stdout.clone()).unwrap();
        assert!(script.contains("linguist-anki-bridge"));
        assert!(script.contains("vocab"));
        assert!(script.contains("completions"));
        assert!(!script.contains("private-sentinel"));
        assert!(!root.exists());
        if shell == "bash" {
            use std::io::Write;
            let mut checker = Command::new("bash")
                .arg("-n")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            checker
                .stdin
                .take()
                .unwrap()
                .write_all(&out.stdout)
                .unwrap();
            assert!(checker.wait().unwrap().success());
        }
    }
    let out = cli()
        .args([
            "--config",
            "/does/not/exist",
            "completions",
            "unsupported-shell",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!root.exists());
}

#[test]
fn notes_show_media_reports_hashes_and_missing_files_without_writes() {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let raw = "<img src='猫%20picture.png'>[sound:voice.mp3]<img src='https://example.invalid/x'>";
    let server = std::thread::spawn(move || {
        let mut actions = Vec::new();
        for _ in 0..11 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
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
                    length = line
                        .split_once(':')
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let action = request["action"].as_str().unwrap();
            let result = match action {
                "getActiveProfile" => serde_json::json!("fixture"),
                "notesInfo" => {
                    serde_json::json!([{"noteId":123,"cards":[],"fields":{"Original":{"value":raw,"order":0}},"modelName":"Basic","tags":[]}])
                }
                "retrieveMediaFile" => {
                    if request["params"]["filename"] == "voice.mp3" {
                        serde_json::json!(false)
                    } else {
                        assert_eq!(request["params"]["filename"], "猫 picture.png");
                        serde_json::json!("b3JpZw==")
                    }
                }
                _ => panic!("unexpected action: {action}"),
            };
            actions.push(action.to_owned());
            let body = serde_json::json!({"result":result,"error":null}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        actions
    });
    let root = std::env::temp_dir().join(format!("lab-show-media-{}", uuid::Uuid::new_v4()));
    let out = cli()
        .args(["--set"])
        .arg(format!("anki.endpoint={endpoint}"))
        .args([
            "--purpose",
            "japanese_vocab",
            "--set",
            "purposes.japanese_vocab.source_model=Basic",
            "--set",
            "purposes.japanese_vocab.fields={\"expression\":\"Original\",\"reading\":\"Original\"}",
        ])
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["notes", "show", "123", "--media"])
        .output()
        .unwrap();
    let actions = server.join().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(actions.iter().filter(|a| *a == "notesInfo").count(), 1);
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["note"]["fields"]["Original"]["value"], raw);
    assert_eq!(
        result["source_mapping"]["roles"]["expression"]["raw_value"],
        raw
    );
    assert_eq!(
        result["source_mapping"]["shared_fields"]["Original"],
        serde_json::json!(["expression", "reading"])
    );
    assert_eq!(
        result["source_mapping"]["missing_required_roles"],
        serde_json::json!(["meaning"])
    );
    assert_eq!(result["source_mapping"]["normalized_facts_verified"], false);
    assert_eq!(
        result["parsed_media"]["references"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        result["parsed_media"]["references"][1]["filename"],
        "猫 picture.png"
    );
    assert_eq!(
        result["parsed_media"]["issues"][0]["code"],
        "UNSAFE_OR_REMOTE_MEDIA_REFERENCE"
    );
    assert_eq!(result["media_bytes_checked"], false);
    assert_eq!(result["media_inspection_requested"], true);
    assert_eq!(result["media_content_validated"], false);
    let metadata = result["media_metadata"].as_array().unwrap();
    assert_eq!(metadata.len(), 2);
    assert_eq!(metadata[0]["exists"], false);
    assert_eq!(
        metadata[1]["digest"],
        linguist_core::canonical::asset_digest(b"orig")
    );
    assert_eq!(metadata[1]["size_bytes"], 4);
    assert_eq!(
        actions.iter().filter(|a| *a == "retrieveMediaFile").count(),
        2
    );
    assert!(!root.exists());
}

fn piped(mut command: Command, bytes: &[u8]) -> std::process::Output {
    use std::io::Write;
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    // Large rejected input may close the reader before the producer finishes.
    let _ = child.stdin.take().unwrap().write_all(bytes);
    child.wait_with_output().unwrap()
}
#[test]
fn piped_add_archives_the_exact_single_json_record_without_an_input_file() {
    let root = std::env::temp_dir().join(format!("lab-stdin-add-{}", uuid::Uuid::new_v4()));
    let raw = b"  {\"schema_version\":2,\"kind\":\"vocabulary\",\"target_language\":\"en\",\"body\":{\"expression\":\"eat\",\"meaning\":\"consume food\",\"sense_key\":\"food\"}}\n";
    let mut command = cli();
    command
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args([
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "vocab",
            "add",
            "--document",
            "-",
        ]);
    let out = piped(command, raw);
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        result["original_input_digest"],
        linguist_core::canonical::asset_digest(raw)
    );
    assert_eq!(result["apply_eligible"], false);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let id = uuid::Uuid::parse_str(result["plan_id"].as_str().unwrap()).unwrap();
    let plan = store.revision(id, 1).unwrap();
    assert_eq!(
        plan.documents[0].sources[0].fields["authored_input"].as_bytes(),
        raw
    );
    assert_eq!(
        store
            .asset(result["original_input_digest"].as_str().unwrap(), 10000)
            .unwrap(),
        raw
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn stdin_limits_encoding_and_multiple_records_fail_before_state_creation() {
    for (bytes, setting) in [
        (vec![], "input.max_record_chars=100"),
        (b"{} {}".to_vec(), "input.max_record_chars=100"),
        (vec![255], "input.max_record_chars=100"),
        (b"{}".to_vec(), "input.max_record_chars=1"),
        (vec![b' '; 1024 * 1024 + 1], "input.max_file_mb=1"),
    ] {
        let root =
            std::env::temp_dir().join(format!("lab-stdin-rejected-{}", uuid::Uuid::new_v4()));
        let mut command = cli();
        command
            .args(["--set"])
            .arg(format!("storage.state_dir={}", root.display()))
            .args(["--set", setting, "vocab", "add", "--document", "-"]);
        let out = piped(command, &bytes);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(out.stdout.is_empty());
        assert!(!root.exists());
    }
    let mut command = cli();
    command.args(["document", "digest", "-"]);
    let fixture = include_bytes!("../../../contracts/v2/fixtures/vocabulary.json");
    let out = piped(command, fixture);
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let document = linguist_core::LearningDocument::from_json(fixture).unwrap();
    assert_eq!(result["digest"], document.semantic_digest().unwrap());
}

#[test]
fn piped_grammar_add_preserves_explicit_explanation_language_and_raw_bytes() {
    let root = std::env::temp_dir().join(format!("lab-stdin-grammar-{}", uuid::Uuid::new_v4()));
    let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../contracts/v2/fixtures/grammar.json"
    ))
    .unwrap();
    let raw = serde_json::to_vec_pretty(&serde_json::json!({"schema_version":2,"kind":"grammar","target_language":"ja","explanation_language":"vi","body":fixture["content"]["body"]})).unwrap();
    let mut command = cli();
    command
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args([
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "grammar",
            "add",
            "--document",
            "-",
        ]);
    let out = piped(command, &raw);
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let id = uuid::Uuid::parse_str(result["plan_id"].as_str().unwrap()).unwrap();
    let plan = store.revision(id, 1).unwrap();
    assert_eq!(plan.documents[0].explanation_language.as_str(), "vi");
    assert_eq!(
        store
            .asset(result["original_input_digest"].as_str().unwrap(), 10000)
            .unwrap(),
        raw
    );
    assert_eq!(result["apply_eligible"], false);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ollama_doctor_probes_only_metadata_and_reports_unverified_generation_without_state() {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut paths = Vec::new();
        for expected in [
            "GET /api/tags HTTP/1.1",
            "POST /api/show HTTP/1.1",
            "GET /api/tags HTTP/1.1",
        ] {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert_eq!(first.trim(), expected);
            paths.push(first.trim().to_owned());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    length = line
                        .split_once(':')
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let body = if expected.starts_with("POST") {
                let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(request, serde_json::json!({"model":"fixture:local","verbose":false}));
                serde_json::json!({"capabilities":["completion"],"details":{"format":"gguf","family":"fixture"},"model_info":{"general.architecture":"fixture","fixture.context_length":131072},"license":"private fixture metadata"})
            } else {
                assert!(bytes.is_empty());
                serde_json::json!({"models":[{"name":"fixture:local","digest":"a".repeat(64),"size":100,"details":{"format":"gguf","family":"fixture"}}]})
            }.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        paths
    });
    let root = std::env::temp_dir().join(format!("lab-ollama-doctor-{}", uuid::Uuid::new_v4()));
    let out = cli()
        .arg("--set")
        .arg(format!("llm.endpoint={endpoint}"))
        .args([
            "--set",
            "llm.model=fixture:local",
            "--set",
            "services.ollama.min_interval_seconds=0",
        ])
        .arg("--set")
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["doctor", "--ollama", "--offline"])
        .output()
        .unwrap();
    assert_eq!(server.join().unwrap().len(), 3);
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["metadata_ready"], true);
    assert_eq!(result["ollama"]["identity"]["name"], "fixture:local");
    assert_eq!(result["ollama"]["generation_ready"], false);
    assert_eq!(result["raw_assets_persisted"], false);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("private fixture metadata"));
    assert!(out.stderr.is_empty());
    assert!(!root.exists());
}
#[test]
fn ollama_doctor_conflicting_modes_and_missing_model_fail_before_service_probes() {
    let out = cli()
        .args(["doctor", "--local", "--ollama"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = cli()
        .args(["--set", "llm.model=null", "doctor", "--ollama"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("CAPABILITY_UNAVAILABLE"));
}

#[test]
fn revamp_commands_publish_recoverable_source_drafts_using_only_anki_reads() {
    use std::io::{BufRead, Read, Write};
    for (command, purpose, mapping, order, count, mode) in [
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            1,
            "ids",
        ),
        (
            "grammar",
            "japanese_grammar",
            r#"{"pattern":"Pattern","meaning":"Meaning","formation":"Formation","use_key":"Key"}"#,
            "note_id",
            1,
            "ids",
        ),
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            2,
            "ids",
        ),
        (
            "grammar",
            "japanese_grammar",
            r#"{"pattern":"Pattern","meaning":"Meaning","formation":"Formation","use_key":"Key"}"#,
            "input",
            2,
            "ids",
        ),
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            2,
            "query",
        ),
        (
            "grammar",
            "japanese_grammar",
            r#"{"pattern":"Pattern","meaning":"Meaning","formation":"Formation","use_key":"Key"}"#,
            "note_id",
            2,
            "deck",
        ),
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            0,
            "query",
        ),
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut actions = Vec::new();
            for _ in 0..28 * count + if mode == "ids" { 0 } else { 3 } {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline);
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
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
                        length = line
                            .split_once(':')
                            .unwrap()
                            .1
                            .trim()
                            .parse::<usize>()
                            .unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let action = request["action"].as_str().unwrap();
                let value = match action {
                    "findNotes" => {
                        let expected = if mode == "query" {
                            "tag:source".to_owned()
                        } else {
                            linguist_anki::deck_query("Legacy \"cards\"").unwrap()
                        };
                        assert_eq!(request["params"]["query"], expected);
                        if count == 0 {
                            serde_json::json!([])
                        } else {
                            serde_json::json!([124, 123, 124])
                        }
                    }
                    "getActiveProfile" => serde_json::json!("Fixture"),
                    "modelNamesAndIds" => serde_json::json!({"Legacy":12}),
                    "notesInfo" => {
                        let id = request["params"]["notes"][0].as_u64().unwrap();
                        serde_json::json!([{"noteId":id,"modelName":"Legacy","fields":{"Word":{"value":"cat","order":0},"Meaning":{"value":"source meaning","order":1},"Pattern":{"value":"なら","order":2},"Formation":{"value":"V + なら","order":3},"Key":{"value":"accepted-key","order":4},"Unused":{"value":"  original\n","order":5}},"cards":[id+333],"tags":["preserved"]}])
                    }
                    "modelFieldNames" => serde_json::json!([
                        "Word",
                        "Meaning",
                        "Pattern",
                        "Formation",
                        "Key",
                        "Unused"
                    ]),
                    "modelTemplates" => serde_json::json!({"Card":{"Front":"front","Back":"back"}}),
                    "modelStyling" => serde_json::json!({"css":"style"}),
                    "cardsInfo" => {
                        let id = request["params"]["cards"][0].as_u64().unwrap();
                        serde_json::json!([{"cardId":id,"note":id-333,"reps":5,"due":10}])
                    }
                    _ => panic!("unexpected action {action}"),
                };
                actions.push(action.to_owned());
                let body = serde_json::json!({"result":value,"error":null}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            actions
        });
        let root = std::env::temp_dir().join(format!("lab-cli-revamp-{}", uuid::Uuid::new_v4()));
        let out = cli()
            .args([
                "--purpose",
                purpose,
                "--set",
                "llm.enabled=false",
                "--set",
                "dictionary.provider=authored",
                "--set",
                "images.search_when_missing=false",
            ])
            .arg("--set")
            .arg(format!("anki.endpoint={endpoint}"))
            .arg("--set")
            .arg(format!("storage.state_dir={}", root.display()))
            .arg("--set")
            .arg(format!("purposes.{purpose}.fields={mapping}"))
            .arg("--set")
            .arg(format!("selection.order={order}"))
            .args([command, "revamp"])
            .args(if mode == "query" {
                vec!["--query", "tag:source"]
            } else if mode == "deck" {
                vec!["--deck", "Legacy \"cards\""]
            } else if count == 1 {
                vec!["--note-id", "123"]
            } else {
                vec!["--note-id", "124", "--note-id", "123"]
            })
            .output()
            .unwrap();
        assert_eq!(
            server.join().unwrap().len(),
            28 * count + if mode == "ids" { 0 } else { 3 }
        );
        assert_eq!(
            out.status.code(),
            Some(if count == 0 { 0 } else { 4 }),
            "{out:?}"
        );
        assert!(out.stderr.is_empty());
        let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result["preparation_stage"], "source_draft");
        if count == 0 {
            assert_eq!(result["result"]["items"], serde_json::json!([]));
            assert_eq!(result["result"]["item_count"], 0);
            assert!(!root.exists());
            continue;
        }
        let item = if count == 1 {
            &result["result"]
        } else {
            assert_eq!(result["result"]["item_count"], count);
            &result["result"]["items"][0]
        };
        assert_eq!(item["apply_eligible"], false);
        let id = item["plan_id"].as_str().unwrap().parse().unwrap();
        let store = linguist_store::Store::read_only(&root).unwrap();
        let plan = store.revision(id, 1).unwrap();
        assert_eq!(plan.documents.len(), count);
        if count == 2 {
            let expected = if order == "note_id" {
                ["anki_note:123", "anki_note:124"]
            } else {
                ["anki_note:124", "anki_note:123"]
            };
            for (document, location) in plan.documents.iter().zip(expected) {
                assert_eq!(document.sources[0].location, location);
            }
        }
        assert_eq!(
            plan.documents[0].sources[0].fields["Unused"],
            "  original\n"
        );
        assert!(plan.binding.is_none() && plan.rendered.is_empty());
        for digest in &plan.documents[0].archives[0].asset_digests {
            assert!(!store.asset(digest, 100000).unwrap().is_empty());
        }
        if command == "grammar" {
            assert_eq!(plan.documents[0].explanation_language.as_str(), "vi");
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn revamp_missing_wrong_purpose_and_requested_generation_fail_before_state_creation() {
    let root =
        std::env::temp_dir().join(format!("lab-cli-revamp-rejected-{}", uuid::Uuid::new_v4()));
    for (extra, exit) in [
        (vec![], 2),
        (vec!["--purpose", "japanese_grammar"], 2),
        (vec!["--purpose", "english_vocab"], 3),
    ] {
        let out = cli()
            .arg("--set")
            .arg(format!("storage.state_dir={}", root.display()))
            .args(extra)
            .args(["vocab", "revamp", "--note-id", "123"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(exit), "{out:?}");
        assert!(out.stdout.is_empty());
        assert!(!root.exists());
    }
}

#[test]
fn revamp_duplicate_and_oversized_selections_fail_before_collection_reads() {
    for (ids, limit, expected) in [
        (["123", "123"], "2", "REVAMP_SELECTION_DUPLICATE"),
        (["123", "124"], "1", "REVAMP_SELECTION_LIMIT"),
    ] {
        let root =
            std::env::temp_dir().join(format!("lab-cli-revamp-selection-{}", uuid::Uuid::new_v4()));
        let out = cli()
            .args([
                "--purpose",
                "english_vocab",
                "--set",
                "llm.enabled=false",
                "--set",
                "dictionary.provider=authored",
                "--set",
                "images.search_when_missing=false",
                "--set",
                "anki.endpoint=http://127.0.0.1:1",
            ])
            .arg("--set")
            .arg(format!("storage.state_dir={}", root.display()))
            .arg("--set")
            .arg(format!("selection.max_notes={limit}"))
            .args(["vocab", "revamp", "--note-id", ids[0], "--note-id", ids[1]])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{out:?}"
        );
        assert!(!root.exists());
    }
}
