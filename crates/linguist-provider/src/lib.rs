//! ALG-PROVIDER: the shared external-service read boundary.
//!
//! Every adapter reads through [`Reader`], which applies the destination policy,
//! per-origin rate gates, per-service concurrency, one total deadline, bounded
//! retries with Retry-After, bounded bodies, MIME checks and the provider cache.
//! Responses are untrusted data; this crate never interprets them.
pub mod cache;
pub mod process;

use linguist_config::Effective;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The requested adapter or option is not available in this build.
    Unavailable,
    /// Destination, offline or configuration policy refused the request.
    Policy,
    Offline,
    CacheMiss,
    Dns,
    Deadline,
    Transport,
    Http(u16),
    Redirect,
    ResponseLimit,
    ContentType,
    Schema,
    RetryAfter,
}
impl ReadError {
    /// Only classified transient read failures may be retried or served from cache.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Dns | Self::Deadline | Self::Transport | Self::Http(408 | 425 | 429 | 500..=599)
        )
    }
}
impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PROVIDER_READ_{self:?}")
    }
}
impl std::error::Error for ReadError {}

/// Service names select the `services.<name>.*` pacing settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Service {
    Dictionary,
    Kanji,
    Image,
    Tts,
}
impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dictionary => "dictionary",
            Self::Kanji => "kanji",
            Self::Image => "image",
            Self::Tts => "tts",
        }
    }
}

/// Public hosts are adapter allowlists that must resolve only to public
/// addresses over HTTPS. Configured hosts are explicitly allowed endpoints
/// (loopback or `network.allowed_remote_service_hosts`) and may be private.
#[derive(Debug, Clone, Default)]
pub struct Destinations {
    pub public: BTreeSet<String>,
    pub configured: BTreeSet<String>,
}
impl Destinations {
    pub fn public(hosts: &[&str]) -> Self {
        Self {
            public: hosts.iter().map(|h| h.to_string()).collect(),
            configured: BTreeSet::new(),
        }
    }
    /// Validate one request or redirect destination before any connection.
    pub fn check(&self, url: &url::Url) -> Result<(), ReadError> {
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(ReadError::Policy);
        }
        let host = url
            .host_str()
            .ok_or(ReadError::Policy)?
            .to_ascii_lowercase();
        if self.configured.contains(&host) {
            return match url.scheme() {
                "https" | "http" => Ok(()),
                _ => Err(ReadError::Policy),
            };
        }
        if url.scheme() != "https" || !self.public.contains(&host) {
            return Err(ReadError::Policy);
        }
        // Allowlisted public names are never IP literals.
        if matches!(url.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
            return Err(ReadError::Policy);
        }
        Ok(())
    }
}

/// True only for globally routable unicast addresses.
pub fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_broadcast()
                && !v.is_unspecified()
                && !v.is_multicast()
                && o[0] != 0
                && o[0] < 240
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 198 && (18..=19).contains(&o[1]))
                && !(o[0] == 192 && o[1] == 0)
                && !(o[0] == 198 && o[1] == 51 && o[2] == 100)
                && !(o[0] == 203 && o[1] == 0 && o[2] == 113)
        }
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public_address(IpAddr::V4(v)))
            .unwrap_or_else(|| {
                let s = v.segments();
                s[0] & 0xe000 == 0x2000 && !(s[0] == 0x2001 && s[1] == 0xdb8)
            }),
    }
}

/// Every connection, including redirects, resolves through this policy. A
/// public host with any private address is refused rather than filtered.
struct PolicyResolver {
    destinations: Destinations,
    timeout: Duration,
}
impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_ascii_lowercase();
        let configured = self.destinations.configured.contains(&host);
        let allowed = configured || self.destinations.public.contains(&host);
        let timeout = self.timeout;
        let state = Arc::new(Mutex::new((
            None::<Result<Vec<SocketAddr>, ReadError>>,
            None::<std::task::Waker>,
        )));
        if allowed {
            let shared = state.clone();
            std::thread::spawn(move || {
                let result = (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|a| a.take(65).collect::<Vec<_>>())
                    .map_err(|_| ReadError::Dns)
                    .and_then(|addresses| {
                        if addresses.is_empty() || addresses.len() > 64 {
                            Err(ReadError::Dns)
                        } else if !configured && addresses.iter().any(|a| !public_address(a.ip())) {
                            Err(ReadError::Policy)
                        } else {
                            Ok(addresses)
                        }
                    });
                let mut guard = shared.lock().unwrap();
                guard.0 = Some(result);
                if let Some(waker) = guard.1.take() {
                    waker.wake();
                }
            });
        } else {
            state.lock().unwrap().0 = Some(Err(ReadError::Policy));
        }
        let started = Instant::now();
        Box::pin(std::future::poll_fn(move |context| {
            let mut guard = state.lock().unwrap();
            match guard.0.take() {
                Some(Ok(addresses)) => std::task::Poll::Ready(Ok(
                    Box::new(addresses.into_iter()) as reqwest::dns::Addrs
                )),
                Some(Err(error)) => std::task::Poll::Ready(Err(error.to_string().into())),
                None if started.elapsed() >= timeout => {
                    std::task::Poll::Ready(Err(ReadError::Dns.to_string().into()))
                }
                None => {
                    guard.1 = Some(context.waker().clone());
                    std::task::Poll::Pending
                }
            }
        }))
    }
}

#[derive(Clone, Copy)]
struct Dispatch {
    started: Instant,
    interval: Duration,
    server_delay: Option<(Instant, Duration)>,
}
/// Rate history lives for the process so a new client cannot erase it.
type OriginGate = Arc<Mutex<Option<Dispatch>>>;
#[derive(Default)]
pub struct Semaphore {
    inflight: Mutex<usize>,
    released: Condvar,
}
pub struct Permit(Arc<Semaphore>);
impl Drop for Permit {
    fn drop(&mut self) {
        *self.0.inflight.lock().unwrap() -= 1;
        self.0.released.notify_all();
    }
}
impl Semaphore {
    pub fn acquire(self: &Arc<Self>, limit: usize, deadline: Instant) -> Result<Permit, ReadError> {
        let mut inflight = self.inflight.lock().map_err(|_| ReadError::Unavailable)?;
        while *inflight >= limit {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(ReadError::Deadline)?;
            inflight = self
                .released
                .wait_timeout(inflight, remaining)
                .map_err(|_| ReadError::Unavailable)?
                .0;
        }
        *inflight += 1;
        Ok(Permit(self.clone()))
    }
}

#[derive(Default)]
pub struct Gates {
    origins: Mutex<HashMap<String, OriginGate>>,
    services: Mutex<HashMap<Service, Arc<Semaphore>>>,
}
impl Gates {
    pub fn global() -> &'static Gates {
        static GATES: OnceLock<Gates> = OnceLock::new();
        GATES.get_or_init(Gates::default)
    }
    fn origin(&self, url: &url::Url) -> OriginGate {
        let key = url.origin().ascii_serialization();
        self.origins.lock().unwrap().entry(key).or_default().clone()
    }
    pub fn service(&self, service: Service) -> Arc<Semaphore> {
        self.services
            .lock()
            .unwrap()
            .entry(service)
            .or_default()
            .clone()
    }
}

/// Pacing and bounds resolved from settings; public so tests and adapters can
/// construct readers for explicitly configured endpoints.
#[derive(Debug, Clone)]
pub struct Limits {
    pub attempts: u32,
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub interval: Duration,
    pub concurrency: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub jitter: f64,
    pub max_body: u64,
    pub max_redirects: u32,
}

#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: url::Url,
    pub final_url: url::Url,
    pub mime: String,
    pub bytes: Vec<u8>,
    /// Seconds since the Unix epoch when the bytes were received.
    pub fetched_at: u64,
    pub from_cache: bool,
}

#[derive(Clone)]
pub struct Reader {
    service: Service,
    http: reqwest::blocking::Client,
    destinations: Destinations,
    limits: Limits,
    user_agent: String,
    cache: Option<cache::Cache>,
    offline: bool,
    gates: &'static Gates,
}

const SETTINGS: [&str; 13] = [
    "network.offline",
    "network.allowed_remote_service_hosts",
    "network.proxy_env",
    "network.connect_timeout_seconds",
    "network.request_timeout_seconds",
    "network.max_response_mb",
    "network.max_redirects",
    "retry.read_attempts",
    "retry.initial_backoff_seconds",
    "retry.max_backoff_seconds",
    "retry.jitter_fraction",
    "dictionary.user_agent",
    "storage.cache_dir",
];

impl Reader {
    /// Build a reader for an adapter's fixed public hosts plus any configured
    /// endpoint hosts. Configured non-loopback hosts must be explicitly allowed.
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
        service: Service,
        public_hosts: &[&str],
        configured_endpoints: &[&url::Url],
    ) -> Result<Self, ReadError> {
        let registry = linguist_config::Registry::builtin();
        let interval_key = format!("services.{}.min_interval_seconds", service.name());
        let concurrency_key = format!("services.{}.concurrency", service.name());
        for key in SETTINGS
            .iter()
            .copied()
            .chain([interval_key.as_str(), concurrency_key.as_str()])
            .chain(cache::SETTINGS)
        {
            registry
                .validate_value(key, settings.values.get(key).ok_or(ReadError::Policy)?)
                .map_err(|_| ReadError::Policy)?;
        }
        let v = &settings.values;
        // Proxy support would let the proxy resolve names outside the destination policy.
        if !v["network.proxy_env"].is_null() {
            return Err(ReadError::Unavailable);
        }
        let allowed: BTreeSet<String> = v["network.allowed_remote_service_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|h| h.as_str().map(str::to_ascii_lowercase))
            .collect();
        let mut destinations = Destinations::public(public_hosts);
        for endpoint in configured_endpoints {
            let host = endpoint
                .host_str()
                .ok_or(ReadError::Policy)?
                .to_ascii_lowercase();
            let loopback = host == "localhost"
                || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
            if !loopback && !allowed.contains(&host) {
                return Err(ReadError::Policy);
            }
            destinations.configured.insert(host);
        }
        let limits = Limits {
            attempts: v["retry.read_attempts"].as_u64().unwrap() as u32,
            timeout: Duration::from_secs(v["network.request_timeout_seconds"].as_u64().unwrap()),
            connect_timeout: Duration::from_secs(
                v["network.connect_timeout_seconds"].as_u64().unwrap(),
            ),
            interval: Duration::from_secs_f64(v[&interval_key].as_f64().unwrap()),
            concurrency: v[&concurrency_key].as_u64().unwrap() as usize,
            initial_backoff: Duration::from_secs_f64(
                v["retry.initial_backoff_seconds"].as_f64().unwrap(),
            ),
            max_backoff: Duration::from_secs_f64(v["retry.max_backoff_seconds"].as_f64().unwrap()),
            jitter: v["retry.jitter_fraction"].as_f64().unwrap(),
            max_body: v["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
            max_redirects: v["network.max_redirects"].as_u64().unwrap() as u32,
        };
        let cache = cache::Cache::from_settings(settings, environment, service)?;
        let mut reader = Self::new(
            service,
            destinations,
            limits,
            v["dictionary.user_agent"].as_str().unwrap(),
            Some(cache),
        )?;
        reader.offline = v["network.offline"] == true;
        Ok(reader)
    }

    pub fn new(
        service: Service,
        destinations: Destinations,
        limits: Limits,
        user_agent: &str,
        cache: Option<cache::Cache>,
    ) -> Result<Self, ReadError> {
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(limits.connect_timeout)
            .dns_resolver(Arc::new(PolicyResolver {
                destinations: destinations.clone(),
                timeout: limits.connect_timeout,
            }))
            .user_agent(user_agent)
            .build()
            .map_err(|_| ReadError::Unavailable)?;
        Ok(Self {
            service,
            http,
            destinations,
            limits,
            user_agent: user_agent.into(),
            cache,
            offline: false,
            gates: Gates::global(),
        })
    }

    pub fn with_gates(mut self, gates: &'static Gates) -> Self {
        self.gates = gates;
        self
    }
    pub fn limits_mut(&mut self) -> &mut Limits {
        &mut self.limits
    }
    pub fn service(&self) -> Service {
        self.service
    }

    /// Read one resource. `accept` lists permitted MIME essences.
    pub fn get(&self, url: &url::Url, accept: &[&str]) -> Result<Fetched, ReadError> {
        self.destinations.check(url)?;
        let key = cache::request_key(self.service, url, accept, &self.user_agent);
        let cached = || self.cache.as_ref().and_then(|c| c.load(&key, url));
        if self.offline {
            // Offline mode never contacts external services; explicitly configured
            // loopback endpoints remain usable.
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            if !self.destinations.configured.contains(&host) {
                return cached().ok_or(ReadError::Offline);
            }
        }
        match self.cache.as_ref().map(|c| c.policy) {
            Some(cache::Policy::CacheOnly) => return cached().ok_or(ReadError::CacheMiss),
            Some(cache::Policy::PreferCache) => {
                if let Some(hit) = cached() {
                    return Ok(hit);
                }
            }
            _ => {}
        }
        match self.fetch(url, accept) {
            Ok(fetched) => {
                if let Some(cache) = &self.cache {
                    // A cache write failure never fails a successful read.
                    let _ = cache.store(&key, &fetched);
                }
                Ok(fetched)
            }
            Err(error) if error.is_transient() => cached().ok_or(error),
            Err(error) => Err(error),
        }
    }

    /// Queueing, rate waits, backoff, redirects and bodies share one deadline.
    fn fetch(&self, url: &url::Url, accept: &[&str]) -> Result<Fetched, ReadError> {
        let deadline = Instant::now()
            .checked_add(self.limits.timeout)
            .ok_or(ReadError::Policy)?;
        let _permit = self
            .gates
            .service(self.service)
            .acquire(self.limits.concurrency, deadline)?;
        let mut last = ReadError::Transport;
        for attempt in 0..self.limits.attempts {
            let mut destination = url.clone();
            let mut redirects = 0;
            let retry_after;
            loop {
                let gate = self.gates.origin(&destination);
                self.pace(&gate, deadline)?;
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(ReadError::Deadline)?;
                let mut response =
                    match self.http.get(destination.clone()).timeout(remaining).send() {
                        Ok(response) => response,
                        Err(error) if error.is_timeout() && Instant::now() >= deadline => {
                            return Err(ReadError::Deadline);
                        }
                        Err(error) if is_policy_refusal(&error) => return Err(ReadError::Policy),
                        Err(error)
                            if error.is_timeout() || error.is_connect() || error.is_body() =>
                        {
                            last = ReadError::Transport;
                            retry_after = Duration::ZERO;
                            break;
                        }
                        Err(_) => return Err(ReadError::Transport),
                    };
                let status = response.status();
                if status.is_redirection() {
                    if redirects >= self.limits.max_redirects {
                        return Err(ReadError::Redirect);
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or(ReadError::Redirect)?;
                    let next = destination
                        .join(location)
                        .map_err(|_| ReadError::Redirect)?;
                    self.destinations
                        .check(&next)
                        .map_err(|_| ReadError::Redirect)?;
                    record_server_delay(&gate, retry_delay(response.headers(), SystemTime::now())?);
                    destination = next;
                    redirects += 1;
                    continue;
                }
                if !status.is_success() {
                    let error = ReadError::Http(status.as_u16());
                    if !error.is_transient() {
                        return Err(error);
                    }
                    retry_after = retry_delay(response.headers(), SystemTime::now())?;
                    record_server_delay(&gate, retry_after);
                    last = error;
                    break;
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > self.limits.max_body)
                {
                    return Err(ReadError::ResponseLimit);
                }
                let mime = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(|v| v.split(';').next().unwrap().trim().to_ascii_lowercase())
                    .ok_or(ReadError::ContentType)?;
                if !accept.contains(&mime.as_str()) {
                    return Err(ReadError::ContentType);
                }
                let mut bytes = Vec::new();
                if response
                    .by_ref()
                    .take(self.limits.max_body + 1)
                    .read_to_end(&mut bytes)
                    .is_err()
                {
                    last = ReadError::Transport;
                    retry_after = Duration::ZERO;
                    break;
                }
                if bytes.len() as u64 > self.limits.max_body {
                    return Err(ReadError::ResponseLimit);
                }
                if Instant::now() >= deadline {
                    return Err(ReadError::Deadline);
                }
                return Ok(Fetched {
                    url: url.clone(),
                    final_url: destination,
                    mime,
                    bytes,
                    fetched_at: unix_now(),
                    from_cache: false,
                });
            }
            if attempt + 1 == self.limits.attempts {
                return Err(last);
            }
            wait(self.backoff(attempt).max(retry_after), deadline)?;
        }
        Err(last)
    }

    /// Wait for both the origin's minimum interval and any server-requested delay.
    fn pace(&self, gate: &OriginGate, deadline: Instant) -> Result<(), ReadError> {
        let mut shared = loop {
            match gate.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(ReadError::Unavailable),
                Err(std::sync::TryLockError::WouldBlock) => {
                    wait(Duration::from_millis(5), deadline)?
                }
            }
        };
        if let Some(previous) = *shared {
            wait(
                self.limits
                    .interval
                    .max(previous.interval)
                    .saturating_sub(previous.started.elapsed())
                    .max(
                        previous
                            .server_delay
                            .map(|(started, d)| d.saturating_sub(started.elapsed()))
                            .unwrap_or(Duration::ZERO),
                    ),
                deadline,
            )?;
        }
        *shared = Some(Dispatch {
            started: Instant::now(),
            interval: self.limits.interval,
            server_delay: None,
        });
        Ok(())
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let random = uuid::Uuid::new_v4();
        let fraction = u32::from_be_bytes(random.as_bytes()[0..4].try_into().unwrap()) as f64
            / u32::MAX as f64;
        let maximum = self.limits.max_backoff.as_secs_f64();
        let base =
            (self.limits.initial_backoff.as_secs_f64() * 2f64.powi(attempt as i32)).min(maximum);
        let jitter = self.limits.jitter;
        Duration::from_secs_f64((base * (1.0 - jitter + 2.0 * jitter * fraction)).min(maximum))
    }
}

fn is_policy_refusal(error: &reqwest::Error) -> bool {
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(current) = source {
        if current.to_string().contains("PROVIDER_READ_Policy") {
            return true;
        }
        source = current.source();
    }
    false
}

fn record_server_delay(gate: &OriginGate, duration: Duration) {
    if let Ok(mut dispatch) = gate.lock()
        && let Some(dispatch) = dispatch.as_mut()
    {
        let previous = dispatch
            .server_delay
            .map(|(started, d)| d.saturating_sub(started.elapsed()))
            .unwrap_or(Duration::ZERO);
        dispatch.server_delay = Some((Instant::now(), previous.max(duration)));
    }
}

/// Convert wall-clock dates once; later waits use the monotonic deadline.
pub fn retry_delay(
    headers: &reqwest::header::HeaderMap,
    now: SystemTime,
) -> Result<Duration, ReadError> {
    let mut values = headers.get_all(reqwest::header::RETRY_AFTER).iter();
    let Some(value) = values.next() else {
        return Ok(Duration::ZERO);
    };
    // Retry-After is not a list field: conflicting values must not be guessed.
    if values.next().is_some() {
        return Err(ReadError::RetryAfter);
    }
    let text = value
        .to_str()
        .map_err(|_| ReadError::RetryAfter)?
        .trim_matches([' ', '\t']);
    if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
        return text
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| ReadError::RetryAfter);
    }
    let date = httpdate::parse_http_date(text).map_err(|_| ReadError::RetryAfter)?;
    Ok(date.duration_since(now).unwrap_or(Duration::ZERO))
}

pub fn wait(duration: Duration, deadline: Instant) -> Result<(), ReadError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(ReadError::Deadline)?;
    if duration >= remaining {
        return Err(ReadError::Deadline);
    }
    if !duration.is_zero() {
        std::thread::sleep(duration);
    }
    Ok(())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_policy_rejects_private_literals_credentials_and_unlisted_hosts() {
        let destinations = Destinations::public(&["commons.wikimedia.org"]);
        for bad in [
            "http://commons.wikimedia.org/x",
            "https://evil.example/x",
            "https://user@commons.wikimedia.org/x",
            "https://commons.wikimedia.org/x#frag",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                destinations.check(&bad.parse().unwrap()),
                Err(ReadError::Policy),
                "{bad}"
            );
        }
        assert!(
            destinations
                .check(&"https://commons.wikimedia.org/w/api.php".parse().unwrap())
                .is_ok()
        );
    }

    #[test]
    fn address_classification_denies_private_and_special_ranges() {
        for private in [
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:10.0.0.1",
            "2001:db8::1",
        ] {
            assert!(!public_address(private.parse().unwrap()), "{private}");
        }
        for public in ["8.8.8.8", "2606:4700::1111", "::ffff:1.1.1.1"] {
            assert!(public_address(public.parse().unwrap()), "{public}");
        }
    }
}

pub mod download;
