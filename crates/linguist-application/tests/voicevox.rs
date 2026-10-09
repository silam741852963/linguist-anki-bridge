//! WP-20: the VOICEVOX adapter speaks the engine's documented HTTP API.
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
};

fn wav() -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    bytes.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
    bytes.extend_from_slice(&24000u32.to_le_bytes());
    bytes.extend_from_slice(&48000u32.to_le_bytes());
    bytes.extend_from_slice(&[2, 0, 16, 0]);
    bytes.extend_from_slice(b"data\x04\0\0\0\0\0\0\0");
    bytes
}

#[test]
fn voicevox_builds_a_query_synthesizes_and_credits_the_character() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..4 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let path = line.split(' ').nth(1).unwrap().to_owned();
            let (kind, reply): (&str, Vec<u8>) = if path == "/version" {
                ("application/json", br#""0.24.1""#.to_vec())
            } else if path == "/speakers" {
                (
                    "application/json",
                    json!([{"name":"春日部つむぎ","styles":[{"id":8,"name":"ノーマル"}]}])
                        .to_string()
                        .into_bytes(),
                )
            } else if path.starts_with("/audio_query") {
                (
                    "application/json",
                    json!({"accent_phrases":[],"speedScale":1.0})
                        .to_string()
                        .into_bytes(),
                )
            } else {
                let query: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(query["speedScale"], 0.9);
                ("audio/wav", wav())
            };
            seen.push(path);
            let mut stream = stream;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.len()).unwrap();
            stream.write_all(&reply).unwrap();
        }
        seen
    });
    let mut settings = linguist_config::resolve(
        &linguist_config::Registry::builtin(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    settings
        .values
        .insert("audio.voicevox.endpoint".into(), json!(endpoint));
    settings.values.insert("audio.speed".into(), json!(0.9));
    let synthesis =
        linguist_application::voicevox::synthesize("おりめをつける", &settings).unwrap();
    let seen = server.join().unwrap();
    assert_eq!(seen[0], "/version");
    assert!(
        seen[2].starts_with("/audio_query?text=%E3%81%8A") && seen[2].ends_with("speaker=8"),
        "{seen:?}"
    );
    assert_eq!(seen[3], "/synthesis?speaker=8");
    assert_eq!(synthesis.provider, "voicevox");
    assert_eq!(synthesis.engine_version, "0.24.1");
    assert_eq!(
        synthesis.voice_dataset.as_deref(),
        Some("VOICEVOX:春日部つむぎ")
    );
    assert_eq!(
        (synthesis.mime.as_str(), synthesis.sample_rate),
        ("audio/wav", 24000)
    );
    assert_eq!(synthesis.bytes, wav());
    // Nothing listening: a clear error, no candidate.
    settings.values.insert(
        "audio.voicevox.endpoint".into(),
        json!("http://127.0.0.1:9"),
    );
    assert!(
        linguist_application::voicevox::synthesize("あ", &settings)
            .unwrap_err()
            .starts_with("VOICEVOX_UNAVAILABLE")
    );
}
