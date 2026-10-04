//! Output decoration and the private diagnostic log (`output.color`,
//! `output.progress`, `output.quiet`, `logging.*`).
//!
//! The log is one JSON line per invocation in `<state_dir>/logs/cli.jsonl`. It
//! is written only when that state already holds a database, so commands never
//! create state to log. Records carry the command path, exit code, stable error
//! code and duration; argument values are never logged, and error text is
//! included only with `logging.include_private_payloads` (URL credentials are
//! still redacted).
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Mutex;

struct Log {
    directory: PathBuf,
    threshold: u8,
    max_bytes: u64,
    retained: u32,
    private_payloads: bool,
    fingerprint: String,
}

struct Diagnostics {
    color: bool,
    progress: bool,
    log: Option<Log>,
}

static STATE: Mutex<Option<Diagnostics>> = Mutex::new(None);

fn severity(level: &str) -> u8 {
    match level {
        "error" => 0,
        "warn" => 1,
        "info" => 2,
        "debug" => 3,
        _ => 4,
    }
}

/// Whether ANSI color applies to human output, given `output.color`.
pub fn color_decision(setting: &str, stdout_terminal: bool, no_color: bool) -> bool {
    match setting {
        "always" => true,
        "never" => false,
        _ => stdout_terminal && !no_color,
    }
}

/// Progress lines go to stderr only when enabled, not quiet and interactive.
pub fn progress_decision(progress: bool, quiet: bool, stderr_terminal: bool) -> bool {
    progress && !quiet && stderr_terminal
}

pub fn configure(settings: &linguist_config::Effective) {
    let v = &settings.values;
    // output.language has the single registered value "en": messages are English.
    debug_assert_eq!(v.get("output.language"), Some(&serde_json::json!("en")));
    let environment: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let color = color_decision(
        v["output.color"].as_str().unwrap_or("auto"),
        std::io::stdout().is_terminal(),
        environment.get("NO_COLOR").is_some_and(|s| !s.is_empty()),
    );
    let progress = progress_decision(
        v["output.progress"] == true,
        v["output.quiet"] == true,
        std::io::stderr().is_terminal(),
    );
    let log = (v["logging.file_enabled"] == true)
        .then(|| {
            let state =
                linguist_config::expand_path(v["storage.state_dir"].as_str()?, &environment)
                    .ok()?;
            // Never create state just to log.
            std::fs::symlink_metadata(state.join("state.sqlite3"))
                .ok()
                .filter(|m| m.is_file())?;
            Some(Log {
                directory: state.join("logs"),
                threshold: severity(v["logging.level"].as_str().unwrap_or("info")),
                max_bytes: v["logging.max_file_mb"].as_u64().unwrap_or(20) * 1024 * 1024,
                retained: v["logging.retained_files"].as_u64().unwrap_or(5) as u32,
                private_payloads: v["logging.include_private_payloads"] == true,
                fingerprint: settings.fingerprint.clone(),
            })
        })
        .flatten();
    linguist_store::set_busy_timeout_ms(
        v["storage.sqlite_busy_timeout_ms"].as_u64().unwrap_or(5000),
    );
    *STATE.lock().unwrap() = Some(Diagnostics {
        color,
        progress,
        log,
    });
}

pub fn color() -> bool {
    STATE.lock().unwrap().as_ref().is_some_and(|d| d.color)
}

/// Routine progress on stderr; JSON stdout stays clean.
pub fn progress(message: &str) {
    if STATE.lock().unwrap().as_ref().is_some_and(|d| d.progress) {
        let _ = writeln!(std::io::stderr(), "… {message}");
    }
}

fn redact(text: &str) -> String {
    text.split_whitespace()
        .map(
            |word| match url::Url::parse(word.trim_matches(|c: char| c == ',' || c == ';')) {
                Ok(url) if !url.username().is_empty() || url.password().is_some() => {
                    "<redacted-url>".to_owned()
                }
                _ => word.to_owned(),
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}

fn rotate(log: &Log, path: &std::path::Path) {
    let keep = log.retained.max(1);
    if keep == 1 {
        let _ = std::fs::remove_file(path);
        return;
    }
    let numbered = |n: u32| log.directory.join(format!("cli.{n}.jsonl"));
    let _ = std::fs::remove_file(numbered(keep - 1));
    for n in (1..keep - 1).rev() {
        let _ = std::fs::rename(numbered(n), numbered(n + 1));
    }
    let _ = std::fs::rename(path, numbered(1));
}

/// Append one record; logging failures never change the command's outcome.
pub fn record(command: &str, exit: u8, error: Option<&str>, elapsed: std::time::Duration) {
    let guard = STATE.lock().unwrap();
    let Some(log) = guard.as_ref().and_then(|d| d.log.as_ref()) else {
        return;
    };
    let level = if error.is_some() { "error" } else { "info" };
    if severity(level) > log.threshold {
        return;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    if builder.recursive(true).create(&log.directory).is_err() {
        return;
    }
    let path = log.directory.join("cli.jsonl");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= log.max_bytes) {
        rotate(log, &path);
    }
    let mut entry = serde_json::json!({
        "time_unix_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        "level": level,
        "command": command,
        "exit_code": exit,
        "error_code": error.map(|e| e.split(':').next().unwrap_or(e).trim()),
        "duration_ms": elapsed.as_millis() as u64,
        "pid": std::process::id(),
    });
    if log.threshold >= severity("debug") {
        entry["settings_fingerprint"] = serde_json::json!(log.fingerprint);
    }
    if log.private_payloads {
        entry["error_message"] = serde_json::json!(error.map(redact));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    if let Ok(mut file) = options.open(&path) {
        let mut line = entry.to_string();
        line.push('\n');
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_and_progress_follow_settings() {
        assert!(color_decision("always", false, true));
        assert!(!color_decision("never", true, false));
        assert!(color_decision("auto", true, false));
        assert!(!color_decision("auto", true, true));
        assert!(!color_decision("auto", false, false));
        assert!(progress_decision(true, false, true));
        assert!(!progress_decision(true, true, true));
        assert!(!progress_decision(false, false, true));
        assert!(!progress_decision(true, false, false));
    }

    #[test]
    fn private_payloads_redact_url_credentials() {
        assert_eq!(
            redact("failed https://user:pw@example.org/x here"),
            "failed <redacted-url> here"
        );
    }
}
