use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::process::Command;

#[test]
fn map_checks_live_manifest_then_unmap_edits_local_config_only() {
    let root = std::env::temp_dir().join(format!("lab-deck-map-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let fields = root.join("fields.json");
    let tasks = root.join("tasks.json");
    std::fs::write(
        &fields,
        br#"{"expression":"Expression","meaning":"Meaning","picture":"Picture"}"#,
    )
    .unwrap();
    std::fs::write(&tasks, br#"{"0":"comprehension","1":"production"}"#).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    std::fs::write(
        &config,
        format!("[config]\nversion=2\n[anki]\nendpoint='{endpoint}'\n"),
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        let mut actions = Vec::new();
        for _ in 0..16 {
            let (mut stream, _) = listener.accept().unwrap();
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
                    length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let action = request["action"].as_str().unwrap();
            actions.push(action.to_owned());
            let result = match action {
                "getActiveProfile" => json!("Fixture"),
                "deckNamesAndIds" => json!({"日本語::語彙 \"A\"":11,"Output":12}),
                "getDeckConfig" => json!({"dyn":false}),
                "modelNamesAndIds" => json!({"Picture Words":21}),
                "modelFieldNames" => json!(["Expression", "Meaning", "Picture", "Private"]),
                "modelTemplates" => {
                    json!({"Recognition":{"Front":"{{Expression}}","Back":"{{Meaning}}"},"Production":{"Front":"{{Meaning}}","Back":"{{Expression}}"}})
                }
                "modelStyling" => json!({"css":"body{}"}),
                "findModelsByName" => json!([{"id":21,"name":"Picture Words","css":"body{}",
                    "flds":[{"name":"Expression","ord":0},{"name":"Meaning","ord":1},
                            {"name":"Picture","ord":2},{"name":"Private","ord":3}],
                    "tmpls":[{"name":"Recognition","ord":0,"qfmt":"{{Expression}}","afmt":"{{Meaning}}"},
                             {"name":"Production","ord":1,"qfmt":"{{Meaning}}","afmt":"{{Expression}}"}]}]),
                _ => panic!("unexpected action {action}"),
            };
            let body = json!({"result":result,"error":null}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        actions
    });
    let binary = env!("CARGO_BIN_EXE_linguist-anki-bridge");
    let mapped = Command::new(binary)
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--config",
            config.to_str().unwrap(),
            "decks",
            "map",
            "japanese_vocab",
            "--source-deck",
            "日本語::語彙 \"A\"",
            "--target-deck",
            "Output",
            "--source-model",
            "Picture Words",
            "--fields",
            fields.to_str().unwrap(),
            "--task-map",
            tasks.to_str().unwrap(),
            "--ocr-language",
            "jpn",
        ])
        .output()
        .unwrap();
    assert!(
        mapped.status.success(),
        "{}",
        String::from_utf8_lossy(&mapped.stderr)
    );
    let mapped: Value = serde_json::from_slice(&mapped.stdout).unwrap();
    assert_eq!(mapped["source_deck"]["name"], "日本語::語彙 \"A\"");
    assert_eq!(mapped["unmapped_fields"], json!(["Private"]));
    assert_eq!(mapped["writes_enabled"], false);
    assert_eq!(mapped["filtered_deck_verified"], true);
    let actions = server.join().unwrap();
    assert!(actions.iter().all(|name| matches!(
        name.as_str(),
        "getActiveProfile"
            | "deckNamesAndIds"
            | "getDeckConfig"
            | "modelNamesAndIds"
            | "modelFieldNames"
            | "modelTemplates"
            | "modelStyling"
            | "findModelsByName"
    )));
    let saved =
        linguist_config::ConfigFile::read(&config, &linguist_config::Registry::builtin()).unwrap();
    assert_eq!(
        saved.values["purposes.japanese_vocab.source_model"],
        "Picture Words"
    );
    assert_eq!(
        saved.values["purposes.japanese_vocab.target_deck"],
        "Output"
    );
    let unmapped = Command::new(binary)
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--config",
            config.to_str().unwrap(),
            "decks",
            "unmap",
            "japanese_vocab",
        ])
        .output()
        .unwrap();
    assert!(
        unmapped.status.success(),
        "{}",
        String::from_utf8_lossy(&unmapped.stderr)
    );
    let unmapped: Value = serde_json::from_slice(&unmapped.stdout).unwrap();
    assert_eq!(unmapped["config"]["changed"], true);
    let again = Command::new(binary)
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--config",
            config.to_str().unwrap(),
            "decks",
            "unmap",
            "japanese_vocab",
        ])
        .output()
        .unwrap();
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["config"]["changed"], false);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deck_show_reports_mixed_models_and_stored_source_mapping() {
    let root = std::env::temp_dir().join(format!("lab-deck-show-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&config, format!(
        "[config]\nversion=2\n[anki]\nendpoint='http://{}'\n[purposes.japanese_vocab]\nsource_deck='語彙 \"quotes\"'\nsource_model='Picture Words'\n",
        listener.local_addr().unwrap()
    )).unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..13 {
            let (mut stream, _) = listener.accept().unwrap();
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
                    length = line.split_once(':').unwrap().1.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let result = match request["action"].as_str().unwrap() {
                "getActiveProfile" => json!("Fixture"),
                "deckNamesAndIds" => json!({"語彙 \"quotes\"":11}),
                "findNotes" => json!([100, 101]),
                "findCards" => json!([200, 201, 202]),
                "notesInfo" => json!([
                    {"noteId":100,"modelName":"Picture Words","cards":[200]},
                    {"noteId":101,"modelName":"Basic","cards":[201,202]}
                ]),
                action => panic!("unexpected action {action}"),
            };
            let body = json!({"result":result,"error":null}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let output = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--config",
            config.to_str().unwrap(),
            "decks",
            "show",
            "語彙 \"quotes\"",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["counts"]["note_count"], 2);
    assert_eq!(report["counts"]["card_count"], 3);
    assert_eq!(report["models_by_note_count"]["Basic"], 1);
    assert_eq!(report["models_by_note_count"]["Picture Words"], 1);
    assert_eq!(report["mixed_models"], true);
    assert_eq!(report["purpose_mappings"][0]["role"], "source");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn filtered_source_deck_cannot_publish_mapping() {
    let root = std::env::temp_dir().join(format!("lab-filtered-map-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let fields = root.join("fields.json");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let original = format!(
        "[config]\nversion=2\n[anki]\nendpoint='http://{}'\n",
        listener.local_addr().unwrap()
    );
    std::fs::write(&config, &original).unwrap();
    std::fs::write(
        &fields,
        br#"{"expression":"Expression","meaning":"Meaning"}"#,
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
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
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let result = match request["action"].as_str().unwrap() {
                "getActiveProfile" => json!("Fixture"),
                "deckNamesAndIds" => json!({"Filtered":11}),
                "getDeckConfig" => json!({"dyn":1}),
                action => panic!("unexpected action {action}"),
            };
            let body = json!({"result":result,"error":null}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let out = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--config",
            config.to_str().unwrap(),
            "decks",
            "map",
            "japanese_vocab",
            "--source-deck",
            "Filtered",
            "--source-model",
            "Basic",
            "--fields",
            fields.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("SOURCE_MAPPING_FILTERED_DECK_UNSUPPORTED")
    );
    server.join().unwrap();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deck_pages_bind_cursor_to_exact_inventory() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("anki.endpoint=http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
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
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let result = match request["action"].as_str().unwrap() {
                "getActiveProfile" => json!("Fixture"),
                "deckNamesAndIds" => json!({"語彙 A":1,"語彙 B":2,"Other":3}),
                action => panic!("unexpected action {action}"),
            };
            let body = json!({"result":result,"error":null}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let binary = env!("CARGO_BIN_EXE_linguist-anki-bridge");
    let first = Command::new(binary)
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--set",
            &endpoint,
            "decks",
            "list",
            "--name-contains",
            "語彙",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(first.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["total"], 2);
    assert_eq!(first["decks"][0]["name"], "語彙 A");
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = Command::new(binary)
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .args([
            "--output",
            "json",
            "--set",
            &endpoint,
            "decks",
            "list",
            "--name-contains",
            "語彙",
            "--limit",
            "1",
            "--cursor",
            cursor,
        ])
        .output()
        .unwrap();
    assert!(second.status.success());
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["decks"][0]["name"], "語彙 B");
    assert!(second["next_cursor"].is_null());
    server.join().unwrap();
}

#[test]
fn unmap_without_default_config_is_no_op() {
    let root = std::env::temp_dir().join(format!("lab-unmap-absent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
        .env("HOME", "/tmp/lab-cli-test-no-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAB_CONFIG")
        .env("XDG_CONFIG_HOME", &root)
        .env_remove("LAB_CONFIG")
        .args(["--output", "json", "decks", "unmap", "japanese_vocab"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["config"]["changed"], false);
    assert!(!root.join("linguist-anki-bridge").exists());
    std::fs::remove_dir_all(root).unwrap();
}
