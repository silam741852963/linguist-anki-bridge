//! Generation providers besides local Ollama: command-line agents (Claude Code
//! `claude -p`, Codex `codex exec`) and an OpenAI-compatible chat API.
//!
//! Each provider receives the same system prompt, input JSON and output schema
//! as the Ollama path and returns the supplement JSON; validation, archival and
//! review are unchanged. Settings: `llm.provider` picks the first provider and
//! `llm.fallback` the ones tried, in order, when it fails. An agent that is not
//! installed (looked up only on the `PATH` of the environment the pipeline was
//! given) or an API without `llm.api.endpoint` is skipped, not failed.
//!
//! Agents run with the user's environment (they need their own sign-in), no
//! tools (`claude --tools ""`, `codex --sandbox read-only`), no saved session,
//! in an empty private working directory, under `llm.agents.timeout_seconds`.
use crate::generation::GenerationRequest;
use linguist_config::Effective;
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
    match provider {
        Provider::Ollama => Err("GENERATION_PROVIDER_INVALID".into()),
        Provider::ClaudeCode => claude_code(request, settings, environment, timeout, limit),
        Provider::Codex => codex(request, settings, environment, timeout, limit),
        Provider::OpenAiCompatible => api(request, settings, environment, timeout, limit),
    }
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
    // Claude Code's validator knows no draft-2020-12 meta-schema; the schema
    // itself uses only keywords both drafts share.
    let mut schema = request.output_schema.clone();
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
    }
    let schema = serde_json::to_string(&schema).map_err(|e| e.to_string())?;
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
        serde_json::to_vec(&request.output_schema).map_err(|e| e.to_string())?,
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
            "name": "supplement", "schema": request.output_schema, "strict": false}},
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
