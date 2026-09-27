use linguist_application::source_archive::*;
use linguist_core::canonical;
use serde_json::{Value, json};
fn fixture() -> (Value, Value, Value) {
    (
        json!({"noteId":"123","modelName":"Legacy","fields":{"Extra":{"value":"  original\n","order":1},"Expression":{"value":"<b>猫</b>[sound:cat.mp3]","order":0}},"cards":["456"],"tags":["original"],"extension":{"retain":true}}),
        json!({"model":{"name":"Legacy","id":"12"},"fields":["Expression","Extra"],"templates":{"Card":{"Front":"front","Back":"back"}},"css":"style","extension":"raw model"}),
        json!([{"cardId":"456","note":"123","due":10,"extension":"retain scheduler payload"}]),
    )
}
fn capture(n: &Value, m: &Value, c: &Value) -> Result<CapturedSource, String> {
    archive_read_capture(
        &serde_json::to_vec_pretty(n).unwrap(),
        &serde_json::to_vec_pretty(m).unwrap(),
        &serde_json::to_vec_pretty(c).unwrap(),
        10000,
    )
}
#[test]
fn raw_note_model_and_card_assets_survive_publication_without_native_history_claims() {
    let (n, m, c) = fixture();
    let captured = capture(&n, &m, &c).unwrap();
    assert_eq!(captured.source.fields, captured.archive.original_fields);
    assert_eq!(captured.source.fields["Extra"], "  original\n");
    assert_eq!(captured.source.media_refs, vec!["cat.mp3"]);
    assert!(captured.source.cards.is_empty()); // No fabricated native CardState/history.
    let manifest: Value = canonical::parse(&captured.assets[&captured.source.digest]).unwrap();
    assert_eq!(manifest["atomic_snapshot_verified"], false);
    assert_eq!(manifest["native_history_verified"], false);
    let root = std::env::temp_dir().join(format!("lab-source-archive-{}", uuid::Uuid::new_v4()));
    let mut store = linguist_store::Store::open(&root).unwrap();
    for (hash, bytes) in &captured.assets {
        assert_eq!(*hash, store.publish_asset(bytes, 10000).unwrap());
    }
    drop(store);
    let store = linguist_store::Store::read_only(&root).unwrap();
    for (name, value) in [("note", n), ("model", m), ("cards", c)] {
        let hash = manifest["payloads"][name].as_str().unwrap();
        assert_eq!(
            store.asset(hash, 10000).unwrap(),
            serde_json::to_vec_pretty(&value).unwrap()
        );
        assert!(captured.archive.asset_digests.iter().any(|s| s == hash));
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn conflicting_ids_field_order_models_and_duplicate_cards_fail_before_publication() {
    for (target, pointer, value) in [
        (0, "/noteId", json!(123)),
        (0, "/fields/Extra/order", json!(0)),
        (1, "/model/name", json!("Other")),
        (1, "/templates/Card/Front", Value::Null),
        (2, "/0/note", json!("124")),
        (2, "/0/cardId", json!("457")),
    ] {
        let (mut n, mut m, mut c) = fixture();
        let payload = match target {
            0 => &mut n,
            1 => &mut m,
            _ => &mut c,
        };
        *payload.pointer_mut(pointer).unwrap() = value;
        assert!(capture(&n, &m, &c).is_err());
    }
    let (mut n, m, mut c) = fixture();
    n["cards"] = json!(["456", "456"]);
    let duplicate = c[0].clone();
    c.as_array_mut().unwrap().push(duplicate);
    assert!(capture(&n, &m, &c).is_err());
    assert_eq!(
        archive_read_capture(b"{}", b"{}", b"[]", 1).err().unwrap(),
        "SOURCE_CAPTURE_LIMIT"
    );
    assert!(archive_read_capture(b"{\"x\":1,\"x\":2}", b"{}", b"[]", 100).is_err());
}

#[test]
fn media_attachment_is_complete_bounded_and_atomic_on_failure() {
    use std::collections::BTreeMap;
    let (n, m, c) = fixture();
    let mut captured = capture(&n, &m, &c).unwrap();
    let original_digest = captured.source.digest.clone();
    let original_assets = captured.assets.clone();
    assert_eq!(
        media::attach_original_media(&mut captured, BTreeMap::new(), 100, 10000).unwrap_err(),
        "SOURCE_MEDIA_SELECTION_CONFLICT"
    );
    assert_eq!(
        media::attach_original_media(
            &mut captured,
            BTreeMap::from([("cat.mp3".into(), Some(vec![1, 2, 3, 4, 5]))]),
            4,
            10000
        )
        .unwrap_err(),
        "SOURCE_MEDIA_LIMIT"
    );
    assert_eq!(captured.source.digest, original_digest);
    assert_eq!(captured.assets, original_assets);
    media::attach_original_media(
        &mut captured,
        BTreeMap::from([("cat.mp3".into(), Some(vec![1, 2, 3]))]),
        100,
        10000,
    )
    .unwrap();
    assert_eq!(captured.source.fields, captured.archive.original_fields);
    assert_eq!(
        captured.assets[&canonical::asset_digest(&[1, 2, 3])],
        [1, 2, 3]
    );
    let manifest: Value = canonical::parse(&captured.assets[&captured.source.digest]).unwrap();
    assert_eq!(manifest["media_bytes_archived"], true);
    assert_eq!(manifest["media_content_verified"], false);
    let archived_digest = captured.source.digest.clone();
    assert_eq!(
        media::attach_original_media(
            &mut captured,
            BTreeMap::from([("cat.mp3".into(), None)]),
            100,
            10000
        )
        .unwrap_err(),
        "SOURCE_MEDIA_ALREADY_CAPTURED"
    );
    assert_eq!(captured.source.digest, archived_digest);
}

#[test]
fn revamp_capture_composes_read_port_mapping_and_restart_safe_assets() {
    use linguist_config::*;
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut actions = Vec::new();
        for _ in 0..34 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
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
                    length = line
                        .split_once(':')
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let action = request["action"].as_str().unwrap();
            let value = match action {
                "getActiveProfile" => json!("Fixture"),
                "modelNamesAndIds" => json!({"Legacy":12}),
                "notesInfo" => {
                    json!([{"noteId":123,"modelName":"Legacy","fields":{"Expression":{"value":"<b>猫</b><img src=\"pic.png\">[sound:cat.mp3]","order":0},"Unused":{"value":"  original\n","order":1}},"cards":[456],"tags":["original"],"extension":"retained"}])
                }
                "cardsInfo" => {
                    json!([{"cardId":456,"note":123,"due":10,"reps":5,"extension":"retained"}])
                }
                "modelFieldNames" => json!(["Expression", "Unused"]),
                "modelTemplates" => json!({"Card":{"Front":"front","Back":"back"}}),
                "modelStyling" => json!({"css":"style"}),
                "retrieveMediaFile" => match request["params"]["filename"].as_str().unwrap() {
                    "cat.mp3" => json!(false),
                    "pic.png" => json!("AQID"),
                    _ => panic!("unexpected filename"),
                },
                _ => panic!("unexpected {action}"),
            };
            actions.push(action.to_owned());
            let body = json!({"result":value,"error":null}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        actions
    });
    let mut options = ResolveOptions {
        purpose: Some("japanese_vocab".into()),
        ..Default::default()
    };
    options
        .flags
        .insert("anki.endpoint".into(), json!(endpoint));
    options.flags.insert(
        "purposes.japanese_vocab.fields".into(),
        json!({"expression":"Expression"}),
    );
    options.flags.insert(
        "purposes.japanese_vocab.source_model".into(),
        json!("Legacy"),
    );
    let mut settings = resolve(&Registry::builtin(), &ConfigFile::default(), &options).unwrap();
    let client = linguist_anki::Client::from_settings(&settings, &Default::default()).unwrap();
    let draft = capture_for_revamp(&client, &settings, "japanese_vocab", "123").unwrap();
    assert_eq!(server.join().unwrap().len(), 34);
    assert_eq!(draft.mapping.unmapped_fields, vec!["Unused"]);
    assert_eq!(draft.mapping.missing_required_roles, vec!["meaning"]);
    let manifest: Value =
        canonical::parse(&draft.captured.assets[&draft.captured.source.digest]).unwrap();
    assert_eq!(manifest["repeated_reads_matched"], true);
    assert_eq!(manifest["native_history_verified"], false);
    assert_eq!(manifest["mapping_digest"], draft.mapping.mapping_digest);
    assert_eq!(manifest["media_bytes_archived"], false);
    assert_eq!(manifest["media_content_verified"], false);
    assert_eq!(manifest["media"][0]["filename"], "cat.mp3");
    assert!(manifest["media"][0]["digest"].is_null());
    let media_digest = canonical::asset_digest(&[1, 2, 3]);
    assert_eq!(manifest["media"][1]["digest"], media_digest);
    assert_eq!(draft.captured.assets[&media_digest], [1, 2, 3]);
    let document =
        linguist_application::revamp::stage_document(&draft, &settings, "japanese_vocab").unwrap();
    assert_eq!(document.media.len(), 1);
    assert_eq!(
        document.media[0].role,
        linguist_core::records::MediaRole::Archive
    );
    assert_eq!(
        document.media[0].original_filename.as_deref(),
        Some("pic.png")
    );
    assert!(
        document
            .issues
            .iter()
            .any(|issue| issue.code == "SOURCE_MEDIA_MISSING_REVIEW")
    );
    assert!(
        document
            .issues
            .iter()
            .any(|issue| issue.code == "SOURCE_MEDIA_CONTENT_REVIEW")
    );
    let root = std::env::temp_dir().join(format!("lab-revamp-capture-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let environment =
        std::collections::BTreeMap::from([("HOME".into(), "/tmp/lab-media-capture".into())]);
    let prepared = linguist_application::revamp::publish_capture_draft(
        &draft,
        &settings,
        "japanese_vocab",
        &environment,
    )
    .unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    assert_eq!(plan.documents[0].media, document.media);
    let failure = plan.documents[0]
        .evidence
        .iter()
        .find(|e| e.field == "media_format")
        .unwrap();
    assert!(failure.ambiguous);
    let receipt: Value = serde_json::from_str(&failure.claim).unwrap();
    assert_eq!(receipt["asset_digest"], media_digest);
    assert_eq!(receipt["failure"]["code"], "IMAGE_FORMAT_UNSUPPORTED");
    assert!(receipt.get("inspection").is_none());
    let review = plan.documents[0]
        .issues
        .iter()
        .find(|i| i.code == "SOURCE_MEDIA_FORMAT_REVIEW")
        .unwrap();
    assert!(review.message.contains("Original bytes remain archived"));
    assert_eq!(store.asset(&media_digest, 100000).unwrap(), [1, 2, 3]);
    for hash in &draft.captured.archive.asset_digests {
        assert_eq!(
            store.asset(hash, 100000).unwrap(),
            draft.captured.assets[hash]
        );
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
