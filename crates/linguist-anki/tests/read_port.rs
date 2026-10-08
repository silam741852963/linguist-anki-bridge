use linguist_anki::*;
use linguist_config::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};
struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let captured = requests.clone();
        let thread = std::thread::spawn(move || {
            for (status, body) in responses {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "expected request did not arrive"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("content-length:") {
                        length = line.split(':').nth(1).unwrap().trim().parse().unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                captured
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&bytes).unwrap());
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:9/\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream
                    .write_all(header.as_bytes())
                    .and_then(|_| stream.write_all(body.as_bytes()));
            }
        });
        Self {
            endpoint,
            requests,
            thread: Some(thread),
        }
    }
    fn finish(mut self) -> Vec<Value> {
        self.thread.take().unwrap().join().unwrap();
        self.requests.lock().unwrap().clone()
    }
}
fn response(value: Value) -> (u16, String) {
    (200, json!({"result":value,"error":null}).to_string())
}
fn settings(endpoint: &str) -> Effective {
    let r = Registry::builtin();
    resolve(
        &r,
        &ConfigFile::default(),
        &ResolveOptions {
            flags: BTreeMap::from([("anki.endpoint".into(), json!(endpoint))]),
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn deck_read_sends_only_profile_and_read_action_and_stringifies_ids() {
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(json!({"日本語":123})),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let decks = client.decks().unwrap();
    assert_eq!(decks[0].id, "123");
    let requests = server.finish();
    assert_eq!(requests[0]["action"], "getActiveProfile");
    assert_eq!(requests[1]["action"], "deckNamesAndIds");
    assert!(requests.iter().all(|r| r["version"] == 6));
}
#[test]
fn deck_kind_requires_pinned_valid_config_evidence() {
    for (config, expected) in [
        (json!({"dyn":false}), Some(false)),
        (json!({"dyn":1}), Some(true)),
        (json!({"dyn":"filtered"}), None),
    ] {
        let server = Server::new(vec![
            response(json!("Fixture")),
            response(config),
            response(json!("Fixture")),
        ]);
        let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
        assert_eq!(client.deck_is_filtered("語彙 \"A\"").ok(), expected);
        let requests = server.finish();
        assert_eq!(requests[1]["action"], "getDeckConfig");
        assert_eq!(requests[1]["params"]["deck"], "語彙 \"A\"");
    }
}
#[test]
fn response_errors_cannot_echo_credentials() {
    let server = Server::new(vec![(
        200,
        json!({"result":null,"error":"bad key private-secret"}).to_string(),
    )]);
    let mut settings = settings(&server.endpoint);
    settings
        .values
        .insert("anki.api_key_env".into(), json!("TEST_KEY"));
    let client = Client::from_settings(
        &settings,
        &BTreeMap::from([("TEST_KEY".into(), "private-secret".into())]),
    )
    .unwrap();
    let error = client.check_profile().unwrap_err();
    assert!(!error.contains("private-secret"));
    assert_eq!(server.finish()[0]["key"], "private-secret");
}
#[test]
fn redirects_are_not_followed() {
    let server = Server::new(vec![(302, "redirect".into())]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert!(
        client
            .check_profile()
            .unwrap_err()
            .contains("ANKI_HTTP_FAILURE")
    );
    assert_eq!(server.finish().len(), 1);
}
#[test]
fn profile_drift_is_rejected_even_without_an_expected_profile() {
    let server = Server::new(vec![response(json!("First")), response(json!("Second"))]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    client.check_profile().unwrap();
    assert!(client.check_profile().unwrap_err().contains("CONFLICT"));
    server.finish();
}
#[test]
fn info_calls_obey_batch_size_and_validate_response_identity() {
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(json!([{"noteId":1,"cards":[]} ])),
        response(json!([{"noteId":2,"cards":[]} ])),
        response(json!("Fixture")),
    ]);
    let mut settings = settings(&server.endpoint);
    settings
        .values
        .insert("anki.read_batch_size".into(), json!(1));
    let client = Client::from_settings(&settings, &BTreeMap::new()).unwrap();
    assert_eq!(
        client.notes_info(&["1".into(), "2".into()]).unwrap().len(),
        2
    );
    let requests = server.finish();
    assert_eq!(requests[1]["params"]["notes"], json!([1]));
    assert_eq!(requests[2]["params"]["notes"], json!([2]));
}
#[test]
fn malformed_duplicate_keys_and_oversized_ids_are_rejected() {
    for body in [
        r#"{"result":"a","result":"b","error":null}"#,
        r#"{"result":9007199254740992,"error":null}"#,
    ] {
        let server = Server::new(vec![(200, body.into())]);
        let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
        assert!(
            client
                .check_profile()
                .unwrap_err()
                .contains("PROTOCOL_INVALID")
        );
        server.finish();
    }
}
#[test]
fn unapproved_remote_and_offline_private_endpoints_are_blocked_before_dispatch() {
    let mut s = settings("http://127.0.0.1:8765");
    s.values
        .insert("anki.endpoint".into(), json!("http://10.0.0.1:8765"));
    assert!(Client::from_settings(&s, &BTreeMap::new()).is_err());
    s.values.insert(
        "network.allowed_remote_service_hosts".into(),
        json!(["10.0.0.1"]),
    );
    assert!(
        Client::from_settings(&s, &BTreeMap::new())
            .unwrap_err_string()
            .contains("ADDRESS_POLICY")
    );
}
#[test]
fn id_conversion_and_selector_escaping_are_lossless() {
    assert_eq!(
        normalize_note_ids(
            json!({"noteId":123,"cards":[456],"fields":{"Meaning":{"value":"private","order":0}}})
        )
        .unwrap()["cards"],
        json!(["456"])
    );
    assert!(wire_id(&json!("0123")).is_err());
    assert!(wire_id(&json!("9007199254740992")).is_err());
    assert!(deck_query("x\ny").is_err());
    assert_eq!(
        deck_query("Japanese \"notes\"").unwrap(),
        "deck:\"Japanese \\\"notes\\\"\""
    );
}
trait ErrorString {
    fn unwrap_err_string(self) -> String;
}
impl<T> ErrorString for std::result::Result<T, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        }
    }
}
#[test]
fn capability_advertisement_never_enables_native_writes() {
    let actions = json!([
        "findNotes",
        "findCards",
        "notesInfo",
        "cardsInfo",
        "deckNamesAndIds",
        "modelFieldNames",
        "modelNamesAndIds",
        "modelTemplates",
        "modelStyling",
        "labCapabilities"
    ]);
    let server = Server::new(vec![
        response(json!(6)),
        response(json!("Fixture")),
        response(json!({"actions":actions,"scopes":["actions"]})),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let report = client.capabilities().unwrap();
    assert!(report.read_ready && report.native_advertised);
    assert!(!report.native_verified && !report.collection_writes_enabled);
    assert_eq!(report.identity_confidence, "weak");
    server.finish();
}
#[test]
fn oversized_responses_and_missing_envelope_keys_are_rejected() {
    let server = Server::new(vec![(200, " ".repeat(2 * 1024 * 1024))]);
    let mut settings = settings(&server.endpoint);
    settings
        .values
        .insert("network.max_response_mb".into(), json!(1));
    let client = Client::from_settings(&settings, &BTreeMap::new()).unwrap();
    assert!(
        client
            .check_profile()
            .unwrap_err()
            .contains("RESPONSE_TOO_LARGE")
    );
    server.finish();
    let server = Server::new(vec![(200, r#"{"result":"Fixture"}"#.into())]);
    settings
        .values
        .insert("anki.endpoint".into(), json!(server.endpoint));
    let client = Client::from_settings(&settings, &BTreeMap::new()).unwrap();
    assert!(
        client
            .check_profile()
            .unwrap_err()
            .contains("PROTOCOL_INVALID")
    );
    server.finish();
}
#[test]
fn changed_info_identity_is_rejected() {
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(json!([{"noteId":2,"cards":[]}])),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert!(
        client
            .notes_info(&["1".into()])
            .unwrap_err()
            .contains("CONFLICT")
    );
    server.finish();
}

#[test]
fn media_read_preserves_bytes_hash_name_and_false_means_missing() {
    use base64::Engine;
    let original = b"original binary\0\xff";
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(json!(
            base64::engine::general_purpose::STANDARD.encode(original)
        )),
        response(json!("Fixture")),
        response(json!("Fixture")),
        response(json!(false)),
        response(json!("Fixture")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let media = client.retrieve_media_file("猫 voice.mp3").unwrap().unwrap();
    assert_eq!(media.filename, "猫 voice.mp3");
    assert_eq!(media.bytes, original);
    assert_eq!(
        media.digest,
        linguist_core::canonical::asset_digest(original)
    );
    assert!(client.retrieve_media_file("missing.mp3").unwrap().is_none());
    let requests = server.finish();
    assert_eq!(requests[1]["action"], "retrieveMediaFile");
    assert_eq!(requests[1]["params"]["filename"], "猫 voice.mp3");
    assert!(requests.iter().all(|r| matches!(
        r["action"].as_str(),
        Some("getActiveProfile" | "retrieveMediaFile")
    )));
}

#[test]
fn media_read_rejects_unsafe_names_before_any_request() {
    let server = Server::new(vec![]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    for name in [
        "../x",
        "https://example.invalid/x",
        "e\u{301}.png",
        "x*.png",
        "x|y.mp3",
        "x.png ",
        "",
        " ",
    ] {
        assert_eq!(
            client.retrieve_media_file(name).unwrap_err(),
            "ANKI_MEDIA_FILENAME_UNSAFE"
        );
    }
    assert!(server.finish().is_empty());
}

#[test]
fn media_read_rejects_malformed_noncanonical_or_oversized_payloads() {
    use base64::Engine;
    for (value, expected) in [
        (json!(null), "ANKI_MEDIA_PROTOCOL_INVALID"),
        (json!(true), "ANKI_MEDIA_PROTOCOL_INVALID"),
        (json!("YQ"), "ANKI_MEDIA_ENCODING_INVALID"),
        (json!("YR=="), "ANKI_MEDIA_ENCODING_INVALID"),
        (json!("!!!!"), "ANKI_MEDIA_ENCODING_INVALID"),
        (
            json!(base64::engine::general_purpose::STANDARD.encode(vec![0u8; 1024 * 1024 + 1])),
            "ANKI_MEDIA_TOO_LARGE",
        ),
    ] {
        let server = Server::new(vec![
            response(json!("Fixture")),
            response(value),
            response(json!("Fixture")),
        ]);
        let mut effective = settings(&server.endpoint);
        effective
            .values
            .insert("media.max_asset_mb".into(), json!(1));
        let client = Client::from_settings(&effective, &BTreeMap::new()).unwrap();
        assert_eq!(client.retrieve_media_file("x.png").unwrap_err(), expected);
        assert_eq!(server.finish().len(), 3);
    }
}

#[test]
fn media_read_rejects_profile_change_before_accepting_any_bytes() {
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(json!("YQ==")),
        response(json!("Changed")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert_eq!(
        client.retrieve_media_file("x.png").unwrap_err(),
        "ANKI_PROFILE_CONFLICT"
    );
    assert_eq!(server.finish().len(), 3);
}

#[test]
fn model_inspection_exposes_differences_and_checks_profile_after_read() {
    let target = linguist_core::model::vocabulary();
    let template_values: BTreeMap<_, _> = target
        .templates
        .iter()
        .map(|t| (t.name.clone(), json!({"Front":t.front,"Back":t.back})))
        .collect();
    for changed in [false, true] {
        let server = Server::new(vec![
            response(json!("Fixture")),
            response(json!({"Linguist Vocabulary v3":123})),
            response(json!(target.fields)),
            response(json!(template_values)),
            response(json!({"css":target.css})),
            response(json!([{"id":123,"name":target.name,"css":target.css,
                "flds":target.fields.iter().enumerate().map(|(ord, name)| json!({"name":name,"ord":ord})).collect::<Vec<_>>(),
                "tmpls":target.templates.iter().map(|t| json!({"name":t.name,"ord":t.ordinal,"qfmt":t.front,"afmt":t.back})).collect::<Vec<_>>()}])),
            response(json!(if changed { "Changed" } else { "Fixture" })),
        ]);
        let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
        let result = client.inspect_model("123");
        if changed {
            assert_eq!(result.unwrap_err(), "ANKI_PROFILE_CONFLICT");
        } else {
            let report = result.unwrap();
            assert!(report.content_matches_managed);
            assert!(!report.managed_verified);
            assert!(report.template_order_verified);
            assert_eq!(report.manifest_digest, target.manifest_digest().unwrap());
            assert_eq!(report.compatibility.len(), 3);
            assert!(report.compatibility[0].exact_content_match);
            // The English model lacks only Kanji; grammar differs entirely.
            assert_eq!(report.compatibility[1].unexpected_fields, ["Kanji"]);
            assert!(!report.compatibility[2].missing_fields.is_empty());
        }
        let requests = server.finish();
        assert_eq!(requests.len(), 7);
        assert!(requests.iter().all(|r| matches!(
            r["action"].as_str(),
            Some(
                "getActiveProfile"
                    | "modelNamesAndIds"
                    | "modelFieldNames"
                    | "modelTemplates"
                    | "modelStyling"
                    | "findModelsByName"
            )
        )));
    }
}

fn capture_cycle(note: Value, card: Value, css: &str) -> Vec<(u16, String)> {
    let profile = json!("Fixture");
    [
        profile.clone(),
        json!([note]),
        profile.clone(),
        profile.clone(),
        json!({"Legacy":12}),
        profile.clone(),
        json!({"Legacy":12}),
        json!(["Expression"]),
        json!({"Card":{"Front":"front","Back":"back"}}),
        json!({"css":css}),
        json!([{"id":12,"name":"Legacy","css":css,"flds":[{"name":"Expression","ord":0}],
            "tmpls":[{"name":"Card","ord":0,"qfmt":"front","afmt":"back"}]}]),
        profile.clone(),
        profile.clone(),
        json!([card]),
        profile,
    ]
    .into_iter()
    .map(response)
    .collect()
}
fn capture_note_fixture() -> Value {
    json!({"noteId":123,"modelName":"Legacy","fields":{"Expression":{"value":"<b>猫</b>","order":0}},"cards":[456],"tags":["original"],"extension":"preserved"})
}
fn capture_card_fixture() -> Value {
    json!({"cardId":456,"note":123,"due":10,"reps":3,"deckId":789,"originalDeckId":0,"extension":"preserved"})
}
#[test]
fn repeated_capture_retains_full_payloads_and_never_claims_native_atomic_history() {
    let mut replies = capture_cycle(capture_note_fixture(), capture_card_fixture(), "css");
    replies.extend(capture_cycle(
        capture_note_fixture(),
        capture_card_fixture(),
        "css",
    ));
    let server = Server::new(replies);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let captured = client.capture_note("123").unwrap();
    assert!(captured.repeated_reads_matched);
    assert!(!captured.atomic_snapshot_verified && !captured.native_history_verified);
    assert_eq!(captured.note["noteId"], "123");
    assert_eq!(captured.note["extension"], "preserved");
    assert_eq!(captured.cards[0]["due"], 10);
    assert_eq!(captured.cards[0]["originalDeckId"], "0");
    assert_eq!(captured.model.css, "css");
    let actions = server.finish();
    assert_eq!(actions.len(), 30);
    assert!(actions.iter().all(|r| {
        [
            "getActiveProfile",
            "notesInfo",
            "modelNamesAndIds",
            "modelFieldNames",
            "modelTemplates",
            "modelStyling",
            "findModelsByName",
            "cardsInfo",
        ]
        .contains(&r["action"].as_str().unwrap())
    }));
}
#[test]
fn field_scheduler_and_model_changes_invalidate_repeated_capture() {
    for changed in ["field", "scheduler", "model"] {
        let mut note = capture_note_fixture();
        let mut card = capture_card_fixture();
        if changed == "field" {
            note["fields"]["Expression"]["value"] = json!("changed");
        }
        if changed == "scheduler" {
            card["reps"] = json!(4);
        }
        let css = if changed == "model" { "changed" } else { "css" };
        let mut replies = capture_cycle(capture_note_fixture(), capture_card_fixture(), "css");
        replies.extend(capture_cycle(note, card, css));
        let server = Server::new(replies);
        let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
        assert_eq!(
            client.capture_note("123").unwrap_err(),
            "ANKI_CAPTURE_SOURCE_CONFLICT"
        );
        assert_eq!(server.finish().len(), 30);
    }
}

fn native_manifest() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../contracts/v2/fixtures/native-capabilities.json"
    ))
    .unwrap()
}
#[test]
fn native_declarations_are_profile_pinned_read_evidence_and_never_enable_writes() {
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(native_manifest()),
        response(json!("Fixture")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let inspected = client.native_capabilities().unwrap();
    assert!(
        !inspected.compatibility_verified
            && !inspected.collection_identity_verified
            && !inspected.collection_writes_enabled
    );
    assert!(inspected.declaration.mutation_variants.is_empty());
    let requests = server.finish();
    assert_eq!(requests[1]["action"], "labCapabilities");
    assert_eq!(requests[1]["params"], json!({}));
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(native_manifest()),
        response(json!("Other")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert_eq!(
        client.native_capabilities().unwrap_err(),
        "ANKI_PROFILE_CONFLICT"
    );
    server.finish();
}
#[test]
fn declared_read_only_session_remains_unverified() {
    use linguist_anki::native::inspect_native_manifest;
    let mut manifest = native_manifest();
    manifest["collection_session"] = json!({
        "lineage_id": "c17625b0-7a88-4aab-a8a5-c1d993c72a01",
        "session_epoch": "c17625b0-7a88-4aab-a8a5-c1d993c72a02",
        "profile_fingerprint": "b".repeat(64),
        "path_fingerprint": "c".repeat(64),
    });
    let inspected = inspect_native_manifest(manifest).unwrap();
    assert!(inspected.declaration.collection_session.is_some());
    assert!(!inspected.compatibility_verified);
    assert!(!inspected.collection_identity_verified);
    assert!(!inspected.collection_writes_enabled);
}
#[test]
fn native_session_profile_claim_must_match_profile_reads() {
    let mut manifest = native_manifest();
    manifest["collection_session"] = json!({
        "lineage_id": "c17625b0-7a88-4aab-a8a5-c1d993c72a01",
        "session_epoch": "c17625b0-7a88-4aab-a8a5-c1d993c72a02",
        "profile_fingerprint": linguist_core::canonical::asset_digest(b"lab-profile-v1\0Fixture"),
        "path_fingerprint": "c".repeat(64),
    });
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(manifest.clone()),
        response(json!("Fixture")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let inspected = client.native_capabilities().unwrap();
    assert!(inspected.declaration.collection_session.is_some());
    assert!(!inspected.collection_writes_enabled);
    server.finish();

    manifest["collection_session"]["profile_fingerprint"] = json!("d".repeat(64));
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(manifest),
        response(json!("Fixture")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert_eq!(
        client.native_capabilities().unwrap_err(),
        "ANKI_NATIVE_PROFILE_FINGERPRINT_CONFLICT"
    );
    server.finish();
}
#[test]
fn malformed_native_declarations_and_unimplemented_effects_are_rejected() {
    use linguist_anki::native::inspect_native_manifest;
    for (pointer, value) in [
        ("/protocol", json!("unknown")),
        ("/bridge_id", json!("00000000-0000-0000-0000-000000000000")),
        ("/actions", json!(["labCapabilities", "labCapabilities"])),
        ("/actions", json!(["labCapabilities", "arbitrarySql"])),
        ("/mutation_variants", json!(["sync"])),
        ("/mutation_variants", json!(["create_note"])),
        ("/integration/anki_connect_source_digest", json!("bad")),
    ] {
        let mut manifest = native_manifest();
        *manifest.pointer_mut(pointer).unwrap() = value;
        assert!(inspect_native_manifest(manifest).is_err());
    }
    let mut manifest = native_manifest();
    manifest["claims_verified"] = json!(true);
    assert!(inspect_native_manifest(manifest).is_err());
    let mut manifest = native_manifest();
    manifest["actions"] = json!([
        "labCapabilities",
        "labBegin",
        "labInspect",
        "labMutate",
        "labOperationStatus",
        "labRebind",
        "labEnd"
    ]);
    manifest["mutation_variants"] = json!(["create_note"]);
    manifest["api_key_configured"] = json!(true);
    manifest["collection_session"] = json!({"lineage_id":"c17625b0-7a88-4aab-a8a5-c1d993c72a01","session_epoch":"c17625b0-7a88-4aab-a8a5-c1d993c72a02","profile_fingerprint":"b".repeat(64),"path_fingerprint":"c".repeat(64)});
    let inspected = inspect_native_manifest(manifest).unwrap();
    assert!(!inspected.collection_writes_enabled);
}

fn native_status() -> Value {
    json!({
        "lineage_id":"c17625b0-7a88-4aab-a8a5-c1d993c72a01",
        "operation_id":"c17625b0-7a88-4aab-a8a5-c1d993c72a03",
        "payload_digest":"a".repeat(64),
        "approved_digest":format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        "session_epoch":"c17625b0-7a88-4aab-a8a5-c1d993c72a02",
        "variant":"create_note",
        "state":"queued",
        "reason":null,
        "event_digest":"c".repeat(64),
        "needs_recovery":false,
        "dispatch_newly_authorized":false
    })
}

#[test]
fn native_operation_status_is_profile_pinned_and_never_authorizes_dispatch() {
    use linguist_anki::native::NativeOperationState;
    let lineage = uuid::Uuid::parse_str(native_status()["lineage_id"].as_str().unwrap()).unwrap();
    let operation =
        uuid::Uuid::parse_str(native_status()["operation_id"].as_str().unwrap()).unwrap();
    let server = Server::new(vec![
        response(json!("Fixture")),
        response(native_status()),
        response(json!("Fixture")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    let status = client.native_operation_status(lineage, operation).unwrap();
    assert_eq!(status.state, NativeOperationState::Queued);
    assert!(!status.needs_recovery && !status.dispatch_newly_authorized);
    let requests = server.finish();
    assert_eq!(requests[1]["action"], "labOperationStatus");
    assert_eq!(
        requests[1]["params"],
        json!({"lineage_id":lineage,"operation_id":operation})
    );

    let server = Server::new(vec![
        response(json!("Fixture")),
        response(native_status()),
        response(json!("Other")),
    ]);
    let client = Client::from_settings(&settings(&server.endpoint), &BTreeMap::new()).unwrap();
    assert_eq!(
        client
            .native_operation_status(lineage, operation)
            .unwrap_err(),
        "ANKI_PROFILE_CONFLICT"
    );
    server.finish();
}

#[test]
fn malformed_native_operation_status_is_rejected() {
    use linguist_anki::native::inspect_native_operation_status;
    let value = native_status();
    let lineage = uuid::Uuid::parse_str(value["lineage_id"].as_str().unwrap()).unwrap();
    let operation = uuid::Uuid::parse_str(value["operation_id"].as_str().unwrap()).unwrap();
    assert!(inspect_native_operation_status(value.clone(), lineage, operation).is_ok());
    let mut unknown = value.clone();
    unknown["state"] = json!("unknown");
    unknown["reason"] = json!("worker_crash");
    unknown["needs_recovery"] = json!(true);
    assert!(inspect_native_operation_status(unknown, lineage, operation).is_ok());
    let mut failed = value.clone();
    failed["state"] = json!("failed_before_write");
    failed["reason"] = json!("preflight_rejected");
    assert!(inspect_native_operation_status(failed, lineage, operation).is_ok());
    for (pointer, replacement) in [
        ("/lineage_id", json!("c17625b0-7a88-4aab-a8a5-c1d993c72a04")),
        (
            "/operation_id",
            json!("c17625b0-7a88-4aab-a8a5-c1d993c72a04"),
        ),
        (
            "/session_epoch",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
        ("/payload_digest", json!("not-a-digest")),
        ("/approved_digest", json!("b".repeat(64))),
        (
            "/approved_digest",
            json!(format!("lab-jcs-v1:plan:{}", "A".repeat(64))),
        ),
        ("/event_digest", json!("short")),
        ("/variant", json!("arbitrary_sql")),
        ("/state", json!("verified")),
        ("/reason", json!("worker_crash")),
        ("/needs_recovery", json!(true)),
        ("/dispatch_newly_authorized", json!(true)),
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            inspect_native_operation_status(invalid, lineage, operation).unwrap_err(),
            "ANKI_NATIVE_STATUS_INVALID",
            "{pointer}"
        );
    }
    let mut extra = value;
    extra["verified"] = json!(true);
    assert_eq!(
        inspect_native_operation_status(extra, lineage, operation).unwrap_err(),
        "ANKI_NATIVE_STATUS_INVALID"
    );
}
