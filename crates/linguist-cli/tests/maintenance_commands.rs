//! WP-14 commands end to end: legacy config import and activation, cache,
//! resources, legacy job import, diagnostics log and setting coverage.
use std::{path::PathBuf, process::Command};
use uuid::Uuid;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("lab-maint-cli-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    Fixture(root)
}
fn bare(f: &Fixture) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.args(["--output", "json"]);
    c.env_clear();
    c.env("HOME", f.0.join("home"));
    c
}
fn cli(f: &Fixture) -> Command {
    let mut c = bare(f);
    c.arg("--config").arg(f.0.join("config.toml"));
    c
}
fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(out)))
}
fn write_config(f: &Fixture, extra: &str) {
    std::fs::write(
        f.0.join("config.toml"),
        format!(
            "[config]\nversion = 2\n[storage]\nstate_dir = \"{0}/state\"\ncache_dir = \"{0}/cache\"\nresource_dir = \"{0}/resources\"\ntemp_dir = \"{0}/tmp\"\nbackup_dir = \"{0}/backups\"\n{extra}",
            f.0.display()
        ),
    )
    .unwrap();
}

#[test]
fn legacy_config_import_then_guarded_activation() {
    let f = fixture();
    let legacy = f.0.join("config.yaml");
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../linguist-config/tests/fixtures/legacy/python-full.yaml"
    ))
    .unwrap();
    std::fs::write(&legacy, &bytes).unwrap();
    let candidate = f.0.join("import/candidate.toml");
    let out = cli(&f)
        .args(["config", "import", "--file"])
        .arg(&legacy)
        .arg("--output")
        .arg(&candidate)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let receipt = json(&out.stdout);
    assert_eq!(receipt["activation_blocked"], true);
    assert_eq!(receipt["activated"], false);
    assert_eq!(std::fs::read(&legacy).unwrap(), bytes, "source unchanged");
    assert!(!f.0.join("config.toml").exists(), "live config untouched");
    // The candidate validates as an ordinary config file.
    let valid = bare(&f)
        .args(["config", "validate", "--file"])
        .arg(&candidate)
        .output()
        .unwrap();
    assert!(json(&valid.stdout)["valid"] == true, "{valid:?}");
    // Activation is blocked until each unresolved key is accepted.
    let blocked = cli(&f)
        .args(["config", "migrate", "--from-import"])
        .arg(&candidate)
        .arg("--execute")
        .output()
        .unwrap();
    assert_eq!(blocked.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("IMPORT_ACTIVATION_BLOCKED"));
    assert!(!f.0.join("config.toml").exists());
    let mut accept = cli(&f);
    accept
        .args(["config", "migrate", "--from-import"])
        .arg(&candidate);
    for key in receipt["blocking_keys"].as_array().unwrap() {
        accept.args(["--accept-unresolved", key.as_str().unwrap()]);
    }
    let preview = accept.output().unwrap();
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(json(&preview.stdout)["activation"]["executed"], false);
    assert!(!f.0.join("config.toml").exists());
    let activated = accept.arg("--execute").output().unwrap();
    assert!(activated.status.success(), "{activated:?}");
    assert_eq!(json(&activated.stdout)["activation"]["created"], true);
    assert_eq!(
        std::fs::read(f.0.join("config.toml")).unwrap(),
        std::fs::read(&candidate).unwrap()
    );
    let show = cli(&f)
        .args(["config", "show", "llm.model"])
        .output()
        .unwrap();
    assert_eq!(json(&show.stdout)["values"]["llm.model"], "gemma3:12b");
}

#[test]
fn cache_status_and_prune_report_reachability_without_creating_state() {
    let f = fixture();
    write_config(&f, "");
    let status = cli(&f).args(["cache", "status"]).output().unwrap();
    assert!(status.status.success(), "{status:?}");
    assert_eq!(json(&status.stdout)["store_assets"]["state"], "absent");
    assert!(!f.0.join("state").exists());
    let prune = cli(&f).args(["cache", "prune"]).output().unwrap();
    assert!(prune.status.success(), "{prune:?}");
    assert_eq!(json(&prune.stdout)["executed"], false);
    let mut store = linguist_store::Store::open(&f.0.join("state")).unwrap();
    store.publish_asset(b"fresh orphan", 1024).unwrap();
    drop(store);
    let execute = cli(&f)
        .args(["cache", "prune", "--execute", "--age-days", "0"])
        .output()
        .unwrap();
    assert!(execute.status.success(), "{execute:?}");
    let receipt = json(&execute.stdout);
    assert_eq!(
        receipt["store_assets"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "orphan grace protects new assets"
    );
}

#[test]
fn resources_install_and_list_through_the_cli() {
    let f = fixture();
    write_config(&f, "");
    let pack = f.0.join("vie.traineddata");
    std::fs::write(&pack, b"vietnamese pack").unwrap();
    let digest = linguist_core::canonical::asset_digest(b"vietnamese pack");
    let install = |execute: bool| {
        let mut c = cli(&f);
        c.args(["resources", "install", "tesseract:vie", "--source"])
            .arg(&pack)
            .args([
                "--version",
                "4.1.0",
                "--sha256",
                &digest,
                "--license",
                "Apache-2.0",
            ]);
        if execute {
            c.arg("--execute");
        }
        c.output().unwrap()
    };
    let preview = install(false);
    assert!(preview.status.success(), "{preview:?}");
    assert!(!f.0.join("resources").exists());
    let done = install(true);
    assert!(done.status.success(), "{done:?}");
    assert!(f.0.join("resources/tessdata/vie.traineddata").is_file());
    let list = cli(&f)
        .args(["resources", "list", "--installed"])
        .output()
        .unwrap();
    assert_eq!(json(&list.stdout)["installed"][0]["status"], "verified");
    let offline = cli(&f)
        .args([
            "--offline",
            "resources",
            "install",
            "tesseract:jpn",
            "--source",
            "https://github.com/x/jpn.traineddata",
            "--version",
            "1",
            "--sha256",
            &digest,
            "--license",
            "Apache-2.0",
        ])
        .output()
        .unwrap();
    assert_eq!(offline.status.code(), Some(2), "{offline:?}");
    assert!(String::from_utf8_lossy(&offline.stderr).contains("RESOURCE_OFFLINE"));
}

#[test]
fn legacy_jobs_migrate_previews_imports_and_lists_read_only_history() {
    let f = fixture();
    write_config(&f, "");
    let legacy = f.0.join("batch_jobs.sqlite3");
    let db = rusqlite::Connection::open(&legacy).unwrap();
    db.execute_batch(
        "CREATE TABLE batch_jobs (id TEXT PRIMARY KEY, deck_key TEXT NOT NULL, deck_name TEXT NOT NULL, status TEXT NOT NULL, dry_run INTEGER NOT NULL DEFAULT 0, settings_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, last_error TEXT NOT NULL DEFAULT '');
         CREATE TABLE batch_items (id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT NOT NULL, ordinal INTEGER NOT NULL, note_id INTEGER NOT NULL, word TEXT NOT NULL, status TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, next_attempt_at TEXT, artifact_path TEXT, snapshot_id TEXT, result_note_id INTEGER, last_error TEXT NOT NULL DEFAULT '', started_at TEXT, finished_at TEXT, updated_at TEXT NOT NULL);
         INSERT INTO batch_jobs VALUES('batch-1','japanese_vocab','森','paused',0,'{}','t','t',NULL,NULL,'');
         INSERT INTO batch_items(job_id,ordinal,note_id,word,status,updated_at) VALUES('batch-1',0,11,'語','committing','t');",
    )
    .unwrap();
    drop(db);
    let original = std::fs::read(&legacy).unwrap();
    let preview = cli(&f)
        .args(["jobs", "migrate", "--legacy"])
        .arg(&legacy)
        .output()
        .unwrap();
    assert_eq!(preview.status.code(), Some(4), "{preview:?}");
    assert_eq!(
        json(&preview.stdout)["report"]["requires_review"][0]["code"],
        "LEGACY_COMMIT_OUTCOME_UNKNOWN"
    );
    assert!(!f.0.join("state").exists(), "preview writes no state");
    let imported = cli(&f)
        .args(["jobs", "migrate", "--execute", "--legacy"])
        .arg(&legacy)
        .output()
        .unwrap();
    assert_eq!(imported.status.code(), Some(4), "{imported:?}");
    let id = json(&imported.stdout)["import_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(std::fs::read(&legacy).unwrap(), original);
    let list = cli(&f)
        .args(["jobs", "migrate", "--list-imports"])
        .output()
        .unwrap();
    assert_eq!(json(&list.stdout)["imports"][0]["import_id"], id);
    let show = cli(&f)
        .args(["jobs", "migrate", "--show-import", &id])
        .output()
        .unwrap();
    assert_eq!(json(&show.stdout)["report"]["jobs"][0]["runnable"], false);
}

#[test]
fn diagnostic_log_follows_logging_settings_and_never_creates_state() {
    let f = fixture();
    write_config(&f, "");
    let ok = cli(&f).args(["cache", "status"]).output().unwrap();
    assert!(ok.status.success());
    assert!(
        !f.0.join("state").exists(),
        "no state is created just to log"
    );
    drop(linguist_store::Store::open(&f.0.join("state")).unwrap());
    cli(&f).args(["cache", "status"]).output().unwrap();
    cli(&f)
        .args(["resources", "install", "bogus"])
        .output()
        .unwrap();
    let failing = cli(&f)
        .args([
            "resources",
            "install",
            "file:x",
            "--source",
            "https://user:secret@github.com/x",
            "--version",
            "1",
            "--sha256",
            &"a".repeat(64),
            "--license",
            "MIT",
        ])
        .output()
        .unwrap();
    assert_eq!(failing.status.code(), Some(2));
    let log = std::fs::read_to_string(f.0.join("state/logs/cli.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["command"], "cache status");
    assert_eq!(lines[0]["exit_code"], 0);
    assert_eq!(
        lines.last().unwrap()["error_code"],
        "RESOURCE_SOURCE_CREDENTIALS"
    );
    assert!(
        !log.contains("secret") && !log.contains("error_message"),
        "no payloads by default"
    );
    // Errors-only level skips successes; file logging can be disabled.
    write_config(
        &f,
        "[logging]\nlevel = \"error\"\ninclude_private_payloads = true\n",
    );
    cli(&f).args(["cache", "status"]).output().unwrap();
    cli(&f)
        .args([
            "resources",
            "install",
            "file:x",
            "--source",
            "https://user:secret@github.com/x",
            "--version",
            "1",
            "--sha256",
            &"a".repeat(64),
            "--license",
            "MIT",
        ])
        .output()
        .unwrap();
    let after = std::fs::read_to_string(f.0.join("state/logs/cli.jsonl")).unwrap();
    let added: Vec<&str> = after.lines().skip(lines.len()).collect();
    assert_eq!(added.len(), 1);
    assert!(added[0].contains("error_message") && !added[0].contains("secret"));
    write_config(&f, "[logging]\nfile_enabled = false\n");
    cli(&f).args(["cache", "status"]).output().unwrap();
    assert_eq!(
        std::fs::read_to_string(f.0.join("state/logs/cli.jsonl")).unwrap(),
        after
    );
    // Rotation keeps logging.retained_files files of at most logging.max_file_mb.
    write_config(&f, "[logging]\nmax_file_mb = 1\nretained_files = 2\n");
    let big = "x".repeat(1024 * 1024);
    std::fs::write(f.0.join("state/logs/cli.jsonl"), &big).unwrap();
    cli(&f).args(["cache", "status"]).output().unwrap();
    assert_eq!(
        std::fs::read_to_string(f.0.join("state/logs/cli.1.jsonl")).unwrap(),
        big
    );
    assert!(
        std::fs::read_to_string(f.0.join("state/logs/cli.jsonl"))
            .unwrap()
            .lines()
            .count()
            == 1
    );
}

#[test]
fn unavailable_features_are_reported_or_refused_never_ignored() {
    let f = fixture();
    write_config(
        &f,
        "[selection]\nduplicate_policy = \"skip_exact\"\n[browser]\nmax_pages = 5\n",
    );
    let validate = cli(&f).args(["config", "validate"]).output().unwrap();
    assert_eq!(validate.status.code(), Some(3));
    let keys: Vec<String> = json(&validate.stdout)["unavailable_settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        keys,
        vec!["browser.max_pages", "selection.duplicate_policy"]
    );
    let refused = cli(&f)
        .args([
            "--purpose",
            "japanese_vocab",
            "vocab",
            "add",
            "--expression",
            "食べる",
        ])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(3), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("selection.duplicate_policy=skip_exact")
    );
    let describe = cli(&f)
        .args(["config", "describe", "browser.max_pages"])
        .output()
        .unwrap();
    assert_eq!(json(&describe.stdout)["coverage"]["status"], "gated");
    write_config(&f, "[anki]\nnative_adapter = \"other-v2\"\n");
    let apply = cli(&f)
        .args(["apply", &Uuid::new_v4().to_string()])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&apply.stderr).contains("NATIVE_ADAPTER_UNSUPPORTED"),
        "{apply:?}"
    );
}

#[test]
fn backup_preview_defaults_to_the_configured_backup_dir() {
    let f = fixture();
    write_config(&f, "");
    let missing = cli(&f)
        .args(["backup", "create", "--scope", "affected"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("storage.backup_dir"),
        "{missing:?}"
    );
    std::fs::create_dir(f.0.join("backups")).unwrap();
    let preview = cli(&f)
        .args(["backup", "create", "--scope", "affected"])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let body = json(&preview.stdout);
    assert_eq!(body["output_source"], "storage.backup_dir");
    assert!(
        body["output"]
            .as_str()
            .unwrap()
            .starts_with(&f.0.join("backups").display().to_string())
    );
}
