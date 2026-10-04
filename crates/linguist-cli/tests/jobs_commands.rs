//! WP-13 job commands for simulate/apply jobs and job tombstones. Execution
//! stays unavailable until a verified native adapter exists; these commands
//! never contact Anki (the endpoint below is unreachable).
use linguist_core::{LearningDocument, approval::ApprovalRequest, records::*, render};
use linguist_store::Store;
use std::{collections::BTreeMap, path::PathBuf, process::Command};
use uuid::Uuid;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("lab-jobs-cli-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    Fixture(root)
}

fn cli(f: &Fixture) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_linguist-anki-bridge"));
    c.args(["--output", "json"]);
    c.env_clear();
    c.env("HOME", "/tmp/lab-command-tests-no-config");
    c.args([
        "--set",
        &format!("storage.state_dir={}", f.0.join("state").display()),
        "--set",
        "anki.endpoint=http://127.0.0.1:9",
    ]);
    c
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).unwrap()
}

/// One approved, ready vocabulary plan revision.
fn seed(f: &Fixture) -> (Uuid, String) {
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
        binding: None,
        source_digest: "fixture".into(),
        selection: None,
        rendered: vec![render::render(&doc, &BTreeMap::new()).unwrap()],
        documents: vec![doc],
        review_decisions: vec![],
    };
    let digest = store.publish_revision(&plan).unwrap();
    let evidence = linguist_core::plan_validation::inspect(&plan).unwrap();
    let warnings: Vec<String> = evidence.items[0]
        .issues
        .iter()
        .filter(|i| i.severity == linguist_core::Severity::Warning)
        .map(|i| i.code.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    store
        .approve_revision(&ApprovalRequest {
            plan_id: plan.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: warnings,
        })
        .unwrap()
        .unwrap();
    (plan.id, digest)
}

fn create_simulate(f: &Fixture) -> (String, Uuid) {
    let (plan, digest) = seed(f);
    let out = cli(f)
        .args([
            "jobs",
            "create",
            "--mode",
            "simulate",
            "--plan",
            &plan.to_string(),
            "--digest",
            &digest,
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = json(&out.stdout);
    assert_eq!(value["job"]["mode"], "simulate");
    assert_eq!(value["job"]["item_count"], 1);
    assert_eq!(value["writes_enabled"], false);
    (value["job"]["job_id"].as_str().unwrap().to_owned(), plan)
}

#[test]
fn simulate_jobs_are_created_inspected_and_never_write() {
    let f = fixture();
    let (job, _) = create_simulate(&f);
    let show = cli(&f).args(["jobs", "show", &job]).output().unwrap();
    assert_eq!(show.status.code(), Some(0));
    let show = json(&show.stdout);
    assert_eq!(show["mode"], "simulate");
    assert_eq!(show["job"]["status"]["state"], "pending");
    let items = json(
        &cli(&f)
            .args(["jobs", "items", &job])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(items["items"][0]["state"], "pending");

    // --apply is refused for a simulate job before any lease or event.
    let out = cli(&f)
        .args(["jobs", "run", &job, "--apply"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_MODE_NEVER_WRITES"));
    // Running needs the native read adapter, which does not exist yet.
    let out = cli(&f).args(["jobs", "run", &job]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("CAPABILITY_UNAVAILABLE"));
    let audit = cli(&f).args(["jobs", "audit", &job]).output().unwrap();
    assert_eq!(audit.status.code(), Some(0));
    let audit = json(&audit.stdout);
    assert_eq!(audit["audit"]["events_verified"], 0);
    assert_eq!(audit["audit"]["native_verified"], false);
    assert_eq!(audit["audit"]["plan_verified"], true);
    // Nothing failed, so nothing is eligible.
    let retry = cli(&f)
        .args(["jobs", "retry", &job, "--failed"])
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(4));
    assert_eq!(
        json(&retry.stdout)["retry"]["accepted"],
        serde_json::json!([])
    );
}

#[test]
fn apply_jobs_need_a_checkpoint_and_the_current_apply_flag() {
    let f = fixture();
    let (plan, digest) = seed(&f);
    let out = cli(&f)
        .args([
            "jobs",
            "create",
            "--mode",
            "apply",
            "--plan",
            &plan.to_string(),
            "--digest",
            &digest,
        ])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_PROTECTED_MANIFEST_REQUIRED"));
    let out = cli(&f)
        .args([
            "jobs",
            "create",
            "--mode",
            "apply",
            "--plan",
            &plan.to_string(),
            "--digest",
            &digest,
            "--checkpoint",
            &Uuid::new_v4().to_string(),
            "--protected-manifest",
            "protected-v1",
        ])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("CHECKPOINT"));
    // Selector and plan flags are exclusive.
    let out = cli(&f)
        .args([
            "jobs",
            "create",
            "--note-id",
            "1",
            "--plan",
            &plan.to_string(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn idle_controls_are_confirmed_and_terminal_jobs_are_tombstoned() {
    let f = fixture();
    let (job, plan) = create_simulate(&f);
    let out = cli(&f).args(["jobs", "delete", &job]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_DELETE_NOT_TERMINAL"));

    let paused = json(
        &cli(&f)
            .args(["jobs", "pause", &job])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(paused["worker_stopped_confirmed"], true);
    let cancelled = cli(&f).args(["jobs", "cancel", &job]).output().unwrap();
    assert_eq!(cancelled.status.code(), Some(0));
    assert_eq!(
        json(&cancelled.stdout)["control"]["status"]["state"],
        "cancelled"
    );
    let out = cli(&f).args(["jobs", "resume", &job]).output().unwrap();
    assert_eq!(out.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&out.stderr).contains("APPLY_JOB_CANCEL_IS_TERMINAL"));

    let preview = cli(&f).args(["jobs", "delete", &job]).output().unwrap();
    assert_eq!(preview.status.code(), Some(0));
    let preview = json(&preview.stdout);
    assert_eq!(preview["delete"]["executed"], false);
    assert_eq!(preview["delete"]["anki_deletion"], false);
    assert_eq!(preview["delete"]["tombstone"]["final_state"], "cancelled");
    let listed = json(&cli(&f).args(["jobs", "list"]).output().unwrap().stdout);
    assert_eq!(listed["jobs"][0]["kind"], "simulate");

    let done = cli(&f)
        .args(["jobs", "delete", &job, "--execute"])
        .output()
        .unwrap();
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(json(&done.stdout)["delete"]["executed"], true);
    let listed = json(&cli(&f).args(["jobs", "list"]).output().unwrap().stdout);
    assert_eq!(listed["jobs"], serde_json::json!([]));
    let all = json(
        &cli(&f)
            .args(["jobs", "list", "--include-deleted", "--mode", "simulate"])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(all["jobs"][0]["tombstoned"], true);
    let show = json(
        &cli(&f)
            .args(["jobs", "show", &job])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(show["job"]["tombstone"]["final_state"], "cancelled");
    // The plan and its approval survive the tombstone.
    let store = Store::read_only(&f.0.join("state")).unwrap();
    assert_eq!(store.latest_revision(plan).unwrap(), 1);
    assert_eq!(store.approvals_for(plan, 1).unwrap().len(), 1);
}

#[test]
fn prepare_jobs_refuse_apply_and_classify_retries_without_running() {
    let f = fixture();
    let created = cli(&f)
        .args([
            "--purpose",
            "japanese_vocab",
            "jobs",
            "create",
            "--note-id",
            "123",
        ])
        .output()
        .unwrap();
    assert_eq!(
        created.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let job = json(&created.stdout)["job_id"].as_str().unwrap().to_owned();
    for args in [
        vec!["jobs", "run", job.as_str(), "--apply"],
        vec!["jobs", "resume", job.as_str(), "--apply"],
        vec!["jobs", "retry", job.as_str(), "--failed", "--apply"],
    ] {
        let out = cli(&f).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_MODE_NEVER_WRITES"));
    }
    let retry = cli(&f)
        .args(["jobs", "retry", &job, "--failed"])
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(4));
    let retry = json(&retry.stdout);
    assert_eq!(retry["envelope_recorded"], false);
    // An idle pause is confirmed for prepare jobs too.
    let paused = json(
        &cli(&f)
            .args(["jobs", "pause", &job])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(paused["worker_stopped_confirmed"], true);
    let show = json(
        &cli(&f)
            .args(["jobs", "show", &job])
            .output()
            .unwrap()
            .stdout,
    );
    assert_eq!(show["stop_acknowledgement"]["idle"], true);
    // Not terminal until cancelled or published.
    let out = cli(&f).args(["jobs", "delete", &job]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_DELETE_NOT_TERMINAL"));
    cli(&f).args(["jobs", "cancel", &job]).output().unwrap();
    let done = cli(&f)
        .args(["jobs", "delete", &job, "--execute"])
        .output()
        .unwrap();
    assert_eq!(
        done.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let out = cli(&f).args(["jobs", "run", &job]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("JOB_TOMBSTONED"));
}
