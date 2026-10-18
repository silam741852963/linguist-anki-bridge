//! Generation providers besides local Ollama: command-line agents (Claude Code
//! `claude -p`, Codex `codex exec`) and an OpenAI-compatible chat API.
//!
//! Each provider receives the same system prompt, input JSON and output schema
//! as the Ollama path and returns the supplement JSON; validation, archival and
//! review are unchanged. Settings: `llm.provider` picks the first provider and
//! `llm.fallback` the ones tried, in order, when it fails. An agent that is not
//! installed (looked up only on the `PATH` of the environment the pipeline was
//! given) or an API without `llm.api.endpoint` is skipped, not failed. A
//! provider that reports its usage limit is skipped for
//! `llm.quota_cooldown_seconds`, so the rest of the chain drafts at once.
//!
//! Agents run with the user's environment (they need their own sign-in), no
//! tools (`claude --tools ""`, `codex --sandbox read-only`), no saved session,
//! in an empty private working directory, under `llm.agents.timeout_seconds`.
use crate::generation::GenerationRequest;
use linguist_config::Effective;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    ClaudeCode,
    Codex,
    OpenAiCompatible,
}

impl Provider {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "ollama" => Self::Ollama,
            "claude_code" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "openai_compatible" => Self::OpenAiCompatible,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenAiCompatible => "OpenAI-compatible API",
        }
    }
}

/// `llm.provider`, then each `llm.fallback` entry not already listed.
pub fn chain(settings: &Effective) -> Result<Vec<Provider>, String> {
    let first = settings
        .values
        .get("llm.provider")
        .and_then(Value::as_str)
        .and_then(Provider::parse)
        .ok_or("GENERATION_PROVIDER_INVALID")?;
    let mut providers = vec![first];
    for name in settings
        .values
        .get("llm.fallback")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let provider = name
            .as_str()
            .and_then(Provider::parse)
            .ok_or("GENERATION_PROVIDER_INVALID")?;
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    Ok(providers)
}

/// The supplement JSON one external provider produced, with what identifies it.
#[derive(Debug)]
pub struct External {
    pub provider: Provider,
    /// Provider, executable or endpoint, version and model, as recorded.
    pub identity: Value,
    /// The supplement JSON text, validated by the generation path.
    pub content: String,
    /// The provider's exact response bytes, archived with the draft.
    pub raw: Vec<u8>,
}

impl External {
    /// One line for the review issue and the plan result.
    pub fn describe(&self) -> String {
        let version = self.identity["version"]
            .as_str()
            .unwrap_or("unknown version");
        let model = self.identity["model"].as_str().unwrap_or("default model");
        format!("{} ({version}, {model})", self.provider.label())
    }
}

/// `Ok(None)` when the provider is not available here (not installed, or the
/// API is not configured): the chain skips it.
pub fn generate(
    provider: Provider,
    request: &GenerationRequest,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Option<External>, String> {
    let timeout = Duration::from_secs(
        settings
            .values
            .get("llm.agents.timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(600),
    );
    let limit = settings
        .values
        .get("network.max_response_mb")
        .and_then(Value::as_u64)
        .unwrap_or(16)
        * 1024
        * 1024;
    if let Some(cooldown) = cooling(provider, settings, environment) {
        return Err(cooldown.error(provider));
    }
    count_call(provider, settings, environment);
    let result = match provider {
        Provider::Ollama => Err("GENERATION_PROVIDER_INVALID".into()),
        Provider::ClaudeCode => claude_code(request, settings, environment, timeout, limit),
        Provider::Codex => codex(request, settings, environment, timeout, limit),
        Provider::OpenAiCompatible => api(request, settings, environment, timeout, limit),
    };
    if let Err(error) = &result
        && usage_limited(error)
    {
        cool_down(provider, error, settings, environment);
    }
    result
}

/// A provider that reported its usage limit, skipped until `until`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Cooldown {
    /// Seconds since the Unix epoch.
    pub until: u64,
    /// The provider's own words.
    pub said: String,
}

impl Cooldown {
    /// The skip as a chain error; it names the usage limit, so a campaign
    /// waits for the quota when no other provider drafted either.
    fn error(&self, provider: Provider) -> String {
        format!(
            "AGENT_QUOTA_COOLDOWN: {} reported its usage limit; skipped until {} ({})",
            provider.name(),
            self.until,
            self.said
        )
    }
}

/// An agent's failure that is its usage limit (Codex "You've hit your usage
/// limit", Claude Code "usage limit reached"), not a fault of the request.
pub fn usage_limited(error: &str) -> bool {
    error.to_lowercase().contains("usage limit")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `<storage.state_dir>/agents/quota-<provider>.json`, shared by every process
/// of one state directory (prepare jobs, ticks).
fn cooldown_path(
    provider: Provider,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let root = linguist_config::expand_path(
        settings.values.get("storage.state_dir")?.as_str()?,
        environment,
    )
    .ok()
    .filter(|root| root.is_absolute())?;
    Some(
        root.join("agents")
            .join(format!("quota-{}.json", provider.name())),
    )
}

/// The provider's current cooldown (`llm.quota_cooldown_seconds` after its
/// last usage limit), if any.
/// A provider that spent its own budget (`llm.agents.max_calls_per_window`
/// calls in `llm.agents.budget_window_seconds`) cools down the same way, so
/// a quota shared with other work is never run out.
pub fn cooling(
    provider: Provider,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Option<Cooldown> {
    let reported = std::fs::read(cooldown_path(provider, settings, environment)?)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Cooldown>(&bytes).ok())
        .filter(|cooldown| cooldown.until > now());
    reported.or_else(|| budget_spent(provider, settings, environment))
}

fn budget(settings: &Effective) -> Option<(u64, u64)> {
    let calls = settings
        .values
        .get("llm.agents.max_calls_per_window")
        .and_then(Value::as_u64)
        .filter(|calls| *calls > 0)?;
    let window = settings
        .values
        .get("llm.agents.budget_window_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(18_000);
    Some((calls, window))
}

/// `<storage.state_dir>/agents/calls-<provider>.log`: one start time per
/// line, appended by every process of this state directory.
fn calls_path(
    provider: Provider,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    cooldown_path(provider, settings, environment)
        .map(|path| path.with_file_name(format!("calls-{}.log", provider.name())))
}

fn calls_since(path: &Path, since: u64) -> Vec<u64> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .filter(|at| *at > since)
        .collect()
}

fn budget_spent(
    provider: Provider,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Option<Cooldown> {
    let (calls, window) = budget(settings)?;
    let path = calls_path(provider, settings, environment)?;
    let recent = calls_since(&path, now().saturating_sub(window));
    (recent.len() as u64 >= calls).then(|| Cooldown {
        until: recent.iter().min().copied().unwrap_or_else(now) + window,
        said: format!(
            "its own usage limit: {calls} calls per {window} s (llm.agents.max_calls_per_window)"
        ),
    })
}

/// Keep a draft the validation refused, with its error, in
/// `<storage.state_dir>/agents/rejected-<provider>.jsonl` (WP-24: 素っ気ない
/// failed GENERATION_EXAMPLE_LANGUAGE three times and nothing showed why).
/// The file is moved to `.old` past 8 MB; a draft is cut at 64 KB.
pub fn keep_rejected(
    provider: Provider,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    document_id: &str,
    error: &str,
    content: &[u8],
) {
    let Some(path) = cooldown_path(provider, settings, environment)
        .map(|path| path.with_file_name(format!("rejected-{}.jsonl", provider.name())))
    else {
        return;
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 8 * 1024 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("jsonl.old"));
    }
    let text = String::from_utf8_lossy(&content[..content.len().min(64 * 1024)]).into_owned();
    let line =
        serde_json::json!({"at": now(), "document_id": document_id, "error": error, "draft": text});
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()));
}

/// Count one call against the budget (only when a budget is set); the log
/// keeps only the current window once it holds four budgets.
fn count_call(provider: Provider, settings: &Effective, environment: &BTreeMap<String, String>) {
    let Some((calls, window)) = budget(settings) else {
        return;
    };
    let Some(path) = calls_path(provider, settings, environment) else {
        return;
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
    let lines = std::fs::read_to_string(&path)
        .map(|text| text.lines().count() as u64)
        .unwrap_or(0);
    if lines >= calls.saturating_mul(4) {
        let keep: String = calls_since(&path, now().saturating_sub(window))
            .iter()
            .map(|at| format!("{at}\n"))
            .collect();
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let _ = std::fs::write(&temporary, keep).and_then(|()| std::fs::rename(&temporary, &path));
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(format!("{}\n", now()).as_bytes()));
}

/// Record the usage limit; failing to record it only costs one more try.
fn cool_down(
    provider: Provider,
    error: &str,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) {
    let Some(path) = cooldown_path(provider, settings, environment) else {
        return;
    };
    let seconds = settings
        .values
        .get("llm.quota_cooldown_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(1800);
    let cooldown = Cooldown {
        until: now() + seconds,
        said: error.chars().take(300).collect(),
    };
    let Ok(bytes) = serde_json::to_vec(&cooldown) else {
        return;
    };
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")))
        .and_then(|()| std::fs::write(&temporary, bytes))
        .and_then(|()| std::fs::rename(&temporary, &path));
}

/// The first provider of the chain that is not cooling down after its usage
/// limit: the one drafts go to now.
pub fn current(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Option<Provider>, String> {
    Ok(chain(settings)?.into_iter().find(|provider| {
        *provider == Provider::Ollama || cooling(*provider, settings, environment).is_none()
    }))
}

/// The prompt an agent reads on stdin: the same instructions and input JSON as
/// the Ollama request, then the answer contract.
fn prompt(request: &GenerationRequest) -> String {
    format!(
        "{}\n\nInput JSON:\n{}\n\nAnswer with one JSON object that matches the given output schema. Do not use tools.",
        request.system_prompt, request.user_json
    )
}

/// A bare name is looked up only on the given environment's `PATH`; a
/// symlink is kept as is, because tool managers (mise) dispatch on the name.
fn locate(reference: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let executable = |path: &Path| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if reference.contains('/') {
        let path = PathBuf::from(reference);
        return (path.is_absolute() && executable(&path)).then_some(path);
    }
    std::env::split_paths(environment.get("PATH")?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(reference))
        .find(|candidate| executable(candidate))
}

struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Run an agent in its own empty working directory with the given stdin,
/// killing it at the deadline; stdout and stderr are bounded.
fn run(
    mut command: Command,
    workdir: &Path,
    input: &str,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    limit: u64,
) -> Result<Output, String> {
    command
        .env_clear()
        .envs(environment)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group: an agent run under a wrapper (codex-meter.sh)
    // is ended whole on a timeout, not just the wrapper (WP-24: orphaned
    // `codex exec` processes outlived their deadline).
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|_| "AGENT_SPAWN_FAILED")?;
    let mut stdin = child.stdin.take().ok_or("AGENT_SPAWN_FAILED")?;
    let input = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let read = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stream) = stream {
                let _ = stream.take(limit + 1).read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "AGENT_WAIT_FAILED")? {
            break status;
        }
        if Instant::now() >= deadline {
            if let Ok(group) = i32::try_from(child.id()) {
                // SAFETY: signals the process group this function created.
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err("AGENT_TIMEOUT".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = writer.join();
    let stdout = stdout.join().map_err(|_| "AGENT_READ_FAILED")?;
    let stderr = stderr.join().map_err(|_| "AGENT_READ_FAILED")?;
    if stdout.len() as u64 > limit {
        return Err("AGENT_OUTPUT_LIMIT".into());
    }
    if !status.success() {
        // The agent's own last words (bounded) say why: login, quota, flags.
        let said = [&stderr, &stdout]
            .iter()
            .filter_map(|bytes| {
                String::from_utf8_lossy(bytes)
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().chars().take(300).collect::<String>())
            })
            .next()
            .unwrap_or_default();
        return Err(format!("AGENT_FAILED: exit {:?}: {said}", status.code()));
    }
    Ok(Output { stdout, stderr })
}

fn version(executable: &Path, environment: &BTreeMap<String, String>) -> String {
    let mut command = Command::new(executable);
    command.arg("--version");
    let workdir = std::env::temp_dir();
    run(
        command,
        &workdir,
        "",
        environment,
        Duration::from_secs(30),
        64 * 1024,
    )
    .ok()
    .and_then(|o| String::from_utf8(o.stdout).ok())
    .map(|v| v.lines().next().unwrap_or_default().trim().to_owned())
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| "unknown version".into())
}

fn private_workdir() -> Result<tempdir::Dir, String> {
    tempdir::Dir::new().map_err(|_| "AGENT_WORKDIR_UNAVAILABLE".into())
}

mod tempdir {
    use std::path::{Path, PathBuf};
    /// An empty private directory, removed on drop.
    pub struct Dir(PathBuf);
    impl Dir {
        pub fn new() -> std::io::Result<Self> {
            use std::os::unix::fs::DirBuilderExt;
            let path = std::env::temp_dir()
                .join(format!("linguist-agent-{}", uuid::Uuid::new_v4().simple()));
            std::fs::DirBuilder::new().mode(0o700).create(&path)?;
            Ok(Self(path))
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// The output schema as agents accept it: an object at the top (Codex and
/// OpenAI refuse anything else), a single `oneOf` branch hoisted into it, and
/// no draft-2020-12 meta-schema marker (Claude Code's validator knows none).
/// Accepted output is unchanged: the branch was the only one.
pub fn agent_schema(schema: &Value) -> Value {
    let mut schema = schema.clone();
    let Some(object) = schema.as_object_mut() else {
        return schema;
    };
    object.remove("$schema");
    if let Some(Value::Array(branches)) = object.get("oneOf")
        && let [Value::Object(branch)] = branches.as_slice()
        && branch.get("type") == Some(&json!("object"))
    {
        let branch = branch.clone();
        object.remove("oneOf");
        object.extend(branch);
    }
    schema
}

fn setting<'a>(settings: &'a Effective, key: &str) -> Option<&'a str> {
    settings.values.get(key).and_then(Value::as_str)
}

fn claude_code(
    request: &GenerationRequest,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    limit: u64,
) -> Result<Option<External>, String> {
    let Some(executable) = setting(settings, "llm.agents.claude_code.executable")
        .and_then(|name| locate(name, environment))
    else {
        return Ok(None);
    };
    let model = setting(settings, "llm.agents.claude_code.model");
    let schema =
        serde_json::to_string(&agent_schema(&request.output_schema)).map_err(|e| e.to_string())?;
    let workdir = private_workdir()?;
    let mut command = Command::new(&executable);
    command.args([
        "-p",
        "--output-format",
        "json",
        "--json-schema",
        &schema,
        "--tools",
        "",
        "--no-session-persistence",
        "--setting-sources",
        "",
    ]);
    if let Some(model) = model {
        command.args(["--model", model]);
    }
    let output = run(
        command,
        workdir.path(),
        &prompt(request),
        environment,
        timeout,
        limit,
    )?;
    let reply: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "AGENT_OUTPUT_INVALID")?;
    if reply["is_error"] != false || reply["subtype"] != "success" {
        return Err("AGENT_REPORTED_ERROR".into());
    }
    let content = reply
        .get("structured_output")
        .filter(|v| v.is_object())
        .ok_or("AGENT_OUTPUT_INVALID")?;
    let used: Vec<&String> = reply["modelUsage"]
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    Ok(Some(External {
        provider: Provider::ClaudeCode,
        identity: json!({
            "provider": "claude_code",
            "executable": executable,
            "version": version(&executable, environment),
            "model": model.map(str::to_owned).or_else(|| used.first().map(|m| m.to_string())),
            "models_used": used,
        }),
        content: serde_json::to_string(content).map_err(|e| e.to_string())?,
        raw: output.stdout,
    }))
}

fn codex(
    request: &GenerationRequest,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    limit: u64,
) -> Result<Option<External>, String> {
    let Some(executable) =
        setting(settings, "llm.agents.codex.executable").and_then(|name| locate(name, environment))
    else {
        return Ok(None);
    };
    let model = setting(settings, "llm.agents.codex.model");
    let workdir = private_workdir()?;
    let schema_path = workdir.path().join("schema.json");
    let answer_path = workdir.path().join("answer.json");
    std::fs::write(
        &schema_path,
        serde_json::to_vec(&agent_schema(&request.output_schema)).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "AGENT_WORKDIR_UNAVAILABLE")?;
    let mut command = Command::new(&executable);
    command.args([
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--output-schema",
    ]);
    command.arg(&schema_path).arg("-o").arg(&answer_path);
    if let Some(model) = model {
        command.args(["-m", model]);
    }
    if let Some(effort) =
        setting(settings, "llm.agents.codex.reasoning_effort").filter(|effort| *effort != "default")
    {
        command.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
    }
    command.arg("-");
    let output = run(
        command,
        workdir.path(),
        &prompt(request),
        environment,
        timeout,
        limit,
    )?;
    let answer = std::fs::read(&answer_path).map_err(|_| "AGENT_OUTPUT_INVALID")?;
    let content: Value = serde_json::from_slice(&answer).map_err(|_| "AGENT_OUTPUT_INVALID")?;
    if !content.is_object() {
        return Err("AGENT_OUTPUT_INVALID".into());
    }
    // Codex reports the model on stderr as `model: NAME`.
    let reported = String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|l| l.strip_prefix("model: ").map(|m| m.trim().to_owned()));
    let mut raw = output.stdout;
    raw.extend_from_slice(b"\n--- answer ---\n");
    raw.extend_from_slice(&answer);
    Ok(Some(External {
        provider: Provider::Codex,
        identity: json!({
            "provider": "codex",
            "executable": executable,
            "version": version(&executable, environment),
            "model": model.map(str::to_owned).or(reported),
        }),
        content: serde_json::to_string(&content).map_err(|e| e.to_string())?,
        raw,
    }))
}

fn api(
    request: &GenerationRequest,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    limit: u64,
) -> Result<Option<External>, String> {
    let (Some(endpoint), Some(model)) = (
        setting(settings, "llm.api.endpoint"),
        setting(settings, "llm.api.model"),
    ) else {
        return Ok(None);
    };
    let key = match setting(settings, "llm.api.api_key_env") {
        Some(name) => Some(
            environment
                .get(name)
                .filter(|k| !k.is_empty())
                .ok_or("AGENT_API_KEY_MISSING")?
                .clone(),
        ),
        None => None,
    };
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": request.system_prompt},
            {"role": "user", "content": request.user_json},
        ],
        "response_format": {"type": "json_schema", "json_schema": {
            "name": "supplement", "schema": agent_schema(&request.output_schema), "strict": false}},
        "temperature": settings.values.get("llm.temperature").cloned().unwrap_or(json!(0.0)),
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "AGENT_API_UNAVAILABLE")?;
    let mut call = client.post(&url).json(&body);
    if let Some(key) = key {
        call = call.bearer_auth(key);
    }
    let response = call.send().map_err(|_| "AGENT_API_UNAVAILABLE")?;
    let status = response.status();
    let mut raw = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "AGENT_API_UNAVAILABLE")?;
    if raw.len() as u64 > limit {
        return Err("AGENT_OUTPUT_LIMIT".into());
    }
    if !status.is_success() {
        return Err(format!("AGENT_API_HTTP_FAILED: {}", status.as_u16()));
    }
    let reply: Value = serde_json::from_slice(&raw).map_err(|_| "AGENT_OUTPUT_INVALID")?;
    let text = reply["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("AGENT_OUTPUT_INVALID")?;
    let content: Value = serde_json::from_str(text).map_err(|_| "AGENT_OUTPUT_INVALID")?;
    Ok(Some(External {
        provider: Provider::OpenAiCompatible,
        identity: json!({
            "provider": "openai_compatible",
            "endpoint": endpoint,
            "version": reply["system_fingerprint"].as_str().unwrap_or("unknown version"),
            "model": reply["model"].as_str().unwrap_or(model),
        }),
        content: serde_json::to_string(&content).map_err(|e| e.to_string())?,
        raw,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_get_an_object_schema_without_the_meta_schema() {
        let schema = json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Supplement", "$defs": {"B": {"type": "object"}},
            "oneOf": [{"type": "object", "properties": {"kind": {"const": "vocabulary"},
                "body": {"$ref": "#/$defs/B"}}, "required": ["kind", "body"],
                "additionalProperties": false}]});
        let agent = agent_schema(&schema);
        assert_eq!(agent["type"], "object");
        assert_eq!(agent["required"], json!(["kind", "body"]));
        assert!(agent.get("oneOf").is_none() && agent.get("$schema").is_none());
        assert_eq!(agent["$defs"]["B"]["type"], "object");
        // Several branches stay as they are.
        let mut two = schema.clone();
        two["oneOf"] = json!([{"type": "object"}, {"type": "object"}]);
        assert!(agent_schema(&two).get("oneOf").is_some());
    }

    #[test]
    fn codex_gets_the_configured_model_and_reasoning_effort() {
        use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("lab-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("codex");
        // Records its arguments (shell builtins only: the agent runs with
        // the given PATH alone) and answers an empty object.
        let recorded = directory.join("args.txt");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nout=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = -o ] && out=$a; prev=$a; done\nprintf '%s\\n' \"$@\" > '{}'\nprintf '{{}}' > \"$out\"\n",
                recorded.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = BTreeMap::from([("PATH".into(), directory.display().to_string())]);
        let document = linguist_core::LearningDocument::from_json(include_bytes!(
            "../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        let mut settings = resolve(
            &Registry::builtin(),
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        let request = crate::generation::build_request(&document, &settings).unwrap();
        let args = |settings: &Effective| {
            let out = codex(&request, settings, &env, Duration::from_secs(10), 1 << 20)
                .unwrap()
                .unwrap();
            assert_eq!(out.content, "{}");
            std::fs::read_to_string(&recorded)
                .unwrap()
                .replace('\n', " ")
        };
        let plain = args(&settings);
        assert!(
            !plain.contains("-m ") && !plain.contains("model_reasoning_effort"),
            "{plain}"
        );
        settings
            .values
            .insert("llm.agents.codex.model".into(), json!("gpt-6.1-sol"));
        settings
            .values
            .insert("llm.agents.codex.reasoning_effort".into(), json!("low"));
        let tuned = args(&settings);
        assert!(tuned.contains("-m gpt-6.1-sol"), "{tuned}");
        assert!(
            tuned.contains("-c model_reasoning_effort=\"low\""),
            "{tuned}"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_agent_at_its_usage_limit_is_skipped_until_the_cooldown_ends() {
        use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("lab-quota-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("codex");
        let calls = directory.join("calls.txt");
        // Answers its usage limit and counts its runs (shell builtins only).
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\necho run >> '{}'\necho \"ERROR: You've hit your usage limit.\" >&2\nexit 1\n",
                calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = BTreeMap::from([("PATH".into(), directory.display().to_string())]);
        let document = linguist_core::LearningDocument::from_json(include_bytes!(
            "../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        let mut settings = resolve(
            &Registry::builtin(),
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        settings.values.insert(
            "storage.state_dir".into(),
            json!(directory.join("state").display().to_string()),
        );
        settings
            .values
            .insert("llm.provider".into(), json!("codex"));
        settings
            .values
            .insert("llm.fallback".into(), json!(["ollama"]));
        let request = crate::generation::build_request(&document, &settings).unwrap();
        assert_eq!(current(&settings, &env).unwrap(), Some(Provider::Codex));
        let first = generate(Provider::Codex, &request, &settings, &env).unwrap_err();
        assert!(
            first.starts_with("AGENT_FAILED") && usage_limited(&first),
            "{first}"
        );
        // Skipped without running it; drafts go to the next provider.
        let second = generate(Provider::Codex, &request, &settings, &env).unwrap_err();
        assert!(
            second.starts_with("AGENT_QUOTA_COOLDOWN: codex") && usage_limited(&second),
            "{second}"
        );
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
        assert_eq!(current(&settings, &env).unwrap(), Some(Provider::Ollama));
        // Once the cooldown ends it is tried again.
        let path = cooldown_path(Provider::Codex, &settings, &env).unwrap();
        std::fs::write(&path, br#"{"until": 1, "said": "old"}"#).unwrap();
        assert_eq!(current(&settings, &env).unwrap(), Some(Provider::Codex));
        let _ = generate(Provider::Codex, &request, &settings, &env).unwrap_err();
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 2);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_agent_past_its_own_budget_cools_down_until_the_window_frees() {
        use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
        let directory = std::env::temp_dir().join(format!("lab-budget-{}", uuid::Uuid::new_v4()));
        let env = BTreeMap::from([("PATH".into(), directory.display().to_string())]);
        let mut settings = resolve(
            &Registry::builtin(),
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        settings.values.insert(
            "storage.state_dir".into(),
            json!(directory.join("state").display().to_string()),
        );
        settings
            .values
            .insert("llm.provider".into(), json!("codex"));
        settings
            .values
            .insert("llm.fallback".into(), json!(["ollama"]));
        // No budget: nothing is counted.
        count_call(Provider::Codex, &settings, &env);
        assert!(calls_path(Provider::Codex, &settings, &env).is_some_and(|p| !p.exists()));
        settings
            .values
            .insert("llm.agents.max_calls_per_window".into(), json!(2));
        settings
            .values
            .insert("llm.agents.budget_window_seconds".into(), json!(3600));
        count_call(Provider::Codex, &settings, &env);
        assert!(cooling(Provider::Codex, &settings, &env).is_none());
        assert_eq!(current(&settings, &env).unwrap(), Some(Provider::Codex));
        count_call(Provider::Codex, &settings, &env);
        let spent = cooling(Provider::Codex, &settings, &env).unwrap();
        assert!(spent.until > now() + 3500 && usage_limited(&spent.error(Provider::Codex)));
        assert_eq!(current(&settings, &env).unwrap(), Some(Provider::Ollama));
        // Calls older than the window no longer count.
        let path = calls_path(Provider::Codex, &settings, &env).unwrap();
        std::fs::write(&path, "1\n2\n").unwrap();
        assert!(cooling(Provider::Codex, &settings, &env).is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_refused_draft_is_kept_with_its_error() {
        use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
        let directory = std::env::temp_dir().join(format!("lab-rejected-{}", uuid::Uuid::new_v4()));
        let env = BTreeMap::new();
        let mut settings = resolve(
            &Registry::builtin(),
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        settings.values.insert(
            "storage.state_dir".into(),
            json!(directory.join("state").display().to_string()),
        );
        keep_rejected(
            Provider::Codex,
            &settings,
            &env,
            "doc-1",
            "GENERATION_EXAMPLE_LANGUAGE",
            b"{\"x\":1}",
        );
        let text =
            std::fs::read_to_string(directory.join("state/agents/rejected-codex.jsonl")).unwrap();
        let line: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(line["document_id"], "doc-1");
        assert_eq!(line["error"], "GENERATION_EXAMPLE_LANGUAGE");
        assert_eq!(line["draft"], "{\"x\":1}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_timed_out_agent_is_ended_with_everything_it_started() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("lab-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("wrapper");
        let pidfile = directory.join("child.pid");
        // A wrapper whose own child would outlive it (codex-meter.sh -> codex).
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
                pidfile.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]);
        let started = Instant::now();
        let result = run(
            Command::new(&script),
            &directory,
            "",
            &env,
            Duration::from_millis(500),
            1024,
        );
        assert_eq!(result.err().as_deref(), Some("AGENT_TIMEOUT"));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        // SAFETY: signal 0 only checks that the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "the wrapper's child {pid} outlived the timeout");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn agents_are_found_only_on_the_given_path() {
        let directory = std::env::temp_dir().join(format!("lab-agents-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("fake-agent");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = directory.join("linked-agent");
        std::os::unix::fs::symlink(&script, &link).unwrap();
        let env = BTreeMap::from([("PATH".into(), directory.display().to_string())]);
        assert_eq!(locate("fake-agent", &env), Some(script.clone()));
        // A tool-manager shim keeps its own name.
        assert_eq!(locate("linked-agent", &env), Some(link));
        assert_eq!(locate("fake-agent", &BTreeMap::new()), None);
        assert_eq!(locate("missing", &env), None);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
