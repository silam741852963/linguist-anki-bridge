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
    fn start(dir: &Path, fsrs: bool, launcher: &[&str]) -> Self {
        // The release check passes the exact dist artifact; otherwise the
        // same deterministic build is made from this checkout.
        let addon = match std::env::var("LAB_COMPANION_ADDON") {
            Ok(path) => PathBuf::from(path),
            Err(_) => {
                let addon = dir.with_extension("ankiaddon");
                let built = Command::new("python3")
                    .arg(repo().join("addons/build_addon.py"))
                    .arg(&addon)
                    .output()
                    .unwrap();
                assert!(built.status.success(), "{built:?}");
                addon
            }
        };
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
        extra.extend(launcher.iter().map(|arg| (*arg).to_owned()));
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
        Self::with(name, purpose, target_deck, fsrs, &[])
    }

    fn with(
        name: &str,
        purpose: &'static str,
        target_deck: Option<&'static str>,
        fsrs: bool,
        launcher: &[&str],
    ) -> Self {
        // Short path: the private Qt socket path must stay under the limit.
        let root = PathBuf::from(format!(
            "/tmp/lab-desk-{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let desktop = Desktop::start(&root.join("anki"), fsrs, launcher);
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
    // A whole-collection checkpoint on demand (OP-52), verified by restore test.
    let backup = s.ok(&["backup", "create", "--scope", "collection", "--apply"]);
    assert_eq!(backup["checkpoint"]["restoration_tested"], true, "{backup}");
    assert_eq!(
        backup["checkpoint"]["checkpoint_eligible"], true,
        "{backup}"
    );
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
    assert_eq!(note["model_name"], "Linguist Vocabulary v3");
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
    assert_eq!(note["model_name"], "Linguist Grammar v4");
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
    edit_after_apply: Option<&str>,
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
    if let Some(field) = edit_after_apply {
        // A personal edit after apply conflicts until a decision names it.
        s.desktop.call(
            "updateNoteFields",
            json!({"note": {"id": note_id, "fields": {field: "my own later note"}}}),
        );
        let (code, _, stderr) = s.restore(&snapshot, |decision, live| {
            assert!(
                live["conflicts"]
                    .to_string()
                    .contains(&format!("RESTORE_FIELD_CONFLICT:{field}")),
                "{live}"
            );
            decision["accept_schema_change"] = json!(true);
        });
        assert_ne!(
            code, 0,
            "restore without a field decision must not proceed: {stderr}"
        );
        s.log
            .push(format!("restore without a field decision refused: {field}"));
    }
    // Later study, then restore: content returns, the later review stays.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 3);
    let (code, restored, stderr) = s.restore(&snapshot, |decision, live| {
        decision["accept_schema_change"] = json!(!live["model"].is_null());
        if let Some(field) = edit_after_apply {
            decision["fields"] = json!({field: {"choice": "original"}});
        }
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
        "Linguist Vocabulary v3",
        &[("SenseKey", "eat-food")],
        None,
    );
}

#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn vocab_revamp_home_deck_edit_conflict_restore() {
    revamp_workflow(
        "vocab_revamp_home_deck",
        "english_vocab",
        "vocab",
        None,
        ("used to", "a past habit"),
        json!({"expression": "Front", "meaning": "Back"}),
        "comprehension",
        "english_vocab",
        "Linguist English Vocabulary v1",
        &[("SenseKey", "past-habit")],
        Some("Meaning"),
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
    let SplitReady {
        mut s,
        plan,
        revision,
        group,
        note_id,
        before,
    } = split_ready("grammar-split");
    let rev = revision.to_string();
    let outcome = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--accept-schema-change",
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
    // WP-23: the sibling's card carries the source card's schedule.
    assert_eq!(
        sibling_note["cards"][0]["scheduler"],
        inherited(&before["cards"][0]["scheduler"]),
        "{sibling_note}"
    );
    // Later study of the anchor, then a group rollback through the CLI.
    s.desktop.study(note_id);
    let studied = s.desktop.note(note_id);
    assert_eq!(studied["cards"][0]["review_count"], 3);
    let rolled = s.ok(&[
        "jobs",
        "rollback",
        &execution,
        "--accept-schema-change",
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

/// WP-23: the scheduler a new split unit's card copies from its source card;
/// it keeps no reviews, lapses, flag or `odue` of its own.
fn inherited(source: &Value) -> Value {
    let mut scheduler = source.clone();
    for key in ["reps", "lapses", "flags", "odue"] {
        scheduler[key] = json!("0");
    }
    scheduler
}

/// Prepare, bind and approve one vocabulary item; returns (plan, revision).
fn approved_vocab(s: &mut Scenario, expression: &str, key: &str) -> (String, String) {
    let (plan, digest) = add_vocab(s, expression, "meaning", key);
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    (plan, revision.to_string())
}

fn notes_tagged(s: &Scenario, expression: &str) -> usize {
    s.desktop
        .call(
            "findNotes",
            json!({"query": format!("Expression:{expression}")}),
        )
        .as_array()
        .unwrap()
        .len()
}

/// EV-03 / EV-04 / EV-07: injected native faults in real Anki; every
/// uncertain outcome is resolved with `recover reconcile --apply`.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn native_faults_recover_through_reconcile() {
    let mut s = Scenario::new(
        "native-faults",
        "japanese_vocab",
        Some("Japanese::Vocab"),
        false,
    );
    s.install_model("japanese_vocab");
    // Short operation deadline so a delayed native worker exceeds it.
    s.settings.push("anki.request_timeout_seconds=5".into());

    // 1. Timeout: the worker is delayed past the CLI deadline; the effect
    // completes later and reconcile adopts the verified read-back.
    let (plan, rev) = approved_vocab(&mut s, "遅い", "slow");
    s.desktop.arm(
        json!({"point": "before_running", "action": "sleep", "seconds": 12,
                         "variant": "create_note"}),
    );
    let (code, timed_out, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_eq!(code, 4, "{stderr} {timed_out}");
    assert_eq!(
        timed_out["items"][0]["state"], "needs_recovery",
        "{timed_out}"
    );
    let operation = timed_out["items"][0]["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::thread::sleep(Duration::from_secs(10));
    let reconciled = s.ok(&["recover", "reconcile", &operation, "--apply"]);
    assert_eq!(
        reconciled["reconcile"]["state"], "committed",
        "{reconciled}"
    );
    assert_eq!(notes_tagged(&s, "遅い"), 1);
    s.log
        .push("timeout: reconcile adopted the delayed verified effect".into());

    // 2. Crash between the native effect and its receipt: Anki exits right
    // after the note is written. After restart (new session epoch) the
    // operation is rebound explicitly and adopted from the read-back.
    let (plan, rev) = approved_vocab(&mut s, "壊れる", "crash");
    s.desktop
        .arm(json!({"point": "after_effect", "action": "crash", "variant": "create_note"}));
    let (code, crashed, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0, "{stderr} {crashed}");
    s.desktop.wait_for_exit();
    s.desktop.restart();
    assert_eq!(
        notes_tagged(&s, "壊れる"),
        1,
        "the effect happened before the crash"
    );
    let operation = crashed["items"][0]["operation_id"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let pending = s.ok(&["recover", "inspect", "--pending"]);
            pending.to_string()
        });
    let (code, _, stderr) = s.cli(&["recover", "reconcile", &operation, "--apply"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("SESSION_CHANGED"), "{stderr}");
    let reconciled = s.ok(&["recover", "reconcile", &operation, "--rebind", "--apply"]);
    assert_eq!(
        reconciled["reconcile"]["state"], "committed",
        "{reconciled}"
    );
    assert_eq!(reconciled["reconcile"]["rebound"], true);
    assert_eq!(notes_tagged(&s, "壊れる"), 1, "never re-created");
    s.log
        .push("crash after effect: rebind + reconcile adopted one note".into());

    // 3. Duplicate UUID: re-sending the committed step's exact intent under a
    // new owner returns the stored receipt and creates nothing.
    let store = linguist_store::Store::read_only(&s.root.join("state")).unwrap();
    let record = store
        .apply_operation(Uuid::parse_str(&operation).unwrap())
        .unwrap();
    let journal = store.journal(record.operation_id).unwrap().journal;
    let intent: linguist_application::apply::ApplyIntent =
        serde_json::from_value(record.intent.clone()).unwrap();
    let step = intent.steps.last().unwrap();
    let payload = String::from_utf8(step.effect.wire_bytes().unwrap()).unwrap();
    let session = s.desktop.call("labCapabilities", json!({}))["collection_session"].clone();
    let mut binding = serde_json::to_value(&journal.binding).unwrap();
    binding["session_epoch"] = session["session_epoch"].clone();
    let owner = s.desktop.call(
        "labBegin",
        json!({"binding": binding, "approved_digest": journal.approval_digest}),
    );
    let replay = |payload: &str| {
        s.desktop.try_call(
            "labMutate",
            json!({"lineage_id": session["lineage_id"], "operation_id": step.step_id,
                   "session_epoch": session["session_epoch"], "owner_token": owner["owner_token"],
                   "fence": owner["fence"], "approved_digest": journal.approval_digest,
                   "variant": "create_note", "payload": payload}),
        )
    };
    // The crashed row stays `unknown(worker_crash)` in the companion ledger
    // (the CLI journal adopted it from the read-back); a duplicate returns
    // exactly that stored state and never dispatches again.
    let stored = s.desktop.call(
        "labOperationStatus",
        json!({"lineage_id": session["lineage_id"], "operation_id": step.step_id}),
    );
    let duplicate = replay(&payload).unwrap();
    assert_eq!(duplicate, stored, "{duplicate}");
    assert_eq!(
        (duplicate["state"].as_str(), duplicate["reason"].as_str()),
        (Some("unknown"), Some("worker_crash"))
    );
    let changed = payload.replace("壊れる", "別物");
    let conflict = replay(&changed).unwrap_err();
    assert!(
        conflict.contains("BRIDGE_OPERATION_REPLAY_CONFLICT"),
        "{conflict}"
    );
    s.desktop.call(
        "labEnd",
        json!({"owner_token": owner["owner_token"], "fence": owner["fence"]}),
    );
    assert_eq!(notes_tagged(&s, "壊れる"), 1);
    assert_eq!(notes_tagged(&s, "別物"), 0);
    s.log
        .push("duplicate UUID: stored receipt returned, replay conflict refused".into());

    // 4. Disk full at the ledger boundary: `running` cannot be recorded, so
    // no effect starts. Reconcile classifies the lost worker as failed
    // before write; a new attempt then succeeds.
    let (plan, rev) = approved_vocab(&mut s, "満杯", "full");
    s.desktop
        .arm(json!({"point": "before_running", "action": "disk_full", "variant": "create_note"}));
    let (code, full, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_eq!(code, 4, "{stderr} {full}");
    let operation = full["items"][0]["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(notes_tagged(&s, "満杯"), 0);
    let reconciled = s.ok(&["recover", "reconcile", &operation, "--apply"]);
    assert_eq!(
        reconciled["reconcile"]["state"], "failed_before_write",
        "{reconciled}"
    );
    let retried = s.ok(&["apply", &plan, "--revision", &rev, "--apply"]);
    applied(&retried);
    assert_eq!(notes_tagged(&s, "満杯"), 1);
    s.log
        .push("disk full: failed before write, then a new attempt committed".into());

    // 5. Session change between acceptance and the critical section: the
    // collection is closed and reopened (as a full sync or import does). The
    // native worker refuses; reconcile needs an explicit rebind.
    let (plan, rev) = approved_vocab(&mut s, "変わる", "change");
    s.desktop
        .arm(json!({"point": "before_session_check", "action": "reopen",
                         "variant": "create_note"}));
    let (code, changed, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0, "{stderr} {changed}");
    let operation = changed["items"][0]["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(notes_tagged(&s, "変わる"), 0);
    let (code, _, stderr) = s.cli(&["recover", "reconcile", &operation, "--apply"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("SESSION_CHANGED"), "{stderr}");
    let reconciled = s.ok(&["recover", "reconcile", &operation, "--rebind", "--apply"]);
    assert_eq!(
        reconciled["reconcile"]["state"], "failed_before_write",
        "{reconciled}"
    );
    let retried = s.ok(&["apply", &plan, "--revision", &rev, "--apply"]);
    applied(&retried);
    assert_eq!(notes_tagged(&s, "変わる"), 1);
    s.log
        .push("session change: refused before write, rebind + reconcile, retry committed".into());

    // 6. Removed marker: after a crash between effect and receipt the user
    // removes the operation marker. Absence cannot be proven, so the
    // operation stays in recovery and is never re-sent or re-created.
    let (plan, rev) = approved_vocab(&mut s, "消す", "erase");
    s.desktop
        .arm(json!({"point": "after_effect", "action": "crash", "variant": "create_note"}));
    let (_, crashed, _) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    s.desktop.wait_for_exit();
    s.desktop.restart();
    let operation = crashed["items"][0]["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = s
        .desktop
        .call("findNotes", json!({"query": "Expression:消す"}));
    let note = created[0].as_i64().unwrap();
    let marker = s.desktop.note(note)["tags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tag| tag.as_str().unwrap().starts_with("lab_op_"))
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    s.desktop
        .call("removeTags", json!({"notes": [note], "tags": marker}));
    let (code, unresolved, stderr) =
        s.cli(&["recover", "reconcile", &operation, "--rebind", "--apply"]);
    assert_eq!(code, 4, "{stderr} {unresolved}");
    assert_eq!(
        unresolved["reconcile"]["state"], "needs_recovery",
        "{unresolved}"
    );
    let (code, _, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("APPLY_RECOVERY_REQUIRED"), "{stderr}");
    assert_eq!(notes_tagged(&s, "消す"), 1, "never re-created");
    s.log
        .push("removed marker: absence unproven, stays in recovery, no second note".into());
    s.report("native_faults");
}

/// Poll until the companion reports a session for `profile`'s fingerprint.
fn wait_for_profile(desktop: &Desktop, profile: &str) {
    let expected =
        linguist_core::canonical::asset_digest(format!("lab-profile-v1\0{profile}").as_bytes());
    for _ in 0..120 {
        if let Ok(manifest) = desktop.try_call("labCapabilities", json!({}))
            && manifest["collection_session"]["profile_fingerprint"] == expected.as_str()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("profile {profile} did not open");
}

/// INV-17 / EV-04: an approved write needs the current binding and a fresh
/// source precondition. A profile switch, lost sidecar lineage or a source
/// edit after approval each stop the write before any effect.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn identity_and_preconditions_block_writes() {
    let mut s = Scenario::with(
        "identity",
        "japanese_vocab",
        Some("Japanese::Vocab"),
        false,
        &["--extra-profile", "Other"],
    );
    s.install_model("japanese_vocab");

    // a) Source precondition (CAS): the note changes after approval.
    let note_id = s.desktop.add_note(
        "Basic",
        "Default",
        json!({"Front": "走る", "Back": "to run"}),
        &[],
    );
    s.settings.push(format!(
        "purposes.japanese_vocab.fields={}",
        json!({"expression": "Front", "meaning": "Back"})
    ));
    s.settings
        .push("purposes.japanese_vocab.source_model=Basic".into());
    let (code, drafted, stderr) = s.cli(&["vocab", "revamp", "--note-id", &note_id.to_string()]);
    assert_eq!(code, 4, "{stderr}");
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
    let shown = s.ok(&["plans", "show", &plan]);
    let patch = s.root.join("sense.json");
    std::fs::write(&patch, json!({"schema_version": 2, "base_digest": root_digest(&shown),
        "items": [{"document_id": item, "fields": {"SenseKey": {"intent": "set", "value": "run"}}}]}).to_string()).unwrap();
    let (code, _, stderr) = s.cli(&[
        "plans",
        "edit",
        &plan,
        "--base-revision",
        &shown["revision"].to_string(),
        "--patch",
        patch.to_str().unwrap(),
        "--save-draft",
    ]);
    assert!(code == 0 || code == 4, "{stderr}");
    let resolved = s.ok(&[
        "plans",
        "resolve-history",
        &plan,
        "--item",
        &item,
        "--map",
        "0=comprehension",
        "--actor",
        "desktop-scenario",
    ]);
    let digest = resolved["digest"].as_str().unwrap().to_owned();
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    s.desktop.call(
        "updateNoteFields",
        json!({"note": {"id": note_id, "fields": {"Back": "to jog"}}}),
    );
    let (code, refused, stderr) = s.cli(&[
        "apply",
        &plan,
        "--revision",
        &revision.to_string(),
        "--accept-schema-change",
        "--apply",
    ]);
    assert_ne!(code, 0, "{refused}");
    assert!(
        format!("{stderr}{refused}").contains("CONFLICT"),
        "{stderr} {refused}"
    );
    let after = s.desktop.note(note_id);
    assert_eq!(after["model_name"], "Basic");
    assert_eq!(after["fields"]["Back"], "to jog");
    s.log
        .push("source edited after approval: apply refused before any effect".into());
    s.settings.clear();

    // b) Profile switch: the approved plan is bound to the Disposable profile.
    let (plan, rev) = approved_vocab(&mut s, "話す", "speak");
    s.desktop.call("loadProfile", json!({"name": "Other"}));
    wait_for_profile(&s.desktop, "Other");
    let (code, refused, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0, "{refused}");
    assert!(
        format!("{stderr}{refused}").contains("MISMATCH") || stderr.contains("CONFLICT"),
        "{stderr} {refused}"
    );
    assert_eq!(notes_tagged(&s, "話す"), 0);
    s.desktop.call("loadProfile", json!({"name": "Disposable"}));
    wait_for_profile(&s.desktop, "Disposable");
    assert_eq!(notes_tagged(&s, "話す"), 0);
    applied(&s.ok(&["apply", &plan, "--revision", &rev, "--apply"]));
    s.log
        .push("profile switch: refused; back on the bound profile the write commits".into());

    // c) Lost sidecar lineage: the collection's lineage is unknown afterwards,
    // so the approved binding no longer matches; bind and approve again.
    let (plan, rev) = approved_vocab(&mut s, "聞く", "listen");
    s.desktop.stop();
    let state = s.root.join("anki/b/linguist-anki-bridge-native");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(state.join(format!("native-sidecar.sqlite3{suffix}")));
    }
    s.desktop.launch(&[]);
    let (code, refused, stderr) = s.cli(&["apply", &plan, "--revision", &rev, "--apply"]);
    assert_ne!(code, 0, "{refused}");
    assert!(
        stderr.contains("APPLY_IDENTITY_MISMATCH"),
        "{stderr} {refused}"
    );
    assert_eq!(notes_tagged(&s, "聞く"), 0);
    let shown = s.ok(&["plans", "show", &plan]);
    let (revision, _) = s.bind_and_approve(&plan, &root_digest(&shown));
    applied(&s.ok(&[
        "apply",
        &plan,
        "--revision",
        &revision.to_string(),
        "--apply",
    ]));
    s.log
        .push("lost sidecar lineage: refused until bound and approved again".into());
    s.report("identity_preconditions");
}

/// A studied managed grammar note captured, split by the CLI into an anchor
/// and a fresh sibling, reviewed, bound and approved.
struct SplitReady {
    s: Scenario,
    plan: String,
    revision: u32,
    group: String,
    note_id: i64,
    before: Value,
}

fn split_ready(name: &str) -> SplitReady {
    let mut s = Scenario::new(name, "japanese_grammar", Some("Japanese::Grammar"), true);
    s.install_model("japanese_grammar");
    // A grammar note on the user's own (legacy) note type.
    s.desktop.call(
        "createModel",
        json!({"modelName": "Grammar Source", "inOrderFields": ["Pattern", "Meaning", "Formation", "Language"],
               "css": ".card {}", "cardTemplates": [{"Name": "Card 1", "Front": "{{Pattern}}",
               "Back": "{{Meaning}}<br>{{Formation}}"}]}),
    );
    let note_id = s.desktop.add_note(
        "Grammar Source",
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
        .push("purposes.japanese_grammar.source_model=Grammar Source".into());
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
    SplitReady {
        s,
        plan,
        revision,
        group,
        note_id,
        before,
    }
}

/// EV-09 partial-group crash: Anki exits right after the sibling is created.
/// After restart the unit is rebound and reconciled, and resuming the group
/// finishes the anchor without re-creating the sibling.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn grammar_split_crash_resume() {
    let SplitReady {
        mut s,
        plan,
        revision,
        group,
        note_id,
        before,
    } = split_ready("grammar-split-crash");
    let rev = revision.to_string();
    s.desktop
        .arm(json!({"point": "after_effect", "action": "crash", "variant": "create_note"}));
    let (code, partial, stderr) = s.cli(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--accept-schema-change",
        "--apply",
    ]);
    assert_ne!(code, 0, "{stderr} {partial}");
    s.desktop.wait_for_exit();
    s.desktop.restart();
    let siblings = |s: &Scenario| {
        s.desktop
            .call("findNotes", json!({"query": "Pattern:〜てもいい"}))
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(siblings(&s), 1, "the sibling was written before the crash");
    let (code, resumed, stderr) = s.cli(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--accept-schema-change",
        "--apply",
    ]);
    s.log.push(format!(
        "resume before reconcile -> {code}: {stderr} {resumed}"
    ));
    let pending = s.ok(&["recover", "inspect", "--pending"]);
    let operations: Vec<String> = pending["journals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|version| version["journal"]["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!operations.is_empty(), "{pending}");
    for operation in &operations {
        let reconciled = s.ok(&["recover", "reconcile", operation, "--rebind", "--apply"]);
        assert_eq!(
            reconciled["reconcile"]["state"], "committed",
            "{reconciled}"
        );
    }
    let finished = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--accept-schema-change",
        "--apply",
    ]);
    assert_eq!(finished["split"]["state"], "complete", "{finished}");
    assert_eq!(siblings(&s), 1, "never re-created");
    let anchor = s.desktop.note(note_id);
    assert!(
        !anchor["fields"]["Pattern"]
            .as_str()
            .unwrap()
            .contains("〜てもいい")
    );
    assert_eq!(anchor["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(
        anchor["cards"][0]["history_digest"],
        before["cards"][0]["history_digest"]
    );
    s.report("grammar_split_crash");
}

const BATCH_PICTURE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAQAAAADCAIAAAA7ljmRAAAAEElEQVR4nGM4YWMDRww4OQArRg8Bc3oMDAAAAABJRU5ErkJggg==";

/// Resolve every open review of a multi-item vocabulary revamp: native
/// history, then source pictures kept as pictures, then any templated choice.
/// Every open review issue of `plan` decided in one `plans resolve-batch`
/// call, with the choices `review_batch` makes one at a time.
fn resolve_with_batch(s: &mut Scenario, plan: &str) -> String {
    let mut template = s.ok(&[
        "plans",
        "resolve-batch",
        plan,
        "--template",
        "--actor",
        "desktop-scenario",
    ]);
    let shown = s.ok(&["plans", "show", plan]);
    for entry in template["decisions"].as_array_mut().unwrap() {
        let options = entry["options"].take();
        if entry.get("history_map").is_some() {
            entry["history_map"] = json!(["0=comprehension"]);
            continue;
        }
        if entry["expect_resolved"] == true {
            continue;
        }
        let issue = &options["issue"];
        entry["choice"] = match issue["code"].as_str().unwrap() {
            "SOURCE_MEDIA_CONTENT_REVIEW" => {
                let doc = shown["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|d| d["id"] == entry["document_id"])
                    .unwrap();
                let name = issue["field"].as_str().unwrap();
                let asset = doc["media"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["filename"] == name || m["original_filename"] == name)
                    .unwrap();
                let evidence = doc["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| {
                        e["field"] == "media_format" && e["target"]["digest"] == asset["digest"]
                    })
                    .unwrap()["id"]
                    .clone();
                json!({"decision": "source_media_role", "value": {
                    "source_id": issue["source_refs"][0], "asset_digest": asset["digest"],
                    "original_filename": name, "evidence_id": evidence, "role": "picture",
                    "attribution": "Disposable scenario picture.", "license": null}})
            }
            code => {
                let choice = options["templates"][0]["choice"].clone();
                assert!(!choice.is_null(), "no decision for {code}: {issue}");
                choice
            }
        };
    }
    let path = s.root.join(format!("batch-{}.json", Uuid::new_v4()));
    std::fs::write(&path, template.to_string()).unwrap();
    let (status, value, stderr) = s.cli(&[
        "plans",
        "resolve-batch",
        plan,
        "--decisions",
        path.to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{stderr} {value}");
    assert!(value["conflict"].is_null(), "{value}");
    s.log.push(format!(
        "resolve-batch: {} decisions in {} passes",
        value["applied"].as_array().unwrap().len(),
        value["passes"]
    ));
    value["digest"].as_str().unwrap().to_owned()
}

fn review_batch(s: &mut Scenario, plan: &str, anchor: Option<&str>) -> String {
    for _ in 0..60 {
        let page = s.ok(&["plans", "show", plan, "--issues-only"]);
        let entries = page["issues"].as_array().cloned().unwrap_or_default();
        let Some(entry) = entries.iter().find(|e| e["issue"]["severity"] == "review") else {
            let shown = s.ok(&["plans", "show", plan]);
            return root_digest(&shown);
        };
        let identity = &entry["request_identity"];
        let issue = &entry["issue"];
        let code = issue["code"].as_str().unwrap();
        let document = identity["document_id"].as_str().unwrap().to_owned();
        let choice = match code {
            "SOURCE_NATIVE_HISTORY_REVIEW" => {
                let (status, value, stderr) = s.cli(&[
                    "plans",
                    "resolve-history",
                    plan,
                    "--item",
                    &document,
                    "--map",
                    "0=comprehension",
                    "--actor",
                    "desktop-scenario",
                ]);
                assert!(status == 0 || status == 4, "{stderr} {value}");
                continue;
            }
            "SOURCE_MEDIA_CONTENT_REVIEW" => {
                let shown = s.ok(&["plans", "show", plan]);
                let doc = shown["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|d| d["id"] == document.as_str())
                    .unwrap()
                    .clone();
                let name = issue["field"].as_str().unwrap();
                let asset = doc["media"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["filename"] == name || m["original_filename"] == name)
                    .unwrap()
                    .clone();
                let evidence = doc["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| {
                        e["field"] == "media_format" && e["target"]["digest"] == asset["digest"]
                    })
                    .unwrap()["id"]
                    .clone();
                json!({"decision": "source_media_role", "value": {
                    "source_id": issue["source_refs"][0], "asset_digest": asset["digest"],
                    "original_filename": name, "evidence_id": evidence, "role": "picture",
                    "attribution": "Disposable scenario picture.", "license": null}})
            }
            "GRAMMAR_SPLIT_NATIVE_REVIEW" => {
                json!({"decision": "anchor", "value": anchor.unwrap()})
            }
            _ => entry["templates"][0]["choice"].clone(),
        };
        assert!(!choice.is_null(), "no decision for {code}: {entry}");
        let mut decision = identity.clone();
        decision["actor"] = json!("desktop-scenario");
        decision["choice"] = choice;
        let path = s.root.join(format!("decision-{}.json", Uuid::new_v4()));
        std::fs::write(&path, decision.to_string()).unwrap();
        let issue_id = identity["issue_id"].as_str().unwrap().to_owned();
        let (status, value, stderr) = s.cli(&[
            "plans",
            "resolve",
            plan,
            &issue_id,
            "--decision",
            path.to_str().unwrap(),
        ]);
        assert!(status == 0 || status == 4, "{code}: {stderr} {value}");
    }
    panic!("review did not converge");
}

/// WP-21: four studied notes revamped as one plan and one apply job with one
/// checkpoint. Anki crashes right after the first picture is stored (item 3).
/// After a restart item 3 is reconciled; the crashed session's checkpoint stops the
/// job before item 4, a follow-up job with a fresh checkpoint applies it,
/// nothing is duplicated, and two rollbacks restore every item.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn batch_vocab_revamp_apply_resume_rollback() {
    let mut s = Scenario::new(
        "batch_vocab_revamp",
        "japanese_vocab",
        Some("Japanese::Vocab"),
        true,
    );
    s.install_model("japanese_vocab");
    s.desktop.call(
        "createModel",
        json!({"modelName": "Batch Source", "inOrderFields": ["Front", "Back", "Picture"],
               "css": ".card {}", "cardTemplates": [{"Name": "Card 1", "Front": "{{Front}}",
               "Back": "{{Back}}<br>{{Picture}}"}]}),
    );
    s.desktop.call(
        "storeMediaFile",
        json!({"filename": "batch-picture.png", "data": BATCH_PICTURE}),
    );
    let words = [
        ("食べる", "to eat", false),
        ("飲む", "to drink", false),
        ("見る", "to see", true),
        ("聞く", "to hear", true),
    ];
    let mut notes = Vec::new();
    for (word, meaning, picture) in words {
        let picture = if picture {
            "<img src=\"batch-picture.png\">"
        } else {
            ""
        };
        let note = s.desktop.add_note(
            "Batch Source",
            "Default",
            json!({"Front": word, "Back": meaning, "Picture": picture}),
            &["legacy"],
        );
        s.desktop.study(note);
        notes.push((note, s.desktop.note(note)));
    }
    s.settings.push(format!(
        "purposes.japanese_vocab.fields={}",
        json!({"expression": "Front", "meaning": "Back", "picture": "Picture"})
    ));
    s.settings
        .push("purposes.japanese_vocab.source_model=Batch Source".into());
    let ids: Vec<String> = notes.iter().map(|(n, _)| n.to_string()).collect();
    // A durable prepare job captures and enriches every note.
    let mut args = vec!["jobs", "create", "--mode", "prepare"];
    for id in &ids {
        args.extend(["--note-id", id.as_str()]);
    }
    let queued = s.ok(&args);
    let prepare = queued["job_id"].as_str().unwrap().to_owned();
    let (code, drafted, stderr) = s.cli(&["jobs", "run", &prepare]);
    assert_eq!(code, 4, "{stderr} {drafted}");
    assert_eq!(drafted["item_counts"]["captured"], 4, "{drafted}");
    let plan = drafted["plan"]["id"].as_str().unwrap().to_owned();
    // One typed patch authors every item's sense key.
    let shown = s.ok(&["plans", "show", &plan]);
    let items: Vec<Value> = shown["documents"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, d)| json!({"document_id": d["id"], "fields": {"SenseKey": {"intent": "set", "value": format!("batch-{i}")}}}))
        .collect();
    assert_eq!(items.len(), 4, "{shown}");
    let patch = s.root.join("batch.patch.json");
    std::fs::write(
        &patch,
        json!({"schema_version": 2, "base_digest": root_digest(&shown), "items": items})
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
    let digest = resolve_with_batch(&mut s, &plan);
    let (revision, digest) = s.bind_and_approve(&plan, &digest);
    let rev = revision.to_string();
    let created = s.ok(&[
        "jobs",
        "create",
        "--mode",
        "apply",
        "--plan",
        &plan,
        "--revision",
        &rev,
        "--digest",
        &digest,
        "--create-checkpoint",
        "--accept-schema-change",
    ]);
    let job = created["job"]["job_id"].as_str().unwrap().to_owned();
    assert_eq!(created["job"]["item_count"], 4, "{created}");
    s.desktop
        .arm(json!({"point": "after_effect", "action": "crash", "variant": "store_media"}));
    let (code, crashed, stderr) = s.cli(&["jobs", "run", &job, "--apply"]);
    assert_ne!(code, 0, "{stderr} {crashed}");
    s.desktop.wait_for_exit();
    s.desktop.restart();
    let model = |s: &Scenario, note: i64| s.desktop.note(note)["model_name"].clone();
    assert_eq!(
        model(&s, notes[0].0),
        "Linguist Vocabulary v3",
        "item 1 before the crash"
    );
    assert_eq!(
        model(&s, notes[1].0),
        "Linguist Vocabulary v3",
        "item 2 before the crash"
    );
    // The interrupted item is reconciled, never re-sent blindly.
    let (code, resumed, stderr) = s.cli(&["jobs", "resume", &job, "--apply"]);
    s.log.push(format!(
        "resume before reconcile -> {code}: {stderr} {resumed}"
    ));
    let pending = s.ok(&["recover", "inspect", "--pending"]);
    for version in pending["journals"].as_array().unwrap() {
        let operation = version["journal"]["id"].as_str().unwrap();
        let reconciled = s.ok(&["recover", "reconcile", operation, "--rebind", "--apply"]);
        s.log.push(format!(
            "reconcile {operation} -> {}",
            reconciled["reconcile"]["state"]
        ));
    }
    // The checkpoint belonged to the crashed session: after accounting for
    // item 3 the job stops before dispatching item 4 and names a follow-up job.
    let (code, halted, stderr) = s.cli(&["jobs", "resume", &job, "--apply"]);
    assert_eq!(code, 5, "{stderr} {halted}");
    assert_eq!(
        halted["run"]["stop_code"], "JOB_CHECKPOINT_SESSION_ENDED",
        "{halted}"
    );
    assert_eq!(
        halted["run"]["status"]["counts"]["committed"], 3,
        "{halted}"
    );
    let follow_up = halted["run"]["next_commands"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let follow_up: Vec<&str> = follow_up.split(' ').skip(1).collect();
    assert_eq!(
        follow_up.iter().filter(|a| **a == "--item-id").count(),
        1,
        "{halted}"
    );
    let created = s.ok(&follow_up);
    let rest = created["job"]["job_id"].as_str().unwrap().to_owned();
    let finished = s.ok(&["jobs", "run", &rest, "--apply"]);
    assert_eq!(
        finished["run"]["status"]["counts"]["committed"], 1,
        "{finished}"
    );
    for ((note, before), (word, _, _)) in notes.iter().zip(words) {
        let after = s.desktop.note(*note);
        assert_eq!(after["model_name"], "Linguist Vocabulary v3", "{after}");
        assert_eq!(after["cards"][0]["id"], before["cards"][0]["id"]);
        assert_eq!(after["cards"][0]["review_count"], 1);
        assert_eq!(
            after["cards"][0]["history_digest"],
            before["cards"][0]["history_digest"]
        );
        let found = s
            .desktop
            .call("findNotes", json!({"query": format!("Expression:{word}")}));
        assert_eq!(found.as_array().unwrap().len(), 1, "no duplicate of {word}");
    }
    for id in [&job, &rest] {
        let audit = s.ok(&["jobs", "audit", id]);
        assert_eq!(audit["audit"]["issues"], json!([]), "{audit}");
    }
    // Each job rolls back as one group.
    for id in [&rest, &job] {
        let rolled = s.ok(&["jobs", "rollback", id, "--accept-schema-change", "--apply"]);
        s.log.push(format!("rollback {id} -> {rolled}"));
    }
    for (note, before) in &notes {
        let back = s.desktop.note(*note);
        assert_eq!(back["model_name"], "Batch Source", "{back}");
        assert_eq!(back["fields"], before["fields"]);
        assert_eq!(back["cards"][0]["id"], before["cards"][0]["id"]);
        assert_eq!(back["cards"][0]["review_count"], 1);
    }
    s.report("batch_vocab_revamp");
}

/// WP-20: one studied note holding two words is detected, split by
/// `plans split-vocab` into an anchor (keeps the note and history) and a new
/// sibling note, applied as one group, and rolled back as one group.
#[test]
#[ignore = "needs Anki desktop and the AnkiConnect zip; scripts/release-check.py runs it"]
fn vocab_split_apply_study_rollback() {
    let mut s = Scenario::new(
        "vocab_split",
        "japanese_vocab",
        Some("Japanese::Vocab"),
        true,
    );
    s.install_model("japanese_vocab");
    let note = s.desktop.add_note(
        "Basic",
        "Default",
        json!({"Front": "私<div>僕</div>", "Back": "わたし<div>ぼく</div>"}),
        &["legacy"],
    );
    s.desktop.study(note);
    let before = s.desktop.note(note);
    s.settings.push(format!(
        "purposes.japanese_vocab.fields={}",
        json!({"expression": "Front", "pronunciation": "Back"})
    ));
    s.settings
        .push("purposes.japanese_vocab.source_model=Basic".into());
    let (code, drafted, stderr) = s.cli(&["vocab", "revamp", "--note-id", &note.to_string()]);
    assert_eq!(code, 4, "{stderr} {drafted}");
    let root = if drafted["plan_id"].is_string() {
        &drafted
    } else {
        &drafted["result"]
    };
    let plan = root["plan_id"].as_str().unwrap().to_owned();
    let item = root["document_id"].as_str().unwrap().to_owned();
    let issues = s.ok(&["plans", "show", &plan, "--issues-only"]);
    assert!(
        issues.to_string().contains("VOCAB_SPLIT_REQUIRED") || {
            let shown = s.ok(&["plans", "validate", &plan]);
            shown.to_string().contains("VOCAB_SPLIT_REQUIRED")
        }
    );
    let mut request = s.ok(&["plans", "split-vocab", &plan, "--template", &item]);
    assert_eq!(request["units"][1]["expression"], "僕", "{request}");
    assert_eq!(request["units"][1]["pronunciation"], "ぼく", "{request}");
    request["actor"] = json!("desktop-scenario");
    let path = s.root.join("vocab-split.json");
    std::fs::write(&path, request.to_string()).unwrap();
    let (code, split, stderr) = s.cli(&[
        "plans",
        "split-vocab",
        &plan,
        "--request",
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{stderr} {split}");
    let group = split["split_groups"][0]["id"].as_str().unwrap().to_owned();
    // Author each unit's meaning and sense (authored dictionary policy).
    let shown = s.ok(&["plans", "show", &plan]);
    let items: Vec<Value> = shown["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            let word = d["content"]["body"]["expression"].as_str().unwrap();
            json!({"document_id": d["id"], "fields": {
                "Meaning": {"intent": "set", "value": format!("I ({word})")},
                "SenseKey": {"intent": "set", "value": format!("first-person-{word}")}}})
        })
        .collect();
    let patch = s.root.join("units.patch.json");
    std::fs::write(
        &patch,
        json!({"schema_version": 2, "base_digest": root_digest(&shown), "items": items})
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
    let anchor = split["split_groups"][0]["anchor_document"]
        .as_str()
        .unwrap()
        .to_owned();
    let digest = review_batch(&mut s, &plan, Some(&anchor));
    let (revision, _) = s.bind_and_approve(&plan, &digest);
    let rev = revision.to_string();
    let applied = s.ok(&[
        "apply",
        &plan,
        "--revision",
        &rev,
        "--split-group",
        &group,
        "--accept-schema-change",
        "--apply",
    ]);
    assert_eq!(applied["split"]["state"], "complete", "{applied}");
    let after = s.desktop.note(note);
    assert_eq!(after["model_name"], "Linguist Vocabulary v3", "{after}");
    assert_eq!(after["fields"]["Expression"], "私");
    assert_eq!(after["cards"][0]["id"], before["cards"][0]["id"]);
    assert_eq!(after["cards"][0]["review_count"], 1);
    let sibling = s
        .desktop
        .call("findNotes", json!({"query": "Expression:僕"}));
    assert_eq!(sibling.as_array().unwrap().len(), 1, "one new note for 僕");
    // WP-23: the sibling's Comprehension card carries the source card's schedule.
    let sibling_note = s.desktop.note(sibling[0].as_i64().unwrap());
    let card = sibling_note["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["ordinal"] == 0)
        .unwrap();
    assert_eq!(
        card["scheduler"],
        inherited(&before["cards"][0]["scheduler"]),
        "{sibling_note}"
    );
    let execution = applied["split"]["execution_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rolled = s.ok(&[
        "jobs",
        "rollback",
        &execution,
        "--accept-schema-change",
        "--delete-unstudied-created",
        "--apply",
    ]);
    s.log.push(format!("rollback -> {rolled}"));
    let back = s.desktop.note(note);
    assert_eq!(back["model_name"], "Basic");
    assert_eq!(back["fields"], before["fields"]);
    assert_eq!(back["cards"][0]["review_count"], 1);
    let gone = s
        .desktop
        .call("findNotes", json!({"query": "Expression:僕"}));
    assert_eq!(
        gone.as_array().unwrap().len(),
        0,
        "the unstudied sibling is removed"
    );
    s.report("vocab_split");
}
