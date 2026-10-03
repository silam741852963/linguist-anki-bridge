use std::process::Command;

#[test]
fn backup_inspection_failure_does_not_create_state_or_contact_anki() {
    let root = std::env::temp_dir().join(format!("lab-backup-inspect-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let out = cli()
        .args([
            "--set",
            &state,
            "backup",
            "inspect",
            "/missing-lab-package.colpkg",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!root.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("CHECKPOINT_FILE_UNAVAILABLE"));
}

#[test]
fn live_plan_validation_reports_no_revamp_sources_without_claiming_apply() {
    use linguist_core::{LearningDocument, records::*};
    use std::{
        collections::BTreeMap,
        io::{BufRead, Read, Write},
    };
    let root = std::env::temp_dir().join(format!("lab-live-cli-{}", uuid::Uuid::new_v4()));
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
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values: BTreeMap::from([("input.max_file_mb".into(), serde_json::json!(1))]),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "fixture".into(),
        selection: None,
        grammar_groups: vec![],
        rendered: vec![linguist_core::render::render(&doc, &BTreeMap::new()).unwrap()],
        documents: vec![doc],
        review_decisions: vec![],
    };
    linguist_store::Store::open(&root)
        .unwrap()
        .publish_revision(&plan)
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["action"], "getActiveProfile");
            let body = serde_json::json!({"result":"Fixture","error":null}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let state = format!("storage.state_dir={}", root.display());
    let out = cli()
        .args([
            "--set",
            &state,
            "--set",
            &endpoint,
            "plans",
            "validate",
            &plan.id.to_string(),
            "--live",
            "--revision",
            "1",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["live"]["total_sources"], 0);
    assert_eq!(result["live"]["all_sources_checked"], true);
    assert_eq!(result["live"]["apply_eligible"], false);
    assert_eq!(result["content"]["evidence"]["content_ready"], true);
    let diff = cli()
        .args([
            "--set",
            &state,
            "--set",
            &endpoint,
            "plans",
            "diff",
            &plan.id.to_string(),
            "--from-revision",
            "1",
            "--revision",
            "1",
            "--live",
        ])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(diff.status.success(), "{diff:?}");
    let diff: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(diff["captured"]["from_revision"], 1);
    assert_eq!(diff["captured"]["to_revision"], 1);
    assert_eq!(diff["live"]["total_sources"], 0);
    assert_eq!(diff["live"]["all_sources_checked"], true);
    assert_eq!(diff["apply_eligible"], false);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn model_install_preview_distinguishes_create_and_name_collision_without_writes() {
    use std::io::{BufRead, Read, Write};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for state in ["missing", "collision", "exact"] {
        let manifest = linguist_core::model::vocabulary();
        let template_map: serde_json::Map<String, serde_json::Value> = manifest
            .templates
            .iter()
            .map(|template| {
                (
                    template.name.clone(),
                    serde_json::json!({"Front":template.front,"Back":template.back}),
                )
            })
            .collect();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
        let stopped = Arc::new(AtomicBool::new(false));
        let signal = stopped.clone();
        let server = std::thread::spawn(move || {
            let mut actions = Vec::new();
            while !signal.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("content-length:") {
                        length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let action = request["action"].as_str().unwrap();
                actions.push(action.to_owned());
                let result = match action {
                    "getActiveProfile" => serde_json::json!("Fixture"),
                    "modelNamesAndIds" if state != "missing" => {
                        serde_json::json!({"Linguist Vocabulary v2":42})
                    }
                    "modelNamesAndIds" => serde_json::json!({}),
                    "modelFieldNames" if state == "exact" => serde_json::json!(&manifest.fields),
                    "modelFieldNames" => serde_json::json!(["Expression"]),
                    "modelTemplates" if state == "exact" => serde_json::json!(&template_map),
                    "modelTemplates" => {
                        serde_json::json!({"Comprehension":{"Front":"old","Back":"old"}})
                    }
                    "modelStyling" if state == "exact" => serde_json::json!({"css":&manifest.css}),
                    "modelStyling" => serde_json::json!({"css":"old"}),
                    _ => panic!("unexpected action {action}"),
                };
                let body = serde_json::json!({"result":result,"error":null}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            actions
        });
        let output = cli()
            .args(["--set", &endpoint, "models", "install", "japanese_vocab"])
            .output()
            .unwrap();
        stopped.store(true, Ordering::SeqCst);
        let actions = server.join().unwrap();
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result["proposal"],
            match state {
                "missing" => "create",
                "collision" => "name_collision",
                _ => "reuse_requires_native_order_verification",
            }
        );
        assert_eq!(
            output.status.code(),
            Some(if state == "collision" { 4 } else { 0 })
        );
        assert_eq!(result["target"]["name"], "Linguist Vocabulary v2");
        assert_eq!(result["apply_eligible"], false);
        assert_eq!(result["writes_enabled"], false);
        assert!(actions.iter().all(|action| matches!(
            action.as_str(),
            "getActiveProfile"
                | "modelNamesAndIds"
                | "modelFieldNames"
                | "modelTemplates"
                | "modelStyling"
        )));
    }
    let rejected = cli()
        .args(["models", "install", "japanese_vocab", "--apply"])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("CAPABILITY_UNAVAILABLE"));
}
fn cli() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.args(["--output", "json"]);
    c.env_clear();
    c.env("HOME", "/tmp/lab-command-tests-no-config");
    c
}
#[test]
fn output_modes_follow_setting_and_explicit_top_level_choice() {
    let binary = env!("CARGO_BIN_EXE_linguist-anki-bridge");
    let human = Command::new(binary)
        .env_clear()
        .env("HOME", "/tmp/lab-command-tests-no-config")
        .args(["config", "show", "--defaults", "output.format"])
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("result.values.output.format: \"text\""),
        "{text}"
    );
    assert!(human.stderr.is_empty());

    let jsonl = Command::new(binary)
        .env_clear()
        .env("HOME", "/tmp/lab-command-tests-no-config")
        .args([
            "--set",
            "output.format=jsonl",
            "config",
            "show",
            "output.format",
        ])
        .output()
        .unwrap();
    assert!(jsonl.status.success(), "{jsonl:?}");
    let line = String::from_utf8(jsonl.stdout).unwrap();
    assert_eq!(line.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["values"]["output.format"],
        "jsonl"
    );

    let explicit = Command::new(binary)
        .env_clear()
        .env("HOME", "/tmp/lab-command-tests-no-config")
        .args([
            "--output",
            "json",
            "--set",
            "output.format=jsonl",
            "config",
            "show",
            "output.format",
        ])
        .output()
        .unwrap();
    assert!(explicit.status.success(), "{explicit:?}");
    let text = String::from_utf8(explicit.stdout).unwrap();
    assert!(text.contains("\n  \"values\""), "{text}");
}
#[cfg(unix)]
#[test]
fn closed_stdout_pipe_does_not_emit_diagnostic() {
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };
    for args in [
        vec!["--output", "json", "config", "show", "--defaults"],
        vec!["completions", "bash"],
    ] {
        let (reader, writer) = UnixStream::pair().unwrap();
        drop(reader);
        let output = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
            .env_clear()
            .env("HOME", "/tmp/lab-command-tests-no-config")
            .args(&args)
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
}
#[test]
fn global_offline_flag_resolves_before_any_service_request() {
    let shown = cli()
        .args(["config", "show", "network.offline", "--offline"])
        .output()
        .unwrap();
    assert!(shown.status.success(), "{shown:?}");
    let value: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(value["values"]["network.offline"], true);
    assert_eq!(
        value["provenance"]["network.offline"],
        serde_json::Value::Null
    );

    let conflicting = cli()
        .args([
            "--offline",
            "--set",
            "network.offline=false",
            "config",
            "validate",
        ])
        .output()
        .unwrap();
    assert_eq!(conflicting.status.code(), Some(2));
    assert!(conflicting.stdout.is_empty());
    assert!(String::from_utf8_lossy(&conflicting.stderr).contains("OFFLINE_OVERRIDE_CONFLICT"));

    let remote = cli()
        .args([
            "--offline",
            "--set",
            "anki.endpoint=https://example.org",
            "--set",
            "network.allowed_remote_service_hosts=[\"example.org\"]",
            "config",
            "validate",
        ])
        .output()
        .unwrap();
    assert_eq!(remote.status.code(), Some(2));
    assert!(remote.stdout.is_empty());
    assert!(String::from_utf8_lossy(&remote.stderr).contains("REMOTE_ENDPOINT_NOT_ALLOWED"));

    let ocr = fixture_ocr_executable();
    let local = cli()
        .args(["--set", &ocr, "doctor", "--local", "--offline"])
        .output()
        .unwrap();
    assert!(local.status.success(), "{local:?}");
    let value: serde_json::Value = serde_json::from_slice(&local.stdout).unwrap();
    assert_eq!(value["offline_requested"], true);
    assert_eq!(value["services_probed"], false);
}
fn fixture_ocr_executable() -> String {
    format!(
        "ocr.executable={}",
        std::env::current_exe().unwrap().display()
    )
}
#[test]
fn queued_job_controls_resume_cancel_and_explicit_migration_preserve_state() {
    let root = std::env::temp_dir().join(format!("lab-controls-cli-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let absent = cli()
        .args(["--set", &state, "jobs", "migrate"])
        .output()
        .unwrap();
    assert!(!absent.status.success());
    assert!(!root.exists());
    std::fs::create_dir(&root).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let empty = cli()
        .args(["--set", &state, "jobs", "migrate"])
        .output()
        .unwrap();
    assert!(!empty.status.success());
    assert!(!root.join("state.sqlite3").exists());
    std::fs::remove_dir(&root).unwrap();
    let created = cli()
        .args([
            "--purpose",
            "english_vocab",
            "--set",
            &state,
            "--set",
            "anki.endpoint=http://127.0.0.1:1",
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "--set",
            "audio.provider=disabled",
            "jobs",
            "create",
            "--note-id",
            "123",
        ])
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let created: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    let id = created["job_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE preparation_controls;PRAGMA user_version=6;")
        .unwrap();
    drop(db);
    let migrated = cli()
        .args(["--set", &state, "jobs", "migrate"])
        .output()
        .unwrap();
    assert!(migrated.status.success(), "{migrated:?}");
    let mut pause_digest = None;
    for _ in 0..2 {
        let pause = cli()
            .args(["--set", &state, "jobs", "pause", id])
            .output()
            .unwrap();
        assert!(pause.status.success());
        let pause: serde_json::Value = serde_json::from_slice(&pause.stdout).unwrap();
        assert_eq!(pause["worker_stopped_confirmed"], false);
        if let Some(digest) = &pause_digest {
            assert_eq!(&pause["control"]["digest"], digest);
        }
        pause_digest = Some(pause["control"]["digest"].clone());
    }
    let paused = cli()
        .args(["--set", &state, "jobs", "run", id])
        .output()
        .unwrap();
    assert!(paused.status.success(), "{paused:?}");
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert!(
        store
            .preparation_events(id.parse().unwrap(), 0, 10)
            .unwrap()
            .is_empty()
    );
    let resumed = cli()
        .args(["--set", &state, "jobs", "resume", id])
        .output()
        .unwrap();
    assert_eq!(resumed.status.code(), Some(3), "{resumed:?}");
    let resumed: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["control"]["event"]["action"], "resume");
    let cancel = cli()
        .args(["--set", &state, "jobs", "cancel", id])
        .output()
        .unwrap();
    assert!(cancel.status.success());
    let rejected = cli()
        .args(["--set", &state, "jobs", "resume", id])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(5));
    let cancelled_run = cli()
        .args(["--set", &state, "jobs", "run", id])
        .output()
        .unwrap();
    assert_eq!(cancelled_run.status.code(), Some(3));
    let events = store
        .preparation_events(id.parse().unwrap(), 0, 10)
        .unwrap();
    assert_eq!(events.len(), 2);
    for (after_checkpoint, after_control, checkpoint_count, control_action) in [
        (0, 0, 1, Some("pause")),
        (1, 1, 1, Some("resume")),
        (2, 2, 0, Some("cancel")),
        (2, 3, 0, None),
    ] {
        let out = cli()
            .args([
                "--set",
                &state,
                "--set",
                "output.page_size=1",
                "jobs",
                "audit",
                id,
                "--after-checkpoint",
                &after_checkpoint.to_string(),
                "--after-control",
                &after_control.to_string(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            value["checkpoints"].as_array().unwrap().len(),
            checkpoint_count
        );
        assert_eq!(value["scope"], "local_history_page");
        assert_eq!(value["full_history_checked"], false);
        assert_eq!(value["native_verified"], false);
        assert_eq!(value["page_hashes_and_links_verified"], true);
        if let Some(action) = control_action {
            assert_eq!(value["controls"][0]["event"]["action"], action);
        } else {
            assert_eq!(value["controls"], serde_json::json!([]));
        }
    }
    let live = cli()
        .args(["--set", &state, "jobs", "audit", id, "--live"])
        .output()
        .unwrap();
    assert_eq!(live.status.code(), Some(3));
    assert!(live.stdout.is_empty());
    assert_eq!(
        store
            .preparation_events(id.parse().unwrap(), 0, 10)
            .unwrap()
            .len(),
        2
    );
    assert!(
        !store
            .preparation_job(id.parse().unwrap())
            .unwrap()
            .job
            .pause_requested
    );
    assert!(std::fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".schema-v6-")
    }));
    let mut control = store
        .preparation_control(id.parse().unwrap())
        .unwrap()
        .unwrap();
    control.event.parent_digest = None;
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TRIGGER preparation_controls_no_update;")
        .unwrap();
    db.execute(
        "UPDATE preparation_controls SET digest=?1,body=?2 WHERE job_id=?3 AND sequence=3",
        rusqlite::params![
            linguist_core::canonical::digest("preparation-control", &control.event).unwrap(),
            linguist_core::canonical::bytes(&control.event).unwrap(),
            id
        ],
    )
    .unwrap();
    let corrupt = cli()
        .args([
            "--set",
            &state,
            "jobs",
            "audit",
            id,
            "--after-control",
            "2",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert_eq!(corrupt.status.code(), Some(7));
    assert!(corrupt.stdout.is_empty());
    drop(db);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn preparation_query_and_deck_jobs_freeze_matches_without_content_reads() {
    use std::io::{BufRead, Read, Write};
    for (kind, mode, limit, max, success, selected_count) in [
        ("query", "normal", None, 1000, true, 2),
        ("deck", "normal", Some(1), 1000, true, 1),
        ("query", "oversized", None, 1, false, 0),
        ("query", "explicit_override", Some(2), 1, true, 2),
        ("query", "empty", None, 1000, true, 0),
        ("query", "profile_drift", None, 1000, false, 0),
    ] {
        let root = std::env::temp_dir().join(format!("lab-query-job-{}", uuid::Uuid::new_v4()));
        let state = format!("storage.state_dir={}", root.display());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
        let deck = "Legacy \"cards\"";
        let expected_query = if kind == "deck" {
            linguist_anki::deck_query(deck).unwrap()
        } else {
            "tag:source".into()
        };
        let query = expected_query.clone();
        let server = std::thread::spawn(move || {
            for (index, expected) in ["getActiveProfile", "findNotes", "getActiveProfile"]
                .into_iter()
                .enumerate()
            {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "missing selection request"
                            );
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
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(request["action"], expected);
                let result = if expected == "findNotes" {
                    assert_eq!(request["params"]["query"], query);
                    if mode == "empty" {
                        serde_json::json!([])
                    } else {
                        serde_json::json!([124, 123, 124])
                    }
                } else if mode == "profile_drift" && index == 2 {
                    serde_json::json!("Other")
                } else {
                    serde_json::json!("Fixture")
                };
                let body = serde_json::json!({"result":result,"error":null}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let mut command = cli();
        command.args([
            "--purpose",
            "english_vocab",
            "--set",
            &state,
            "--set",
            &endpoint,
            "--set",
            &format!("selection.max_notes={max}"),
            "jobs",
            "create",
        ]);
        command.args(if kind == "deck" {
            ["--deck", deck]
        } else {
            ["--query", "tag:source"]
        });
        if let Some(limit) = limit {
            command.args(["--limit", &limit.to_string()]);
        }
        let out = command.output().unwrap();
        server.join().unwrap();
        assert_eq!(out.status.success(), success, "{mode}: {out:?}");
        if !success {
            assert_eq!(
                out.status.code(),
                Some(if mode == "profile_drift" { 5 } else { 2 }),
                "{out:?}"
            );
            assert!(out.stdout.is_empty());
            assert!(!root.exists());
            continue;
        }
        let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result["input_count"], selected_count);
        assert_eq!(result["worker_started"], false);
        if selected_count == 0 {
            assert!(result["job_id"].is_null());
            assert!(!root.exists());
            continue;
        }
        assert_eq!(result["matched_count"], 2);
        let id = result["job_id"].as_str().unwrap().parse().unwrap();
        let store = linguist_store::Store::read_only(&root).unwrap();
        let definition = store.preparation_job(id).unwrap();
        assert_eq!(definition.selection.matched_note_ids, ["123", "124"]);
        assert_eq!(definition.selection.selected_note_ids.len(), selected_count);
        assert_eq!(definition.selection.command_limit, limit);
        assert_eq!(definition.selection.max_notes, max);
        if kind == "deck" {
            assert_eq!(
                definition.selection.selector,
                linguist_core::records::SelectionInput::Deck {
                    name: deck.into(),
                    query: expected_query
                }
            );
        } else {
            assert_eq!(
                definition.selection.selector,
                linguist_core::records::SelectionInput::Query(expected_query)
            );
        }
        assert!(store.preparation_events(id, 0, 10).unwrap().is_empty());
        assert!(store.list_revisions(10).unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    let root =
        std::env::temp_dir().join(format!("lab-invalid-job-selector-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    for args in [
        vec![],
        vec!["--note-id", "123", "--query", "tag:source"],
        vec!["--query", " "],
        vec!["--note-id", "123", "--limit", "1"],
        vec!["--query", "tag:source", "--deck", "Legacy"],
    ] {
        let out = cli()
            .args([
                "--purpose",
                "english_vocab",
                "--set",
                &state,
                "--set",
                "anki.endpoint=http://127.0.0.1:1",
                "jobs",
                "create",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!root.exists());
    }
}
#[test]
fn preparation_concurrency_is_frozen_bounded_and_stop_drains_dispatched_items() {
    use std::io::{BufRead, Read, Write};
    for (workers, policy, dispatched) in [
        (1, "stop", 1),
        (2, "stop", 2),
        (2, "continue", 5),
        (2, "pause", 2),
        (2, "cancel", 2),
    ] {
        let root = std::env::temp_dir().join(format!("lab-parallel-cli-{}", uuid::Uuid::new_v4()));
        let state = format!("storage.state_dir={}", root.display());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
        let created = cli()
            .args([
                "--purpose",
                "english_vocab",
                "--set",
                &state,
                "--set",
                &endpoint,
                "--set",
                "llm.enabled=false",
                "--set",
                "dictionary.provider=authored",
                "--set",
                "images.search_when_missing=false",
                "--set",
                "audio.provider=disabled",
                "--set",
                &format!("jobs.prepare_workers={workers}"),
                "--set",
                &format!(
                    "jobs.on_item_error={}",
                    if matches!(policy, "pause" | "cancel") {
                        "continue"
                    } else {
                        policy
                    }
                ),
                "--set",
                "jobs.lease_seconds=10",
                "--set",
                "jobs.heartbeat_seconds=1",
                "jobs",
                "create",
                "--note-id",
                "123",
                "--note-id",
                "124",
                "--note-id",
                "125",
                "--note-id",
                "126",
                "--note-id",
                "127",
            ])
            .output()
            .unwrap();
        assert!(created.status.success(), "{created:?}");
        let created: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
        let id = created["job_id"].as_str().unwrap().to_owned();
        let job = id.parse().unwrap();
        let state_root = root.clone();
        let server = std::thread::spawn(move || {
            for offset in (0..dispatched).step_by(workers) {
                let wave = workers.min(dispatched - offset);
                let mut streams = Vec::new();
                // Hold replies until all configured workers have dispatched a read.
                // A serial implementation cannot satisfy this barrier for two workers.
                for _ in 0..wave {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    let stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "parallel dispatch barrier timed out"
                                );
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
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(request["action"], "getActiveProfile");
                    streams.push(stream);
                }
                let store = linguist_store::Store::read_only(&state_root).unwrap();
                let items = store.preparation_items(job, 0, 10).unwrap();
                assert_eq!(
                    items.iter().filter(|item| item.state == "started").count(),
                    wave
                );
                assert_eq!(
                    items.iter().filter(|item| item.state == "failed").count(),
                    offset
                );
                assert_eq!(
                    items.iter().filter(|item| item.state == "pending").count(),
                    5 - offset - wave
                );
                if workers == 2 && policy == "stop" {
                    // Keep every worker awaiting HTTP and observe durable heartbeat renewal.
                    let db = rusqlite::Connection::open_with_flags(
                        state_root.join("state.sqlite3"),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .unwrap();
                    let resource = format!("job:{job}");
                    let expiry = || {
                        db.query_row(
                            "SELECT expires_ms FROM leases WHERE resource=?1",
                            [&resource],
                            |row| row.get::<_, i64>(0),
                        )
                        .unwrap()
                    };
                    let initial = expiry();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while expiry() <= initial {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "worker coordinator did not renew its lease during blocked reads"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                if matches!(policy, "pause" | "cancel") {
                    let result = cli()
                        .args([
                            "--set",
                            &format!("storage.state_dir={}", state_root.display()),
                            "jobs",
                            policy,
                            &job.to_string(),
                        ])
                        .output()
                        .unwrap();
                    assert!(result.status.success(), "{result:?}");
                    let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
                    assert_eq!(result["worker_stopped_confirmed"], false);
                    assert_eq!(result["control"]["event"]["action"], policy);
                }
                for mut stream in streams {
                    write!(stream, "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
            }
        });
        // These current flags must not replace the frozen concurrency/error policy.
        let run = cli()
            .args([
                "--set",
                &state,
                "--set",
                "jobs.prepare_workers=16",
                "--set",
                "jobs.on_item_error=continue",
                "jobs",
                "run",
                &id,
            ])
            .output()
            .unwrap();
        server.join().unwrap();
        assert_eq!(run.status.code(), Some(3), "{run:?}");
        let run: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
        assert_eq!(run["failed_this_run"], dispatched);
        assert_eq!(run["plan_published"], false);
        if matches!(policy, "pause" | "cancel") {
            assert_eq!(run["control"]["event"]["action"], policy);
        }
        let store = linguist_store::Store::read_only(&root).unwrap();
        let items = store.preparation_items(job, 0, 10).unwrap();
        assert_eq!(
            items.iter().filter(|item| item.state == "failed").count(),
            dispatched
        );
        assert_eq!(
            items.iter().filter(|item| item.state == "pending").count(),
            5 - dispatched
        );
        assert_eq!(
            store.preparation_events(job, 0, 100).unwrap().len(),
            dispatched * 2
        );
        assert!(items.iter().all(|item| item.state != "started"));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn preparation_worker_freezes_settings_and_bounds_retries() {
    let root = std::env::temp_dir().join(format!("lab-worker-cli-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let created = cli()
        .args([
            "--purpose",
            "english_vocab",
            "--set",
            &state,
            "--set",
            "anki.endpoint=http://127.0.0.1:1",
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "--set",
            "audio.provider=disabled",
            "--set",
            "jobs.max_item_attempts=2",
            "jobs",
            "create",
            "--note-id",
            "123",
        ])
        .output()
        .unwrap();
    assert!(created.status.success(), "{:?}", created);
    let created: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    let id = created["job_id"].as_str().unwrap();
    for expected in [1, 2, 2] {
        // Current defaults enable generation; only the frozen settings may govern execution.
        let run = cli()
            .args(["--set", &state, "jobs", "run", id])
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(3), "{:?}", run);
        let run: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
        assert_eq!(run["writes_enabled"], false);
        assert_eq!(run["plan_published"], false);
        assert_eq!(run["item_counts"]["failed"], 1);
        assert_eq!(run["error_counts"]["SOURCE_READ_CONNECTION_FAILED"], 1);
        let items = cli()
            .args(["--set", &state, "jobs", "items", id])
            .output()
            .unwrap();
        let items: serde_json::Value = serde_json::from_slice(&items.stdout).unwrap();
        assert_eq!(items["items"][0]["attempt"], expected);
        assert_eq!(items["items"][0]["state"], "failed");
        assert_eq!(
            items["items"][0]["error_code"],
            "SOURCE_READ_CONNECTION_FAILED"
        );
    }
    let mut store = linguist_store::Store::open(&root).unwrap();
    let mut interrupted = store.preparation_job(id.parse().unwrap()).unwrap();
    interrupted.job.id = uuid::Uuid::new_v4();
    interrupted.job.item_ids = vec![uuid::Uuid::new_v4(), uuid::Uuid::new_v4()];
    interrupted.job.plan_refs = vec!["anki-note:123".into(), "anki-note:124".into()];
    interrupted.selection.matched_note_ids = vec!["123".into(), "124".into()];
    interrupted.selection.selected_note_ids = interrupted.selection.matched_note_ids.clone();
    interrupted.selection.selector = linguist_core::records::SelectionInput::NoteIds(
        interrupted.selection.matched_note_ids.clone(),
    );
    store.create_preparation_job(&interrupted).unwrap();
    store
        .append_preparation_event(
            interrupted.job.id,
            interrupted.job.item_ids[1],
            1,
            linguist_store::preparation::PreparationStage::Started,
            None,
        )
        .unwrap();
    let run = cli()
        .args([
            "--set",
            &state,
            "jobs",
            "run",
            &interrupted.job.id.to_string(),
        ])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(7), "{run:?}");
    assert!(run.stdout.is_empty());
    assert_eq!(
        store
            .preparation_events(interrupted.job.id, 0, 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.preparation_items(interrupted.job.id, 0, 1).unwrap()[0].state,
        "pending"
    );
    let job = interrupted.job.id.to_string();
    let preview = cli()
        .args(["--set", &state, "jobs", "recover", &job])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["executed"], false);
    assert_eq!(value["started"].as_array().unwrap().len(), 1);
    let missing_actor = cli()
        .args(["--set", &state, "jobs", "recover", &job, "--execute"])
        .output()
        .unwrap();
    assert_eq!(missing_actor.status.code(), Some(2));
    let token = store
        .acquire_lease(
            &linguist_store::lease::Resource::JobWorker(interrupted.job.id),
            60,
        )
        .unwrap();
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute(
        "UPDATE leases SET expires_ms=0 WHERE resource=?1",
        [format!("job:{}", interrupted.job.id)],
    )
    .unwrap();
    let args = [
        "--set",
        &state,
        "jobs",
        "recover",
        &job,
        "--actor",
        "operator",
        "--execute",
    ];
    let held = cli().args(args).output().unwrap();
    assert_eq!(held.status.code(), Some(5), "{held:?}");
    assert_eq!(
        store
            .preparation_events(interrupted.job.id, 0, 10)
            .unwrap()
            .len(),
        1
    );
    // Model an expired, absent Linux process identity; expiry with a live owner was rejected above.
    db.execute(
        "UPDATE leases SET pid=4294967295 WHERE resource=?1",
        [format!("job:{}", interrupted.job.id)],
    )
    .unwrap();
    drop(db);
    let recovered = cli().args(args).output().unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let value: serde_json::Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(value["reconciled"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["reconciled"][0]["event"]["stage"]["state"],
        "interrupted"
    );
    assert_eq!(value["work_dispatched"], false);
    assert_eq!(value["attempts_reset"], false);
    assert!(store.validate_lease(&token).is_err());
    let items = store.preparation_items(interrupted.job.id, 0, 10).unwrap();
    assert_eq!(items[0].state, "pending");
    assert_eq!(items[1].attempt, 1);
    assert_eq!(
        items[1].error_code.as_deref(),
        Some("SOURCE_READ_INTERRUPTED")
    );
    assert!(items[1].retry_eligible);
    let again = cli().args(args).output().unwrap();
    assert!(again.status.success());
    let value: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(value["reconciled"].as_array().unwrap().len(), 0);
    assert_eq!(
        store
            .preparation_events(interrupted.job.id, 0, 10)
            .unwrap()
            .len(),
        2
    );
    let resumed = cli()
        .args(["--set", &state, "jobs", "run", &job])
        .output()
        .unwrap();
    assert_eq!(resumed.status.code(), Some(3), "{resumed:?}");
    let items = store.preparation_items(interrupted.job.id, 0, 10).unwrap();
    assert_eq!(items[1].attempt, 2);
    assert!(!items[1].retry_eligible);
    let history = store.preparation_events(interrupted.job.id, 0, 10).unwrap();
    assert!(matches!(&history[1].event.stage,
        linguist_store::preparation::PreparationStage::Interrupted { actor } if actor == "operator"));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn preparation_jobs_queue_and_paginate_without_anki_or_worker_effects() {
    let root = std::env::temp_dir().join(format!("lab-job-cli-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let list = cli()
        .args(["--set", &state, "jobs", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let value: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(value["jobs"], serde_json::json!([]));
    assert!(!root.exists());
    for ids in [vec!["0"], vec!["123", "123"]] {
        let mut command = cli();
        command.args([
            "--purpose",
            "english_vocab",
            "--set",
            &state,
            "jobs",
            "create",
        ]);
        for id in ids {
            command.args(["--note-id", id]);
        }
        let out = command.output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!root.exists());
    }
    let created = cli()
        .args([
            "--purpose",
            "english_vocab",
            "--set",
            &state,
            "--set",
            "anki.endpoint=http://127.0.0.1:1",
            "jobs",
            "create",
            "--note-id",
            "124",
            "--note-id",
            "123",
        ])
        .output()
        .unwrap();
    assert!(created.status.success(), "{:?}", created);
    let value: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(value["worker_started"], false);
    assert_eq!(value["writes_enabled"], false);
    let id = value["job_id"].as_str().unwrap();
    let blocked = cli()
        .args(["--set", &state, "jobs", "run", id])
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(blocked.stdout.is_empty());
    let shown = cli()
        .args(["--set", &state, "jobs", "show", id])
        .output()
        .unwrap();
    assert!(shown.status.success());
    let shown: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(
        shown["definition"]["selection"]["selected_note_ids"],
        serde_json::json!(["123", "124"])
    );
    assert_eq!(shown["worker_liveness"], "unverified");
    let items = cli()
        .args([
            "--set",
            &state,
            "--set",
            "output.page_size=1",
            "jobs",
            "items",
            id,
        ])
        .output()
        .unwrap();
    assert!(items.status.success());
    let items: serde_json::Value = serde_json::from_slice(&items.stdout).unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert_eq!(items["items"][0]["input_ref"], "anki-note:123");
    assert_eq!(items["items"][0]["state"], "pending");
    assert_eq!(items["next_index"], 1);
    let items = cli()
        .args(["--set", &state, "jobs", "items", id, "--after-index", "1"])
        .output()
        .unwrap();
    let items: serde_json::Value = serde_json::from_slice(&items.stdout).unwrap();
    assert_eq!(items["items"][0]["input_ref"], "anki-note:124");
    let listed = cli()
        .args(["--set", &state, "jobs", "list"])
        .output()
        .unwrap();
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["jobs"][0]["item_count"], 2);
    assert_eq!(listed["jobs"][0]["checkpoint_count"], 0);
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert!(store.list_revisions(100).unwrap().is_empty());
    let id: uuid::Uuid = id.parse().unwrap();
    assert!(store.preparation_events(id, 0, 100).unwrap().is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
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
fn clap_syntax_errors_are_structured_and_do_not_echo_argument_values() {
    for args in [
        vec!["--output", "private-token", "config", "show"],
        vec!["--set", "llm.model=private-token", "unknown-command"],
        vec!["config", "set", "llm.model"],
    ] {
        let out = cli().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}: {out:?}");
        let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(
            error["error"],
            "USAGE: invalid command syntax; run linguist-anki-bridge --help"
        );
        assert!(!String::from_utf8_lossy(&out.stderr).contains("private-token"));
    }
    let help = cli().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    let version = cli().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
}
#[test]
fn config_describe_shows_cross_field_rules_and_nearest_names_without_loading_config() {
    let described = cli()
        .args([
            "--config",
            "/does/not/exist",
            "--set",
            "made.up=secret",
            "config",
            "describe",
            "llm.model",
        ])
        .output()
        .unwrap();
    assert!(described.status.success(), "{described:?}");
    let value: serde_json::Value = serde_json::from_slice(&described.stdout).unwrap();
    assert_eq!(value["key"], "llm.model");
    assert_eq!(value["resolved_key"], "llm.model");
    assert!(
        value["cross_field_checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["selector"] == "llm.enabled" && rule["required"] == "llm.model")
    );

    let unknown = cli()
        .args(["config", "describe", "llm.modle"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    assert!(unknown.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&unknown.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("llm.model"));
}
#[cfg(unix)]
#[test]
fn config_reads_reject_symlinks_and_nonregular_paths() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(format!("lab-config-read-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let real = root.join("real.toml");
    std::fs::write(&real, "[config]\nversion = 2\n").unwrap();
    let link = root.join("link.toml");
    symlink(&real, &link).unwrap();
    for (path, command, exit) in [(link.as_path(), "show", 6), (root.as_path(), "validate", 2)] {
        let out = cli()
            .arg("--config")
            .arg(path)
            .args(["config", command])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(exit), "{out:?}");
        assert!(out.stdout.is_empty());
        let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        assert!(error["error"].as_str().unwrap().starts_with("CONFIG_"));
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn config_migrate_current_version_is_idempotent_and_unknown_version_is_rejected() {
    let root = std::env::temp_dir().join(format!("lab-config-migrate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.toml");
    let output = root.join("candidate.toml");
    let original = b"# keep this exact source\n[config]\nversion = 2\n";
    std::fs::write(&source, original).unwrap();
    for execute in [false, true] {
        let mut command = cli();
        command
            .arg("--config")
            .arg(&source)
            .args(["config", "migrate", "--output"])
            .arg(&output);
        if execute {
            command.arg("--execute");
        }
        let out = command.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["source_version"], 2);
        assert_eq!(report["changed"], false);
        assert_eq!(report["candidate_written"], false);
        assert_eq!(report["requested_execute"], execute);
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert!(!output.exists());
    }
    std::fs::write(&source, b"[config]\nversion = 3\n").unwrap();
    let out = cli()
        .arg("--config")
        .arg(&source)
        .args(["config", "migrate", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("UNSUPPORTED_CONFIG_VERSION"));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn config_validate_reports_local_resource_gaps_separately_from_settings_errors() {
    let root = std::env::temp_dir().join(format!("lab-config-resource-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let resource = root.join("schema.json");
    let setting = format!("dictionary.schema_path={}", resource.display());
    let ocr = fixture_ocr_executable();
    let run = || {
        cli()
            .args(["--set", &setting, "--set", &ocr, "config", "validate"])
            .output()
            .unwrap()
    };
    let missing = run();
    assert_eq!(missing.status.code(), Some(3), "{missing:?}");
    assert!(missing.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(report["valid"], true);
    assert_eq!(
        report["local_resources"]["missing"][0],
        "dictionary.schema_path"
    );
    assert_eq!(report["runtime_resources_checked"], false);

    std::fs::write(&resource, b"{}").unwrap();
    let available = run();
    assert!(available.status.success(), "{available:?}");
    let report: serde_json::Value = serde_json::from_slice(&available.stdout).unwrap();
    assert!(
        report["local_resources"]["missing"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(report["local_resources"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["key"] == "dictionary.schema_path" && check["status"] == "available"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = root.join("schema-link.json");
        symlink(&resource, &link).unwrap();
        let linked = cli()
            .args([
                "--set",
                &format!("dictionary.schema_path={}", link.display()),
                "config",
                "validate",
            ])
            .output()
            .unwrap();
        assert_eq!(linked.status.code(), Some(3));
        let report: serde_json::Value = serde_json::from_slice(&linked.stdout).unwrap();
        assert!(
            report["local_resources"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["key"] == "dictionary.schema_path"
                    && check["status"] == "symlink")
        );
    }
    let unknown_builtin = cli()
        .args([
            "--set",
            "llm.prompts.vocabulary=builtin:unknown",
            "config",
            "validate",
        ])
        .output()
        .unwrap();
    assert_eq!(unknown_builtin.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&unknown_builtin.stdout).unwrap();
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "llm.prompts.vocabulary"
                && check["status"] == "unknown_builtin")
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn config_validate_rejects_selected_provider_without_required_setting() {
    let out = cli()
        .args(["--set", "images.provider=custom", "config", "validate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("images.custom_endpoint")
    );
}
#[cfg(unix)]
#[test]
fn config_validate_inspects_configured_helper_without_running_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("lab-config-exec-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let executable = root.join("lab-browser-helper");
    let run = || {
        cli()
            .env("PATH", &root)
            .args([
                "--set",
                "browser.enabled=true",
                "--set",
                "browser.executable=lab-browser-helper",
                "--set",
                "ocr.executable=lab-browser-helper",
                "config",
                "validate",
            ])
            .output()
            .unwrap()
    };
    let missing = run();
    assert_eq!(missing.status.code(), Some(3), "{missing:?}");
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "browser.executable" && check["status"] == "missing")
    );
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "ocr.executable"
                && check["status"] == "missing"
                && check["required"] == true)
    );

    std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o600)).unwrap();
    let not_executable = run();
    assert_eq!(not_executable.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&not_executable.stdout).unwrap();
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |check| check["key"] == "browser.executable" && check["status"] == "not_executable"
            )
    );

    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let available = run();
    assert!(available.status.success(), "{available:?}");
    assert!(available.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&available.stdout).unwrap();
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "ocr.executable" && check["status"] == "available")
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unfinished_catalogue_commands_fail_before_config_or_effects() {
    let id = "11111111-1111-4111-8111-111111111111";
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "OP-09",
            vec![
                "config",
                "import",
                "--file",
                "/missing",
                "--output",
                "/missing-out",
                "--replace",
            ],
        ),
        (
            "OP-13",
            vec![
                "decks",
                "map",
                "japanese_vocab",
                "--source-deck",
                "Source",
                "--source-model",
                "Basic",
                "--fields",
                "/missing",
            ],
        ),
        ("OP-14", vec!["decks", "unmap", "japanese_vocab"]),
        (
            "OP-30",
            vec![
                "plans",
                "regenerate",
                id,
                "--base-revision",
                "1",
                "--digest",
                "abc",
            ],
        ),
        (
            "OP-34",
            vec!["apply", id, "--revision", "1", "--digest", "abc", "--apply"],
        ),
        ("OP-42", vec!["jobs", "retry", id, "--failed"]),
        ("OP-44", vec!["jobs", "rollback", id, "--apply"]),
        ("OP-45", vec!["jobs", "delete", id, "--execute"]),
        ("OP-50", vec!["snapshots", "restore", id, "--apply"]),
        (
            "OP-51",
            vec!["snapshots", "export", id, "--output", "/missing-out"],
        ),
        (
            "OP-52",
            vec![
                "backup",
                "create",
                "--scope",
                "collection",
                "--output",
                "/missing-out",
                "--apply",
            ],
        ),
        ("OP-53", vec!["backup", "list"]),
        ("OP-54", vec!["backup", "verify", "/missing"]),
        ("OP-55", vec!["cache", "status"]),
        ("OP-56", vec!["cache", "prune", "--execute"]),
        ("OP-57", vec!["resources", "list"]),
        (
            "OP-58",
            vec![
                "resources",
                "install",
                "model",
                "--source",
                "https://example.org/x",
                "--version",
                "1",
                "--sha256",
                "abc",
                "--license",
                "CC0",
                "--destination",
                "/missing-out",
            ],
        ),
        (
            "OP-60",
            vec!["recover", "reconcile", id, "--apply", "--rebind"],
        ),
    ];
    for (operation, args) in cases {
        let out = cli()
            .args(["--config", "/does/not/exist", "--set", "made.up=secret"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3), "{operation}: {out:?}");
        assert!(out.stdout.is_empty(), "{operation}");
        let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        let message = error["error"].as_str().unwrap();
        assert!(message.contains(operation), "{message}");
        assert!(!message.contains("secret"));
    }
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
    let preview: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(preview["executed"], false);
    assert_eq!(preview["changes"][0]["key"], "llm.temperature");
    assert_eq!(preview["changes"][0]["before_effective"], 0.6);
    assert_eq!(preview["changes"][0]["after_effective"], 0.0);
    assert_eq!(preview["changes"][0]["after_provenance"], "builtin");
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
fn config_init_replace_saves_exact_prior_file_and_reports_backup() {
    let root = std::env::temp_dir().join(format!("lab-init-replace-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    let original = b"# authored comment\n[config]\nversion=2\n[llm]\nmodel='chosen:model'\n";
    std::fs::write(&path, original).unwrap();
    let protected = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(!protected.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let replaced = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "init", "--replace"])
        .output()
        .unwrap();
    assert!(replaced.status.success(), "{:?}", replaced);
    let receipt: serde_json::Value = serde_json::from_slice(&replaced.stdout).unwrap();
    assert_eq!(receipt["created"], false);
    assert_eq!(receipt["replaced"], true);
    assert_eq!(
        std::fs::read(receipt["backup"].as_str().unwrap()).unwrap(),
        original
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"[config]\nversion = 2\n");
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn config_init_prevalidates_and_reports_purpose_setup_without_installing() {
    let root = std::env::temp_dir().join(format!("lab-init-setup-{}", uuid::Uuid::new_v4()));
    let path = root.join("config.toml");
    let invalid = cli()
        .env("LAB_JOBS__HEARTBEAT_SECONDS", "30")
        .arg("--config")
        .arg(&path)
        .args(["config", "init"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(!root.exists());

    let initialized = cli()
        .arg("--config")
        .arg(&path)
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&initialized.stdout).unwrap();
    let setup = receipt["purpose_setup"].as_array().unwrap();
    assert_eq!(setup.len(), 4);
    let japanese = setup
        .iter()
        .find(|row| row["purpose"] == "japanese_vocab")
        .unwrap();
    assert_eq!(japanese["target_language"], "ja");
    assert_eq!(japanese["model_candidate"], "gemma4:12b");
    assert_eq!(japanese["model_verified"], false);
    assert!(
        japanese["add_missing_mapping_keys"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("purposes.japanese_vocab.target_deck"))
    );
    assert_eq!(japanese["runtime_resources_checked"], false);
    assert_eq!(std::fs::read(&path).unwrap(), b"[config]\nversion = 2\n");
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn recovery_inspection_never_initializes_state_or_connects_when_no_journals_exist() {
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
        .args(["--set"])
        .arg(format!("storage.state_dir={}", root.display()))
        .args(["recover", "inspect", "--pending", "--live"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["live_requested"], true);
    assert_eq!(value["native_status_checked"], false);
    assert_eq!(value["live"], serde_json::Value::Null);
    assert!(!root.exists());
}
#[test]
fn note_selector_conflicts_and_invalid_ids_fail_before_anki_requests() {
    for args in [
        vec!["notes", "list", "--deck", "A", "--query", "x"],
        vec!["notes", "list"],
        vec!["notes", "show", "0123"],
        vec!["notes", "list", "--query", "x", "--limit", "0"],
        vec![
            "--purpose",
            "english_vocab",
            "vocab",
            "revamp",
            "--query",
            "x",
            "--limit",
            "0",
        ],
        vec![
            "--purpose",
            "english_vocab",
            "vocab",
            "revamp",
            "--query",
            "x",
            "--limit",
            "100001",
        ],
        vec![
            "--purpose",
            "english_vocab",
            "vocab",
            "revamp",
            "--note-id",
            "123",
            "--limit",
            "1",
        ],
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
    let ocr = fixture_ocr_executable();
    for args in [vec!["doctor", "--local"], vec!["models", "builtin"]] {
        let out = cli().args(["--set", &ocr]).args(args).output().unwrap();
        assert!(out.status.success(), "{:?}", out);
        assert!(out.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap();
    }
}
#[test]
fn local_doctor_distinguishes_optional_and_required_resource_gaps() {
    let root = std::env::temp_dir().join(format!("lab-local-doctor-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let ocr = fixture_ocr_executable();
    let optional = cli()
        .args([
            "--set",
            &state,
            "--set",
            &ocr,
            "--set",
            "dictionary.schema_path=/does/not/exist/schema.json",
            "doctor",
            "--local",
        ])
        .output()
        .unwrap();
    assert!(optional.status.success(), "{optional:?}");
    let report: serde_json::Value = serde_json::from_slice(&optional.stdout).unwrap();
    assert_eq!(report["services_probed"], false);
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "storage.free_space_reserve_mb"
                && check["status"] == "available"
                && check["available_bytes"].as_u64().unwrap()
                    >= check["required_bytes"].as_u64().unwrap())
    );
    assert_eq!(
        report["local_resources"]["missing"][0],
        "dictionary.schema_path"
    );
    assert!(
        report["local_resources"]["required_missing"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let required = cli()
        .env("PATH", "/does/not/exist")
        .args([
            "--set",
            &state,
            "--set",
            &ocr,
            "--set",
            "browser.enabled=true",
            "--set",
            "browser.executable=lab-browser-helper",
            "doctor",
            "--local",
        ])
        .output()
        .unwrap();
    assert_eq!(required.status.code(), Some(3), "{required:?}");
    assert!(required.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&required.stdout).unwrap();
    assert_eq!(report["services_probed"], false);
    assert_eq!(
        report["local_resources"]["required_missing"][0],
        "browser.executable"
    );
    assert!(!root.exists());
}

#[test]
fn local_doctor_requires_tesseract_candidate_only_for_selected_engine() {
    let required = cli()
        .env("PATH", "/does/not/exist")
        .args(["doctor", "--local"])
        .output()
        .unwrap();
    assert_eq!(required.status.code(), Some(3), "{required:?}");
    let report: serde_json::Value = serde_json::from_slice(&required.stdout).unwrap();
    assert_eq!(
        report["local_resources"]["required_missing"][0],
        "ocr.executable"
    );
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "ocr.executable"
                && check["status"] == "missing"
                && check["required"] == true)
    );

    let optional = cli()
        .env("PATH", "/does/not/exist")
        .args([
            "--set",
            "ocr.engine=ollama",
            "--set",
            "llm.vision_model=vision:model",
            "doctor",
            "--local",
        ])
        .output()
        .unwrap();
    assert!(optional.status.success(), "{optional:?}");
    let report: serde_json::Value = serde_json::from_slice(&optional.stdout).unwrap();
    assert!(
        report["local_resources"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["key"] == "ocr.executable"
                && check["status"] == "missing"
                && check["required"] == false)
    );
}

#[test]
fn plan_diff_loads_exact_immutable_revisions_and_reports_unavailable_live_transport() {
    use linguist_core::records::{PlanRevision, ResolvedSettings};
    use std::collections::BTreeMap;
    let root = std::env::temp_dir().join(format!("lab-diff-cli-{}", uuid::Uuid::new_v4()));
    let mut store = linguist_store::Store::open(&root).unwrap();
    let mut plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values: BTreeMap::from([("input.max_file_mb".into(), serde_json::json!(1))]),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "first".into(),
        selection: None,
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
            "--set",
            "anki.endpoint=http://127.0.0.1:1",
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
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "fixture".into(),
        selection: None,
        rendered: vec![linguist_core::render::render(&doc, &BTreeMap::new()).unwrap()],
        documents: vec![doc],
        review_decisions: vec![],
    };
    let mut store = linguist_store::Store::open(&root).unwrap();
    let digest = store.publish_revision(&plan).unwrap();
    drop(store);
    // Enrichment rejects mismatched identities before provider or settings lookup.
    let state = format!("storage.state_dir={}", root.display());
    let id_for_enrichment = plan.id.to_string();
    let out = cli()
        .args([
            "--set",
            &state,
            "plans",
            "enrich",
            &id_for_enrichment,
            "--base-revision",
            "1",
            "--digest",
            "wrong",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
    // Legacy/incomplete frozen settings return a structured error rather than panic.
    let out = cli()
        .args([
            "--set",
            &state,
            "plans",
            "enrich",
            &id_for_enrichment,
            "--base-revision",
            "1",
            "--digest",
            &digest,
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("DICTIONARY_SETTING_MISSING"));
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
        .args([
            "--set",
            &setting,
            "--set",
            "anki.endpoint=http://127.0.0.1:1",
            "plans",
            "validate",
            &id_text,
            "--live",
        ])
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
fn inline_vocabulary_and_grammar_add_create_recoverable_plans() {
    for grammar in [false, true] {
        let root = std::env::temp_dir().join(format!("lab-inline-add-{}", uuid::Uuid::new_v4()));
        let setting = format!("storage.state_dir={}/state", root.display());
        let mut cmd = cli();
        cmd.args([
            "--set",
            &setting,
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            "--set",
            "kanji.enabled=false",
        ]);
        if grammar {
            cmd.args([
                "grammar",
                "add",
                "--pattern",
                "〜ために",
                "--meaning",
                "in order to",
                "--formation",
                "verb dictionary form + ために",
                "--use-key",
                "purpose",
                "--target-language",
                "ja",
                "--explanation-language",
                "en",
                "--recognition-prompt",
                "What purpose does this express?",
                "--example-sentence",
                "学ぶために行く。",
                "--example-translation",
                "I go to learn.",
            ]);
        } else {
            cmd.args([
                "vocab",
                "add",
                "--expression",
                "eat",
                "--meaning",
                "consume food",
                "--sense-key",
                "food",
                "--target-language",
                "en",
                "--tag",
                "study",
            ]);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["ready"], true);
        assert_eq!(value["apply_eligible"], false);
        let store = linguist_store::Store::read_only(&root.join("state")).unwrap();
        let id = uuid::Uuid::parse_str(value["plan_id"].as_str().unwrap()).unwrap();
        let plan = store.revision(id, 1).unwrap();
        assert_eq!(plan.documents[0].sources[0].kind, "authored_inline_v1");
        let bytes = store
            .asset(
                value["original_input_digest"].as_str().unwrap(),
                1024 * 1024,
            )
            .unwrap();
        let archived: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            archived["kind"],
            if grammar { "grammar" } else { "vocabulary" }
        );
        if grammar {
            assert_eq!(archived["body"]["examples"][0]["provenance"], "user");
        } else {
            assert_eq!(archived["tags"][0], "study");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn inline_add_rejects_mixed_modes_incomplete_pairs_and_wrong_kind_before_state() {
    let root = std::env::temp_dir().join(format!("lab-inline-invalid-{}", uuid::Uuid::new_v4()));
    let setting = format!("storage.state_dir={}/state", root.display());
    let common = [
        "--set",
        setting.as_str(),
        "--set",
        "llm.enabled=false",
        "--set",
        "dictionary.provider=authored",
        "--set",
        "images.search_when_missing=false",
    ];
    for args in [
        vec!["vocab", "add"],
        vec![
            "vocab",
            "add",
            "--document",
            "/missing.json",
            "--expression",
            "eat",
        ],
        vec![
            "grammar",
            "add",
            "--expression",
            "eat",
            "--meaning",
            "consume",
            "--target-language",
            "en",
        ],
        vec![
            "vocab",
            "add",
            "--expression",
            "eat",
            "--meaning",
            "consume",
            "--sense-key",
            "food",
            "--target-language",
            "en",
            "--example-sentence",
            "I eat.",
        ],
        vec!["vocab", "add", "--format", "csv"],
    ] {
        let out = cli().args(common).args(args).output().unwrap();
        assert!(!out.status.success(), "{out:?}");
        assert!(!root.join("state").exists());
    }
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
fn explicit_jsonl_add_publishes_one_ordered_plan_and_rejects_bad_second_line() {
    let root = std::env::temp_dir().join(format!("lab-jsonl-add-{}", uuid::Uuid::new_v4()));
    let first = b"{\"schema_version\":2,\"kind\":\"vocabulary\",\"target_language\":\"en\",\"body\":{\"expression\":\"eat\",\"meaning\":\"consume food\",\"sense_key\":\"food\"}}\n";
    let second = b"{\"schema_version\":2,\"kind\":\"vocabulary\",\"target_language\":\"en\",\"body\":{\"expression\":\"drink\",\"meaning\":\"consume liquid\",\"sense_key\":\"liquid\"}}\n";
    let make_command = || {
        let mut command = cli();
        command
            .arg("--set")
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
                "--format",
                "jsonl",
            ]);
        command
    };
    let bad = piped(make_command(), &[first.as_slice(), b"{broken}\n"].concat());
    assert!(!bad.status.success());
    assert!(!root.exists());
    let output = piped(
        make_command(),
        &[first.as_slice(), second.as_slice()].concat(),
    );
    assert!(output.status.success(), "{output:?}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["ready"], true);
    assert_eq!(result["apply_eligible"], false);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan_id = uuid::Uuid::parse_str(result["plan_id"].as_str().unwrap()).unwrap();
    let plan = store.revision(plan_id, 1).unwrap();
    assert_eq!(plan.documents.len(), 2);
    assert_eq!(
        plan.documents[0].sources[0].fields["authored_input"].as_bytes(),
        first
    );
    assert_eq!(
        plan.documents[1].sources[0].fields["authored_input"].as_bytes(),
        second
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn explicit_csv_prepares_vocabulary_and_grammar_plans() {
    let root = std::env::temp_dir().join(format!("lab-csv-add-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let bytes = b"expression,meaning,target_language,sense_key\neat,consume food,en,food\ndrink,consume liquid,en,liquid\n";
    let make_command = |kind| {
        let mut command = cli();
        command.args([
            "--set",
            &state,
            "--set",
            "llm.enabled=false",
            "--set",
            "dictionary.provider=authored",
            "--set",
            "images.search_when_missing=false",
            kind,
            "add",
            "--document",
            "-",
            "--format",
            "csv",
        ]);
        command
    };
    let grammar = piped(make_command("grammar"), bytes);
    assert!(!grammar.status.success());
    assert!(String::from_utf8_lossy(&grammar.stderr).contains("INPUT_CSV_HEADER_INVALID"));
    assert!(!root.exists());
    let output = piped(make_command("vocab"), bytes);
    assert!(output.status.success(), "{output:?}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["ready"], true);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan_id = uuid::Uuid::parse_str(result["plan_id"].as_str().unwrap()).unwrap();
    let plan = store.revision(plan_id, 1).unwrap();
    assert_eq!(plan.documents.len(), 2);
    assert_eq!(plan.documents[0].sources[0].kind, "authored_csv_v1");
    assert_eq!(
        store
            .asset(&plan.documents[1].sources[0].digest, 10000)
            .unwrap(),
        bytes
    );
    drop(store);
    let grammar_csv = b"pattern,meaning,formation,use_key,target_language,recognition_prompt,example_sentence,example_translation\nif,conditional,if + clause,condition,en,What relation is expressed?,If it rains we stay,We stay when it rains\n";
    let grammar = piped(make_command("grammar"), grammar_csv);
    assert!(grammar.status.success(), "{grammar:?}");
    let result: serde_json::Value = serde_json::from_slice(&grammar.stdout).unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan_id = uuid::Uuid::parse_str(result["plan_id"].as_str().unwrap()).unwrap();
    let plan = store.revision(plan_id, 1).unwrap();
    assert!(matches!(
        plan.documents[0].content,
        linguist_core::LearningContent::Grammar(_)
    ));
    assert_eq!(
        store
            .asset(&plan.documents[0].sources[0].digest, 10000)
            .unwrap(),
        grammar_csv
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn duplicate_candidates_command_reads_managed_note_without_changing_plan() {
    use std::io::{BufRead, Read, Write};
    let root =
        std::env::temp_dir().join(format!("lab-duplicate-candidates-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let input = b"{\"schema_version\":2,\"kind\":\"vocabulary\",\"target_language\":\"en\",\"body\":{\"expression\":\"eat\",\"meaning\":\"consume food\",\"sense_key\":\"food\"}}";
    let prepared = piped(
        {
            let mut command = cli();
            command.args([
                "--set",
                &state,
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
            command
        },
        input,
    );
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let plan_id = uuid::Uuid::parse_str(prepared["plan_id"].as_str().unwrap()).unwrap();
    let item_id = uuid::Uuid::parse_str(prepared["document_id"].as_str().unwrap()).unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let before = store.revision(plan_id, 1).unwrap();
    let fields = before.rendered[0]
        .fields
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::json!({"value":value})))
        .collect::<serde_json::Map<_, _>>();
    drop(store);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut actions = Vec::new();
        for _ in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let action = request["action"].as_str().unwrap().to_owned();
            let result = match action.as_str() {
                "getActiveProfile" => serde_json::json!("Fixture"),
                "findNotes" => {
                    assert_eq!(
                        request["params"]["query"],
                        "note:\"Linguist Vocabulary v2\" Expression:\"eat\""
                    );
                    serde_json::json!([123])
                }
                "notesInfo" => {
                    assert_eq!(request["params"]["notes"], serde_json::json!([123]));
                    serde_json::json!([{"noteId":123,"modelName":"Linguist Vocabulary v2","fields":fields}])
                }
                _ => panic!("unexpected action: {action}"),
            };
            actions.push(action);
            let body = serde_json::json!({"result":result,"error":null}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        actions
    });
    let output = cli()
        .args([
            "--set",
            &state,
            "--set",
            &endpoint,
            "plans",
            "duplicate-candidates",
            &plan_id.to_string(),
            "--item-id",
            &item_id.to_string(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        server.join().unwrap(),
        [
            "getActiveProfile",
            "findNotes",
            "getActiveProfile",
            "notesInfo",
            "getActiveProfile"
        ]
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["candidates"][0]["note_id"], "123");
    assert_eq!(report["candidates"][0]["compared_fields_match"], true);
    assert_eq!(report["collection_duplicate_check_complete"], false);
    assert_eq!(report["semantic_identity_verified"], false);
    assert_eq!(report["apply_eligible"], false);
    assert_eq!(
        linguist_store::Store::read_only(&root)
            .unwrap()
            .revision(plan_id, 1)
            .unwrap(),
        before
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn plan_generate_requires_explicit_settings_and_enabled_engine_before_inference() {
    let root = std::env::temp_dir().join(format!("lab-generation-cli-{}", uuid::Uuid::new_v4()));
    let state = format!("storage.state_dir={}", root.display());
    let input = b"{\"schema_version\":2,\"kind\":\"vocabulary\",\"target_language\":\"en\",\"body\":{\"expression\":\"eat\",\"meaning\":\"consume food\",\"sense_key\":\"food\"}}";
    let prepared = piped(
        {
            let mut command = cli();
            command.args([
                "--set",
                &state,
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
            command
        },
        input,
    );
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let plan_id = prepared["plan_id"].as_str().unwrap();
    let item_id = prepared["document_id"].as_str().unwrap();
    let digest = prepared["digest"].as_str().unwrap();
    let command = |ack: bool| {
        let mut command = cli();
        command.args([
            "--set",
            &state,
            "--set",
            "llm.enabled=false",
            "plans",
            "generate",
            plan_id,
            "--item-id",
            item_id,
            "--base-revision",
            "1",
            "--digest",
            digest,
        ]);
        if ack {
            command.arg("--use-current-settings");
        }
        command.output().unwrap()
    };
    let no_ack = command(false);
    assert!(!no_ack.status.success());
    assert!(
        String::from_utf8_lossy(&no_ack.stderr)
            .contains("GENERATION_CURRENT_SETTINGS_ACK_REQUIRED")
    );
    let disabled = command(true);
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("GENERATION_DISABLED"));
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(
        store
            .latest_revision(uuid::Uuid::parse_str(plan_id).unwrap())
            .unwrap(),
        1
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
            2,
            "job",
        ),
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
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            1,
            "query_limit",
        ),
        (
            "vocab",
            "english_vocab",
            r#"{"expression":"Word","meaning":"Meaning","sense_key":"Key"}"#,
            "note_id",
            2,
            "query_large",
        ),
        (
            "grammar",
            "japanese_grammar",
            r#"{"pattern":"Pattern","meaning":"Meaning","formation":"Formation","use_key":"Key"}"#,
            "note_id",
            1,
            "deck_limit",
        ),
    ] {
        let root = std::env::temp_dir().join(format!("lab-cli-revamp-{}", uuid::Uuid::new_v4()));
        let state_root = root.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut actions = Vec::new();
            let mut held_first = None;
            let mut first_was_held = false;
            let mut second_card_reads = 0;
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
                        let expected = if mode.starts_with("query") || mode == "job" {
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
                        if mode == "job" && id == 457 {
                            second_card_reads += 1;
                        }
                        serde_json::json!([{"cardId":id,"note":id-333,"reps":5,"due":10}])
                    }
                    _ => panic!("unexpected action {action}"),
                };
                actions.push(action.to_owned());
                let body = serde_json::json!({"result":value,"error":null}).to_string();
                if mode == "job"
                    && action == "notesInfo"
                    && request["params"]["notes"][0] == 123
                    && !first_was_held
                {
                    first_was_held = true;
                    held_first = Some((stream, body));
                } else {
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
                if held_first.is_some() && second_card_reads == 2 && action == "getActiveProfile" {
                    // Force the second item to commit before the first capture can continue.
                    let store = linguist_store::Store::read_only(&state_root).unwrap();
                    let job = store.list_preparation_jobs(None, 1).unwrap()[0].id;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while store.preparation_items(job, 1, 1).unwrap()[0].state != "captured" {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "second capture never became durable"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    let (mut stream, body) = held_first.take().unwrap();
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
            }
            actions
        });
        let mut out = cli()
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
            .arg("--set")
            .arg(if mode == "query_large" {
                "selection.max_notes=1"
            } else {
                "selection.max_notes=1000"
            })
            .args(if mode == "job" {
                ["jobs", "create"]
            } else {
                [command, "revamp"]
            })
            .args(if mode.starts_with("query") || mode == "job" {
                vec!["--query", "tag:source"]
            } else if mode.starts_with("deck") {
                vec!["--deck", "Legacy \"cards\""]
            } else if count == 1 {
                vec!["--note-id", "123"]
            } else {
                vec!["--note-id", "124", "--note-id", "123"]
            })
            .args(if mode == "query_limit" || mode == "deck_limit" {
                vec!["--limit", "1"]
            } else if mode == "query_large" {
                vec!["--limit", "2"]
            } else {
                vec![]
            })
            .output()
            .unwrap();
        let job_id = if mode == "job" {
            assert!(out.status.success(), "{out:?}");
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let id = value["job_id"].as_str().unwrap().to_owned();
            out = cli()
                .args([
                    "--set",
                    &format!("storage.state_dir={}", root.display()),
                    "jobs",
                    "run",
                    &id,
                ])
                .output()
                .unwrap();
            Some(id)
        } else {
            None
        };
        let actions = server.join().unwrap();
        assert_eq!(
            actions.len(),
            28 * count + if mode == "ids" { 0 } else { 3 }
        );
        if let Some(id) = job_id {
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| action.as_str() == "findNotes")
                    .count(),
                1
            );
            assert_eq!(out.status.code(), Some(4), "{out:?}");
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["captured_this_run"], 2);
            assert_eq!(value["plan_published"], true);
            assert_eq!(value["plan"]["id"], id);
            let store = linguist_store::Store::read_only(&root).unwrap();
            let job = id.parse().unwrap();
            let items = store.preparation_items(job, 0, 10).unwrap();
            assert!(
                items
                    .iter()
                    .all(|item| item.state == "captured" && item.attempt == 1)
            );
            assert_eq!(store.preparation_events(job, 0, 10).unwrap().len(), 4);
            let events = store.preparation_events(job, 0, 10).unwrap();
            assert_eq!(events[2].event.item_id, items[1].item_id);
            assert_eq!(events[3].event.item_id, items[0].item_id);
            assert_eq!(store.list_revisions(10).unwrap().len(), 1);
            let plan = store.revision(job, 1).unwrap();
            assert_eq!(
                plan.selection.as_ref().unwrap().selector,
                linguist_core::records::SelectionInput::Query("tag:source".into())
            );
            assert_eq!(
                plan.selection.as_ref().unwrap().selected_note_ids,
                ["123", "124"]
            );
            assert_eq!(plan.documents.len(), 2);
            assert!(plan.binding.is_none());
            assert!(plan.rendered.is_empty());
            let repeat = cli()
                .args([
                    "--set",
                    &format!("storage.state_dir={}", root.display()),
                    "jobs",
                    "run",
                    &id,
                ])
                .output()
                .unwrap();
            assert_eq!(repeat.status.code(), Some(4), "{repeat:?}");
            let repeat: serde_json::Value = serde_json::from_slice(&repeat.stdout).unwrap();
            assert_eq!(repeat["captured_this_run"], 0);
            assert_eq!(repeat["plan"], value["plan"]);
            let audited = cli()
                .args([
                    "--set",
                    &format!("storage.state_dir={}", root.display()),
                    "jobs",
                    "audit",
                    &id,
                ])
                .output()
                .unwrap();
            assert!(audited.status.success(), "{audited:?}");
            let audited: serde_json::Value = serde_json::from_slice(&audited.stdout).unwrap();
            assert_eq!(audited["checkpoints"].as_array().unwrap().len(), 4);
            assert_eq!(audited["checkpoints"][2]["detail"]["state"], "captured");
            assert!(audited["checkpoints"][2]["detail"]["document"].is_null());
            assert!(
                !audited["checkpoints"][2]["detail"]["asset_digests"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(audited["source_plan"]["id"], id);
            assert_eq!(audited["source_plan"]["checkpoint_binding_verified"], false);
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
            continue;
        }
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
        let receipt = plan.selection.as_ref().unwrap();
        assert_eq!(receipt.schema_version, 1);
        assert_eq!(receipt.purpose, purpose);
        assert_eq!(receipt.order, order);
        assert_eq!(
            receipt.max_notes,
            if mode == "query_large" { 1 } else { 1000 }
        );
        assert_eq!(
            receipt.command_limit,
            if mode == "query_limit" || mode == "deck_limit" {
                Some(1)
            } else if mode == "query_large" {
                Some(2)
            } else {
                None
            }
        );
        let matched = if mode.starts_with("query") || mode.starts_with("deck") {
            vec!["123", "124"]
        } else if count == 1 {
            vec!["123"]
        } else if mode == "ids" {
            vec!["124", "123"]
        } else {
            vec!["123", "124"]
        };
        assert_eq!(receipt.matched_note_ids, matched);
        let selected = if count == 1 {
            vec!["123"]
        } else if order == "input" {
            vec!["124", "123"]
        } else {
            vec!["123", "124"]
        };
        assert_eq!(receipt.selected_note_ids, selected);
        match (&receipt.selector, mode) {
            (linguist_core::records::SelectionInput::NoteIds(ids), "ids") => {
                assert_eq!(ids, &receipt.matched_note_ids)
            }
            (linguist_core::records::SelectionInput::Query(query), mode)
                if mode.starts_with("query") =>
            {
                assert_eq!(query, "tag:source")
            }
            (linguist_core::records::SelectionInput::Deck { name, query }, mode)
                if mode.starts_with("deck") =>
            {
                assert_eq!(name, "Legacy \"cards\"");
                assert_eq!(*query, linguist_anki::deck_query(name).unwrap());
            }
            _ => panic!("wrong selection receipt"),
        }
        let mut forged = plan.clone();
        forged
            .selection
            .as_mut()
            .unwrap()
            .selected_note_ids
            .push("125".into());
        assert_eq!(
            forged.approval_digest().unwrap_err().to_string(),
            "PLAN_SELECTION_INVALID"
        );
        if mode.starts_with("query") {
            let mut changed = plan.clone();
            changed.selection.as_mut().unwrap().selector =
                linguist_core::records::SelectionInput::Query("tag:other".into());
            assert_ne!(
                plan.approval_digest().unwrap(),
                changed.approval_digest().unwrap()
            );
        }
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
        if command == "grammar" {
            let document = &plan.documents[0];
            let linguist_core::LearningContent::Grammar(grammar) = &document.content else {
                panic!()
            };
            let mut first = grammar.clone();
            first.pattern = "なら".into();
            first.use_key = "conditional".into();
            let mut second = grammar.clone();
            second.pattern = "ので".into();
            second.use_key = "reason".into();
            let request = linguist_application::grammar::SplitRequest {
                schema_version: 2,
                base_revision: 1,
                base_digest: plan.approval_digest().unwrap(),
                document_id: document.id,
                input_digest: document.semantic_digest().unwrap(),
                actor: "reviewer".into(),
                anchor_index: 0,
                units: vec![first, second],
            };
            let file = root.join("split.json");
            std::fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
            let state = format!("storage.state_dir={}", root.display());
            let plan_id = plan.id.to_string();
            let args = [
                "--set",
                &state,
                "plans",
                "split-grammar",
                &plan_id,
                "--request",
                file.to_str().unwrap(),
            ];
            let out = cli().args(args).output().unwrap();
            assert_eq!(out.status.code(), Some(4), "{out:?}");
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["revision"], 2);
            assert_eq!(value["writes_enabled"], false);
            assert_eq!(
                value["grammar_groups"][0]["anchor_document"],
                document.id.to_string()
            );
            let stale = cli().args(args).output().unwrap();
            assert_eq!(stale.status.code(), Some(5), "{stale:?}");
            let store = linguist_store::Store::read_only(&root).unwrap();
            assert_eq!(store.revision(plan.id, 1).unwrap(), plan);
            assert_eq!(
                store.revision(plan.id, 2).unwrap().documents.len(),
                plan.documents.len() + 1
            );
            drop(store);
        }
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

#[test]
fn cue_resolution_cli_repairs_content_and_rejects_stale_replay() {
    let root = std::env::temp_dir().join(format!("lab-cue-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let input = root.join("input.json");
    std::fs::write(&input, br#"{"schema_version":2,"kind":"vocabulary","target_language":"en","requested_tasks":["comprehension","production"],"body":{"expression":"eat","meaning":"consume food","sense_key":"food"}}"#).unwrap();
    let state = format!("storage.state_dir={}/state", root.display());
    let out = cli()
        .args([
            "--set",
            &state,
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
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let prepared: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = uuid::Uuid::parse_str(prepared["plan_id"].as_str().unwrap()).unwrap();
    let store = linguist_store::Store::read_only(&root.join("state")).unwrap();
    let base = store.revision(id, 1).unwrap();
    let doc = &base.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|issue| issue.code == "MISSING_CUE")
        .unwrap();
    let request = linguist_core::review::ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: base.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "author".into(),
        choice: linguist_core::records::ReviewChoice::Cue {
            task: linguist_core::Task::Production,
            text: "Name the verb for consuming food.".into(),
        },
    };
    let compact = cli()
        .args([
            "--set",
            &state,
            "plans",
            "show",
            &id.to_string(),
            "--issues-only",
            "--item",
            &doc.id.to_string(),
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(compact.status.success(), "{compact:?}");
    let page: serde_json::Value = serde_json::from_slice(&compact.stdout).unwrap();
    assert_eq!(page["issues"].as_array().unwrap().len(), 1);
    assert_eq!(
        page["issues"][0]["request_identity"]["base_digest"],
        request.base_digest
    );
    assert_eq!(
        page["issues"][0]["request_identity"]["input_digest"],
        request.input_digest
    );
    assert_eq!(
        page["issues"][0]["templates"][0]["choice"]["decision"],
        "cue"
    );
    assert_eq!(
        page["issues"][0]["templates"][0]["choice"]["value"]["task"],
        "production"
    );
    assert_eq!(page["issues"][0]["actor_required"], true);
    assert_eq!(page["archives_included"], false);
    assert!(!String::from_utf8_lossy(&compact.stdout).contains("authored_input"));
    let focused = cli()
        .args([
            "--set",
            &state,
            "plans",
            "show",
            &id.to_string(),
            "--item",
            &doc.id.to_string(),
        ])
        .output()
        .unwrap();
    assert!(focused.status.success(), "{focused:?}");
    let focused: serde_json::Value = serde_json::from_slice(&focused.stdout).unwrap();
    assert_eq!(focused["document"]["id"], doc.id.to_string());
    assert_eq!(focused["archives_included"], true);
    assert_eq!(
        focused["document"]["sources"][0]["fields"]["authored_input"],
        std::fs::read_to_string(&input).unwrap()
    );
    let missing = cli()
        .args([
            "--set",
            &state,
            "plans",
            "show",
            &id.to_string(),
            "--issues-only",
            "--item",
            &uuid::Uuid::new_v4().to_string(),
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    let empty = cli()
        .args([
            "--set",
            &state,
            "plans",
            "show",
            &id.to_string(),
            "--issues-only",
            "--after-index",
            "1",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(empty.status.success(), "{empty:?}");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap()["issues"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let file = root.join("decision.json");
    std::fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
    drop(store);
    let id = id.to_string();
    let args = [
        "--set",
        &state,
        "plans",
        "resolve",
        &id,
        &request.issue_id,
        "--decision",
        file.to_str().unwrap(),
    ];
    let out = cli().args(args).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["revision"], 2);
    assert_eq!(result["ready"], true);
    let stale = cli().args(args).output().unwrap();
    assert_eq!(stale.status.code(), Some(5), "{stale:?}");
    let store = linguist_store::Store::read_only(&root.join("state")).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    let child = store.revision(base.id, 2).unwrap();
    assert_eq!(
        child.documents[0].requested_tasks,
        base.documents[0].requested_tasks
    );
    assert_eq!(
        child.rendered[0].fields["ProductionPrompt"],
        "Name the verb for consuming food."
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
