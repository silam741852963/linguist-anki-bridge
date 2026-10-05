//! WP-16 review: credentials are configured by environment-variable name
//! only. Their values never reach stdout, stderr, the diagnostic log,
//! durable state, exported bundles or the config file, across the commands
//! that touch them. Plan listings never print private personal notes.
use std::{path::Path, process::Command};

const SECRET: &str = "lab-review-secret-7d1c2f";
const PERSONAL: &str = "my private diary line";

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn credential_values_never_leave_the_environment() {
    let home = std::env::temp_dir().join(format!("lab-review-privacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("REVIEW_ANKI_KEY", SECRET)
            .env("REVIEW_LLM_KEY", SECRET)
            .args([
                "--output",
                "json",
                "--set",
                "anki.endpoint=http://127.0.0.1:9",
                "--set",
                "llm.endpoint=http://127.0.0.1:9",
                "--set",
                "logging.level=debug",
            ])
            .args(args)
            .output()
            .unwrap()
    };
    let mut outputs = Vec::new();
    let init = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
        .env_clear()
        .env("HOME", &home)
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    for (key, name) in [
        ("anki.api_key_env", "REVIEW_ANKI_KEY"),
        ("llm.api_key_env", "REVIEW_LLM_KEY"),
    ] {
        let set = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"))
            .env_clear()
            .env("HOME", &home)
            .args(["config", "set", key, name])
            .output()
            .unwrap();
        assert!(set.status.success(), "{set:?}");
        outputs.push(set);
    }
    let add = cli(&[
        "--offline",
        "--set",
        "dictionary.provider=authored",
        "--set",
        "llm.enabled=false",
        "--set",
        "images.search_when_missing=false",
        "--set",
        "kanji.enabled=false",
        "vocab",
        "add",
        "--expression",
        "秘密",
        "--meaning",
        "secret",
        "--sense-key",
        "secret",
        "--target-language",
        "ja",
        "--personal-notes",
        PERSONAL,
    ]);
    assert!(add.status.success(), "{add:?}");
    let value: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    let plan = value["plan_id"].as_str().unwrap().to_owned();
    outputs.push(add);
    let bundle = home.join("bundle.json");
    for args in [
        vec!["config", "show"],
        vec!["config", "show", "--provenance"],
        vec!["config", "describe", "anki.api_key_env"],
        vec!["config", "validate"],
        vec!["doctor", "--local"],
        vec!["doctor"],
        vec!["doctor", "--ollama"],
        vec!["decks", "list"],
        vec!["plans", "list"],
        vec!["plans", "show", &plan],
        vec![
            "plans",
            "export",
            &plan,
            "--output",
            bundle.to_str().unwrap(),
        ],
    ] {
        outputs.push(cli(&args));
    }
    for output in &outputs {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!text.contains(SECRET), "credential value printed: {text}");
    }
    // Plan listings never print private personal notes.
    let listed = cli(&["plans", "list"]);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains(PERSONAL));
    // No file anywhere under HOME (config, state, assets, logs, bundle) holds it.
    let mut files = Vec::new();
    walk(&home, &mut files);
    assert!(files.iter().any(|f| f.ends_with("state.sqlite3")));
    assert!(bundle.is_file());
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        assert!(
            !bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
            "credential value stored in {}",
            file.display()
        );
    }
    std::fs::remove_dir_all(home).unwrap();
}
