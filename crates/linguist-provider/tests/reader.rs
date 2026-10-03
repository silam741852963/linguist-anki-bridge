use linguist_provider::{
    Destinations, Gates, Limits, ReadError, Reader, Service,
    cache::{Cache, Policy},
    retry_delay, wait,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

struct Server {
    url: url::Url,
    handle: JoinHandle<Vec<String>>,
}
impl Server {
    fn start(replies: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/item", listener.local_addr().unwrap())
            .parse()
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
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let _ = stream.write_all(reply.as_bytes());
            }
            requests
        });
        Self { url, handle }
    }
    fn requests(self) -> Vec<String> {
        self.handle.join().unwrap()
    }
}

fn response(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
}
fn json(body: &str) -> String {
    response(
        "200 OK",
        "Content-Type: application/json; charset=utf-8\r\n",
        body,
    )
}
fn limits() -> Limits {
    Limits {
        attempts: 3,
        timeout: Duration::from_secs(2),
        connect_timeout: Duration::from_secs(2),
        interval: Duration::ZERO,
        concurrency: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
        jitter: 0.0,
        max_body: 1024,
        max_redirects: 2,
    }
}
fn gates() -> &'static Gates {
    Box::leak(Box::default())
}
fn local() -> Destinations {
    let mut destinations = Destinations::default();
    destinations.configured.insert("127.0.0.1".into());
    destinations
}
fn reader(cache: Option<Cache>) -> Reader {
    Reader::new(Service::Dictionary, local(), limits(), "test-agent", cache)
        .unwrap()
        .with_gates(gates())
}
fn cache(policy: Policy, ttl_seconds: u64) -> (Cache, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("lab-provider-cache-{}", uuid::Uuid::new_v4()));
    (
        Cache {
            root: root.join("dictionary"),
            policy,
            ttl_seconds,
        },
        root,
    )
}

#[test]
fn transient_reads_retry_but_permanent_faults_do_not() {
    let server = Server::start(vec![
        response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
        json("{}"),
    ]);
    let fetched = reader(None)
        .get(&server.url, &["application/json"])
        .unwrap();
    assert_eq!(fetched.bytes, b"{}");
    assert_eq!(fetched.mime, "application/json");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /api/item "));
    assert!(
        requests[0]
            .to_ascii_lowercase()
            .contains("user-agent: test-agent")
    );
    for (reply, error) in [
        (
            response("401 Unauthorized", "", "private credential detail"),
            ReadError::Http(401),
        ),
        (response("404 Not Found", "", ""), ReadError::Http(404)),
        (
            response("200 OK", "Content-Type: text/html\r\n", "<p>"),
            ReadError::ContentType,
        ),
        (response("200 OK", "", "{}"), ReadError::ContentType),
        (json(&"x".repeat(1025)), ReadError::ResponseLimit),
        (
            response("429 Too Many Requests", "Retry-After: invalid\r\n", ""),
            ReadError::RetryAfter,
        ),
    ] {
        let server = Server::start(vec![reply]);
        assert_eq!(
            reader(None)
                .get(&server.url, &["application/json"])
                .unwrap_err(),
            error
        );
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn exhausted_retries_return_last_transient_error() {
    let server = Server::start(vec![
        response("500 Internal Server Error", "", ""),
        response("502 Bad Gateway", "", ""),
        response("503 Service Unavailable", "", ""),
    ]);
    assert_eq!(
        reader(None)
            .get(&server.url, &["application/json"])
            .unwrap_err(),
        ReadError::Http(503)
    );
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn redirects_are_revalidated_and_bounded() {
    let server = Server::start(vec![
        response("302 Found", "Location: /next\r\n", ""),
        json("{}"),
    ]);
    let fetched = reader(None)
        .get(&server.url, &["application/json"])
        .unwrap();
    assert_eq!(fetched.final_url.path(), "/next");
    assert_eq!(server.requests().len(), 2);
    for location in [
        "http://10.0.0.1/secret",
        "http://evil.invalid/secret",
        "http://user@127.0.0.1/x",
        "file:///etc/passwd",
    ] {
        let server = Server::start(vec![response(
            "302 Found",
            &format!("Location: {location}\r\n"),
            "",
        )]);
        assert_eq!(
            reader(None)
                .get(&server.url, &["application/json"])
                .unwrap_err(),
            ReadError::Redirect,
            "{location}"
        );
        assert_eq!(server.requests().len(), 1);
    }
    let server = Server::start(vec![
        response("302 Found", "Location: /a\r\n", ""),
        response("302 Found", "Location: /b\r\n", ""),
        response("302 Found", "Location: /c\r\n", ""),
    ]);
    assert_eq!(
        reader(None)
            .get(&server.url, &["application/json"])
            .unwrap_err(),
        ReadError::Redirect
    );
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn public_names_resolving_to_private_addresses_are_denied() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let reader = Reader::new(
        Service::Image,
        Destinations::public(&["localhost"]),
        limits(),
        "test-agent",
        None,
    )
    .unwrap()
    .with_gates(gates());
    let url = format!("https://localhost:{port}/x").parse().unwrap();
    assert_eq!(
        reader.get(&url, &["image/png"]).unwrap_err(),
        ReadError::Policy
    );
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "no connection may be attempted");
    // Unlisted hosts and private literals fail before any connection.
    for bad in [
        "https://127.0.0.1/x",
        "https://10.0.0.1/x",
        "http://localhost/x",
    ] {
        assert_eq!(
            reader
                .get(&bad.parse().unwrap(), &["image/png"])
                .unwrap_err(),
            ReadError::Policy
        );
    }
}

#[test]
fn rate_and_retry_after_cannot_exceed_total_deadline() {
    let server = Server::start(vec![response(
        "429 Too Many Requests",
        "Retry-After: 10\r\n",
        "",
    )]);
    let mut client = reader(None);
    client.limits_mut().timeout = Duration::from_millis(100);
    let start = Instant::now();
    assert_eq!(
        client.get(&server.url, &["application/json"]).unwrap_err(),
        ReadError::Deadline
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    server.requests();
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
    let server = Server::start(vec![
        response(
            "503 Service Unavailable",
            "Retry-After: Sun, 06 Nov 1994 08:49:37 GMT\r\n",
            "",
        ),
        json("{}"),
    ]);
    reader(None)
        .get(&server.url, &["application/json"])
        .unwrap();
    assert_eq!(server.requests().len(), 2);
    let future = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
    for (status, location) in [
        ("429 Too Many Requests", ""),
        ("302 Found", "Location: /again\r\n"),
    ] {
        let server = Server::start(vec![response(
            status,
            &format!("{location}Retry-After: {future}\r\n"),
            "",
        )]);
        let mut client = reader(None);
        client.limits_mut().timeout = Duration::from_millis(100);
        let start = Instant::now();
        assert_eq!(
            client.get(&server.url, &["application/json"]).unwrap_err(),
            ReadError::Deadline
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn independently_constructed_readers_share_origin_rate_history() {
    let shared = gates();
    let server = Server::start(vec![json("{}")]);
    let mut first = reader(None).with_gates(shared);
    first.limits_mut().interval = Duration::from_secs(60);
    first.get(&server.url, &["application/json"]).unwrap();
    drop(first);
    // A new reader with faster settings still observes the previous interval.
    let mut next = reader(None).with_gates(shared);
    next.limits_mut().timeout = Duration::from_millis(20);
    assert_eq!(
        next.get(&server.url, &["application/json"]).unwrap_err(),
        ReadError::Deadline
    );
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn exhausted_or_timed_out_reads_preserve_server_cooldown() {
    for (attempts, expected) in [(1, ReadError::Http(429)), (3, ReadError::Deadline)] {
        let shared = gates();
        let server = Server::start(vec![response(
            "429 Too Many Requests",
            "Retry-After: 18446744073709551615\r\n",
            "",
        )]);
        let mut first = reader(None).with_gates(shared);
        first.limits_mut().timeout = Duration::from_millis(20);
        first.limits_mut().attempts = attempts;
        assert_eq!(
            first.get(&server.url, &["application/json"]).unwrap_err(),
            expected
        );
        let mut second = reader(None).with_gates(shared);
        second.limits_mut().timeout = Duration::from_millis(20);
        let started = Instant::now();
        assert_eq!(
            second.get(&server.url, &["application/json"]).unwrap_err(),
            ReadError::Deadline
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn service_semaphore_bounds_inflight_reads_within_deadline() {
    let shared = gates();
    // Hold the only permit with a slow request, then a second read times out queued.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url: url::Url = format!("http://{}/slow", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let slow = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = stream.write_all(json("{}").as_bytes());
    });
    let first = reader(None).with_gates(shared);
    let first_url = url.clone();
    let holder = std::thread::spawn(move || first.get(&first_url, &["application/json"]));
    std::thread::sleep(Duration::from_millis(100));
    let other = Server::start(vec![]);
    let mut queued = reader(None).with_gates(shared);
    queued.limits_mut().timeout = Duration::from_millis(50);
    assert_eq!(
        queued.get(&other.url, &["application/json"]).unwrap_err(),
        ReadError::Deadline
    );
    assert!(other.requests().is_empty());
    // With concurrency 2 the second read proceeds immediately.
    let other = Server::start(vec![json("{}")]);
    let mut parallel = reader(None).with_gates(shared);
    parallel.limits_mut().concurrency = 2;
    parallel.get(&other.url, &["application/json"]).unwrap();
    assert!(holder.join().unwrap().is_ok());
    slow.join().unwrap();
}

#[test]
fn cache_policies_serve_only_intact_unexpired_entries() {
    let (fresh, root) = cache(Policy::PreferFresh, 3600);
    let server = Server::start(vec![
        json("{\"v\":1}"),
        response("503 Service Unavailable", "", ""),
    ]);
    let mut client = reader(Some(fresh.clone()));
    client.limits_mut().attempts = 1;
    let first = client.get(&server.url, &["application/json"]).unwrap();
    assert!(!first.from_cache);
    // Transient failure falls back to the unexpired entry with original provenance.
    let fallback = client.get(&server.url, &["application/json"]).unwrap();
    assert!(fallback.from_cache);
    assert_eq!(
        (fallback.bytes, fallback.fetched_at),
        (first.bytes.clone(), first.fetched_at)
    );
    assert_eq!(server.requests().len(), 2);

    // Permanent faults never fall back.
    let server = Server::start(vec![response("404 Not Found", "", "")]);
    let other = Reader::new(
        Service::Dictionary,
        local(),
        limits(),
        "test-agent",
        Some(fresh.clone()),
    )
    .unwrap()
    .with_gates(gates());
    assert_eq!(
        other.get(&server.url, &["application/json"]).unwrap_err(),
        ReadError::Http(404)
    );
    server.requests();

    let entry_url = first.url.clone();
    let prefer = Cache {
        policy: Policy::PreferCache,
        ..fresh.clone()
    };
    let hit = reader(Some(prefer))
        .get(&entry_url, &["application/json"])
        .unwrap();
    assert!(hit.from_cache);
    let only = Cache {
        policy: Policy::CacheOnly,
        ..fresh.clone()
    };
    assert!(
        reader(Some(only.clone()))
            .get(&entry_url, &["application/json"])
            .unwrap()
            .from_cache
    );
    // A different accepted type is a different request fingerprint.
    assert_eq!(
        reader(Some(only.clone()))
            .get(&entry_url, &["text/plain"])
            .unwrap_err(),
        ReadError::CacheMiss
    );
    let expired = Cache {
        ttl_seconds: 0,
        ..only.clone()
    };
    assert_eq!(
        reader(Some(expired))
            .get(&entry_url, &["application/json"])
            .unwrap_err(),
        ReadError::CacheMiss
    );
    // Corrupt bodies are misses, never served.
    for entry in std::fs::read_dir(&only.root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "body") {
            std::fs::write(&path, b"tampered").unwrap();
        }
    }
    assert_eq!(
        reader(Some(only))
            .get(&entry_url, &["application/json"])
            .unwrap_err(),
        ReadError::CacheMiss
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&fresh.root).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn settings() -> linguist_config::Effective {
    linguist_config::resolve(
        &linguist_config::Registry::builtin(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn settings_select_offline_cache_proxy_and_endpoint_policy() {
    use serde_json::json;
    let root = std::env::temp_dir().join(format!("lab-provider-settings-{}", uuid::Uuid::new_v4()));
    let mut config = settings();
    config
        .values
        .insert("storage.cache_dir".into(), json!(root.to_str().unwrap()));
    let env = Default::default();
    let public = "https://jisho.org/api".parse::<url::Url>().unwrap();

    config.values.insert("network.offline".into(), json!(true));
    let offline =
        Reader::from_settings(&config, &env, Service::Dictionary, &["jisho.org"], &[]).unwrap();
    assert_eq!(
        offline.get(&public, &["application/json"]).unwrap_err(),
        ReadError::Offline
    );
    // Offline still permits an explicitly configured loopback service.
    let server = Server::start(vec![json("{}")]);
    let loopback = Reader::from_settings(&config, &env, Service::Tts, &[], &[&server.url]).unwrap();
    loopback.get(&server.url, &["application/json"]).unwrap();
    server.requests();
    config.values.insert("network.offline".into(), json!(false));

    // The configured user agent identifies every request.
    config
        .values
        .insert("dictionary.user_agent".into(), json!("Custom-Agent/7"));
    let server = Server::start(vec![json("{}")]);
    Reader::from_settings(&config, &env, Service::Kanji, &[], &[&server.url])
        .unwrap()
        .get(&server.url, &["application/json"])
        .unwrap();
    assert!(
        server.requests()[0]
            .to_ascii_lowercase()
            .contains("user-agent: custom-agent/7")
    );

    config
        .values
        .insert("cache.policy".into(), json!("cache_only"));
    let only =
        Reader::from_settings(&config, &env, Service::Dictionary, &["jisho.org"], &[]).unwrap();
    assert_eq!(
        only.get(&public, &["application/json"]).unwrap_err(),
        ReadError::CacheMiss
    );
    config
        .values
        .insert("cache.policy".into(), json!("prefer_fresh"));

    // A remote custom endpoint requires an explicit allowed host.
    let remote = "https://tts.example.net/speak".parse::<url::Url>().unwrap();
    assert_eq!(
        Reader::from_settings(&config, &env, Service::Tts, &[], &[&remote]).err(),
        Some(ReadError::Policy)
    );
    config.values.insert(
        "network.allowed_remote_service_hosts".into(),
        json!(["tts.example.net"]),
    );
    assert!(Reader::from_settings(&config, &env, Service::Tts, &[], &[&remote]).is_ok());

    config
        .values
        .insert("network.proxy_env".into(), json!("HTTPS_PROXY"));
    assert_eq!(
        Reader::from_settings(&config, &env, Service::Dictionary, &["jisho.org"], &[]).err(),
        Some(ReadError::Unavailable)
    );
    config
        .values
        .insert("network.proxy_env".into(), json!(null));

    // Nondefault pacing values are accepted per service.
    config
        .values
        .insert("services.image.concurrency".into(), json!(4));
    config
        .values
        .insert("services.image.min_interval_seconds".into(), json!(0.0));
    config
        .values
        .insert("network.max_redirects".into(), json!(0));
    config
        .values
        .insert("network.max_response_mb".into(), json!(1));
    config.values.insert("retry.read_attempts".into(), json!(1));
    let server = Server::start(vec![response("302 Found", "Location: /next\r\n", "")]);
    let strict = Reader::from_settings(&config, &env, Service::Image, &[], &[&server.url]).unwrap();
    assert_eq!(
        strict.get(&server.url, &["image/png"]).unwrap_err(),
        ReadError::Redirect
    );
    server.requests();
    let _ = std::fs::remove_dir_all(root);
}
