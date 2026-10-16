//! WP-24 campaign commands against an in-process fake AnkiConnect: the
//! selection reads only, the stored campaign is driven by local commands,
//! the user-only steps refuse an agent, and the merge commands refuse
//! without the verified companion.
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

const BIN: &str = env!("CARGO_BIN_EXE_linguist-anki-bridge");

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

fn note(id: u64, word: &str, reading: &str) -> Value {
    json!({"noteId": id, "modelName": "2. Picture Words", "tags": [], "cards": [id * 10],
        "fields": {"Word": {"value": word, "order": 0}, "Picture": {"value": "<img src=\"p.jpg\">", "order": 1},
            "Pronunciation": {"value": format!("[sound:{id}.mp3]{reading}"), "order": 2}}})
}

fn collection(action: &str, params: &Value) -> Value {
    let notes = [
        note(1, "冷房", "れいぼう"),
        note(2, "神秘", "しんぴ"),
        note(3, "祖父 お祖父さん", "そふ"),
        note(4, "本", "ほん"),
        note(5, "本", "ほん"),
    ];
    match action {
        "getActiveProfile" => json!("Disposable"),
        "findNotes" => json!([1, 2, 3, 4, 5]),
        "notesInfo" => {
            let ids: Vec<u64> = params["notes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.parse().unwrap())
                        .or(v.as_u64())
                        .unwrap()
                })
                .collect();
            json!(
                notes
                    .iter()
                    .filter(|n| ids.contains(&n["noteId"].as_u64().unwrap()))
                    .collect::<Vec<_>>()
            )
        }
        "cardsInfo" => {
            let ids: Vec<u64> = params["cards"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.parse().unwrap())
                        .or(v.as_u64())
                        .unwrap()
                })
                .collect();
            json!(ids.iter().map(|c| json!({"cardId": c, "note": c / 10, "reps": if *c == 50 { 9 } else { 2 }})).collect::<Vec<_>>())
        }
        other => panic!("unexpected AnkiConnect action {other}"),
    }
}

struct Home(std::path::PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home() -> Home {
    let path = std::env::temp_dir().join(format!("lab-campaign-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    Home(path)
}

fn cli(home: &Home, endpoint: &str) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("HOME", &home.0)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .args([
            "--output",
            "json",
            "--set",
            &format!("anki.endpoint={endpoint}"),
            "--set",
            &format!("storage.state_dir={}", home.0.join("state").display()),
        ]);
    command
}

fn run(command: &mut Command) -> (i32, Value, String) {
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_str(stdout.trim()).unwrap_or(Value::Null),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn spec(home: &Home) -> std::path::PathBuf {
    let path = home.0.join("spec.json");
    std::fs::write(
        &path,
        json!({
            "schema_version": 1, "name": "cli test",
            "sources": [{"id": "ja", "query": "deck:森の言葉", "purpose": "japanese_vocab",
                "settings": ["audio.provider=dictionary"], "target_models": ["Linguist Vocabulary v3"],
                "word_field": "Word", "reading_field": "Pronunciation", "picture_field": "Picture",
                "audio_field": "Pronunciation", "language": "ja", "merge_duplicates": true}],
            "order": ["ja:plain", "ja:split_candidate", "ja:duplicate"],
            "batch_size": 10,
            "pilot": {"cohort": "ja:plain", "size": 1},
            "limits": {"agent_runs_per_day": 1}
        })
        .to_string(),
    )
    .unwrap();
    path
}

#[test]
fn campaign_create_reads_only_and_freezes_cohorts_batches_and_merges() {
    let home = home();
    let (endpoint, actions) = fake_anki(collection);
    let spec = spec(&home);
    let (code, dry, stderr) = run(cli(&home, &endpoint).args([
        "campaign",
        "create",
        "--spec",
        spec.to_str().unwrap(),
        "--dry-run",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        dry["selection"]["cohorts"],
        json!({"ja:duplicate": 2, "ja:plain": 2, "ja:split_candidate": 1})
    );
    assert_eq!(
        dry["selection"]["merge_groups"],
        json!([["5", "4"]]),
        "the most-studied note survives"
    );
    assert!(!home.0.join("state").exists(), "a dry run stores nothing");
    let (code, created, stderr) =
        run(cli(&home, &endpoint).args(["campaign", "create", "--spec", spec.to_str().unwrap()]));
    assert_eq!(code, 0, "{stderr}");
    let id = created["campaign"].as_str().unwrap().to_owned();
    assert!(std::path::Path::new(created["policy_path"].as_str().unwrap()).is_file());
    // Selection used only reads.
    assert!(actions.lock().unwrap().iter().all(|a| {
        ["getActiveProfile", "findNotes", "notesInfo", "cardsInfo"].contains(&a.as_str())
    }));
    let (_, listed, _) = run(cli(&home, &endpoint).args(["campaign", "list"]));
    assert_eq!(listed["campaigns"][0]["campaign"], id);
    let (code, status, stderr) = run(cli(&home, &endpoint).args(["campaign", "status"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(status["batches"], 4, "{status}");
    assert_eq!(status["phases"], json!({"selected": 4}));
    assert_eq!(status["agent_needed"], false);
    let (_, pilot, _) = run(cli(&home, &endpoint).args(["campaign", "batch", &id, "1"]));
    assert_eq!(pilot["data"]["pilot"], true);
    assert_eq!(pilot["data"]["notes"], json!(["1"]));
    let (_, merge, _) = run(cli(&home, &endpoint).args(["campaign", "batch", &id, "4"]));
    assert_eq!(merge["data"]["notes"], json!(["5"]));
    assert_eq!(merge["data"]["merge"], json!(["4"]));
    let (_, events, _) = run(cli(&home, &endpoint).args(["campaign", "events", &id]));
    assert_eq!(events["events"][0]["kind"], "created");

    // A key the spec sets per source cannot also be set globally.
    let (code, _, stderr) = run(cli(&home, &endpoint).args([
        "--set",
        "audio.provider=voicevox",
        "campaign",
        "create",
        "--spec",
        spec.to_str().unwrap(),
    ]));
    assert_eq!(code, 2);
    assert!(stderr.contains("CAMPAIGN_SETTING_DUPLICATE"), "{stderr}");
}

#[test]
fn user_only_steps_refuse_an_agent_and_local_controls_record_events() {
    let home = home();
    let (endpoint, _) = fake_anki(collection);
    let spec = spec(&home);
    let (_, created, _) =
        run(cli(&home, &endpoint).args(["campaign", "create", "--spec", spec.to_str().unwrap()]));
    let id = created["campaign"].as_str().unwrap().to_owned();
    // Approval and item decisions need a batch under review.
    let (code, _, stderr) = run(cli(&home, &endpoint).args([
        "campaign",
        "approve",
        &id,
        "1",
        "--actor",
        "claude-agent",
    ]));
    assert_ne!(code, 0);
    assert!(stderr.contains("CAMPAIGN_BATCH_NOT_READY"), "{stderr}");
    let (_, _, stderr) =
        run(cli(&home, &endpoint).args(["campaign", "item", &id, "1", "1", "--settled"]));
    assert!(stderr.contains("CAMPAIGN_BATCH_LOCKED"), "{stderr}");
    // Only an approved batch without an apply job reopens.
    let (_, _, stderr) =
        run(cli(&home, &endpoint).args(["campaign", "reopen", &id, "1", "--reason", "test"]));
    assert!(stderr.contains("CAMPAIGN_BATCH_LOCKED"), "{stderr}");
    // Skipping a batch before approval is allowed.
    let (code, skipped, stderr) =
        run(cli(&home, &endpoint).args(["campaign", "skip", &id, "2", "--reason", "later"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(skipped["phase"], "deferred");
    // The agent limit is one run per day here.
    let (code, slot, _) = run(cli(&home, &endpoint).args(["campaign", "agent-slot", &id]));
    assert_eq!((code, slot["granted"].clone()), (0, json!(true)));
    let (code, slot, _) = run(cli(&home, &endpoint).args(["campaign", "agent-slot", &id]));
    assert_eq!((code, slot["granted"].clone()), (5, json!(false)));
    // Throughput limits are tunable within range; the next slot follows them.
    let (code, _, stderr) =
        run(cli(&home, &endpoint).args(["campaign", "tune", &id, "--prepare-parallel", "9"]));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("CAMPAIGN_TUNING_INVALID"), "{stderr}");
    let (code, tuned, stderr) = run(cli(&home, &endpoint).args([
        "campaign",
        "tune",
        &id,
        "--agent-runs-per-day",
        "2",
        "--prepare-ahead",
        "3",
        "--prepare-parallel",
        "3",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(tuned["limits"]["prepare_parallel"], 3);
    let (code, slot, _) = run(cli(&home, &endpoint).args(["campaign", "agent-slot", &id]));
    assert_eq!((code, slot["granted"].clone()), (0, json!(true)));
    let (_, status, _) = run(cli(&home, &endpoint).args(["campaign", "status", &id]));
    assert_eq!(status["agent_active"], true);
    assert_eq!(status["agent_runs_per_day"], 2);
    let (code, _, stderr) = run(cli(&home, &endpoint).args(["campaign", "agent-done", &id]));
    assert_eq!(code, 0, "{stderr}");
    // Nothing is finished yet, so nothing goes round again.
    let (code, requeued, stderr) = run(cli(&home, &endpoint).args(["campaign", "requeue", &id]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(requeued["requeued"], json!([]));
    let (_, status, _) = run(cli(&home, &endpoint).args(["campaign", "status", &id]));
    assert_eq!(status["agent_active"], false);
    // Pause; a run does nothing; only the user resumes.
    let (code, _, _) =
        run(cli(&home, &endpoint).args(["campaign", "pause", &id, "--reason", "holiday"]));
    assert_eq!(code, 0);
    let (code, ran, stderr) = run(cli(&home, &endpoint).args(["campaign", "run", &id]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(ran["stopped"], "paused");
    assert_eq!(ran["steps"], json!([]));
    let (code, _, stderr) = run(cli(&home, &endpoint).args(["campaign", "resume", &id]));
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("CAMPAIGN_RESUME_BY_USER_REQUIRED"));
    let (code, resumed, _) =
        run(cli(&home, &endpoint).args(["campaign", "resume", &id, "--by-user"]));
    assert_eq!((code, resumed["status"].clone()), (0, json!("running")));
    // The timer is previewed without --execute.
    let script = home.0.join("tick.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let (code, units, stderr) = run(cli(&home, &endpoint).args([
        "campaign",
        "install-timer",
        &id,
        "--script",
        script.to_str().unwrap(),
        "--workdir",
        home.0.to_str().unwrap(),
        "--agent-script",
        script.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(units["written"], json!([]));
    assert_eq!(units["units"][2]["name"], "linguist-campaign-agent.service");
    assert!(
        units["units"][3]["body"]
            .as_str()
            .unwrap()
            .contains("OnUnitInactiveSec=5min")
    );
    assert!(
        units["units"][0]["body"]
            .as_str()
            .unwrap()
            .contains(&format!("LAB_CAMPAIGN={id}"))
    );
    assert!(
        units["units"][1]["body"]
            .as_str()
            .unwrap()
            .contains("Persistent=true")
    );
    // An unknown asset is refused; cancel is terminal.
    let (_, _, stderr) = run(cli(&home, &endpoint).args([
        "campaign",
        "asset",
        &"a".repeat(64),
        "--output",
        home.0.join("x.png").to_str().unwrap(),
    ]));
    assert!(stderr.contains("ASSET_NOT_FOUND"), "{stderr}");
    run(cli(&home, &endpoint).args(["campaign", "cancel", &id]));
    let (code, _, stderr) = run(cli(&home, &endpoint).args(["campaign", "close", &id]));
    assert_ne!(code, 0);
    assert!(stderr.contains("CAMPAIGN_TERMINAL"), "{stderr}");
}

#[test]
fn merge_commands_and_failed_item_publication_without_a_companion() {
    let home = home();
    let dead = "http://127.0.0.1:9";
    // Merge commands need a recorded plan and the verified companion.
    let plan = uuid::Uuid::new_v4().to_string();
    let (code, _, stderr) = run(cli(&home, dead).args([
        "notes",
        "delete-merged",
        "5",
        "--survivor",
        "4",
        "--plan",
        &plan,
    ]));
    assert_ne!(code, 0);
    assert!(
        stderr.contains("STORE_NOT_FOUND") || stderr.contains("PLAN_NOT_FOUND"),
        "{stderr}"
    );
    let (code, _, stderr) = run(cli(&home, dead).args(["notes", "restore-merged", "5"]));
    assert_ne!(code, 0, "{stderr}");
    // A prepare job whose only note could not be read publishes nothing.
    let (code, created, stderr) = run(cli(&home, dead).args([
        "--purpose",
        "japanese_vocab",
        "jobs",
        "create",
        "--note-id",
        "123",
    ]));
    assert_eq!(code, 0, "{stderr}");
    let job = created["job_id"].as_str().unwrap().to_owned();
    let (code, _, _) = run(cli(&home, dead).args(["jobs", "run", &job]));
    assert_ne!(code, 0);
    let (code, published, stderr) =
        run(cli(&home, dead).args(["jobs", "publish", &job, "--skip-failed"]));
    assert_eq!(code, 5, "{stderr}");
    assert_eq!(published["plan_published"], false);
    assert_eq!(published["skipped"][0]["note_id"], "123");
    let (code, _, stderr) = run(cli(&home, dead).args([
        "plans",
        "merge-notes",
        &plan,
        "--item",
        &plan,
        "--note",
        "5",
    ]));
    assert_ne!(code, 0);
    assert!(stderr.contains("PLAN_NOT_FOUND"), "{stderr}");
}

#[test]
fn wrap_up_pauses_closes_the_agent_turn_and_ends_orphaned_agents() {
    use std::os::unix::process::CommandExt;
    let home = home();
    let (endpoint, _) = fake_anki(collection);
    let spec = spec(&home);
    let (_, created, _) =
        run(cli(&home, &endpoint).args(["campaign", "create", "--spec", spec.to_str().unwrap()]));
    let id = created["campaign"].as_str().unwrap().to_owned();
    let (code, _, _) = run(cli(&home, &endpoint).args(["campaign", "agent-slot", &id]));
    assert_eq!(code, 0);
    // An agent whose generation process is gone: its own process group, in
    // a private linguist-agent-* directory.
    let workdir =
        std::env::temp_dir().join(format!("linguist-agent-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&workdir).unwrap();
    let mut agent = Command::new("sleep")
        .arg("60")
        .current_dir(&workdir)
        .process_group(0)
        .spawn()
        .unwrap();
    let (code, handoff, stderr) = run(cli(&home, &endpoint)
        .env("TMPDIR", std::env::temp_dir())
        .args(["campaign", "wrap-up", &id, "--wait-seconds", "5"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(handoff["was"], "running");
    assert_eq!(handoff["status"], "paused");
    assert_eq!(handoff["tick"], "stopped");
    assert!(
        handoff["agents_ended"]
            .as_array()
            .unwrap()
            .contains(&json!(agent.id())),
        "{handoff}"
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(agent.try_wait().unwrap().is_some(), "the orphan was ended");
    let _ = std::fs::remove_dir_all(&workdir);
    let (_, status, _) = run(cli(&home, &endpoint).args(["campaign", "status", &id]));
    assert_eq!(
        status["agent_active"], false,
        "the open agent turn is closed"
    );
    assert!(
        status["detail"].as_str().unwrap().contains("wrapped up"),
        "{status}"
    );
    let (_, events, _) = run(cli(&home, &endpoint).args(["campaign", "events", &id]));
    assert!(events.to_string().contains("wrapped_up"), "{events}");
    // A paused campaign keeps its reason; only the user resumes it.
    let (code, _, _) =
        run(cli(&home, &endpoint).args(["campaign", "pause", &id, "--reason", "quota"]));
    assert_eq!(code, 0);
    let (code, again, _) = run(cli(&home, &endpoint).args(["campaign", "wrap-up", &id]));
    assert_eq!((code, again["was"].clone()), (0, json!("paused")));
    assert_eq!(again["detail"], "quota");
}
