//! WP-15 release UX checks of the built binary: fresh local setup, help,
//! offline authored preparation, pipes, JSONL, Unicode, SSH/no-terminal
//! sessions, interruption exits, dependency guidance, conservative batch and
//! free-space limits. Every command runs with a cleared environment and a
//! private HOME; nothing contacts Anki, Ollama or the internet (Anki and
//! Ollama endpoints point at a closed loopback port).
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_linguist-anki-bridge");
/// Authored, offline preparation: no dictionary, generation, kanji or image reads.
const AUTHORED: [&str; 11] = [
    "--offline",
    "--set",
    "dictionary.provider=authored",
    "--set",
    "llm.enabled=false",
    "--set",
    "images.search_when_missing=false",
    "--set",
    "kanji.enabled=false",
    "--set",
    "anki.endpoint=http://127.0.0.1:9",
];

struct Home(PathBuf);
impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lab-release-ux-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .env_clear()
            .env("HOME", &self.0)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null());
        command
    }
    fn state(&self) -> PathBuf {
        self.0.join(".local/state/linguist-anki-bridge")
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stderr_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stderr)
        .unwrap_or_else(|_| panic!("stderr is not one JSON value: {output:?}"))
}

fn vocab_line(index: usize) -> String {
    serde_json::json!({
        "schema_version": 2,
        "kind": "vocabulary",
        "target_language": "ja",
        "body": {"expression": format!("語{index}"), "meaning": format!("word {index}"), "sense_key": format!("w{index}")}
    })
    .to_string()
}

/// Recursively collect `--help` pages: (command path, help text).
fn help_pages(home: &Home, path: Vec<String>, out: &mut Vec<(Vec<String>, String)>) {
    let output = home.cli().args(&path).arg("--help").output().unwrap();
    assert!(output.status.success(), "{path:?}: {output:?}");
    assert!(output.stderr.is_empty(), "{path:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let mut in_commands = false;
    let mut children = Vec::new();
    for line in text.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if !line.starts_with("  ") {
                in_commands = false;
                continue;
            }
            let name = line.split_whitespace().next().unwrap();
            if name != "help" {
                children.push(name.to_owned());
            }
        }
    }
    out.push((path.clone(), text));
    for child in children {
        let mut next = path.clone();
        next.push(child);
        help_pages(home, next, out);
    }
}

#[test]
fn every_command_and_argument_has_help_text() {
    let home = Home::new("help");
    let mut pages = Vec::new();
    help_pages(&home, vec![], &mut pages);
    assert!(pages.len() > 60, "{}", pages.len());
    let mut missing = Vec::new();
    for (path, text) in &pages {
        let lines: Vec<&str> = text.lines().collect();
        let mut section = "";
        for (index, line) in lines.iter().enumerate() {
            if let Some(name) = ["Commands:", "Options:", "Arguments:"]
                .iter()
                .find(|name| line.starts_with(**name))
            {
                section = name;
                continue;
            }
            if !line.starts_with("  ") || line.trim().is_empty() {
                if !line.starts_with(' ') {
                    section = "";
                }
                continue;
            }
            if section.is_empty() || line.starts_with("          ") {
                continue;
            }
            let trimmed = line.trim_start();
            // Name column ends at the first run of two spaces.
            let (name, description) = match trimmed.find("  ") {
                Some(split) => (&trimmed[..split], trimmed[split..].trim()),
                None => (trimmed, ""),
            };
            if name == "help" || name.starts_with("-h, --help") {
                continue;
            }
            let long_form = lines
                .get(index + 1)
                .is_some_and(|next| next.starts_with("          ") && !next.trim().is_empty());
            if description.is_empty() && !long_form {
                missing.push(format!("{} {name}", path.join(" ")));
            }
        }
    }
    assert!(missing.is_empty(), "missing help: {missing:#?}");
}

#[test]
fn fresh_home_setup_and_offline_authored_preparation() {
    let home = Home::new("fresh");
    // Help, version and completions need no configuration or state.
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["completions", "bash"],
    ] {
        let output = home.cli().args(&args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);
    let version = home.cli().arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("linguist-anki-bridge {}", env!("CARGO_PKG_VERSION"))
    );

    // Local doctor never contacts services and writes nothing.
    let doctor = home
        .cli()
        .args(["--output", "json", "--offline", "doctor", "--local"])
        .output()
        .unwrap();
    assert!(doctor.status.success(), "{doctor:?}");
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(report["services_probed"], false);
    assert_eq!(report["collection_writes_enabled"], false);
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);

    // config init writes a private minimal file and validates.
    let init = home.cli().args(["config", "init"]).output().unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = home.0.join(".config/linguist-anki-bridge/config.toml");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let validate = home.cli().args(["config", "validate"]).output().unwrap();
    assert!(validate.status.success(), "{validate:?}");

    // Offline authored preparation, then review commands on the stored plan.
    let add = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args([
            "vocab",
            "add",
            "--expression",
            "食べる",
            "--meaning",
            "to eat",
            "--sense-key",
            "eat",
            "--target-language",
            "ja",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    let add: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    assert_eq!(add["ready"], true, "{add}");
    let plan = add["plan_id"].as_str().unwrap();
    let digest = add["digest"].as_str().unwrap();
    for args in [
        vec!["plans", "list"],
        vec!["plans", "show", plan],
        vec!["plans", "validate", plan],
    ] {
        let output = home
            .cli()
            .args(["--output", "json"])
            .args(AUTHORED)
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    let approve = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args([
            "plans",
            "approve",
            plan,
            "--revision",
            "1",
            "--digest",
            digest,
            "--actor",
            "release-check",
        ])
        .output()
        .unwrap();
    assert!(approve.status.success(), "{approve:?}");
    // Apply is a preview; the write stays unavailable with guidance.
    let preview = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args(["apply", plan, "--revision", "1"])
        .output()
        .unwrap();
    // Exit 4: the preview lists blockers. CLI preparation records no
    // collection binding and this setup maps no target deck.
    assert_eq!(preview.status.code(), Some(4), "{preview:?}");
    let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["collection_writes_enabled"], false);
    let blockers = preview["items"][0]["blockers"].to_string();
    assert!(blockers.contains("APPLY_BINDING_WEAK"), "{blockers}");
    let write = home
        .cli()
        .args(AUTHORED)
        .args(["apply", plan, "--revision", "1", "--apply"])
        .output()
        .unwrap();
    assert_eq!(write.status.code(), Some(3), "{write:?}");
    let error = stderr_json(&write);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("CAPABILITY_UNAVAILABLE"),
        "{error}"
    );
    assert!(error["next"].as_str().unwrap().contains("native adapter"));
    // Export works offline as the alternative to apply.
    let bundle = home.0.join("plan.bundle.json");
    let export = home
        .cli()
        .args(AUTHORED)
        .args(["plans", "export", plan, "--output"])
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(export.status.success(), "{export:?}");
    assert!(bundle.is_file());
}

#[test]
fn offline_dictionary_lookup_explains_the_authored_alternative() {
    let home = Home::new("offline-dictionary");
    let output = home
        .cli()
        .args([
            "--offline",
            "--set",
            "llm.enabled=false",
            "--set",
            "images.search_when_missing=false",
            "--set",
            "kanji.enabled=false",
            "vocab",
            "add",
            "--expression",
            "食べる",
            "--meaning",
            "to eat",
            "--sense-key",
            "eat",
            "--target-language",
            "ja",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let error = stderr_json(&output);
    // The default provider's cache path resolves (no policy refusal); only
    // --offline stops the uncached lookup.
    assert_eq!(
        error["error"], "DICTIONARY_PROVIDER_FAILED: PROVIDER_READ_Offline",
        "{error}"
    );
    assert!(
        error["next"]
            .as_str()
            .unwrap()
            .contains("dictionary.provider=authored")
    );
    assert!(!home.state().join("state.sqlite3").exists());
}

#[test]
fn ssh_no_terminal_and_dumb_terminal_output_is_plain() {
    let home = Home::new("ssh");
    for args in [
        vec!["--help"],
        vec!["config", "show", "--defaults", "output"],
        vec!["doctor", "--local"],
        vec!["models", "builtin"],
    ] {
        let output = home
            .cli()
            .env("SSH_CONNECTION", "192.0.2.1 50000 192.0.2.2 22")
            .env("SSH_TTY", "/dev/pts/9")
            .env("TERM", "dumb")
            .env("LANG", "C")
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(
            !output.stdout.contains(&0x1b),
            "{args:?} emitted escape codes"
        );
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
    // Forced colour is honoured only when asked for, still without a terminal.
    let colored = home
        .cli()
        .args([
            "--set",
            "output.color=always",
            "config",
            "show",
            "output.color",
        ])
        .output()
        .unwrap();
    assert!(colored.status.success(), "{colored:?}");
}

#[test]
fn jsonl_unicode_paths_and_piped_documents_round_trip() {
    let home = Home::new("unicode");
    let state = home.0.join("状態 データ/ベトナム");
    let state_flag = format!("storage.state_dir={}", state.display());
    let add = home
        .cli()
        .args(["--output", "jsonl", "--set", &state_flag])
        .args(AUTHORED)
        .args([
            "vocab",
            "add",
            "--expression",
            "勉強する",
            "--meaning",
            "học tập — to study ✓",
            "--sense-key",
            "study",
            "--target-language",
            "ja",
            "--tag",
            "日本語",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    let text = String::from_utf8(add.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "jsonl is one line: {text}");
    let value: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    let plan = value["plan_id"].as_str().unwrap().to_owned();
    assert!(state.join("state.sqlite3").is_file());
    let show = home
        .cli()
        .args(["--output", "json", "--set", &state_flag])
        .args(AUTHORED)
        .args(["plans", "show", &plan])
        .output()
        .unwrap();
    assert!(show.status.success(), "{show:?}");
    let shown = String::from_utf8(show.stdout).unwrap();
    assert!(shown.contains("勉強する") && shown.contains("học tập — to study ✓"));
    // Human output keeps Unicode intact.
    let human = home
        .cli()
        .args(["--set", &state_flag])
        .args(AUTHORED)
        .args(["plans", "show", &plan])
        .output()
        .unwrap();
    assert!(
        String::from_utf8(human.stdout)
            .unwrap()
            .contains("勉強する")
    );

    // A v2 document piped on stdin.
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/v2/fixtures/vocabulary.json");
    let mut child = home
        .cli()
        .args(["--output", "jsonl", "document", "validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&std::fs::read(fixture).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);

    // Non-UTF-8 arguments are a structured usage error, not a panic.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let output = home
            .cli()
            .args(["notes", "show"])
            .arg(std::ffi::OsStr::from_bytes(b"\xff\xfe"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stderr_json(&output)["usage_kind"], "invalid_utf8");
    }
}

#[test]
fn closed_stdout_pipe_exits_quietly() {
    let home = Home::new("pipe");
    for args in [
        vec!["--help"],
        vec!["config", "show", "--defaults"],
        vec!["--output", "json", "models", "builtin"],
        vec!["completions", "zsh"],
    ] {
        let mut child = home
            .cli()
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Read one byte, then close the read end like `| head -c1`.
        let mut stdout = child.stdout.take().unwrap();
        let mut byte = [0u8; 1];
        let _ = stdout.read(&mut byte);
        drop(stdout);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
}

#[cfg(unix)]
fn interrupt(signal: i32, expected: i32) {
    let home = Home::new(&format!("signal-{signal}"));
    // One committed plan exists before the interruption.
    let add = home
        .cli()
        .args(AUTHORED)
        .args([
            "vocab",
            "add",
            "--expression",
            "犬",
            "--meaning",
            "dog",
            "--sense-key",
            "dog",
            "--target-language",
            "ja",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "{add:?}");
    // A command blocked reading a never-closed stdin pipe.
    let mut child = home
        .cli()
        .args(AUTHORED)
        .args(["vocab", "add", "--document", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(vocab_line(1).as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    // SAFETY: plain kill(2) on our own child process.
    assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
    let started = Instant::now();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(output.status.code(), Some(expected), "{output:?}");
    let error = stderr_json(&output);
    assert!(
        error["error"].as_str().unwrap().starts_with("INTERRUPTED"),
        "{error}"
    );
    assert!(error["next"].as_str().unwrap().contains("recover inspect"));
    // The committed plan is intact and state reopens normally.
    let list = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args(["plans", "list"])
        .output()
        .unwrap();
    assert!(list.status.success(), "{list:?}");
    let list: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(list["revisions"].as_array().unwrap().len(), 1, "{list}");
    let pending = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args(["recover", "inspect", "--pending"])
        .output()
        .unwrap();
    assert!(pending.status.success(), "{pending:?}");
}

#[cfg(unix)]
#[test]
fn sigint_exits_130_and_sigterm_exits_143_without_damaging_state() {
    interrupt(libc::SIGINT, 130);
    interrupt(libc::SIGTERM, 143);
}

#[test]
fn dependency_and_capability_errors_name_the_next_step() {
    let home = Home::new("guidance");
    let cases: Vec<(Vec<&str>, i32, &str, &str)> = vec![
        (
            vec!["--set", "anki.endpoint=http://127.0.0.1:9", "decks", "list"],
            3,
            "ANKI_DEPENDENCY_UNAVAILABLE",
            "AnkiConnect",
        ),
        (
            vec![
                "--set",
                "llm.endpoint=http://127.0.0.1:9",
                "doctor",
                "--ollama",
            ],
            6,
            "OLLAMA_",
            "ollama serve",
        ),
        (
            vec![
                "--set",
                "anki.endpoint=http://127.0.0.1:9",
                "backup",
                "create",
                "--scope",
                "affected",
                "--output",
                "/tmp/lab-release-ux-never.colpkg",
                "--apply",
            ],
            3,
            "CAPABILITY_UNAVAILABLE",
            "native adapter",
        ),
        (
            vec!["plans", "list", "--set", "anki.api_key_env=LAB_MISSING_KEY"],
            0,
            "",
            "",
        ),
        (
            vec![
                "--set",
                "anki.api_key_env=LAB_MISSING_KEY",
                "--set",
                "anki.endpoint=http://127.0.0.1:9",
                "decks",
                "list",
            ],
            3,
            "ANKI_CREDENTIAL_UNAVAILABLE",
            "anki.api_key_env",
        ),
        (
            vec![
                "--set",
                "anki.endpoint=http://192.0.2.10:8765",
                "--offline",
                "decks",
                "list",
            ],
            2,
            "REMOTE_ENDPOINT_NOT_ALLOWED",
            "loopback",
        ),
    ];
    for (args, code, error_prefix, hint) in cases {
        let output = home.cli().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{args:?}: {output:?}");
        if code == 0 {
            continue;
        }
        let error = stderr_json(&output);
        assert!(
            error["error"].as_str().unwrap().starts_with(error_prefix),
            "{args:?}: {error}"
        );
        assert!(
            error["next"].as_str().unwrap_or_default().contains(hint),
            "{args:?}: {error}"
        );
    }
    // The editor fallback explains its three alternatives.
    let add = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args([
            "vocab",
            "add",
            "--expression",
            "猫",
            "--meaning",
            "cat",
            "--sense-key",
            "cat",
            "--target-language",
            "ja",
        ])
        .output()
        .unwrap();
    let add: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    let edit = home
        .cli()
        .args(AUTHORED)
        .args([
            "plans",
            "edit",
            add["plan_id"].as_str().unwrap(),
            "--base-revision",
            "1",
            "--editor",
        ])
        .output()
        .unwrap();
    assert!(!edit.status.success(), "{edit:?}");
    let error = stderr_json(&edit);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("EDITOR_UNAVAILABLE"),
        "{error}"
    );
    assert!(error["next"].as_str().unwrap().contains("--patch"));
}

#[test]
fn large_batches_are_bounded_before_any_state_is_written() {
    let home = Home::new("batch");
    let input = home.0.join("batch.jsonl");
    let lines: Vec<String> = (0..300).map(vocab_line).collect();
    std::fs::write(&input, lines.join("\n") + "\n").unwrap();
    // Above selection.max_notes: refused with guidance, nothing stored.
    let refused = home
        .cli()
        .args(AUTHORED)
        .args(["--set", "selection.max_notes=299"])
        .args(["vocab", "add", "--format", "jsonl", "--document"])
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    let error = stderr_json(&refused);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("INPUT_BATCH_TOO_LARGE")
    );
    assert!(
        error["next"]
            .as_str()
            .unwrap()
            .contains("selection.max_notes")
    );
    assert!(!home.state().join("state.sqlite3").exists());
    // Above input.max_file_mb is refused the same way (1 MiB limit, 2 MiB file).
    let big = home.0.join("big.jsonl");
    std::fs::write(&big, vec![b' '; 2 * 1024 * 1024]).unwrap();
    let refused = home
        .cli()
        .args(AUTHORED)
        .args(["--set", "input.max_file_mb=1"])
        .args(["vocab", "add", "--format", "jsonl", "--document"])
        .arg(&big)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    assert!(
        stderr_json(&refused)["error"]
            .as_str()
            .unwrap()
            .starts_with("INPUT_TOO_LARGE")
    );
    assert!(!home.state().join("state.sqlite3").exists());
    // Within the limit, one plan holds every record.
    let accepted = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args(["vocab", "add", "--format", "jsonl", "--document"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(accepted.status.success(), "{accepted:?}");
    let value: serde_json::Value = serde_json::from_slice(&accepted.stdout).unwrap();
    let plan = value["plan_id"].as_str().unwrap();
    let show = home
        .cli()
        .args(["--output", "json"])
        .args(AUTHORED)
        .args(["plans", "show", plan])
        .output()
        .unwrap();
    assert!(show.status.success(), "{show:?}");
    let shown: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    let text = shown.to_string();
    assert!(text.contains("語0") && text.contains("語299"));
}

#[test]
fn free_space_reserve_refuses_state_writes_but_not_reads() {
    let home = Home::new("space");
    // A reserve larger than any test file system (1 TiB).
    let reserve = ["--set", "storage.free_space_reserve_mb=1048576"];
    let add = home
        .cli()
        .args(AUTHORED)
        .args(reserve)
        .args([
            "vocab",
            "add",
            "--expression",
            "水",
            "--meaning",
            "water",
            "--sense-key",
            "water",
            "--target-language",
            "ja",
        ])
        .output()
        .unwrap();
    assert!(!add.status.success(), "{add:?}");
    let error = stderr_json(&add);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .starts_with("STORAGE_FREE_SPACE"),
        "{error}"
    );
    assert!(
        error["next"]
            .as_str()
            .unwrap()
            .contains("storage.state_dir")
    );
    assert!(!home.state().exists());
    for args in [
        vec!["plans", "list"],
        vec!["jobs", "list"],
        vec!["cache", "status"],
        vec!["cache", "prune"],
        vec!["recover", "inspect", "--pending"],
    ] {
        let output = home
            .cli()
            .args(AUTHORED)
            .args(reserve)
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    let job = home
        .cli()
        .args(AUTHORED)
        .args(reserve)
        .args(["jobs", "create", "--note-id", "1"])
        .output()
        .unwrap();
    assert!(
        stderr_json(&job)["error"]
            .as_str()
            .unwrap()
            .starts_with("STORAGE_FREE_SPACE"),
        "{job:?}"
    );
}
