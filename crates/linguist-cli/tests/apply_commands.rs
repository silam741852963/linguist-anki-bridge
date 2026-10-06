//! OP-34 preview and OP-60 proposal. Collection writes stay unavailable until a
//! verified native mutation adapter exists; these commands never contact Anki.
use linguist_core::{
    LearningDocument, approval::ApprovalRequest, records::*, render, validation::Issue,
};
use linguist_store::Store;
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

fn plan(target_deck: bool, bound: bool) -> PlanRevision {
    let doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    let mut values = BTreeMap::new();
    if target_deck {
        values.insert(
            "purposes.japanese_vocab.target_deck".to_owned(),
            serde_json::json!("Japanese::Vocab"),
        );
    }
    PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values,
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: bound.then(binding),
        source_digest: "fixture".into(),
        selection: None,
        documents: vec![doc],
        rendered: vec![rendered],
        review_decisions: vec![],
    }
}

fn fixture() -> Fixture {
    Fixture(std::env::temp_dir().join(format!("lab-apply-cli-{}", Uuid::new_v4())))
}

fn stored(f: &Fixture, plan: &PlanRevision, approve: bool) -> String {
    let mut store = Store::open(&f.0.join("state")).unwrap();
    let digest = store.publish_revision(plan).unwrap();
    if approve {
        store
            .approve_revision(&ApprovalRequest {
                plan_id: plan.id,
                revision: 1,
                digest: digest.clone(),
                item_ids: None,
                actor: "reviewer".into(),
                accepted_warnings: vec![],
            })
            .unwrap()
            .unwrap();
    }
    digest
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).unwrap()
}

#[test]
fn apply_preview_reports_ready_items_without_any_effect() {
    let f = fixture();
    let plan = plan(true, true);
    let digest = stored(&f, &plan, true);
    let out = cli(&f)
        .args(["apply", &plan.id.to_string(), "--digest", &digest])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = json(&out.stdout);
    assert_eq!(value["mode"], "preview");
    assert_eq!(value["collection_writes_enabled"], false);
    let item = &value["items"][0];
    assert_eq!(item["approved"], true);
    assert_eq!(item["action"], "create");
    assert_eq!(item["target_deck"], "Japanese::Vocab");
    assert_eq!(item["blockers"], serde_json::json!([]));
    assert_eq!(
        item["required_variants"],
        serde_json::json!(["create_note"])
    );
    let store = Store::read_only(&f.0.join("state")).unwrap();
    assert_eq!(store.pending_journal_count().unwrap(), 0);
}

#[test]
fn apply_preview_lists_blockers_and_exits_nonzero() {
    let f = fixture();
    let plan = plan(false, false);
    stored(&f, &plan, false);
    let out = cli(&f)
        .args(["apply", &plan.id.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let blockers = json(&out.stdout)["items"][0]["blockers"].clone();
    for code in [
        "APPLY_BINDING_WEAK",
        "APPLY_APPROVAL_MISSING",
        "APPLY_TARGET_DECK_UNCONFIGURED",
    ] {
        assert!(
            blockers.as_array().unwrap().iter().any(|b| b == code),
            "{code}: {blockers}"
        );
    }
}

#[test]
fn apply_flag_and_wrong_digest_fail_before_lease_or_journal() {
    let f = fixture();
    let plan = plan(true, true);
    let digest = stored(&f, &plan, true);
    let out = cli(&f)
        .args([
            "apply",
            &plan.id.to_string(),
            "--digest",
            "lab-jcs-v1:plan:0",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("APPLY_DIGEST_MISMATCH"));
    let out = cli(&f)
        .args([
            "apply",
            &plan.id.to_string(),
            "--digest",
            &digest,
            "--apply",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    let message = json(&out.stderr)["error"].as_str().unwrap().to_owned();
    assert!(message.starts_with("CAPABILITY_UNAVAILABLE"), "{message}");
    let mut store = Store::open(&f.0.join("state")).unwrap();
    assert_eq!(store.pending_journal_count().unwrap(), 0);
    // No lease was left behind.
    store
        .acquire_lease(
            &linguist_store::lease::Resource::CollectionWriter(Uuid::from_u128(2)),
            30,
        )
        .unwrap();
}

#[test]
fn reconcile_proposal_is_local_and_writes_stay_unavailable() {
    let f = fixture();
    let mut store = Store::open(&f.0.join("state")).unwrap();
    let operation = Uuid::new_v4();
    store
        .append_journal(
            &OperationJournal {
                id: operation,
                group_id: None,
                approval_digest: "lab-model-v1:fixture".into(),
                binding: binding(),
                snapshot_id: operation,
                backup_id: Uuid::new_v4(),
                state: OperationState::Prepared,
                steps: vec![JournalStep {
                    id: Uuid::new_v4(),
                    action: "install_model".into(),
                    payload_digest: "p".into(),
                    precondition_digest: "q".into(),
                    expected_post_digest: "r".into(),
                    state: StepState::IntentRecorded,
                    observed_digest: None,
                }],
                issues: Vec::<Issue>::new(),
            },
            None,
        )
        .unwrap();
    drop(store);
    let out = cli(&f)
        .args(["recover", "reconcile", &operation.to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = json(&out.stdout);
    assert_eq!(value["reconciliation_kind"], "unsupported_journal_kind");
    assert_eq!(value["live_checked"], false);
    assert_eq!(value["reconciliation_available"], true);
    let out = cli(&f)
        .args(["recover", "reconcile", &operation.to_string(), "--rebind"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("APPLY_FLAG_REQUIRED"));
    let out = cli(&f)
        .args(["recover", "reconcile", &operation.to_string(), "--apply"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("CAPABILITY_UNAVAILABLE"));
    let out = cli(&f)
        .args(["recover", "reconcile", &Uuid::new_v4().to_string()])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOURNAL_NOT_FOUND"));
    let store = Store::read_only(&f.0.join("state")).unwrap();
    assert_eq!(
        store.journal(operation).unwrap().journal.state,
        OperationState::Prepared
    );
}
