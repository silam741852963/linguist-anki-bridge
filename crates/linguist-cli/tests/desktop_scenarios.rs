//! EV-14 / EV-03 / EV-04 / EV-06 / EV-07 / EV-09: the four workflows and the
//! native fault cases against REAL Anki desktop on a DISPOSABLE base folder.
//!
//! `scripts/disposable-anki-desktop.py` creates a new base folder and
//! profile, installs AnkiConnect (pinned bytes) and the Linguist companion
//! (`dist`-style `.ankiaddon` built from this checkout) through Anki's own
//! add-on installer and runs `anki -b BASE` offscreen. No user profile is
//! opened. Every product step is a CLI command; the test only acts as the
//! person using Anki (creating decks and source notes, studying cards
//! through standard AnkiConnect actions) and observes results through
//! `labInspect`.
//!
//! Run explicitly (needs Anki's Python and the AnkiConnect zip):
//! `LAB_ANKICONNECT_ZIP=... cargo test -p linguist-cli --test desktop_scenarios -- --ignored --test-threads 1`
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_linguist-anki-bridge");
const KEY: &str = "lab-desktop-scenario-key";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn anki_python() -> String {
    std::env::var("LAB_ANKI_PYTHON").unwrap_or("/usr/bin/python3.14".into())
}

fn ankiconnect_zip() -> PathBuf {
    std::env::var("LAB_ANKICONNECT_ZIP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo().join("target/lab/ankiconnect-2055492159.zip"))
}

// ---------------------------------------------------------------- desktop

struct Desktop {
    dir: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    endpoint: String,
    fault: PathBuf,
}

impl Desktop {
    fn start(dir: &Path, fsrs: bool) -> Self {
        let addon = dir.with_extension("ankiaddon");
        let built = Command::new("python3")
            .arg(repo().join("addons/build_addon.py"))
            .arg(&addon)
            .output()
            .unwrap();
        assert!(built.status.success(), "{built:?}");
        let mut desktop = Self {
            dir: dir.to_owned(),
            child: None,
            stdin: None,
            endpoint: String::new(),
            fault: dir.with_extension("fault.json"),
        };
        let mut extra = vec![
            "--ankiconnect".to_owned(),
            ankiconnect_zip().display().to_string(),
            "--addon".to_owned(),
            addon.display().to_string(),
        ];
        if fsrs {
            extra.push("--fsrs".into());
        }
        desktop.launch(&extra);
        desktop
    }

    fn launch(&mut self, extra: &[String]) {
        let mut child = Command::new(anki_python())
            .arg(repo().join("scripts/disposable-anki-desktop.py"))
            .arg("--dir")
            .arg(&self.dir)
            .args(["--api-key", KEY, "--fault-file"])
            .arg(&self.fault)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("LAB_ANKI_PYTHON must run Anki's Python");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let info: Value = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("disposable Anki did not start: {line:?}"));
        assert!(info["error"].is_null(), "{info}");
        assert_eq!(
            info["anki_version"], "25.09.2",
            "evidence is pinned to this build"
        );
        assert_eq!(info["mutation_variants"].as_array().unwrap().len(), 7);
        self.endpoint = info["endpoint"].as_str().unwrap().to_owned();
        self.stdin = child.stdin.take();
        self.child = Some(child);
    }

    /// Restart on the same base folder: a new process and a new session epoch.
    fn restart(&mut self) {
        self.stop();
        self.launch(&[]);
    }

    fn stop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }

    /// Wait for the Anki process to end by itself (an injected crash).
    fn wait_for_exit(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }

    fn arm(&self, fault: Value) {
        std::fs::write(&self.fault, fault.to_string()).unwrap();
    }

    fn try_call(&self, action: &str, params: Value) -> Result<Value, String> {
        let address = self.endpoint.trim_start_matches("http://");
        let body =
            json!({"action": action, "version": 6, "key": KEY, "params": params}).to_string();
        let mut stream = std::net::TcpStream::connect(address).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();
        write!(
            stream,
            "POST / HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|e| e.to_string())?;
        let split = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("bad response")?;
        let envelope: Value =
            serde_json::from_slice(&response[split + 4..]).map_err(|e| e.to_string())?;
        if envelope["error"].is_null() {
            Ok(envelope["result"].clone())
        } else {
            Err(envelope["error"].to_string())
        }
    }

    fn call(&self, action: &str, params: Value) -> Value {
        self.try_call(action, params)
            .unwrap_or_else(|e| panic!("{action}: {e}"))
    }

    fn epoch(&self) -> String {
        self.call("labCapabilities", json!({}))["collection_session"]["session_epoch"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Observed note (`ObservedNote` shape) through `labInspect`.
    fn note(&self, note_id: i64) -> Value {
        self.call(
            "labInspect",
            json!({"kind": "note", "note_id": note_id, "session_epoch": self.epoch()}),
        )
    }

    fn create_deck(&self, name: &str) -> i64 {
        self.call("createDeck", json!({"deck": name}))
            .as_i64()
            .unwrap()
    }

    fn deck_id(&self, name: &str) -> i64 {
        self.call("deckNamesAndIds", json!({}))[name]
            .as_i64()
            .unwrap()
    }

    fn add_note(&self, model: &str, deck: &str, fields: Value, tags: &[&str]) -> i64 {
        self.call(
            "addNote",
            json!({"note": {"deckName": deck, "modelName": model, "fields": fields, "tags": tags,
                            "options": {"allowDuplicate": true}}}),
        )
        .as_i64()
        .unwrap()
    }

    /// Study every card of the note once (Good), as a person reviewing it.
    fn study(&self, note_id: i64) {
        let cards = self.call("findCards", json!({"query": format!("nid:{note_id}")}));
        let answers: Vec<Value> = cards
            .as_array()
            .unwrap()
            .iter()
            .map(|card| json!({"cardId": card, "ease": 3}))
            .collect();
        let done = self.call("answerCards", json!({"answers": answers}));
        assert!(
            done.as_array().unwrap().iter().all(|ok| ok == true),
            "{done}"
        );
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------- scenario

struct Scenario {
    root: PathBuf,
    desktop: Desktop,
    purpose: &'static str,
    target_deck: Option<&'static str>,
    settings: Vec<String>,
    log: Vec<String>,
}

impl Scenario {
    fn new(
        name: &str,
        purpose: &'static str,
        target_deck: Option<&'static str>,
        fsrs: bool,
    ) -> Self {
        // Short path: the private Qt socket path must stay under the limit.
        let root = PathBuf::from(format!(
            "/tmp/lab-desk-{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let desktop = Desktop::start(&root.join("anki"), fsrs);
        let scenario = Self {
            root,
            desktop,
            purpose,
            target_deck,
            settings: vec![],
            log: vec![format!("scenario {name}")],
        };
        if let Some(deck) = target_deck {
            scenario.desktop.create_deck(deck);
        }
        scenario
    }

    fn cli(&mut self, args: &[&str]) -> (i32, Value, String) {
        let mut command = Command::new(BIN);
        command
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .env("LAB_SCENARIO_KEY", KEY)
            .args(["--output", "json"])
            .args(["--set", &format!("anki.endpoint={}", self.desktop.endpoint)])
            .args(["--set", "anki.api_key_env=LAB_SCENARIO_KEY"])
            .args([
                "--set",
                &format!("storage.state_dir={}", self.root.join("state").display()),
            ])
            .args([
                "--set",
                &format!("storage.backup_dir={}", self.root.join("backups").display()),
            ])
            .args([
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
            .args(["--purpose", self.purpose]);
        if let Some(deck) = self.target_deck {
            command.args([
                "--set",
                &format!("purposes.{}.target_deck={deck}", self.purpose),
            ]);
        }
        for setting in &self.settings {
            command.args(["--set", setting]);
        }
        let output = command.args(args).output().unwrap();
        let code = output.status.code().unwrap_or(-1);
        self.log.push(format!("{} -> {code}", args.join(" ")));
        (
            code,
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn ok(&mut self, args: &[&str]) -> Value {
        let (code, value, stderr) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {stderr} {value}");
        value
    }

    fn install_model(&mut self, purpose: &str) {
        let installed = self.ok(&["models", "install", purpose, "--apply"]);
        assert_eq!(installed["model_install"]["verified"], true, "{installed}");
    }

    /// `plans bind` the latest revision, validate it and approve it with
    /// every reported warning accepted explicitly.
    fn bind_and_approve(&mut self, plan: &str, digest: &str) -> (u32, String) {
        let bound = self.ok(&["plans", "bind", plan, "--digest", digest]);
        let revision = bound["revision"].as_u64().unwrap() as u32;
        let digest = bound["digest"].as_str().unwrap().to_owned();
        let rev = revision.to_string();
        let validated = self.ok(&["plans", "validate", plan, "--revision", &rev]);
        let mut warnings: Vec<String> = validated["evidence"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|item| item["issues"].as_array().unwrap().iter())
            .filter(|issue| issue["severity"] == "warning")
            .map(|issue| issue["code"].as_str().unwrap().to_owned())
            .collect();
        warnings.sort();
        warnings.dedup();
        let mut args = vec![
            "plans",
            "approve",
            plan,
            "--revision",
            &rev,
            "--digest",
            &digest,
        ];
        args.extend(["--actor", "desktop-scenario"]);
        for warning in &warnings {
            args.extend(["--accept-warning", warning.as_str()]);
        }
        let args: Vec<&str> = args.into_iter().collect();
        self.ok(&args);
        (revision, digest)
    }

    /// Live restore preview, a decision bound to its observed state, then
    /// `snapshots restore --apply`.
    fn restore(
        &mut self,
        snapshot: &str,
        edit: impl FnOnce(&mut Value, &Value),
    ) -> (i32, Value, String) {
        let preview = self.ok(&["snapshots", "restore", snapshot]);
        assert_eq!(preview["live_checked"], true, "{preview}");
        let mut decision = preview["decision_template"].clone();
        decision["actor"] = json!("desktop-scenario");
        edit(&mut decision, &preview["live"]);
        let path = self.root.join(format!("decision-{snapshot}.json"));
        std::fs::write(&path, decision.to_string()).unwrap();
        self.cli(&[
            "snapshots",
            "restore",
            snapshot,
            "--decision",
            path.to_str().unwrap(),
            "--apply",
        ])
    }

    fn report(&self, name: &str) {
        let text = format!("{name}\n{}\n", self.log.join("\n"));
        println!("{text}");
        if let Ok(dir) = std::env::var("LAB_SCENARIO_REPORT_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(Path::new(&dir).join(format!("{name}.txt")), text).unwrap();
        }
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        self.desktop.stop();
        if std::env::var("LAB_KEEP_DISPOSABLE").is_err() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn add_vocab(s: &mut Scenario, expression: &str, meaning: &str, key: &str) -> (String, String) {
    let added = s.ok(&[
        "vocab",
        "add",
        "--expression",
        expression,
        "--meaning",
        meaning,
        "--sense-key",
        key,
        "--target-language",
        "ja",
        "--tag",
        "release",
    ]);
    (
        added["plan_id"].as_str().unwrap().to_owned(),
        added["digest"].as_str().unwrap().to_owned(),
    )
}

/// Approval digest of a full `plans show` revision.
fn root_digest(revision: &Value) -> String {
    let parsed: linguist_core::records::PlanRevision =
        serde_json::from_value(revision.clone()).unwrap();
    parsed.approval_digest().unwrap()
}

fn applied(value: &Value) -> (i64, String) {
    let item = &value["items"][0];
    assert_eq!(item["state"], "committed", "{value}");
    (
        item["note_id"].as_i64().unwrap(),
        item["snapshot_id"].as_str().unwrap().to_owned(),
    )
}

// ---------------------------------------------------------------- workflows

#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn vocab_add_apply_study_restore() {
    let mut s = Scenario::new(
        "vocab-add",
        "japanese_vocab",
        Some("Japanese::Vocab"),
        false,
    );
    s.install_model("japanese_vocab");
    // An unbound plan is preparation/export only.
    let (plan, digest) = add_vocab(&mut s, "食べる", "to eat", "eat");
    let (code, preview, _) = s.cli(&["apply", &plan, "--revision", "1"]);
    assert_eq!(code, 4);
    assert!(preview.to_string().contains("APPLY_BINDING_WEAK"));
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    let rev = revision.to_string();
    let first = s.ok(&["apply", &plan, "--revision", &rev, "--apply"]);
    let (note_id, snapshot) = applied(&first);
    let note = s.desktop.note(note_id);
    assert_eq!(note["model_name"], "Linguist Vocabulary v2");
    assert_eq!(note["fields"]["Expression"], "食べる");
    assert_eq!(
        note["cards"][0]["deck_id"],
        s.desktop.deck_id("Japanese::Vocab")
    );
    assert_eq!(note["cards"][0]["review_count"], 0);
    assert!(note["tags"].to_string().contains("lab_op_"));
    // Applying the same approved item again is refused.
    let (code, _, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("APPLY_ALREADY_COMMITTED"), "{stderr}");
    // Later study: a studied created note is never deleted by restore.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 1);
    let (code, _, stderr) = s.restore(&snapshot, |decision, _| {
        decision["delete_created_notes"] = json!([note_id]);
    });
    assert_ne!(code, 0, "{stderr}");
    assert!(stderr.contains("RESTORE_CREATED_NOTE_STUDIED"), "{stderr}");
    assert_eq!(
        s.desktop.note(note_id)["cards"],
        studied["cards"],
        "history kept"
    );
    // A second, unstudied note is removed by an explicit restore decision.
    let (plan2, digest2) = add_vocab(&mut s, "飲む", "to drink", "drink");
    let (revision2, _) = s.bind_and_approve(&plan2, &digest2);
    let second = s.ok(&[
        "apply",
        &plan2,
        "--revision",
        &revision2.to_string(),
        "--apply",
    ]);
    let (note2, snapshot2) = applied(&second);
    let (code, restored, stderr) = s.restore(&snapshot2, |decision, _| {
        decision["delete_created_notes"] = json!([note2]);
    });
    assert_eq!(code, 0, "{stderr} {restored}");
    assert_eq!(
        restored["restore"]["target_state"], "restored",
        "{restored}"
    );
    assert!(s.desktop.note(note2).is_null());
    assert!(!s.desktop.note(note_id).is_null());
    s.report("vocab_add");
}

#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn grammar_add_apply_study_restore_keeps_history() {
    let mut s = Scenario::new(
        "grammar-add",
        "japanese_grammar",
        Some("Japanese::Grammar"),
        false,
    );
    s.install_model("japanese_grammar");
    let document = s.root.join("grammar.json");
    std::fs::write(
        &document,
        json!({
            "schema_version": 2, "kind": "grammar", "target_language": "ja",
            "explanation_language": "vi",
            "body": {"pattern": "〜てもいい", "use_key": "permission", "meaning": "được phép",
                     "formation": "V-て + もいい", "recognition_prompt": "Mẫu này nghĩa là gì?",
                     "examples": [{"sentence": "ここで写真を撮ってもいいですか。",
                                   "translation": "Tôi chụp ảnh ở đây được không?", "provenance": "user"}]}
        })
        .to_string(),
    )
    .unwrap();
    let added = s.ok(&["grammar", "add", "--document", document.to_str().unwrap()]);
    let plan = added["plan_id"].as_str().unwrap().to_owned();
    let digest = added["digest"].as_str().unwrap().to_owned();
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    let outcome = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &revision.to_string(),
        "--apply",
    ]);
    let (note_id, snapshot) = applied(&outcome);
    let note = s.desktop.note(note_id);
    assert_eq!(note["model_name"], "Linguist Grammar v2");
    assert!(
        note["fields"]["Meaning"]
            .as_str()
            .unwrap()
            .contains("được phép")
    );
    // Later study, then restore: the studied created note is kept with history.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 1);
    let (code, _, stderr) = s.restore(&snapshot, |decision, _| {
        decision["delete_created_notes"] = json!([note_id]);
    });
    assert_ne!(code, 0);
    assert!(stderr.contains("RESTORE_CREATED_NOTE_STUDIED"), "{stderr}");
    assert_eq!(s.desktop.note(note_id)["cards"], studied["cards"]);
    s.report("grammar_add");
}

#[allow(clippy::too_many_arguments)]
/// Captures a studied Basic note through the CLI, resolves its native history
/// from companion evidence, migrates it to the managed model keeping the card
/// and its FSRS state, studies again and restores.
fn revamp_workflow(
    name: &str,
    purpose: &'static str,
    command: &str,
    target_deck: Option<&'static str>,
    source: (&str, &str),
    roles: Value,
    task: &str,
    model_purpose: &str,
    expected_model: &str,
    edits: &[(&str, &str)],
) {
    let mut s = Scenario::new(name, purpose, target_deck, true);
    s.install_model(model_purpose);
    let note_id = s.desktop.add_note(
        "Basic",
        "Default",
        json!({"Front": source.0, "Back": source.1}),
        &["legacy"],
    );
    s.desktop.study(note_id);
    s.desktop.study(note_id);
    let before = s.desktop.note(note_id);
    assert_eq!(before["cards"][0]["review_count"], 2);
    assert_ne!(
        before["cards"][0]["scheduler"]["memory_state"], "None",
        "FSRS state"
    );
    s.settings
        .push(format!("purposes.{purpose}.fields={roles}"));
    s.settings
        .push(format!("purposes.{purpose}.source_model=Basic"));
    let (code, drafted, stderr) = s.cli(&[command, "revamp", "--note-id", &note_id.to_string()]);
    assert_eq!(code, 4, "{stderr} {drafted}");
    let root = if drafted["plan_id"].is_string() {
        &drafted
    } else {
        &drafted["result"]
    };
    let plan = root["plan_id"].as_str().unwrap().to_owned();
    let item = root["document_id"]
        .as_str()
        .or(root["items"][0]["document_id"].as_str())
        .unwrap()
        .to_owned();
    let issues = s.ok(&["plans", "show", &plan, "--issues-only"]);
    assert!(
        issues.to_string().contains("SOURCE_NATIVE_HISTORY_REVIEW"),
        "{issues}"
    );
    // Review: author what the source does not carry (OP-28 typed patch).
    if !edits.is_empty() {
        let shown = s.ok(&["plans", "show", &plan]);
        let fields: serde_json::Map<String, Value> = edits
            .iter()
            .map(|(field, value)| {
                (
                    (*field).to_owned(),
                    json!({"intent": "set", "value": value}),
                )
            })
            .collect();
        let patch = s.root.join("review.patch.json");
        std::fs::write(
            &patch,
            json!({"schema_version": 2, "base_digest": root_digest(&shown),
                   "items": [{"document_id": item, "fields": fields}]})
            .to_string(),
        )
        .unwrap();
        let revision = shown["revision"].to_string();
        let (code, edited, stderr) = s.cli(&[
            "plans",
            "edit",
            &plan,
            "--base-revision",
            &revision,
            "--patch",
            patch.to_str().unwrap(),
            "--save-draft",
        ]);
        assert!(code == 0 || code == 4, "{stderr} {edited}");
    }
    // Native history from the companion: map the one Basic card.
    let (code, resolved, stderr) = s.cli(&[
        "plans",
        "resolve-history",
        &plan,
        "--item",
        &item,
        "--map",
        &format!("0={task}"),
        "--actor",
        "desktop-scenario",
    ]);
    assert!(code == 0 || code == 4, "{stderr} {resolved}");
    assert_eq!(
        resolved["choice"]["value"]["cards"][0]["review_count"], 2,
        "{resolved}"
    );
    assert_eq!(resolved["ready"], true, "{resolved}");
    let digest = resolved["digest"].as_str().unwrap().to_owned();
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    let rev = revision.to_string();
    let outcome = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--accept-schema-change",
        "--apply",
    ]);
    let (applied_note, snapshot) = applied(&outcome);
    assert_eq!(applied_note, note_id);
    let after = s.desktop.note(note_id);
    assert_eq!(after["model_name"], expected_model);
    assert_eq!(after["cards"].as_array().unwrap().len(), 1);
    assert_eq!(after["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(
        after["cards"][0]["history_digest"],
        before["cards"][0]["history_digest"]
    );
    assert_eq!(after["cards"][0]["review_count"], 2);
    assert_eq!(
        after["cards"][0]["scheduler"], before["cards"][0]["scheduler"],
        "FSRS kept"
    );
    match target_deck {
        Some(deck) => assert_eq!(after["cards"][0]["deck_id"], s.desktop.deck_id(deck)),
        None => assert_eq!(after["cards"][0]["deck_id"], before["cards"][0]["deck_id"]),
    }
    // Later study, then restore: content returns, the later review stays.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 3);
    let (code, restored, stderr) = s.restore(&snapshot, |decision, live| {
        decision["accept_schema_change"] = json!(!live["model"].is_null());
        assert_eq!(live["blockers"], json!([]), "{live}");
    });
    assert_eq!(code, 0, "{stderr} {restored}");
    assert_eq!(
        restored["restore"]["target_state"], "restored",
        "{restored}"
    );
    let back = s.desktop.note(note_id);
    assert_eq!(back["model_name"], "Basic");
    assert_eq!(back["fields"], before["fields"]);
    assert_eq!(back["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(back["cards"][0]["review_count"], 3);
    assert_eq!(
        back["cards"][0]["history_digest"],
        studied["cards"][0]["history_digest"]
    );
    assert_eq!(
        back["cards"][0]["scheduler"],
        studied["cards"][0]["scheduler"]
    );
    s.report(name);
}

#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn vocab_revamp_migrate_study_restore() {
    revamp_workflow(
        "vocab_revamp",
        "japanese_vocab",
        "vocab",
        Some("Japanese::Vocab"),
        ("食べる", "to consume"),
        json!({"expression": "Front", "meaning": "Back"}),
        "comprehension",
        "japanese_vocab",
        "Linguist Vocabulary v2",
        &[("SenseKey", "eat-food")],
    );
}

fn grammar_unit(pattern: &str, key: &str, meaning: &str) -> Value {
    json!({"pattern": pattern, "use_key": key, "meaning": meaning, "formation": "V-て + も",
           "recognition_prompt": "Mẫu này thể hiện quan hệ gì?", "usage": "",
           "exercise_prompt": "", "exercise_answer": "",
           "examples": [{"sentence": format!("雨が降{pattern}行きます。"),
                         "translation": "Ví dụ.", "provenance": "user", "evidence_ids": []}]})
}

/// Resolve every open review issue the scenario knows how to decide, through
/// CLI commands only; returns the latest digest.
fn review_all(s: &mut Scenario, plan: &str, anchor: Option<&str>, history_task: &str) -> String {
    for _ in 0..20 {
        let page = s.ok(&["plans", "show", plan, "--issues-only"]);
        let entries = page["issues"].as_array().cloned().unwrap_or_default();
        let Some(entry) = entries.iter().find(|e| e["issue"]["severity"] != "warning") else {
            let shown = s.ok(&["plans", "show", plan]);
            return root_digest(&shown);
        };
        let identity = &entry["request_identity"];
        let code = entry["issue"]["code"].as_str().unwrap();
        let document = identity["document_id"].as_str().unwrap().to_owned();
        let (status, value, stderr) = match code {
            "SOURCE_NATIVE_HISTORY_REVIEW" => s.cli(&[
                "plans",
                "resolve-history",
                plan,
                "--item",
                &document,
                "--map",
                &format!("0={history_task}"),
                "--actor",
                "desktop-scenario",
            ]),
            "GRAMMAR_SPLIT_NATIVE_REVIEW" => {
                let mut decision = identity.clone();
                decision["actor"] = json!("desktop-scenario");
                decision["choice"] = json!({"decision": "anchor", "value": anchor.unwrap()});
                let path = s.root.join(format!("decision-{}.json", Uuid::new_v4()));
                std::fs::write(&path, decision.to_string()).unwrap();
                let issue = identity["issue_id"].as_str().unwrap().to_owned();
                s.cli(&[
                    "plans",
                    "resolve",
                    plan,
                    &issue,
                    "--decision",
                    path.to_str().unwrap(),
                ])
            }
            other => panic!("unexpected open issue {other}: {entry}"),
        };
        assert!(status == 0 || status == 4, "{code}: {stderr} {value}");
    }
    panic!("review did not converge");
}

/// EV-09: a managed grammar note holding two patterns is captured, split by
/// the CLI into an anchor and a fresh sibling, reviewed, applied (sibling
/// first, anchor last), studied, and rolled back as a group.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn grammar_revamp_multi_unit_split_apply_study_rollback() {
    let mut s = Scenario::new(
        "grammar-split",
        "japanese_grammar",
        Some("Japanese::Grammar"),
        true,
    );
    s.install_model("japanese_grammar");
    let note_id = s.desktop.add_note(
        "Linguist Grammar v2",
        "Japanese::Grammar",
        json!({"Pattern": "〜ても / 〜てもいい", "Meaning": "dù / được phép",
               "Formation": "V-て + も", "Language": "ja"}),
        &["legacy"],
    );
    s.desktop.study(note_id);
    s.desktop.study(note_id);
    let before = s.desktop.note(note_id);
    s.settings.push(format!(
        "purposes.japanese_grammar.fields={}",
        json!({"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "language": "Language"})
    ));
    s.settings
        .push("purposes.japanese_grammar.source_model=Linguist Grammar v2".into());
    let (code, drafted, stderr) = s.cli(&["grammar", "revamp", "--note-id", &note_id.to_string()]);
    assert_eq!(code, 4, "{stderr} {drafted}");
    let root = if drafted["plan_id"].is_string() {
        &drafted
    } else {
        &drafted["result"]
    };
    let plan = root["plan_id"].as_str().unwrap().to_owned();
    // CLI split: the first unit keeps the note, the second is a fresh sibling.
    let page = s.ok(&["plans", "show", &plan, "--issues-only"]);
    let identity = page["issues"][0]["request_identity"].clone();
    let request = json!({
        "schema_version": 2, "base_revision": identity["base_revision"],
        "base_digest": identity["base_digest"], "document_id": identity["document_id"],
        "input_digest": identity["input_digest"], "actor": "desktop-scenario", "anchor_index": 0,
        "units": [grammar_unit("〜ても", "concession", "dù"),
                  grammar_unit("〜てもいい", "permission", "được phép")],
    });
    let request_path = s.root.join("split.json");
    std::fs::write(&request_path, request.to_string()).unwrap();
    let (code, split, stderr) = s.cli(&[
        "plans",
        "split-grammar",
        &plan,
        "--request",
        request_path.to_str().unwrap(),
    ]);
    assert!(code == 0 || code == 4, "{stderr} {split}");
    let group = split["grammar_groups"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let anchor = split["grammar_groups"][0]["anchor_document"]
        .as_str()
        .unwrap()
        .to_owned();
    let digest = review_all(&mut s, &plan, Some(&anchor), "recognition");
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    let rev = revision.to_string();
    let outcome = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--apply",
    ]);
    let split = &outcome["split"];
    assert_eq!(split["state"], "complete", "{outcome}");
    let execution = split["execution_id"].as_str().unwrap().to_owned();
    let anchor_after = s.desktop.note(note_id);
    let pattern = anchor_after["fields"]["Pattern"].as_str().unwrap();
    assert!(
        pattern.contains("〜ても") && !pattern.contains("〜てもいい"),
        "{pattern}"
    );
    assert_eq!(anchor_after["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(
        anchor_after["cards"][0]["history_digest"],
        before["cards"][0]["history_digest"]
    );
    assert_eq!(
        anchor_after["cards"][0]["scheduler"],
        before["cards"][0]["scheduler"]
    );
    let sibling = split["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["role"] != "anchor")
        .unwrap()["note_id"]
        .as_i64()
        .unwrap();
    let sibling_note = s.desktop.note(sibling);
    assert!(
        sibling_note["fields"]["Pattern"]
            .as_str()
            .unwrap()
            .contains("〜てもいい")
    );
    assert_eq!(sibling_note["cards"][0]["review_count"], 0);
    // Later study of the anchor, then a group rollback through the CLI.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 3);
    let rolled = s.ok(&[
        "jobs",
        "rollback",
        &execution,
        "--delete-unstudied-created",
        "--apply",
    ]);
    for item in rolled["items"].as_array().unwrap() {
        assert_eq!(item["restore"]["target_state"], "restored", "{rolled}");
    }
    let restored = s.desktop.note(note_id);
    assert_eq!(restored["fields"], before["fields"]);
    assert_eq!(restored["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(restored["cards"][0]["review_count"], 3);
    assert_eq!(
        restored["cards"][0]["scheduler"],
        studied["cards"][0]["scheduler"]
    );
    assert!(s.desktop.note(sibling).is_null());
    s.report("grammar_split");
}
