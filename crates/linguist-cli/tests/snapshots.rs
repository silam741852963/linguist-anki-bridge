use linguist_core::{
    canonical,
    records::{Snapshot, SourceArchive, SourceRecord},
};
use linguist_store::Store;
use serde_json::Value;
use uuid::Uuid;

#[test]
fn snapshot_list_and_show_read_immutable_local_evidence_without_creating_state() {
    let root = std::env::temp_dir().join(format!("lab-cli-snapshot-{}", Uuid::new_v4()));
    let cli = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
        command
            .env("HOME", "/tmp/lab-cli-test-no-home")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LAB_CONFIG")
            .args(["--output", "json", "--set"])
            .arg(format!("storage.state_dir={}", root.display()));
        command
    };
    let output = cli().args(["snapshots", "list"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["state_exists"], false);
    assert_eq!(value["snapshots"], serde_json::json!([]));
    assert!(!root.exists());

    let mut store = Store::open(&root).unwrap();
    let mut snapshot = Snapshot {
        id: Uuid::new_v4(),
        operation_id: Uuid::new_v4(),
        originals: vec![],
        archives: vec![],
        media: vec![],
        before_digest: String::new(),
    };
    let digest = store.publish_asset(b"original note", 1024).unwrap();
    let source_id = Uuid::new_v4();
    snapshot.originals.push(SourceRecord {
        id: source_id,
        kind: "anki_read_capture_v2".into(),
        location: "anki_note:123".into(),
        digest: digest.clone(),
        text: None,
        fields: Default::default(),
        model_manifest: "model".into(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    snapshot.archives.push(SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: None,
        original_fields: Default::default(),
        asset_digests: vec![digest],
    });
    snapshot.before_digest = canonical::digest(
        "snapshot-original",
        &(&snapshot.originals, &snapshot.archives, &snapshot.media),
    )
    .unwrap();
    store.publish_snapshot(&snapshot).unwrap();
    drop(store);
    let output = cli().args(["snapshots", "list"]).output().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["snapshots"][0]["snapshot"]["id"],
        snapshot.id.to_string()
    );
    assert!(value["snapshots"][0]["after"].is_null());
    let output = cli()
        .args(["snapshots", "show", &snapshot.id.to_string()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["snapshot"]["before_digest"], snapshot.before_digest);
    assert!(value["after"].is_null());
    assert_eq!(value["post_state_status"], "unknown");
    let output = cli()
        .args([
            "snapshots",
            "list",
            "--status",
            "unknown",
            "--note-id",
            "123",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["snapshots"].as_array().unwrap().len(), 1);
    let output = cli()
        .args(["snapshots", "list", "--job", &Uuid::new_v4().to_string()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["snapshots"].as_array().unwrap().is_empty());
    let output = cli()
        .args(["snapshots", "list", "--status", "verified"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let output = cli()
        .args(["snapshots", "restore", &snapshot.id.to_string(), "--apply"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        Store::read_only(&root)
            .unwrap()
            .snapshot(snapshot.id)
            .unwrap()
            .snapshot,
        snapshot
    );
    let mut second = snapshot.clone();
    second.id = Uuid::new_v4();
    second.operation_id = Uuid::new_v4();
    Store::open_existing(&root)
        .unwrap()
        .publish_snapshot(&second)
        .unwrap();
    let page = |cursor: Option<&str>| {
        let mut command = cli();
        command.args(["--set", "output.page_size=1", "snapshots", "list"]);
        if let Some(cursor) = cursor {
            command.args(["--cursor", cursor]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let first = page(None);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second_page = page(Some(cursor));
    assert_ne!(
        first["snapshots"][0]["snapshot"]["id"],
        second_page["snapshots"][0]["snapshot"]["id"]
    );
    assert!(second_page["next_cursor"].is_null());
    std::fs::remove_dir_all(root).unwrap();
}
