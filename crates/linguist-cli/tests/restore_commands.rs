//! OP-50 and OP-44 local previews. Restore writes stay unavailable until a
//! verified native mutation adapter exists; these commands never contact Anki.
use linguist_application::apply::{
    ApplyIntent, CardProjection, Effect, IntentStep, ItemAction, NoteProjection, UpdateNote,
};
use linguist_core::{
    LearningDocument, approval::ApprovalRequest, canonical, records::*, render, validation::Issue,
};
use linguist_store::{Store, apply::ApplyOperationRecord};
use std::{collections::BTreeMap, path::PathBuf, process::Command};
use uuid::Uuid;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli(f: &Fixture) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.args(["--output", "json"]);
    c.env_clear();
    c.env("HOME", "/tmp/lab-command-tests-no-config");
    c.args([
        "--set",
        &format!("storage.state_dir={}", f.0.join("state").display()),
        // An unreachable endpoint proves no command below contacts Anki.
        "--set",
        "anki.endpoint=http://127.0.0.1:9",
    ]);
    c
}

fn binding() -> CollectionBinding {
    CollectionBinding {
        endpoint: "http://127.0.0.1:8765".into(),
        profile_fingerprint: "a".repeat(64),
        path_fingerprint: "b".repeat(64),
        bridge_id: Uuid::from_u128(1),
        lineage_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(3),
        capability_digest: "c".repeat(64),
    }
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).unwrap()
}

fn fields(meaning: &str) -> BTreeMap<String, String> {
    [("Expression", "食べる"), ("Meaning", meaning)]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

fn projection(meaning: &str, tags: &[&str], deck: i64) -> NoteProjection {
    NoteProjection {
        model_name: "Linguist Vocabulary v2".into(),
        model_manifest_digest: "f".repeat(64),
        fields: fields(meaning),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        cards: vec![CardProjection {
            ordinal: 0,
            deck_id: deck,
            id: Some(20),
            scheduler: Some(BTreeMap::new()),
            history_digest: Some("1".repeat(64)),
            review_count: 1,
        }],
    }
}

/// A committed update operation with its intent, snapshot and journal, as
/// apply records them. `commit` false leaves the request started.
fn seed(f: &Fixture, group: Option<Uuid>, commit: bool) -> (Uuid, Uuid) {
    let mut store = Store::open(&f.0.join("state")).unwrap();
    let doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    let plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: Some(binding()),
        source_digest: "fixture".into(),
        selection: None,
        rendered: vec![render::render(&doc, &BTreeMap::new()).unwrap()],
        documents: vec![doc],
        review_decisions: vec![],
    };
    let digest = store.publish_revision(&plan).unwrap();
    let approval = store
        .approve_revision(&ApprovalRequest {
            plan_id: plan.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: vec![],
        })
        .unwrap()
        .unwrap()
        .id;
    let operation = Uuid::new_v4();
    let desired = projection("to eat", &["linguist", "old"], 500);
    let effect = Effect::UpdateNote(UpdateNote {
        note_id: 10,
        expected_pre_digest: "pre".into(),
        migration: None,
        fields: desired.fields.clone(),
        add_tags: vec!["linguist".into()],
        deck_id: 500,
    });
    let expected = serde_json::to_value(&desired).unwrap();
    let expected_digest = canonical::asset_digest(&canonical::bytes(&expected).unwrap());
    let step = IntentStep {
        step_id: Uuid::new_v4(),
        payload_digest: effect.payload_digest().unwrap(),
        precondition_digest: "pre".into(),
        expected,
        expected_digest: expected_digest.clone(),
        effect,
    };
    let intent = ApplyIntent {
        schema_version: 1,
        action: ItemAction::Update,
        note_id: Some(10),
        marker_tag: None,
        target_model_name: "Linguist Vocabulary v2".into(),
        target_model_id: 1001,
        target_manifest_digest: "f".repeat(64),
        deck_id: 500,
        retained_card_ids: vec![20],
        pre_state: Some(projection("to consume", &["old"], 400)),
        desired,
        steps: vec![step.clone()],
        checkpoint: serde_json::json!({}),
        reused_media: vec![],
    };
    store
        .publish_apply_operation(&ApplyOperationRecord {
            operation_id: operation,
            plan_id: plan.id,
            revision: 1,
            item_id: plan.documents[0].id,
            approval_id: approval,
            created_ms: 1,
            intent: serde_json::to_value(&intent).unwrap(),
        })
        .unwrap();
    let original = canonical::bytes(&fields("to consume")).unwrap();
    let asset = store.publish_asset(&original, 1024 * 1024).unwrap();
    let source_id = Uuid::new_v4();
    let originals = vec![SourceRecord {
        id: source_id,
        kind: "lab_apply_prestate_v1".into(),
        location: "anki_note:10".into(),
        digest: asset.clone(),
        text: None,
        fields: fields("to consume"),
        model_manifest: "f".repeat(64),
        template_manifest: None,
        captured_at_unix_seconds: Some(1),
        tags: vec!["old".into()],
        cards: vec![],
        media_refs: vec![],
    }];
    let archives = vec![SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: asset.clone(),
        original_text: None,
        original_fields: fields("to consume"),
        asset_digests: vec![asset],
    }];
    let snapshot = Snapshot {
        id: Uuid::new_v4(),
        operation_id: operation,
        before_digest: canonical::digest(
            "snapshot-original",
            &(&originals, &archives, &Vec::<MediaAsset>::new()),
        )
        .unwrap(),
        originals,
        archives,
        media: vec![],
    };
    store.publish_snapshot(&snapshot).unwrap();
    let mut journal = OperationJournal {
        id: operation,
        group_id: group,
        approval_digest: digest,
        binding: binding(),
        snapshot_id: snapshot.id,
        backup_id: Uuid::new_v4(),
        state: OperationState::Prepared,
        steps: vec![JournalStep {
            id: step.step_id,
            action: "update_note".into(),
            payload_digest: step.payload_digest.clone(),
            precondition_digest: "pre".into(),
            expected_post_digest: expected_digest.clone(),
            state: StepState::IntentRecorded,
            observed_digest: None,
        }],
        issues: Vec::<Issue>::new(),
    };
    let mut version = store.append_journal(&journal, None).unwrap();
    let mut advance = |journal: &OperationJournal| {
        version = store.append_journal(journal, Some(&version)).unwrap();
    };
    journal.state = OperationState::Preflight;
    advance(&journal);
    journal.state = OperationState::Checkpointed;
    advance(&journal);
    journal.state = OperationState::Mutating;
    journal.steps[0].state = StepState::RequestStarted;
    advance(&journal);
    if commit {
        journal.steps[0].state = StepState::ObservedSuccess;
        journal.steps[0].observed_digest = Some(expected_digest);
        journal.state = OperationState::Verifying;
        advance(&journal);
        journal.steps[0].state = StepState::Verified;
        advance(&journal);
        journal.state = OperationState::Committed;
        advance(&journal);
    }
    (operation, snapshot.id)
}

#[test]
fn restore_preview_is_local_and_restore_writes_stay_unavailable() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-restore-cli-{}", Uuid::new_v4())));
    let (operation, snapshot) = seed(&f, None, true);
    let out = cli(&f)
        .args(["snapshots", "restore", &snapshot.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = json(&out.stdout);
    assert_eq!(value["mode"], "preview");
    assert_eq!(value["live_checked"], false);
    assert_eq!(value["collection_writes_enabled"], false);
    let restore = &value["restore"];
    assert_eq!(restore["target_operation"], operation.to_string());
    assert_eq!(
        restore["fields_changed_by_apply"],
        serde_json::json!(["Meaning"])
    );
    assert_eq!(
        restore["tags_added_by_apply"],
        serde_json::json!(["linguist"])
    );
    assert_eq!(restore["deck_moves"], serde_json::json!([[20, 400, 500]]));
    assert_eq!(restore["blockers"], serde_json::json!([]));
    // A decision file must name this snapshot.
    let decision = f.0.join("decision.json");
    let mut body = serde_json::json!({
        "schema_version": 1,
        "snapshot_id": Uuid::new_v4(),
        "observed_state_digest": "x",
        "actor": "reviewer",
    });
    std::fs::write(&decision, body.to_string()).unwrap();
    let out = cli(&f)
        .args(["snapshots", "restore", &snapshot.to_string(), "--decision"])
        .arg(&decision)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("RESTORE_DECISION_INVALID"));
    body["snapshot_id"] = serde_json::json!(snapshot);
    std::fs::write(&decision, body.to_string()).unwrap();
    let out = cli(&f)
        .args(["snapshots", "restore", &snapshot.to_string(), "--decision"])
        .arg(&decision)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        json(&out.stdout)["decision_digest"]
            .as_str()
            .unwrap()
            .starts_with("lab-jcs-v1:")
    );
    // --apply is refused before any lease, journal or Anki request.
    let out = cli(&f)
        .args(["snapshots", "restore", &snapshot.to_string(), "--apply"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("CAPABILITY_UNAVAILABLE"));
    let store = Store::read_only(&f.0.join("state")).unwrap();
    assert!(store.restore_operations_for(operation).unwrap().is_empty());
    assert_eq!(
        store.journal(operation).unwrap().journal.state,
        OperationState::Committed
    );
}

#[test]
fn unresolved_apply_must_be_reconciled_before_restore() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-restore-cli-{}", Uuid::new_v4())));
    let (_, snapshot) = seed(&f, None, false);
    let out = cli(&f)
        .args(["snapshots", "restore", &snapshot.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert_eq!(
        json(&out.stdout)["restore"]["blockers"],
        serde_json::json!(["RESTORE_RECONCILE_FIRST"])
    );
}

#[test]
fn rollback_preview_covers_the_group_and_writes_stay_unavailable() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-rollback-cli-{}", Uuid::new_v4())));
    let group = Uuid::new_v4();
    let (operation, snapshot) = seed(&f, Some(group), true);
    seed(&f, None, true);
    let out = cli(&f)
        .args(["jobs", "rollback", &group.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = json(&out.stdout);
    let items = value["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["operation_id"], operation.to_string());
    assert_eq!(items[0]["snapshot_id"], snapshot.to_string());
    assert!(value["split_execution"].is_null());
    let out = cli(&f)
        .args([
            "jobs",
            "rollback",
            &group.to_string(),
            "--item-id",
            &Uuid::new_v4().to_string(),
        ])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("ROLLBACK_GROUP_EMPTY"));
    let out = cli(&f)
        .args(["jobs", "rollback", &group.to_string(), "--apply"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("CAPABILITY_UNAVAILABLE"));
    let out = cli(&f)
        .args(["jobs", "rollback", &Uuid::new_v4().to_string()])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("ROLLBACK_GROUP_EMPTY"));
}

#[test]
fn split_group_preview_rejects_unknown_groups() {
    let f = Fixture(std::env::temp_dir().join(format!("lab-split-cli-{}", Uuid::new_v4())));
    seed(&f, None, true);
    let store = Store::read_only(&f.0.join("state")).unwrap();
    let plan = store.list_revisions(10).unwrap()[0].id;
    drop(store);
    let out = cli(&f)
        .args([
            "apply",
            &plan.to_string(),
            "--split-group",
            &Uuid::new_v4().to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("SPLIT_GROUP_NOT_FOUND"),
        "{out:?}"
    );
}
