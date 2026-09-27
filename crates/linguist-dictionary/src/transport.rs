//! Fixed-origin dictionary reads with shared rate limits, total deadlines and bounded retries.
use crate::{JishoPage, parse_jisho};
use linguist_config::Effective;
use linguist_core::Language;
use std::{
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    Unavailable,
    Policy,
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
impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DICTIONARY_READ_{self:?}")
    }
}
impl std::error::Error for ReadError {}
#[derive(Clone, Copy)]
enum Provider {
    Jisho,
    Wiktionary,
}
#[derive(Clone, Copy)]
struct Dispatch {
    started: Instant,
    interval: Duration,
    server_delay: Option<(Instant, Duration)>,
}
type Gate = Arc<Mutex<Option<Dispatch>>>;
// Only these two fixed origins are supported. Keep gates for the process lifetime:
// dropping the last client must not erase the previous request's rate limit.
#[derive(Default)]
struct ServiceGates {
    jisho: Gate,
    wiktionary: Gate,
}
impl ServiceGates {
    fn get(&self, provider: Provider) -> Gate {
        match provider {
            Provider::Jisho => self.jisho.clone(),
            Provider::Wiktionary => self.wiktionary.clone(),
        }
    }
}
fn service_gate(provider: Provider) -> Gate {
    static GATES: OnceLock<ServiceGates> = OnceLock::new();
    GATES.get_or_init(ServiceGates::default).get(provider)
}

#[derive(Clone)]
pub struct JishoClient {
    provider: Provider,
    http: reqwest::blocking::Client,
    endpoint: url::Url,
    attempts: u32,
    timeout: Duration,
    interval: Duration,
    initial: Duration,
    maximum: Duration,
    jitter: f64,
    limit: u64,
    redirects: u32,
    shared: Gate,
}
fn public_address(ip: IpAddr) -> bool {
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

impl JishoClient {
    pub fn from_settings(settings: &Effective) -> Result<Self, ReadError> {
        Self::for_target(
            settings,
            &"ja".to_owned().try_into().map_err(|_| ReadError::Policy)?,
        )
    }
    pub fn for_target(settings: &Effective, target: &Language) -> Result<Self, ReadError> {
        let provider = match (
            settings
                .values
                .get("dictionary.provider")
                .and_then(serde_json::Value::as_str),
            target.as_str().split('-').next(),
        ) {
            (Some("auto" | "jisho"), Some("ja")) => Provider::Jisho,
            (Some("auto" | "wiktionary"), Some("en")) => Provider::Wiktionary,
            _ => return Err(ReadError::Unavailable),
        };
        let host = match provider {
            Provider::Jisho => "jisho.org",
            Provider::Wiktionary => "en.wiktionary.org",
        };
        let registry = linguist_config::Registry::builtin();
        for key in [
            "dictionary.provider",
            "dictionary.user_agent",
            "dictionary.browser_fallback",
            "services.dictionary.concurrency",
            "services.dictionary.min_interval_seconds",
            "network.offline",
            "network.proxy_env",
            "network.connect_timeout_seconds",
            "network.request_timeout_seconds",
            "network.max_response_mb",
            "network.max_redirects",
            "retry.read_attempts",
            "retry.initial_backoff_seconds",
            "retry.max_backoff_seconds",
            "retry.jitter_fraction",
        ] {
            registry
                .validate_value(key, settings.values.get(key).ok_or(ReadError::Policy)?)
                .map_err(|_| ReadError::Policy)?;
        }
        if settings.values["network.offline"] == true {
            return Err(ReadError::Policy);
        }
        if !settings.values["network.proxy_env"].is_null()
            || settings.values["dictionary.browser_fallback"] == true
            || settings.values["services.dictionary.concurrency"] != 1
        {
            return Err(ReadError::Unavailable);
        }
        let connect = Duration::from_secs(
            settings.values["network.connect_timeout_seconds"]
                .as_u64()
                .unwrap(),
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (host, 443)
                .to_socket_addrs()
                .map(|a| a.take(65).collect::<Vec<SocketAddr>>());
            let _ = sender.send(result);
        });
        let addresses = receiver
            .recv_timeout(connect)
            .map_err(|_| ReadError::Dns)?
            .map_err(|_| ReadError::Dns)?;
        if addresses.is_empty()
            || addresses.len() > 64
            || addresses
                .iter()
                .any(|address| !public_address(address.ip()))
        {
            return Err(ReadError::Policy);
        }
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(connect)
            .resolve_to_addrs(host, &addresses)
            .user_agent(settings.values["dictionary.user_agent"].as_str().unwrap())
            .build()
            .map_err(|_| ReadError::Unavailable)?;
        Ok(Self {
            http,
            provider,
            endpoint: url::Url::parse(match provider {
                Provider::Jisho => "https://jisho.org/api/v1/search/words",
                Provider::Wiktionary => "https://en.wiktionary.org/api/rest_v1/page/definition/",
            })
            .map_err(|_| ReadError::Policy)?,
            attempts: settings.values["retry.read_attempts"].as_u64().unwrap() as u32,
            timeout: Duration::from_secs(
                settings.values["network.request_timeout_seconds"]
                    .as_u64()
                    .unwrap(),
            ),
            interval: Duration::from_secs_f64(
                settings.values["services.dictionary.min_interval_seconds"]
                    .as_f64()
                    .unwrap(),
            ),
            initial: Duration::from_secs_f64(
                settings.values["retry.initial_backoff_seconds"]
                    .as_f64()
                    .unwrap(),
            ),
            maximum: Duration::from_secs_f64(
                settings.values["retry.max_backoff_seconds"]
                    .as_f64()
                    .unwrap(),
            ),
            jitter: settings.values["retry.jitter_fraction"].as_f64().unwrap(),
            limit: settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
            redirects: settings.values["network.max_redirects"].as_u64().unwrap() as u32,
            shared: service_gate(provider),
        })
    }
    fn parse(
        &self,
        query: &str,
        target: &Language,
        bytes: &[u8],
        limit: usize,
    ) -> Result<JishoPage, ReadError> {
        match self.provider {
            Provider::Jisho => parse_jisho(query, target, bytes, self.limit, limit),
            Provider::Wiktionary => {
                crate::wiktionary::parse_definition(query, target, bytes, self.limit, limit)
            }
        }
        .map_err(|_| ReadError::Schema)
    }
    /// Queueing, rate waits, backoff, redirects and response bodies share one lookup deadline.
    pub fn lookup(
        &self,
        query: &str,
        target: &Language,
        max_entries: usize,
    ) -> Result<JishoPage, ReadError> {
        self.parse(
            query,
            target,
            match self.provider {
                Provider::Jisho => b"{\"meta\":{\"status\":200},\"data\":[]}".as_slice(),
                Provider::Wiktionary => b"{}".as_slice(),
            },
            max_entries,
        )?;
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or(ReadError::Policy)?;
        let mut shared = loop {
            match self.shared.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(ReadError::Unavailable),
                Err(std::sync::TryLockError::WouldBlock) => {
                    wait(Duration::from_millis(10), deadline)?
                }
            }
        };
        let mut url = self.endpoint.clone();
        match self.provider {
            Provider::Jisho => {
                url.query_pairs_mut().append_pair("keyword", query);
            }
            Provider::Wiktionary => {
                url.path_segments_mut()
                    .map_err(|_| ReadError::Policy)?
                    .pop_if_empty()
                    .push(query);
            }
        }
        let mut last = ReadError::Transport;
        for attempt in 0..self.attempts {
            let mut destination = url.clone();
            let mut redirects = 0;
            let retry_after;
            loop {
                if let Some(previous) = *shared {
                    wait(
                        self.interval
                            .max(previous.interval)
                            .saturating_sub(previous.started.elapsed())
                            .max(
                                previous
                                    .server_delay
                                    .map(|(started, duration)| {
                                        duration.saturating_sub(started.elapsed())
                                    })
                                    .unwrap_or(Duration::ZERO),
                            ),
                        deadline,
                    )?;
                }
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(ReadError::Deadline)?;
                *shared = Some(Dispatch {
                    started: Instant::now(),
                    interval: self.interval,
                    server_delay: None,
                });
                let response = self.http.get(destination.clone()).timeout(remaining).send();
                let mut response = match response {
                    Ok(response) => response,
                    Err(error) if error.is_timeout() || error.is_connect() || error.is_body() => {
                        last = ReadError::Transport;
                        retry_after = Duration::ZERO;
                        break;
                    }
                    Err(_) => return Err(ReadError::Transport),
                };
                let status = response.status();
                if status.is_redirection() {
                    if redirects >= self.redirects {
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
                    if next.origin() != self.endpoint.origin()
                        || !next.username().is_empty()
                        || next.password().is_some()
                        || next.fragment().is_some()
                    {
                        return Err(ReadError::Redirect);
                    }
                    record_server_delay(
                        &mut shared,
                        retry_delay(response.headers(), SystemTime::now())?,
                    );
                    destination = next;
                    redirects += 1;
                    continue;
                }
                if !status.is_success() {
                    if !matches!(status.as_u16(), 408 | 425 | 429 | 500..=599) {
                        return Err(ReadError::Http(status.as_u16()));
                    }
                    retry_after = retry_delay(response.headers(), SystemTime::now())?;
                    record_server_delay(&mut shared, retry_after);
                    last = ReadError::Http(status.as_u16());
                    break;
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > self.limit)
                {
                    return Err(ReadError::ResponseLimit);
                }
                let mime = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(ReadError::ContentType)?;
                if mime.split(';').next().unwrap().trim() != "application/json" {
                    return Err(ReadError::ContentType);
                }
                let mut bytes = Vec::new();
                match response
                    .by_ref()
                    .take(self.limit + 1)
                    .read_to_end(&mut bytes)
                {
                    Ok(_) => {}
                    Err(_) => {
                        last = ReadError::Transport;
                        retry_after = Duration::ZERO;
                        break;
                    }
                }
                if bytes.len() as u64 > self.limit {
                    return Err(ReadError::ResponseLimit);
                }
                if Instant::now() >= deadline {
                    return Err(ReadError::Deadline);
                }
                return self.parse(query, target, &bytes, max_entries);
            }
            if attempt + 1 == self.attempts {
                return Err(last);
            }
            let random = uuid::Uuid::new_v4();
            let fraction = u32::from_be_bytes(random.as_bytes()[0..4].try_into().unwrap()) as f64
                / u32::MAX as f64;
            let base = (self.initial.as_secs_f64() * 2f64.powi(attempt as i32))
                .min(self.maximum.as_secs_f64());
            let seconds = (base * (1.0 - self.jitter + 2.0 * self.jitter * fraction))
                .min(self.maximum.as_secs_f64());
            wait(Duration::from_secs_f64(seconds).max(retry_after), deadline)?;
        }
        Err(last)
    }
}
fn record_server_delay(dispatch: &mut Option<Dispatch>, duration: Duration) {
    if let Some(dispatch) = dispatch {
        let previous = dispatch
            .server_delay
            .map(|(started, duration)| duration.saturating_sub(started.elapsed()))
            .unwrap_or(Duration::ZERO);
        dispatch.server_delay = Some((Instant::now(), previous.max(duration)));
    }
}

// Convert wall-clock dates once; subsequent waits use the monotonic lookup deadline.
fn retry_delay(
    headers: &reqwest::header::HeaderMap,
    now: SystemTime,
) -> Result<Duration, ReadError> {
    let mut values = headers.get_all(reqwest::header::RETRY_AFTER).iter();
    let Some(value) = values.next() else {
        return Ok(Duration::ZERO);
    };
    // Retry-After is not a list field: conflicting/multiple values must not be guessed.
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

fn wait(duration: Duration, deadline: Instant) -> Result<(), ReadError> {
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

/// Both adapters use the same fixed-origin request and retry policy.
pub type DictionaryClient = JishoClient;

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(replies: Vec<String>) -> (JishoClient, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = url::Url::parse(&format!(
            "http://{}/api/v1/search/words",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8(request).unwrap());
                use std::io::Write;
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        let client = JishoClient {
            provider: Provider::Jisho,
            http: reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()
                .unwrap(),
            endpoint,
            attempts: 3,
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
            initial: Duration::from_millis(1),
            maximum: Duration::from_millis(2),
            jitter: 0.0,
            limit: 1024,
            redirects: 2,
            shared: Arc::new(Mutex::new(None)),
        };
        (client, handle)
    }
    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
            body.len()
        )
    }
    fn ja() -> Language {
        "ja".to_owned().try_into().unwrap()
    }
    #[test]
    fn transient_reads_retry_but_bad_schema_and_auth_do_not() {
        let good = response(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"meta":{"status":200},"data":[]}"#,
        );
        let (client, server) = fixture(vec![
            response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
            good,
        ]);
        assert!(
            client
                .lookup("食べる", &ja(), 10)
                .unwrap()
                .entries
                .is_empty()
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/v1/search/words?keyword="));
        for (reply, error) in [
            (
                response("401 Unauthorized", "", "private credential detail"),
                ReadError::Http(401),
            ),
            (
                response(
                    "200 OK",
                    "Content-Type: application/json\r\n",
                    "invalid private content",
                ),
                ReadError::Schema,
            ),
        ] {
            let (client, server) = fixture(vec![reply]);
            assert_eq!(client.lookup("食べる", &ja(), 10).unwrap_err(), error);
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn redirect_origin_and_response_limits_are_enforced() {
        for (reply, error) in [
            (
                response("302 Found", "Location: http://evil.invalid/secret\r\n", ""),
                ReadError::Redirect,
            ),
            (
                response("200 OK", "Content-Type: text/html\r\n", "private HTML"),
                ReadError::ContentType,
            ),
            (
                response(
                    "200 OK",
                    "Content-Type: application/json\r\n",
                    &"x".repeat(1025),
                ),
                ReadError::ResponseLimit,
            ),
            (
                response("429 Too Many Requests", "Retry-After: invalid\r\n", ""),
                ReadError::RetryAfter,
            ),
        ] {
            let (client, server) = fixture(vec![reply]);
            assert_eq!(client.lookup("食べる", &ja(), 10).unwrap_err(), error);
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn rate_and_retry_after_cannot_exceed_total_deadline() {
        let (mut client, server) = fixture(vec![response(
            "429 Too Many Requests",
            "Retry-After: 10\r\n",
            "",
        )]);
        client.timeout = Duration::from_millis(100);
        let start = Instant::now();
        assert_eq!(
            client.lookup("食べる", &ja(), 10).unwrap_err(),
            ReadError::Deadline
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
        let deadline = Instant::now() + Duration::from_millis(5);
        assert_eq!(
            wait(Duration::from_secs(1), deadline),
            Err(ReadError::Deadline)
        );
    }
    #[test]
    fn retry_after_supports_dates_and_strict_nonnegative_seconds() {
        let future = httpdate::parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        let now = future - Duration::from_secs(12);
        for value in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
            "12",
            " 12\t",
        ] {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(reqwest::header::RETRY_AFTER, value.parse().unwrap());
            assert_eq!(retry_delay(&headers, now), Ok(Duration::from_secs(12)));
            if !value.trim().bytes().all(|byte| byte.is_ascii_digit()) {
                assert_eq!(
                    retry_delay(&headers, future + Duration::from_secs(1)),
                    Ok(Duration::ZERO)
                );
            }
        }
        for value in [
            "+1",
            "-1",
            "1.5",
            "18446744073709551616",
            "",
            "tomorrow",
            "1, 2",
        ] {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(reqwest::header::RETRY_AFTER, value.parse().unwrap());
            assert_eq!(retry_delay(&headers, now), Err(ReadError::RetryAfter));
        }
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_delay(&headers, now), Ok(Duration::ZERO));
        headers.append(reqwest::header::RETRY_AFTER, "0".parse().unwrap());
        headers.append(reqwest::header::RETRY_AFTER, "1".parse().unwrap());
        assert_eq!(retry_delay(&headers, now), Err(ReadError::RetryAfter));
    }
    #[test]
    fn date_retry_after_is_honored_for_retries_and_redirects() {
        let good = response(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"meta":{"status":200},"data":[]}"#,
        );
        let (client, server) = fixture(vec![
            response(
                "503 Service Unavailable",
                "Retry-After: Sun, 06 Nov 1994 08:49:37 GMT\r\n",
                "",
            ),
            good,
        ]);
        assert!(
            client
                .lookup("食べる", &ja(), 10)
                .unwrap()
                .entries
                .is_empty()
        );
        assert_eq!(server.join().unwrap().len(), 2);
        let future = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
        for (status, location) in [
            ("429 Too Many Requests", ""),
            ("302 Found", "Location: /again\r\n"),
        ] {
            let (mut client, server) = fixture(vec![response(
                status,
                &format!("{location}Retry-After: {future}\r\n"),
                "",
            )]);
            client.timeout = Duration::from_millis(100);
            let start = Instant::now();
            assert_eq!(
                client.lookup("食べる", &ja(), 10).unwrap_err(),
                ReadError::Deadline
            );
            assert!(start.elapsed() < Duration::from_secs(1));
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn english_transport_encodes_titles_and_uses_english_parser() {
        let body = r#"{"en":[{"language":"English","partOfSpeech":"Verb","definitions":[{"definition":"To eat"}]}]}"#;
        let (mut client, server) = fixture(vec![response(
            "200 OK",
            "Content-Type: application/json\r\n",
            body,
        )]);
        client.provider = Provider::Wiktionary;
        let page = client
            .lookup("eat/word", &"en".to_owned().try_into().unwrap(), 10)
            .unwrap();
        assert_eq!(page.entries[0].senses[0].definitions, vec!["To eat"]);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api/v1/search/words/eat%2Fword "));
        assert!(!requests[0].contains("keyword="));
    }
    #[test]
    fn independently_constructed_clients_preserve_endpoint_rate_history() {
        let gates = ServiceGates::default();
        let good = response(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"meta":{"status":200},"data":[]}"#,
        );
        let (mut first, server) = fixture(vec![good]);
        first.shared = gates.get(Provider::Jisho);
        first.interval = Duration::from_secs(60);
        first.lookup("食べる", &ja(), 10).unwrap();
        // A separately built HTTP client and a new handle after dropping the first
        // must still observe its interval, even if the new settings are faster.
        let mut next = JishoClient {
            http: reqwest::blocking::Client::builder()
                .no_proxy()
                .build()
                .unwrap(),
            shared: gates.get(Provider::Jisho),
            ..first.clone()
        };
        drop(first);
        next.interval = Duration::ZERO;
        next.timeout = Duration::from_millis(20);
        assert_eq!(
            next.lookup("食べる", &ja(), 10).unwrap_err(),
            ReadError::Deadline
        );
        assert_eq!(server.join().unwrap().len(), 1);
        assert!(!Arc::ptr_eq(
            &gates.get(Provider::Jisho),
            &gates.get(Provider::Wiktionary)
        ));
        assert!(Arc::ptr_eq(
            &service_gate(Provider::Jisho),
            &service_gate(Provider::Jisho)
        ));
    }
    #[test]
    fn exhausted_or_timed_out_reads_preserve_server_cooldown_for_next_client() {
        for (attempts, expected) in [(1, ReadError::Http(429)), (3, ReadError::Deadline)] {
            let (mut first, server) = fixture(vec![response(
                "429 Too Many Requests",
                "Retry-After: 18446744073709551615\r\n",
                "",
            )]);
            let gates = ServiceGates::default();
            first.shared = gates.get(Provider::Jisho);
            first.timeout = Duration::from_millis(20);
            first.attempts = attempts;
            assert_eq!(first.lookup("食べる", &ja(), 10).unwrap_err(), expected);
            let second = JishoClient {
                shared: gates.get(Provider::Jisho),
                ..first.clone()
            };
            drop(first);
            let started = Instant::now();
            assert_eq!(
                second.lookup("食べる", &ja(), 10).unwrap_err(),
                ReadError::Deadline
            );
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn endpoint_queue_wait_uses_lookup_deadline_without_a_request() {
        let (mut client, server) = fixture(vec![]);
        let gates = ServiceGates::default();
        let gate = gates.get(Provider::Jisho);
        client.shared = gate.clone();
        client.timeout = Duration::from_millis(20);
        let _occupied = gate.lock().unwrap();
        let start = Instant::now();
        assert_eq!(
            client.lookup("食べる", &ja(), 10).unwrap_err(),
            ReadError::Deadline
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(server.join().unwrap().is_empty());
        // An occupied Japanese origin must not block the English origin.
        assert!(gates.get(Provider::Wiktionary).try_lock().is_ok());
    }
    #[test]
    fn offline_and_unavailable_features_fail_before_dns() {
        use linguist_config::*;
        let registry = Registry::builtin();
        let mut settings = resolve(
            &registry,
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        settings
            .values
            .insert("network.offline".into(), serde_json::json!(true));
        assert!(matches!(
            JishoClient::from_settings(&settings),
            Err(ReadError::Policy)
        ));
        settings
            .values
            .insert("network.offline".into(), serde_json::json!(false));
        settings.values.insert(
            "services.dictionary.concurrency".into(),
            serde_json::json!(2),
        );
        assert!(matches!(
            JishoClient::from_settings(&settings),
            Err(ReadError::Unavailable)
        ));
    }
}
