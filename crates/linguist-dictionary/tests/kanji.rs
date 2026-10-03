use linguist_dictionary::kanji::{KanjiClient, characters, parse_page, render_reference};
use linguist_provider::{Destinations, Limits, ReadError, Reader, Service};
use serde_json::json;
use std::{
    io::{Read, Write},
    time::Duration,
};

const PAGE: &[u8] = include_bytes!("fixtures/jisho-kanji-food.html");

#[test]
fn detail_page_keeps_rich_facts_and_drops_script_text() {
    let facts = parse_page('食', PAGE).unwrap().unwrap();
    assert_eq!(facts.meanings, ["eat", "food"]);
    assert_eq!(facts.kun_readings, ["く.う", "た.べる"]);
    assert_eq!(facts.on_readings, ["ショク", "ジキ"]);
    assert_eq!(facts.strokes, Some(9));
    assert_eq!(facts.radical.as_deref(), Some("食 (飠) — eat, food"));
    assert_eq!(facts.parts, ["食"]);
    assert_eq!(facts.grade.as_deref(), Some("grade 2"));
    assert_eq!(facts.jlpt.as_deref(), Some("N5"));
    assert_eq!(facts.frequency, Some(328));
    assert!(!format!("{facts:?}").contains("Ignore previous"));
}

#[test]
fn missing_details_are_not_found_and_mismatches_fail() {
    assert_eq!(
        parse_page('食', b"<html><body>No matches</body></html>"),
        Ok(None)
    );
    assert!(parse_page('飲', PAGE).is_err());
    assert!(parse_page('食', b"\xff\xfe").is_err());
    let empty = b"<div class='kanji details'><h1 class='character'>\xe9\xa3\x9f</h1></div>";
    assert!(parse_page('食', empty).is_err());
}

#[test]
fn applicable_characters_are_unique_and_ordered() {
    assert_eq!(characters("食べ物を食べる"), ['食', '物']);
    assert!(characters("たべる cafe").is_empty());
}

fn serve(
    bodies: Vec<(&'static str, &'static [u8])>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in bodies {
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
            let head = format!(
                "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
        requests
    });
    (base, handle)
}

fn client(base: &str) -> KanjiClient {
    let mut destinations = Destinations::default();
    destinations.configured.insert("127.0.0.1".into());
    let limits = Limits {
        attempts: 1,
        timeout: Duration::from_secs(2),
        connect_timeout: Duration::from_secs(2),
        interval: Duration::ZERO,
        concurrency: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        jitter: 0.0,
        max_body: 1024 * 1024,
        max_redirects: 0,
    };
    let reader = Reader::new(Service::Kanji, destinations, limits, "test", None)
        .unwrap()
        .with_gates(Box::leak(Box::default()));
    KanjiClient::with_reader(reader, &format!("{base}/search/{{char}}%23kanji"))
}

#[test]
fn expression_lookup_encodes_characters_and_preserves_provenance() {
    let (base, server) = serve(vec![("200 OK", PAGE), ("200 OK", b"<html>none</html>")]);
    let results = client(&base).lookup_expression("食べ物").unwrap();
    let entry = results[0].as_ref().unwrap();
    assert_eq!(entry.raw_bytes, PAGE);
    assert_eq!(entry.raw_digest.len(), 64);
    assert!(entry.source_url.ends_with("/search/%E9%A3%9F%23kanji"));
    assert!(results[1].is_none());
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /search/%E9%A3%9F%23kanji "));
    assert!(requests[1].starts_with("GET /search/%E7%89%A9%23kanji "));
    let text = render_reference(std::slice::from_ref(entry));
    assert!(text.starts_with("食 — eat, food\nOn: ショク、ジキ\nKun: く.う、た.べる"));
    let serialized = serde_json::to_value(entry).unwrap();
    assert!(serialized.get("raw_bytes").is_none());
}

#[test]
fn server_errors_and_wrong_types_fail_the_item() {
    let (base, server) = serve(vec![("500 Internal Server Error", b"")]);
    assert_eq!(
        client(&base).lookup('食').unwrap_err(),
        ReadError::Http(500)
    );
    server.join().unwrap();
    assert_eq!(
        client("http://127.0.0.1:9").lookup('a').unwrap_err(),
        ReadError::Policy
    );
}

#[test]
fn nondefault_kanji_settings_are_explicitly_unavailable() {
    let mut settings = linguist_config::resolve(
        &linguist_config::Registry::builtin(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    settings.values.insert(
        "storage.cache_dir".into(),
        json!(std::env::temp_dir().join("lab-kanji-cache-unused")),
    );
    let env = Default::default();
    assert!(KanjiClient::from_settings(&settings, &env).is_ok());
    for (key, value) in [
        ("kanji.enabled", json!(false)),
        ("kanji.schema", json!("/opt/custom-kanji.json")),
        ("kanji.explanation_language", json!("vi")),
        (
            "kanji.url_template",
            json!("https://kanjiapi.dev/v1/kanji/{char}"),
        ),
        (
            "kanji.url_template",
            json!("http://jisho.org/search/{char}%23kanji"),
        ),
    ] {
        let mut changed = settings.clone();
        changed.values.insert(key.into(), value);
        assert_eq!(
            KanjiClient::from_settings(&changed, &env).err(),
            Some(ReadError::Unavailable),
            "{key}"
        );
    }
}
