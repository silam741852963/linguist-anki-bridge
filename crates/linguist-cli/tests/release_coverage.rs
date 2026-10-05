//! WP-15 coverage for operations that had no command-level test: OP-07
//! `config unset`, OP-15 `models list`, OP-16 `models inspect` and OP-20
//! `notes count`. Collection reads go to an in-process fake AnkiConnect that
//! records every action, so each test also proves the command stays read-only.
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    process::Command,
    sync::{Arc, Mutex},
};

const BIN: &str = env!("CARGO_BIN_EXE_linguist-anki-bridge");
const READS: [&str; 11] = [
    "version",
    "getActiveProfile",
    "apiReflect",
    "deckNamesAndIds",
    "modelNamesAndIds",
    "modelFieldNames",
    "modelTemplates",
    "modelStyling",
    "findModelsByName",
    "findNotes",
    "findCards",
];

/// Serves AnkiConnect v6 envelopes from `respond` until the test ends.
fn fake_anki(respond: fn(&str, &Value) -> Value) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let actions = Arc::new(Mutex::new(Vec::new()));
    let seen = actions.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
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
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            let request: Value = serde_json::from_slice(&body).unwrap();
            let action = request["action"].as_str().unwrap().to_owned();
            seen.lock().unwrap().push(action.clone());
            let body =
                json!({"result": respond(&action, &request["params"]), "error": null}).to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (endpoint, actions)
}

fn cli(home: &std::path::Path, endpoint: &str) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .args([
            "--output",
            "json",
            "--set",
            &format!("anki.endpoint={endpoint}"),
        ]);
    command
}

fn home(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("lab-release-cov-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn models(action: &str, params: &Value) -> Value {
    let vocabulary = linguist_core::model::vocabulary();
    match action {
        "getActiveProfile" => json!("Disposable"),
        "modelNamesAndIds" => json!({"Basic": 1, "Linguist Vocabulary v2": 2}),
        "modelFieldNames" if params["modelName"] == "Basic" => json!(["Front", "Back"]),
        "modelFieldNames" => json!(vocabulary.fields),
        "modelTemplates" if params["modelName"] == "Basic" => {
            json!({"Card 1": {"Front": "{{Front}}", "Back": "{{FrontSide}}<hr id=answer>{{Back}}"}})
        }
        "modelTemplates" => {
            let mut templates = serde_json::Map::new();
            for template in &vocabulary.templates {
                templates.insert(
                    template.name.clone(),
                    json!({"Front": template.front, "Back": template.back}),
                );
            }
            Value::Object(templates)
        }
        "modelStyling" if params["modelName"] == "Basic" => json!({"css": ".card {}"}),
        "modelStyling" => json!({"css": vocabulary.css}),
        "findModelsByName" if params["modelNames"][0] == "Basic" => {
            json!([{"id": 1, "name": "Basic",
            "css": ".card {}", "flds": [{"name": "Front", "ord": 0}, {"name": "Back", "ord": 1}],
            "tmpls": [{"name": "Card 1", "ord": 0, "qfmt": "{{Front}}",
                       "afmt": "{{FrontSide}}<hr id=answer>{{Back}}"}]}])
        }
        "findModelsByName" => json!([{"id": 2, "name": vocabulary.name, "css": vocabulary.css,
            "flds": vocabulary.fields.iter().enumerate().map(|(ord, name)| json!({"name": name, "ord": ord})).collect::<Vec<_>>(),
            "tmpls": vocabulary.templates.iter().map(|t| json!({"name": t.name, "ord": t.ordinal, "qfmt": t.front, "afmt": t.back})).collect::<Vec<_>>()}]),
        other => panic!("unexpected action {other}"),
    }
}

#[test]
fn models_list_and_inspect_compare_managed_manifests_read_only() {
    let home = home("models");
    let (endpoint, actions) = fake_anki(models);
    let list = cli(&home, &endpoint)
        .args(["models", "list"])
        .output()
        .unwrap();
    assert!(list.status.success(), "{list:?}");
    let list: Value = serde_json::from_slice(&list.stdout).unwrap();
    let names: Vec<_> = list["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].clone())
        .collect();
    assert_eq!(names, [json!("Basic"), json!("Linguist Vocabulary v2")]);
    assert_eq!(list["managed_classification_verified"], false);

    let basic = cli(&home, &endpoint)
        .args(["models", "inspect", "Basic"])
        .output()
        .unwrap();
    assert!(basic.status.success(), "{basic:?}");
    let basic: Value = serde_json::from_slice(&basic.stdout).unwrap();
    assert_eq!(basic["fields"], json!(["Front", "Back"]));
    assert_eq!(basic["content_matches_managed"], false);
    assert_eq!(basic["managed_verified"], false);

    let managed = cli(&home, &endpoint)
        .args(["models", "inspect", "Linguist Vocabulary v2"])
        .output()
        .unwrap();
    assert!(managed.status.success(), "{managed:?}");
    let managed: Value = serde_json::from_slice(&managed.stdout).unwrap();
    // Identical content is reported, but never as a verified managed model.
    assert_eq!(managed["content_matches_managed"], true, "{managed}");
    assert_eq!(managed["managed_verified"], false);

    let missing = cli(&home, &endpoint)
        .args(["models", "inspect", "Nope"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let actions = actions.lock().unwrap().clone();
    assert!(
        actions.iter().all(|a| READS.contains(&a.as_str())),
        "{actions:?}"
    );
    assert_eq!(
        std::fs::read_dir(&home).unwrap().count(),
        0,
        "no local state"
    );
    std::fs::remove_dir_all(home).unwrap();
}

fn counts(action: &str, params: &Value) -> Value {
    match action {
        "getActiveProfile" => json!("Disposable"),
        "findNotes" if params["query"] == "deck:\"Empty\"" => json!([]),
        "findNotes" => json!([1700000000001_u64, 1700000000002_u64]),
        "findCards" if params["query"] == "deck:\"Empty\"" => json!([]),
        "findCards" => json!([1, 2, 3]),
        other => panic!("unexpected action {other}"),
    }
}

#[test]
fn notes_count_reports_notes_and_cards_separately_and_read_only() {
    let home = home("count");
    let (endpoint, actions) = fake_anki(counts);
    let output = cli(&home, &endpoint)
        .args(["notes", "count", "--query", "tag:verb"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = value.to_string();
    assert!(
        text.contains("\"note_count\":2") && text.contains("\"card_count\":3"),
        "{value}"
    );
    let empty = cli(&home, &endpoint)
        .args(["notes", "count", "--deck", "Empty"])
        .output()
        .unwrap();
    assert!(empty.status.success(), "{empty:?}");
    let empty: Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert!(empty.to_string().contains("\"note_count\":0"), "{empty}");
    // Explicit IDs must all exist; a missing ID fails instead of undercounting.
    let missing = cli(&home, &endpoint)
        .args([
            "notes",
            "count",
            "--note-id",
            "1700000000001",
            "--note-id",
            "1700000000099",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success(), "{missing:?}");
    let actions = actions.lock().unwrap().clone();
    assert!(
        actions.iter().all(|a| READS.contains(&a.as_str())),
        "{actions:?}"
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn config_unset_restores_inheritance_and_missing_override_is_a_no_op() {
    let home = home("unset");
    let config = home.join("config.toml");
    let run = |args: &[&str]| {
        Command::new(BIN)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .args(["--output", "json", "--config", config.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        run(&["config", "init", "--path", config.to_str().unwrap()])
            .status
            .success()
    );
    let set = run(&["config", "set", "llm.model", "custom:7b"]);
    assert!(set.status.success(), "{set:?}");
    let shown = run(&["config", "show", "llm.model", "--provenance"]);
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["values"]["llm.model"], "custom:7b");
    let unset = run(&["config", "unset", "llm.model"]);
    assert!(unset.status.success(), "{unset:?}");
    let shown = run(&["config", "show", "llm.model", "--provenance"]);
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["values"]["llm.model"], "gemma4:12b");
    assert_eq!(shown["provenance"]["llm.model"], "builtin");
    let before = std::fs::read(&config).unwrap();
    let again = run(&["config", "unset", "llm.model"]);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(
        std::fs::read(&config).unwrap(),
        before,
        "no-op leaves the file"
    );
    let unknown = run(&["config", "unset", "llm.modle"]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    std::fs::remove_dir_all(home).unwrap();
}
