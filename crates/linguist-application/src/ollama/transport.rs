//! Bounded local metadata and candidate-generation transport. No pull or arbitrary-action API.
use super::{
    Completion, ModelEvidence, identity, parse_completion, response, selected, verify_local_model,
};
use crate::generation::{GeneratedDraft, GenerationRequest, build_request, merge_ollama_response};
use linguist_config::Effective;
use linguist_core::{
    LearningDocument, canonical,
    validation::{Issue, Severity},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::Read,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
#[derive(Clone, Copy)]
struct Dispatch {
    started: Instant,
    interval: Duration,
    server_delay: Option<(Instant, Duration)>,
}
type Gate = Arc<Mutex<Option<Dispatch>>>;
fn endpoint_gate(endpoint: &url::Url) -> Result<Gate, String> {
    static GATES: OnceLock<Mutex<BTreeMap<String, Gate>>> = OnceLock::new();
    let mut gates = GATES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "OLLAMA_GATE_UNAVAILABLE")?;
    let key = endpoint.origin().ascii_serialization();
    if let Some(gate) = gates.get(&key) {
        return Ok(gate.clone());
    }
    if gates.len() >= 64 {
        return Err("OLLAMA_ENDPOINT_COUNT_LIMIT".into());
    }
    let gate = Arc::new(Mutex::new(None));
    gates.insert(key, gate.clone());
    Ok(gate)
}
#[derive(Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
    endpoint: url::Url,
    settings: Effective,
    shared: Gate,
}
#[derive(Clone, Copy)]
enum ReadAction {
    Tags,
    Show,
    Version,
}
/// Candidate only: engine compatibility/input preservation have not been certified.
/// Callers must archive these bytes and perform supplement validation/review.
#[derive(Debug)]
pub struct Candidate {
    pub request: GenerationRequest,
    pub request_bytes: Vec<u8>,
    pub evidence: ModelEvidence,
    pub evidence_after: ModelEvidence,
    pub completion: Completion,
}

impl Client {
    /// Run inference, validate the supplement and return a complete asset-first draft.
    /// The engine verification error deliberately keeps development candidates unready.
    pub fn generate_draft(&self, document: &LearningDocument) -> Result<GeneratedDraft, String> {
        let candidate = self.generate_candidate(document)?;
        let mut draft = merge_ollama_response(
            document,
            &self.settings,
            &candidate.request,
            &candidate.completion.raw,
            &candidate.evidence.identity,
        )?;
        let source = draft
            .document
            .sources
            .last_mut()
            .ok_or("GENERATION_ARCHIVE_MISSING")?;
        source.fields.insert(
            "wire_request".into(),
            String::from_utf8(candidate.request_bytes.clone())
                .map_err(|_| "GENERATION_ENCODING")?,
        );
        source.fields.insert(
            "model_evidence_before".into(),
            String::from_utf8(
                canonical::bytes(&candidate.evidence).map_err(|_| "GENERATION_ENCODING")?,
            )
            .map_err(|_| "GENERATION_ENCODING")?,
        );
        source.fields.insert(
            "engine_identity_digest".into(),
            super::certify::identity_digest(&super::certify::identity(
                &candidate.evidence,
                &self.settings,
            ))?,
        );
        source.fields.insert(
            "model_evidence_after".into(),
            String::from_utf8(
                canonical::bytes(&candidate.evidence_after).map_err(|_| "GENERATION_ENCODING")?,
            )
            .map_err(|_| "GENERATION_ENCODING")?,
        );
        draft.assets.insert(
            canonical::asset_digest(&candidate.request_bytes),
            candidate.request_bytes,
        );
        draft.assets.extend(candidate.evidence.assets);
        draft.assets.extend(candidate.evidence_after.assets);
        let manifest = canonical::bytes(&source.fields).map_err(|_| "GENERATION_ENCODING")?;
        if manifest.len() > 100 * 1024 * 1024 {
            return Err("GENERATION_ARCHIVE_LIMIT".into());
        }
        let digest = canonical::asset_digest(&manifest);
        draft.assets.insert(digest.clone(), manifest);
        source.digest = digest.clone();
        let archive = draft
            .document
            .archives
            .last_mut()
            .filter(|archive| archive.source_id == source.id)
            .ok_or("GENERATION_ARCHIVE_MISSING")?;
        archive.digest = digest;
        archive.original_fields = source.fields.clone();
        archive.asset_digests = draft.assets.keys().cloned().collect();
        let mut issue = Issue::new(
            "GENERATION_ENGINE_UNVERIFIED",
            Severity::Error,
            None,
            "Installed engine parameter support and full prompt preservation require compatibility verification.",
        );
        issue.stage = "generation".into();
        issue.source_refs = vec![source.id.to_string()];
        draft.document.issues.push(issue);
        Ok(draft)
    }
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let registry = linguist_config::Registry::builtin();
        for key in [
            "llm.endpoint",
            "llm.api_key_env",
            "llm.model",
            "llm.context_tokens",
            "llm.max_output_tokens",
            "llm.timeout_seconds",
            "llm.temperature",
            "llm.seed",
            "llm.keep_alive",
            "network.offline",
            "network.proxy_env",
            "network.connect_timeout_seconds",
            "network.request_timeout_seconds",
            "network.max_response_mb",
            "services.ollama.concurrency",
            "services.ollama.min_interval_seconds",
            "retry.read_attempts",
            "retry.initial_backoff_seconds",
            "retry.max_backoff_seconds",
            "retry.jitter_fraction",
        ] {
            registry.validate_value(
                key,
                settings.values.get(key).ok_or("OLLAMA_SETTING_MISSING")?,
            )?;
        }
        if settings.values["llm.model"].is_null() {
            return Err("CAPABILITY_UNAVAILABLE: select an installed generation model".into());
        }
        if settings.values["llm.max_output_tokens"].as_u64().unwrap()
            >= settings.values["llm.context_tokens"].as_u64().unwrap()
        {
            return Err("OLLAMA_CONTEXT_LIMIT_CONFLICT".into());
        }
        if !settings.values["network.proxy_env"].is_null()
            || settings.values["services.ollama.concurrency"] != 1
        {
            return Err("CAPABILITY_UNAVAILABLE: Ollama proxy transport or concurrency above one is not implemented".into());
        }
        let endpoint = url::Url::parse(settings.values["llm.endpoint"].as_str().unwrap())
            .map_err(|_| "OLLAMA_ENDPOINT_INVALID")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("OLLAMA_ENDPOINT_INVALID".into());
        }
        if endpoint.path() != "/" {
            return Err(
                "CAPABILITY_UNAVAILABLE: Ollama endpoint subpaths are not supported".into(),
            );
        }
        let host = endpoint.host_str().ok_or("OLLAMA_ENDPOINT_INVALID")?;
        let bare = host.trim_matches(['[', ']']);
        let port = endpoint
            .port_or_known_default()
            .ok_or("OLLAMA_ENDPOINT_INVALID")?;
        let addresses = if bare == "localhost" {
            vec![
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
            ]
        } else if let Ok(ip) = bare.parse::<IpAddr>() {
            if !ip.is_loopback() {
                return Err(
                    "CAPABILITY_UNAVAILABLE: remote Ollama transport is not implemented".into(),
                );
            }
            vec![SocketAddr::new(ip, port)]
        } else {
            return Err(
                "CAPABILITY_UNAVAILABLE: remote Ollama transport is not implemented".into(),
            );
        };
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(name) = settings.values["llm.api_key_env"].as_str() {
            let secret = environment
                .get(name)
                .filter(|key| !key.trim().is_empty())
                .ok_or("OLLAMA_CREDENTIAL_UNAVAILABLE")?;
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {secret}"))
                .map_err(|_| "OLLAMA_CREDENTIAL_INVALID")?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .resolve_to_addrs(bare, &addresses)
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(
                settings.values["network.connect_timeout_seconds"]
                    .as_u64()
                    .unwrap(),
            ))
            .build()
            .map_err(|_| "OLLAMA_TRANSPORT_UNAVAILABLE")?;
        let shared = endpoint_gate(&endpoint)?;
        Ok(Self {
            http,
            endpoint,
            settings: settings.clone(),
            shared,
        })
    }
    /// Development boundary, deliberately not wired into CLI preparation until
    /// engine-specific parameter/no-truncation compatibility evidence is available.
    /// Inference is sent once; ambiguous transport errors never trigger blind retry.
    pub fn generate_candidate(&self, document: &LearningDocument) -> Result<Candidate, String> {
        let request = build_request(document, &self.settings)?;
        let body = json!({
            "model": self.settings.values["llm.model"],
            "messages": [
                {"role":"system", "content":request.system_prompt},
                {"role":"user", "content":request.user_json}
            ],
            "format":request.output_schema,
            "stream":false,
            // Structured output only: reasoning tokens would consume the
            // num_predict budget and end in done_reason=length.
            "think":false,
            "truncate":false,
            "shift":false,
            "keep_alive":self.settings.values["llm.keep_alive"],
            "options": {
                "num_ctx":self.settings.values["llm.context_tokens"],
                "num_predict":self.settings.values["llm.max_output_tokens"],
                "temperature":self.settings.values["llm.temperature"],
                "seed":self.settings.values["llm.seed"]
            }
        });
        let request_bytes = canonical::bytes(&body).map_err(|_| "OLLAMA_REQUEST_INVALID")?;
        // Transport ceiling, not a tokenizer estimate or a claim of context fit.
        if request_bytes.len() > 100 * 1024 * 1024 {
            return Err("OLLAMA_REQUEST_LIMIT".into());
        }
        let deadline = self.deadline()?;
        let mut gate = self.lock(deadline)?;
        let mut retries = self.settings.values["retry.read_attempts"]
            .as_u64()
            .unwrap()
            - 1;
        let evidence = self.inspect(deadline, &mut retries, &mut gate)?;
        self.throttle(deadline, &gate)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("OLLAMA_DEADLINE")?;
        *gate = Some(Dispatch {
            started: Instant::now(),
            interval: self.interval(),
            server_delay: None,
        });
        let mut reply = self
            .http
            .post(
                self.endpoint
                    .join("api/chat")
                    .map_err(|_| "OLLAMA_ENDPOINT_INVALID")?,
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request_bytes.clone())
            // Inference is bounded by the remaining llm.timeout_seconds budget;
            // network.request_timeout_seconds governs metadata reads only.
            .timeout(remaining)
            .send()
            .map_err(|_| "OLLAMA_INFERENCE_OUTCOME_UNKNOWN")?;
        let status = reply.status();
        if status.is_redirection() {
            return Err("OLLAMA_REDIRECT_REJECTED".into());
        }
        if !status.is_success() {
            if matches!(status.as_u16(), 408 | 425 | 429 | 500..=599) {
                let cooldown = delay(reply.headers())?;
                if let Some(dispatch) = &mut *gate {
                    dispatch.server_delay = Some((Instant::now(), cooldown));
                }
            }
            return Err(format!("OLLAMA_INFERENCE_HTTP_FAILED: {}", status.as_u16()));
        }
        let limit = self.settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        if reply.content_length().is_some_and(|size| size > limit) {
            return Err("OLLAMA_RESPONSE_LIMIT".into());
        }
        let mime = reply
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .ok_or("OLLAMA_CONTENT_TYPE_INVALID")?;
        if mime.split(';').next().unwrap().trim() != "application/json" {
            return Err("OLLAMA_CONTENT_TYPE_INVALID".into());
        }
        let mut raw = Vec::new();
        reply
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "OLLAMA_INFERENCE_OUTCOME_UNKNOWN")?;
        if Instant::now() >= deadline {
            return Err("OLLAMA_DEADLINE".into());
        }
        let completion = parse_completion(&raw, &self.settings)?;
        // Reserve the configured full output cap, not just the observed completion.
        if completion
            .prompt_tokens
            .checked_add(
                self.settings.values["llm.max_output_tokens"]
                    .as_u64()
                    .unwrap(),
            )
            .is_none_or(|total| {
                total > self.settings.values["llm.context_tokens"].as_u64().unwrap()
            })
        {
            return Err("OLLAMA_COMPLETION_TOKEN_LIMIT".into());
        }
        let after = self.inspect(deadline, &mut retries, &mut gate)?;
        if after.identity.digest != evidence.identity.digest
            || after.show_digest != evidence.show_digest
        {
            return Err("OLLAMA_MODEL_MANIFEST_CONFLICT".into());
        }
        Ok(Candidate {
            request,
            request_bytes,
            evidence,
            evidence_after: after,
            completion,
        })
    }
    /// The running engine's self-reported version (`/api/version`).
    pub fn engine_version(&self) -> Result<String, String> {
        let deadline = self.deadline()?;
        let mut gate = self.lock(deadline)?;
        let mut retries = self.settings.values["retry.read_attempts"]
            .as_u64()
            .unwrap()
            - 1;
        let bytes = self.read(ReadAction::Version, deadline, &mut retries, &mut gate)?;
        let limit = self.settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        response(&bytes, limit)?["version"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 64 && !v.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or_else(|| "OLLAMA_VERSION_INVALID".into())
    }
    pub(super) fn settings(&self) -> &Effective {
        &self.settings
    }
    /// One nonstreaming chat call for certification probes: the HTTP status
    /// and the bounded body, success or not. Never retried.
    pub(super) fn post_chat(&self, body: &serde_json::Value) -> Result<(u16, Vec<u8>), String> {
        let deadline = self.deadline()?;
        let mut gate = self.lock(deadline)?;
        self.throttle(deadline, &gate)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("OLLAMA_DEADLINE")?;
        *gate = Some(Dispatch {
            started: Instant::now(),
            interval: self.interval(),
            server_delay: None,
        });
        let bytes = canonical::bytes(body).map_err(|_| "OLLAMA_REQUEST_INVALID")?;
        let mut reply = self
            .http
            .post(
                self.endpoint
                    .join("api/chat")
                    .map_err(|_| "OLLAMA_ENDPOINT_INVALID")?,
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes)
            .timeout(remaining)
            .send()
            .map_err(|_| "OLLAMA_INFERENCE_OUTCOME_UNKNOWN")?;
        let status = reply.status().as_u16();
        let limit = self.settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        let mut raw = Vec::new();
        reply
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "OLLAMA_INFERENCE_OUTCOME_UNKNOWN")?;
        if raw.len() as u64 > limit {
            return Err("OLLAMA_RESPONSE_LIMIT".into());
        }
        Ok((status, raw))
    }
    pub fn model_evidence(&self) -> Result<ModelEvidence, String> {
        let deadline = self.deadline()?;
        let mut gate = self.lock(deadline)?;
        let mut retries = self.settings.values["retry.read_attempts"]
            .as_u64()
            .unwrap()
            - 1;
        self.inspect(deadline, &mut retries, &mut gate)
    }
    fn deadline(&self) -> Result<Instant, String> {
        Instant::now()
            .checked_add(Duration::from_secs(
                self.settings.values["llm.timeout_seconds"]
                    .as_u64()
                    .unwrap(),
            ))
            .ok_or_else(|| "OLLAMA_DEADLINE".into())
    }
    fn lock(
        &self,
        deadline: Instant,
    ) -> Result<std::sync::MutexGuard<'_, Option<Dispatch>>, String> {
        loop {
            match self.shared.try_lock() {
                Ok(gate) => return Ok(gate),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err("OLLAMA_GATE_UNAVAILABLE".into());
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    wait(Duration::from_millis(10), deadline)?
                }
            }
        }
    }
    fn inspect(
        &self,
        deadline: Instant,
        retries: &mut u64,
        gate: &mut Option<Dispatch>,
    ) -> Result<ModelEvidence, String> {
        // Initial reads are necessary; all additional attempts share this one retry budget.
        let before = self.read(ReadAction::Tags, deadline, retries, gate)?;
        let limit = self.settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        let parsed = response(&before, limit)?;
        let name = self.settings.values["llm.model"]
            .as_str()
            .ok_or("OLLAMA_MODEL_UNAVAILABLE")?;
        identity(selected(&parsed, name)?)?; // Never show a remote-forwarded or absent model.
        let show = self.read(ReadAction::Show, deadline, retries, gate)?;
        response(&show, limit)?;
        let after = self.read(ReadAction::Tags, deadline, retries, gate)?;
        let evidence = verify_local_model(&before, &show, &after, &self.settings)?;
        if Instant::now() >= deadline {
            return Err("OLLAMA_DEADLINE".into());
        }
        Ok(evidence)
    }
    fn interval(&self) -> Duration {
        Duration::from_secs_f64(
            self.settings.values["services.ollama.min_interval_seconds"]
                .as_f64()
                .unwrap(),
        )
    }
    fn throttle(&self, deadline: Instant, last: &Option<Dispatch>) -> Result<(), String> {
        if let Some(previous) = *last {
            let cooldown = previous
                .server_delay
                .map(|(start, duration)| duration.saturating_sub(start.elapsed()))
                .unwrap_or(Duration::ZERO);
            wait(
                self.interval()
                    .max(previous.interval)
                    .saturating_sub(previous.started.elapsed())
                    .max(cooldown),
                deadline,
            )?;
        }
        Ok(())
    }
    fn read(
        &self,
        action: ReadAction,
        deadline: Instant,
        retries: &mut u64,
        last: &mut Option<Dispatch>,
    ) -> Result<Vec<u8>, String> {
        let limit = self.settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap()
            * 1024
            * 1024;
        let interval = Duration::from_secs_f64(
            self.settings.values["services.ollama.min_interval_seconds"]
                .as_f64()
                .unwrap(),
        );
        let mut attempt = 0;
        loop {
            if let Some(previous) = *last {
                let cooldown = previous
                    .server_delay
                    .map(|(start, duration)| duration.saturating_sub(start.elapsed()))
                    .unwrap_or(Duration::ZERO);
                wait(
                    interval
                        .max(previous.interval)
                        .saturating_sub(previous.started.elapsed())
                        .max(cooldown),
                    deadline,
                )?;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("OLLAMA_DEADLINE")?;
            let timeout = remaining.min(Duration::from_secs(
                self.settings.values["network.request_timeout_seconds"]
                    .as_u64()
                    .unwrap(),
            ));
            let path = match action {
                ReadAction::Tags => "api/tags",
                ReadAction::Show => "api/show",
                ReadAction::Version => "api/version",
            };
            let url = self
                .endpoint
                .join(path)
                .map_err(|_| "OLLAMA_ENDPOINT_INVALID")?;
            let request = match action {
                ReadAction::Tags | ReadAction::Version => self.http.get(url),
                ReadAction::Show => self
                    .http
                    .post(url)
                    .json(&json!({"model":self.settings.values["llm.model"],"verbose":false})),
            };
            *last = Some(Dispatch {
                started: Instant::now(),
                interval,
                server_delay: None,
            });
            let reply = request.timeout(timeout).send();
            let mut retry_after = Duration::ZERO;
            let failure = match reply {
                Err(error) if error.is_timeout() || error.is_connect() || error.is_body() => {
                    "OLLAMA_READ_FAILED".to_owned()
                }
                Err(_) => return Err("OLLAMA_READ_FAILED".into()),
                Ok(mut reply) => {
                    let status = reply.status();
                    if status.is_redirection() {
                        return Err("OLLAMA_REDIRECT_REJECTED".into());
                    }
                    if !status.is_success() {
                        if !matches!(status.as_u16(), 408 | 425 | 429 | 500..=599) {
                            return Err(format!("OLLAMA_HTTP_FAILED: {}", status.as_u16()));
                        }
                        retry_after = delay(reply.headers())?;
                        if let Some(dispatch) = last {
                            dispatch.server_delay = Some((Instant::now(), retry_after));
                        }
                        format!("OLLAMA_HTTP_FAILED: {}", status.as_u16())
                    } else {
                        if reply.content_length().is_some_and(|size| size > limit) {
                            return Err("OLLAMA_RESPONSE_LIMIT".into());
                        }
                        let mime = reply
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .ok_or("OLLAMA_CONTENT_TYPE_INVALID")?;
                        if mime.split(';').next().unwrap().trim() != "application/json" {
                            return Err("OLLAMA_CONTENT_TYPE_INVALID".into());
                        }
                        let mut bytes = Vec::new();
                        match reply.by_ref().take(limit + 1).read_to_end(&mut bytes) {
                            Ok(_) => {
                                if bytes.len() as u64 > limit {
                                    return Err("OLLAMA_RESPONSE_LIMIT".into());
                                }
                                if Instant::now() >= deadline {
                                    return Err("OLLAMA_DEADLINE".into());
                                }
                                return Ok(bytes);
                            }
                            Err(_) => "OLLAMA_READ_FAILED".into(),
                        }
                    }
                }
            };
            if *retries == 0 {
                return Err(failure);
            }
            *retries -= 1;
            let initial = self.settings.values["retry.initial_backoff_seconds"]
                .as_f64()
                .unwrap();
            let maximum = self.settings.values["retry.max_backoff_seconds"]
                .as_f64()
                .unwrap();
            let jitter = self.settings.values["retry.jitter_fraction"]
                .as_f64()
                .unwrap();
            let random = uuid::Uuid::new_v4();
            let fraction = u32::from_be_bytes(random.as_bytes()[..4].try_into().unwrap()) as f64
                / u32::MAX as f64;
            let backoff = (initial * 2f64.powi(attempt)).min(maximum)
                * (1.0 - jitter + 2.0 * jitter * fraction);
            wait(
                Duration::from_secs_f64(backoff.min(maximum)).max(retry_after),
                deadline,
            )?;
            attempt += 1;
        }
    }
}
fn delay(headers: &reqwest::header::HeaderMap) -> Result<Duration, String> {
    let mut values = headers.get_all(reqwest::header::RETRY_AFTER).iter();
    let Some(value) = values.next() else {
        return Ok(Duration::ZERO);
    };
    if values.next().is_some() {
        return Err("OLLAMA_RETRY_AFTER_INVALID".into());
    }
    let text = value
        .to_str()
        .map_err(|_| "OLLAMA_RETRY_AFTER_INVALID")?
        .trim_matches([' ', '\t']);
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return text
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| "OLLAMA_RETRY_AFTER_INVALID".into());
    }
    let date = httpdate::parse_http_date(text).map_err(|_| "OLLAMA_RETRY_AFTER_INVALID")?;
    Ok(date
        .duration_since(SystemTime::now())
        .unwrap_or(Duration::ZERO))
}
fn wait(delay: Duration, deadline: Instant) -> Result<(), String> {
    if delay
        >= deadline
            .checked_duration_since(Instant::now())
            .ok_or("OLLAMA_DEADLINE")?
    {
        return Err("OLLAMA_DEADLINE".into());
    }
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    Ok(())
}
