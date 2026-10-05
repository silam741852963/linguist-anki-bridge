//! WP-15 / EV-14: four disposable prepare→review→apply→restore scenarios
//! against a real Anki backend (`scripts/disposable-anki-lab.py`, a new
//! collection in a temporary directory; no user profile is opened).
//!
//! Each scenario uses the built CLI for every step it can perform today:
//! preparation, capture, inspection, validation, approval, the apply preview,
//! the refused `--apply`, and post-apply inspection. Two steps the shipped
//! product cannot take are performed by this harness and are recorded as
//! such in the evidence:
//!
//! - binding: CLI preparation records no collection binding, so the harness
//!   publishes a child revision bound to the disposable collection, which the
//!   CLI then validates and approves like any other revision;
//! - native transport: there is no Rust `labMutate` transport, so apply,
//!   checkpoint export and restore run through the real
//!   `linguist_application` orchestration over [`LabPort`], an HTTP port to
//!   the lab that executes the companion's real effect functions.
//!
//! The revamp workflows additionally cannot reach `ready`
//! (`SOURCE_NATIVE_HISTORY_REVIEW` has no resolution); the scenarios assert
//! that the CLI draft stops there, then the harness authors the reviewed
//! migration document from the CLI's captured source archive.
//!
//! Run explicitly (needs Anki's Python, default `/usr/bin/python3.14`):
//! `cargo test -p linguist-cli --test release_scenarios -- --ignored --nocapture`
use linguist_application::{
    apply::{
        ApplyPort, ApplyRequest, MutationRequest, NativeStatus, ObservedDeck, ObservedMedia,
        ObservedNote, OwnerToken, apply_item,
    },
    backup::{
        CheckpointExporter, CheckpointRequest, ExportClaim, ExportRequest, PortFailure,
        ScopePreference, create_checkpoint,
    },
    checkpoint::{PackageLimits, ScopeManifest},
    model_install::{ObservedModel, manifest_digest},
    restore::{RestoreDecision, RestoreRequest, plan_restore, restore},
};
use linguist_core::{
    LearningDocument, Task, canonical, model,
    records::{
        CollectionBinding, OperationState, PlanRevision, SourceTaskMap, SourceTaskMapEntry,
        TargetModelKind,
    },
    render,
};
use linguist_store::{
    Store,
    lease::{LeaseToken, Resource},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_linguist-anki-bridge");
const PROTECTED: &str = "release-scenario-protected-v1";

// ---------------------------------------------------------------- lab process

struct Lab {
    child: Child,
    address: String,
    endpoint: String,
}
impl Lab {
    fn start(dir: &Path) -> Self {
        let python = std::env::var("LAB_ANKI_PYTHON").unwrap_or("/usr/bin/python3.14".into());
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/disposable-anki-lab.py");
        let mut child = Command::new(python)
            .arg(script)
            .arg("--dir")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("LAB_ANKI_PYTHON must run Anki's Python");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let info: Value = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("lab did not start (is Anki importable?): {line:?}"));
        assert_eq!(
            info["anki_version"], "25.09.2",
            "scenario evidence is pinned to this Anki build"
        );
        let endpoint = info["endpoint"].as_str().unwrap().to_owned();
        let address = endpoint.trim_start_matches("http://").to_owned();
        Self {
            child,
            address,
            endpoint,
        }
    }
    fn try_call(&self, action: &str, params: Value) -> Result<Value, String> {
        let body = json!({"action": action, "version": 6, "params": params}).to_string();
        let mut stream = std::net::TcpStream::connect(&self.address).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();
        write!(
            stream,
            "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.address,
            body.len()
        )
        .map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|e| e.to_string())?;
        let split = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("bad response")?;
        let envelope: Value =
            serde_json::from_slice(&response[split + 4..]).map_err(|e| e.to_string())?;
        if envelope["error"].is_null() {
            Ok(envelope["result"].clone())
        } else {
            Err(envelope["error"].as_str().unwrap_or("lab error").to_owned())
        }
    }
    fn call(&self, action: &str, params: Value) -> Value {
        self.try_call(action, params)
            .unwrap_or_else(|e| panic!("{action}: {e}"))
    }
    fn binding(&self) -> CollectionBinding {
        serde_json::from_value(self.call("labTest.binding", json!({}))).unwrap()
    }
    fn note(&self, id: i64) -> Option<ObservedNote> {
        serde_json::from_value(self.call("labTest.note", json!({"note_id": id}))).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = self.try_call("labTest.shutdown", json!({}));
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

/// The `ApplyPort` boundary over the lab. Reads and effects go to real Anki
/// through the companion's effect functions; there is no AnkiConnect
/// registration, persistent ledger or crash window here.
struct LabPort<'a> {
    lab: &'a Lab,
    state: PathBuf,
}
impl ApplyPort for LabPort<'_> {
    fn execution_binding(&mut self) -> Result<CollectionBinding, String> {
        Ok(self.lab.binding())
    }
    fn mutation_variants(&mut self) -> Result<Vec<String>, String> {
        serde_json::from_value(self.lab.try_call("labTest.variants", json!({}))?)
            .map_err(|e| e.to_string())
    }
    fn begin(&mut self, binding: &CollectionBinding, _: &str) -> Result<OwnerToken, String> {
        serde_json::from_value(
            self.lab
                .try_call("labTest.begin", json!({"binding": binding}))?,
        )
        .map_err(|e| e.to_string())
    }
    fn end(&mut self, owner: &OwnerToken) -> Result<(), String> {
        self.lab
            .try_call("labTest.end", serde_json::to_value(owner).unwrap())?;
        Ok(())
    }
    fn note(&mut self, id: i64) -> Result<Option<ObservedNote>, String> {
        serde_json::from_value(self.lab.try_call("labTest.note", json!({"note_id": id}))?)
            .map_err(|e| e.to_string())
    }
    fn notes_tagged(&mut self, tag: &str) -> Result<Vec<ObservedNote>, String> {
        serde_json::from_value(
            self.lab
                .try_call("labTest.notesTagged", json!({"tag": tag}))?,
        )
        .map_err(|e| e.to_string())
    }
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>, String> {
        serde_json::from_value(
            self.lab
                .try_call("labTest.modelsNamed", json!({"name": name}))?,
        )
        .map_err(|e| e.to_string())
    }
    fn deck(&mut self, name: &str) -> Result<Option<ObservedDeck>, String> {
        serde_json::from_value(self.lab.try_call("labTest.deck", json!({"name": name}))?)
            .map_err(|e| e.to_string())
    }
    fn media(&mut self, filename: &str) -> Result<Option<ObservedMedia>, String> {
        serde_json::from_value(
            self.lab
                .try_call("labTest.media", json!({"filename": filename}))?,
        )
        .map_err(|e| e.to_string())
    }
    fn media_bytes(&mut self, filename: &str, max: u64) -> Result<Option<Vec<u8>>, String> {
        let value = self
            .lab
            .try_call("labTest.mediaBytes", json!({"filename": filename}))?;
        let Some(text) = value.as_str() else {
            return Ok(None);
        };
        let bytes = base64_decode(text);
        if bytes.len() as u64 > max {
            return Err("MEDIA_TOO_LARGE".into());
        }
        Ok(Some(bytes))
    }
    fn mutate(&mut self, request: &MutationRequest) -> Result<NativeStatus, PortFailure> {
        let mut params = json!({"request": request});
        if let linguist_application::apply::Effect::StoreMedia { staged_asset, .. } =
            &request.effect
        {
            let bytes = Store::read_only(&self.state)
                .and_then(|store| store.asset(staged_asset, 64 << 20))
                .map_err(PortFailure::Rejected)?;
            params["media_base64"] = json!(base64_encode(&bytes));
        }
        let value = self
            .lab
            .try_call("labTest.mutate", params)
            .map_err(PortFailure::Unknown)?;
        serde_json::from_value(value).map_err(|e| PortFailure::Unknown(e.to_string()))
    }
    fn status(&mut self, operation_id: Uuid) -> Result<NativeStatus, String> {
        serde_json::from_value(
            self.lab
                .try_call("labTest.status", json!({"operation_id": operation_id}))?,
        )
        .map_err(|e| e.to_string())
    }
}

struct LabExporter<'a>(&'a Lab);
impl CheckpointExporter for LabExporter<'_> {
    fn export_checkpoint(&mut self, request: &ExportRequest) -> Result<ExportClaim, PortFailure> {
        let claim = self
            .0
            .try_call(
                "labTest.exportPackage",
                json!({"path": request.destination}),
            )
            .map_err(PortFailure::Rejected)?;
        Ok(ExportClaim {
            path: request.destination.clone(),
            size_bytes: claim["size_bytes"].as_u64().unwrap(),
            sha256: claim["sha256"].as_str().unwrap().to_owned(),
        })
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes().filter(|b| *b != b'=') {
        buffer = buffer << 6 | B64.iter().position(|c| *c == byte).unwrap() as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

// ---------------------------------------------------------------- scenario fixture

struct Scenario {
    root: PathBuf,
    lab: Lab,
    purpose: &'static str,
    target_deck: Option<&'static str>,
    log: Vec<String>,
}
impl Scenario {
    fn new(name: &str, purpose: &'static str, target_deck: Option<&'static str>) -> Self {
        let root =
            std::env::temp_dir().join(format!("lab-release-scenario-{name}-{}", Uuid::new_v4()));
        for dir in ["home", "out", "scratch"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let lab = Lab::start(&root.join("anki"));
        // Fixed managed models, the target deck and one unrelated studied
        // note so that every checkpoint has real scheduling to cover.
        for managed in [model::vocabulary(), model::grammar()] {
            let id = lab.call("labTest.installModel", json!({"manifest": managed}));
            for kind in ["observed", "managed"] {
                lab.call(
                    "labTest.registerDigest",
                    json!({"model_id": id, "digest": manifest_digest(&managed).unwrap(), "kind": kind}),
                );
            }
        }
        for deck in ["Japanese::Vocab", "Japanese::Grammar", "English::Grammar"] {
            lab.call("labTest.createDeck", json!({"name": deck}));
        }
        let anchor = lab.call(
            "labTest.addNote",
            json!({"model": "Basic", "deck": "Default", "fields": {"Front": "anchor", "Back": "unrelated"}}),
        );
        lab.call("labTest.study", json!({"note_id": anchor}));
        Self {
            root,
            lab,
            purpose,
            target_deck,
            log: vec![],
        }
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    /// Run the CLI against the disposable collection; returns exit code and JSON stdout.
    fn cli(&mut self, args: &[&str]) -> (i32, Value, String) {
        let target = self
            .target_deck
            .map(|deck| {
                vec![
                    "--set".to_owned(),
                    format!("purposes.{}.target_deck={deck}", self.purpose),
                ]
            })
            .unwrap_or_default();
        let output = Command::new(BIN)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .args(["--output", "json", "--offline"])
            .args(["--set", &format!("anki.endpoint={}", self.lab.endpoint)])
            .args([
                "--set",
                &format!("storage.state_dir={}", self.state().display()),
            ])
            .args([
                "--set",
                "dictionary.provider=authored",
                "--set",
                "llm.enabled=false",
            ])
            .args([
                "--set",
                "images.search_when_missing=false",
                "--set",
                "kanji.enabled=false",
            ])
            .args(["--purpose", self.purpose])
            .args(&target)
            .args(args)
            .output()
            .unwrap();
        let code = output.status.code().unwrap_or(-1);
        self.log.push(format!("{} -> {code}", args.join(" ")));
        let stdout = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        (
            code,
            stdout,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
    fn ok(&mut self, args: &[&str]) -> Value {
        let (code, value, stderr) = self.cli(args);
        assert_eq!(code, 0, "{args:?}: {stderr} {value}");
        value
    }
    fn store(&self) -> Store {
        Store::open(&self.state()).unwrap()
    }
    /// Harness step: publish a child revision bound to this collection.
    fn bind(&mut self, plan: Uuid, documents: Option<Vec<LearningDocument>>) -> (u32, String) {
        let mut store = self.store();
        let latest = store.latest_revision(plan).unwrap();
        let base = store.revision(plan, latest).unwrap();
        let mut child: PlanRevision = base.clone();
        child.revision = latest + 1;
        child.parent_digest = Some(base.approval_digest().unwrap());
        child.binding = Some(self.lab.binding());
        if let Some(documents) = documents {
            child.rendered = documents
                .iter()
                .map(|doc| {
                    let fields = doc
                        .sources
                        .first()
                        .map(|s| s.fields.clone())
                        .unwrap_or_default();
                    render::render(doc, &fields).unwrap()
                })
                .collect();
            child.documents = documents;
            child.review_decisions.clear();
        }
        let digest = store.publish_revision(&child).unwrap();
        self.log.push(format!(
            "harness: bound revision {} of {plan}",
            child.revision
        ));
        (child.revision, digest)
    }
    fn approve(&mut self, plan: Uuid, revision: u32, digest: &str, warnings: &[&str]) -> Uuid {
        let rev = revision.to_string();
        self.ok(&["plans", "validate", &plan.to_string(), "--revision", &rev]);
        let mut args = vec![
            "plans",
            "approve",
            "",
            "--revision",
            &rev,
            "--digest",
            digest,
            "--actor",
            "release-check",
        ];
        let plan_text = plan.to_string();
        args[2] = &plan_text;
        for warning in warnings {
            args.extend(["--accept-warning", warning]);
        }
        let approved = self.ok(&args);
        approved["receipt"]["id"].as_str().unwrap().parse().unwrap()
    }
    fn writer(&self, store: &mut Store) -> LeaseToken {
        store
            .acquire_lease(
                &Resource::CollectionWriter(self.lab.binding().lineage_id),
                600,
            )
            .unwrap()
    }
    fn checkpoint(&mut self, store: &mut Store, token: &LeaseToken) -> Uuid {
        let scope: ScopeManifest =
            serde_json::from_value(self.lab.call("labTest.scope", json!({}))).unwrap();
        let tag = Uuid::new_v4();
        let restore_target = self.root.join(format!("restore-{tag}"));
        std::fs::create_dir_all(&restore_target).unwrap();
        let outcome = create_checkpoint(
            store,
            token,
            &mut LabExporter(&self.lab),
            CheckpointRequest {
                binding: self.lab.binding(),
                scope,
                preference: ScopePreference::Collection,
                output: self.root.join("out").join(format!("{tag}.colpkg")),
                group_id: None,
                protected_manifest_digest: PROTECTED.into(),
                restore_target,
                limits: PackageLimits {
                    max_package_bytes: 256 << 20,
                    max_collection_bytes: 256 << 20,
                    max_media_bytes: 64 << 20,
                    max_media_map_bytes: 1 << 20,
                    max_entries: 10000,
                    timeout: Duration::from_secs(60),
                    scratch_dir: self.root.join("scratch"),
                },
                now_ms: now_ms(),
            },
        )
        .map_err(|e| format!("{}: {:?}", e.code, e))
        .unwrap();
        self.log
            .push("harness: verified real .colpkg checkpoint".into());
        outcome.record.receipt.id
    }
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        plan: Uuid,
        revision: u32,
        digest: &str,
        item: Uuid,
        approval: Uuid,
        schema: bool,
    ) -> linguist_application::apply::ApplyItemOutcome {
        let mut store = self.store();
        let token = self.writer(&mut store);
        let checkpoint = self.checkpoint(&mut store, &token);
        let mut port = LabPort {
            lab: &self.lab,
            state: self.state(),
        };
        let outcome = apply_item(
            &mut store,
            &token,
            &mut port,
            &ApplyRequest {
                apply: true,
                plan_id: plan,
                revision,
                digest,
                item_id: item,
                approval_id: approval,
                checkpoint_id: checkpoint,
                group_id: None,
                protected_manifest_digest: PROTECTED,
                reuse_max_age_seconds: 600,
                max_package_bytes: 256 << 20,
                max_media_bytes: 64 << 20,
                accept_schema_change: schema,
                now_ms: now_ms(),
            },
        )
        .unwrap();
        store.release_lease(&token).ok();
        self.log
            .push(format!("harness: apply_item -> {:?}", outcome.state));
        outcome
    }
    fn restore(
        &mut self,
        snapshot: Uuid,
        edit: impl FnOnce(&mut RestoreDecision, &linguist_application::restore::RestorePlan),
    ) -> Result<linguist_application::restore::RestoreOutcome, String> {
        let mut store = self.store();
        let preview = plan_restore(
            &store,
            &mut LabPort {
                lab: &self.lab,
                state: self.state(),
            },
            snapshot,
            None,
        )?;
        let mut decision = RestoreDecision {
            schema_version: 1,
            snapshot_id: snapshot,
            observed_state_digest: preview.observed_state_digest.clone(),
            actor: "release-check".into(),
            fields: BTreeMap::new(),
            decks: BTreeMap::new(),
            remove_unstudied_cards: vec![],
            delete_created_notes: vec![],
            accept_missing_media: vec![],
            accept_schema_change: false,
        };
        edit(&mut decision, &preview);
        let token = self.writer(&mut store);
        let checkpoint = self.checkpoint(&mut store, &token);
        let mut port = LabPort {
            lab: &self.lab,
            state: self.state(),
        };
        let result = restore(
            &mut store,
            &token,
            &mut port,
            &RestoreRequest {
                apply: true,
                snapshot_id: snapshot,
                decision: Some(decision),
                checkpoint_id: checkpoint,
                group_id: None,
                protected_manifest_digest: PROTECTED,
                reuse_max_age_seconds: 600,
                max_package_bytes: 256 << 20,
                max_media_bytes: 64 << 20,
                now_ms: now_ms(),
            },
        );
        store.release_lease(&token).ok();
        self.log.push(format!(
            "harness: restore -> {:?}",
            result.as_ref().map(|o| o.state)
        ));
        result
    }
    /// CLI inspection that must work after the effects.
    fn inspect_after(&mut self, plan: Uuid, snapshot: Uuid) {
        let listed = self.ok(&["snapshots", "list"]);
        assert!(
            listed.to_string().contains(&snapshot.to_string()),
            "{listed}"
        );
        self.ok(&["snapshots", "show", &snapshot.to_string()]);
        let pending = self.ok(&["recover", "inspect", "--pending"]);
        assert!(
            !pending.to_string().contains("request_started"),
            "{pending}"
        );
        self.ok(&["plans", "show", &plan.to_string()]);
    }
    fn report(&self, name: &str) {
        let path = std::env::var("LAB_SCENARIO_REPORT_DIR")
            .map(PathBuf::from)
            .ok();
        let text = format!("{name}\n{}\n", self.log.join("\n"));
        println!("{text}");
        if let Some(dir) = path {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{name}.txt")), text).unwrap();
        }
    }
}
impl Drop for Scenario {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn plan_of(value: &Value) -> (Uuid, String, Uuid) {
    let root = if value["plan_id"].is_string() {
        value
    } else {
        &value["result"]
    };
    (
        root["plan_id"].as_str().unwrap().parse().unwrap(),
        root["digest"].as_str().unwrap().to_owned(),
        root["document_id"]
            .as_str()
            .or(root["items"][0]["document_id"].as_str())
            .unwrap()
            .parse()
            .unwrap(),
    )
}

fn assert_preview_ready(s: &mut Scenario, plan: Uuid, revision: u32, schema: bool) {
    let rev = revision.to_string();
    let plan_text = plan.to_string();
    let mut args = vec!["apply", plan_text.as_str(), "--revision", rev.as_str()];
    if schema {
        args.push("--accept-schema-change");
    }
    let preview = s.ok(&args);
    assert_eq!(preview["blocked_items"], 0, "{preview}");
    assert_eq!(preview["collection_writes_enabled"], false);
    args.push("--apply");
    let (code, _, stderr) = s.cli(&args);
    assert_eq!(code, 3, "{stderr}");
    assert!(stderr.contains("CAPABILITY_UNAVAILABLE"), "{stderr}");
}

// ---------------------------------------------------------------- scenarios

#[test]
#[ignore = "needs Anki's Python; run with --ignored (scripts/release-check.py does)"]
fn vocab_add_prepare_review_apply_restore() {
    let mut s = Scenario::new("vocab-add", "japanese_vocab", Some("Japanese::Vocab"));
    let added = s.ok(&[
        "vocab",
        "add",
        "--expression",
        "食べる",
        "--meaning",
        "to eat",
        "--sense-key",
        "eat",
        "--target-language",
        "ja",
        "--reading",
        "たべる",
        "--tag",
        "release",
    ]);
    let (plan, digest, item) = plan_of(&added);
    assert_eq!(added["ready"], true);
    // Unbound plans are review/export only.
    let (code, preview, _) = s.cli(&["apply", &plan.to_string(), "--revision", "1"]);
    assert_eq!(code, 4);
    assert!(preview.to_string().contains("APPLY_BINDING_WEAK"));
    let _ = digest;
    let (revision, digest) = s.bind(plan, None);
    let approval = s.approve(plan, revision, &digest, &[]);
    assert_preview_ready(&mut s, plan, revision, false);
    let outcome = s.apply(plan, revision, &digest, item, approval, false);
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note = s.lab.note(outcome.note_id.unwrap()).unwrap();
    assert_eq!(note.model_name, "Linguist Vocabulary v2");
    assert_eq!(note.fields["Expression"], "食べる");
    assert!(note.tags.iter().any(|t| t.starts_with("lab_op_")));
    assert_eq!(note.cards.len(), 1);
    assert_eq!(note.cards[0].review_count, 0);
    let target: ObservedDeck = serde_json::from_value(
        s.lab
            .call("labTest.deck", json!({"name": "Japanese::Vocab"})),
    )
    .unwrap();
    assert_eq!(note.cards[0].deck_id, target.id);
    let snapshot = outcome.snapshot_id.unwrap();
    s.inspect_after(plan, snapshot);
    // Restore: the created note is unchanged and unstudied, so it may go.
    let note_id = note.id;
    let restored = s
        .restore(snapshot, |decision, preview| {
            assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
            decision.delete_created_notes = vec![note_id];
        })
        .unwrap();
    assert_eq!(
        restored.state,
        Some(OperationState::Committed),
        "{:?}",
        restored.issues
    );
    assert_eq!(restored.target_state, OperationState::Restored);
    assert!(s.lab.note(note_id).is_none());
    s.ok(&["snapshots", "show", &snapshot.to_string()]);
    s.report("vocab_add");
}

#[test]
#[ignore = "needs Anki's Python; run with --ignored (scripts/release-check.py does)"]
fn grammar_add_prepare_review_apply_study_restore_keeps_history() {
    let mut s = Scenario::new("grammar-add", "japanese_grammar", Some("Japanese::Grammar"));
    let document = s.root.join("grammar.json");
    std::fs::write(
        &document,
        serde_json::to_vec(&json!({
            "schema_version": 2, "kind": "grammar", "target_language": "ja",
            "explanation_language": "vi",
            "body": {"pattern": "〜てもいい", "use_key": "permission", "meaning": "được phép",
                     "formation": "V-て + もいい", "recognition_prompt": "Mẫu này nghĩa là gì?",
                     "examples": [{"sentence": "ここで写真を撮ってもいいですか。",
                                   "translation": "Tôi chụp ảnh ở đây được không?", "provenance": "user"}]}
        }))
        .unwrap(),
    )
    .unwrap();
    let added = s.ok(&["grammar", "add", "--document", document.to_str().unwrap()]);
    let (plan, _, item) = plan_of(&added);
    let (revision, digest) = s.bind(plan, None);
    let approval = s.approve(plan, revision, &digest, &[]);
    assert_preview_ready(&mut s, plan, revision, false);
    let outcome = s.apply(plan, revision, &digest, item, approval, false);
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note_id = outcome.note_id.unwrap();
    let note = s.lab.note(note_id).unwrap();
    assert_eq!(note.model_name, "Linguist Grammar v2");
    assert!(
        note.fields["Meaning"].contains("được phép"),
        "{:?}",
        note.fields
    );
    // Normal study after apply.
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    let studied = s.lab.note(note_id).unwrap();
    assert_eq!(studied.cards[0].review_count, 1);
    let snapshot = outcome.snapshot_id.unwrap();
    s.inspect_after(plan, snapshot);
    // A studied created note is never deleted; restore refuses to remove it.
    let refused = s.restore(snapshot, |decision, _| {
        decision.delete_created_notes = vec![note_id];
    });
    let kept = s.lab.note(note_id).unwrap();
    assert_eq!(
        kept.cards, studied.cards,
        "history and scheduling untouched"
    );
    let error = refused.expect_err("a studied created note must not be removed");
    assert!(error.contains("RESTORE_CREATED_NOTE_STUDIED"), "{error}");
    s.log.push(format!("restore refused as required: {error}"));
    s.report("grammar_add");
}

/// Harness-authored reviewed migration document from the CLI's captured
/// source: authored content, Basic ordinal 0 mapped to the first task.
fn migration_document(
    draft: &LearningDocument,
    content: Value,
    task: Task,
    kind: TargetModelKind,
) -> LearningDocument {
    let source = draft.sources[0].clone();
    let mut doc = LearningDocument::from_json(
        &serde_json::to_vec(&json!({
            "schema_version": 2, "id": Uuid::new_v4(), "target_language": draft.target_language,
            "explanation_language": draft.explanation_language, "content": content,
            "requested_tasks": [task], "tags": ["release"],
        }))
        .unwrap(),
    )
    .unwrap();
    doc.task_maps.push(SourceTaskMap {
        schema_version: 1,
        source_id: source.id,
        source_model_digest: source.model_manifest.clone(),
        target_model: kind,
        entries: vec![SourceTaskMapEntry {
            source_ordinal: 0,
            target_task: task,
            target_ordinal: 0,
        }],
    });
    doc.sources.push(source);
    doc.archives.push(draft.archives[0].clone());
    doc.issues = linguist_core::validation::validate(&doc);
    assert!(
        doc.issues
            .iter()
            .all(|i| i.severity == linguist_core::validation::Severity::Warning),
        "{:?}",
        doc.issues
    );
    doc
}

#[allow(clippy::too_many_arguments)]
fn revamp_scenario(
    name: &str,
    purpose: &'static str,
    command: &str,
    target_deck: Option<&'static str>,
    edit_after_apply: Option<&str>,
    source: (&str, &str),
    content: Value,
    task: Task,
    kind: TargetModelKind,
    expected_model: &str,
) {
    let mut s = Scenario::new(name, purpose, target_deck);
    // FSRS on: memory state is part of the scheduling that must survive.
    s.lab
        .call("labTest.setConfig", json!({"key": "fsrs", "value": true}));
    let note_id = s.lab.call(
        "labTest.addNote",
        json!({"model": "Basic", "deck": "Default", "fields": {"Front": source.0, "Back": source.1}, "tags": ["legacy"]}),
    );
    let note_id = note_id.as_i64().unwrap();
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    let before = s.lab.note(note_id).unwrap();
    assert_eq!(before.cards[0].review_count, 2);
    assert_ne!(
        before.cards[0].scheduler["memory_state"], "None",
        "FSRS memory state present"
    );
    let fields = format!(
        "purposes.{purpose}.fields={}",
        json!({if kind == TargetModelKind::Vocabulary { "expression" } else { "pattern" }: "Front", "meaning": "Back"})
    );
    let model = format!("purposes.{purpose}.source_model=Basic");
    let (code, drafted, stderr) = s.cli(&[
        "--set",
        &fields,
        "--set",
        &model,
        command,
        "revamp",
        "--note-id",
        &note_id.to_string(),
    ]);
    assert_eq!(code, 4, "{stderr}");
    let (plan, _, _) = plan_of(&drafted);
    // The shipped review path stops here: native history is unresolvable.
    let issues = s.ok(&["plans", "show", &plan.to_string(), "--issues-only"]);
    assert!(
        issues.to_string().contains("SOURCE_NATIVE_HISTORY_REVIEW"),
        "{issues}"
    );
    let draft = s.store().revision(plan, 1).unwrap().documents[0].clone();
    // The capture archived the exact original fields.
    assert_eq!(draft.archives[0].original_fields, before.fields);
    // The lab reports the Basic note type with the digest the capture recorded.
    s.lab.call(
        "labTest.registerDigest",
        json!({"model_id": before.model_id, "digest": draft.sources[0].model_manifest}),
    );
    let reviewed = migration_document(&draft, content, task, kind);
    let item = reviewed.id;
    s.log
        .push("harness: reviewed migration document from the CLI capture".into());
    let (revision, digest) = s.bind(plan, Some(vec![reviewed]));
    let (code, validated, stderr) = s.cli(&[
        "plans",
        "validate",
        &plan.to_string(),
        "--revision",
        &revision.to_string(),
    ]);
    assert_eq!(code, 0, "{stderr} {validated}");
    let warnings: Vec<String> = validated["evidence"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|i| i["issues"].as_array().unwrap().iter())
        .filter(|i| i["severity"] == "warning")
        .map(|i| i["code"].as_str().unwrap().to_owned())
        .collect();
    let warnings: Vec<&str> = warnings.iter().map(String::as_str).collect();
    let approval = s.approve(plan, revision, &digest, &warnings);
    assert_preview_ready(&mut s, plan, revision, true);
    let outcome = s.apply(plan, revision, &digest, item, approval, true);
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let after = s.lab.note(note_id).unwrap();
    assert_eq!(after.model_name, expected_model);
    // History preservation: same card ID, scheduling and review log.
    assert_eq!(after.cards.len(), 1);
    assert_eq!(after.cards[0].id, before.cards[0].id);
    assert_eq!(
        after.cards[0].history_digest,
        before.cards[0].history_digest
    );
    assert_eq!(after.cards[0].review_count, 2);
    assert_eq!(after.cards[0].scheduler, before.cards[0].scheduler);
    match target_deck {
        Some(deck) => {
            let target: ObservedDeck =
                serde_json::from_value(s.lab.call("labTest.deck", json!({"name": deck}))).unwrap();
            assert_eq!(after.cards[0].deck_id, target.id);
        }
        // No target mapping: the retained card stays in its home deck.
        None => assert_eq!(after.cards[0].deck_id, before.cards[0].deck_id),
    }
    let snapshot = outcome.snapshot_id.unwrap();
    s.inspect_after(plan, snapshot);
    if let Some(field) = edit_after_apply {
        // A personal edit after apply conflicts until a decision names it.
        s.lab.call(
            "labTest.editField",
            json!({"note_id": note_id, "field": field, "value": "my own later note"}),
        );
        let mut port = LabPort {
            lab: &s.lab,
            state: s.state(),
        };
        let preview = plan_restore(&s.store(), &mut port, snapshot, None).unwrap();
        let conflict = format!("RESTORE_FIELD_CONFLICT:{field}");
        assert!(
            preview.conflicts.contains(&conflict),
            "{:?}",
            preview.conflicts
        );
        let refused = s.restore(snapshot, |decision, _| {
            decision.accept_schema_change = true;
        });
        assert!(
            refused.is_err() || refused.as_ref().unwrap().target_state != OperationState::Restored
        );
        s.log.push(format!(
            "restore without a field decision refused: {conflict}"
        ));
    }
    // Later study, then restore: content returns, the later review stays.
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    let studied = s.lab.note(note_id).unwrap();
    assert_eq!(studied.cards[0].review_count, 3);
    let restored = s
        .restore(snapshot, |decision, preview| {
            assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
            decision.accept_schema_change = preview.model.is_some();
            if let Some(field) = edit_after_apply {
                decision.fields.insert(
                    field.to_owned(),
                    linguist_application::restore::FieldChoice::Original,
                );
            }
        })
        .unwrap();
    assert_eq!(
        restored.state,
        Some(OperationState::Committed),
        "{:?}",
        restored.issues
    );
    assert_eq!(restored.target_state, OperationState::Restored);
    let back = s.lab.note(note_id).unwrap();
    assert_eq!(back.model_name, "Basic");
    assert_eq!(back.fields, before.fields);
    assert_eq!(back.cards[0].id, before.cards[0].id);
    assert_eq!(back.cards[0].review_count, 3);
    assert_eq!(
        back.cards[0].history_digest,
        studied.cards[0].history_digest
    );
    assert_eq!(back.cards[0].scheduler, studied.cards[0].scheduler);
    s.report(name);
}

#[test]
#[ignore = "needs Anki's Python; run with --ignored (scripts/release-check.py does)"]
fn vocab_revamp_capture_migrate_study_restore() {
    revamp_scenario(
        "vocab_revamp",
        "japanese_vocab",
        "vocab",
        Some("Japanese::Vocab"),
        None,
        ("食べる", "to consume"),
        json!({"kind": "vocabulary", "body": {"expression": "食べる", "reading": "たべる",
               "meaning": "to eat", "sense_key": "eat-food", "examples": []}}),
        Task::Comprehension,
        TargetModelKind::Vocabulary,
        "Linguist Vocabulary v2",
    );
}

#[test]
#[ignore = "needs Anki's Python; run with --ignored (scripts/release-check.py does)"]
fn grammar_revamp_capture_migrate_study_restore() {
    revamp_scenario(
        "grammar_revamp",
        "english_grammar",
        "grammar",
        None,
        Some("Meaning"),
        ("used to + V", "past habit"),
        json!({"kind": "grammar", "body": {"pattern": "used to + V", "use_key": "past-habit",
               "meaning": "a past habit or state", "formation": "used to + base verb",
               "recognition_prompt": "Which structure is used here?",
               "examples": [{"sentence": "I used to live in Hanoi.", "translation": "I lived in Hanoi before.",
                             "provenance": "user"}]}}),
        Task::Recognition,
        TargetModelKind::Grammar,
        "Linguist Grammar v2",
    );
}

fn grammar_unit(pattern: &str, key: &str, meaning: &str) -> Value {
    json!({"pattern": pattern, "use_key": key, "meaning": meaning, "formation": "V-て + も",
           "recognition_prompt": "Mẫu này thể hiện quan hệ gì?", "usage": "",
           "exercise_prompt": "", "exercise_answer": "",
           "examples": [{"sentence": format!("雨が降{pattern}行きます。"),
                         "translation": "Ví dụ.", "provenance": "user", "evidence_ids": []}]})
}

/// EV-09: a managed grammar note holding two patterns is captured by the CLI,
/// split by the CLI into an anchor and a fresh sibling, reviewed with CLI
/// decisions, applied (sibling first, anchor last) and rolled back as a group.
#[test]
#[ignore = "needs Anki's Python; run with --ignored (scripts/release-check.py does)"]
fn grammar_revamp_multi_unit_split_apply_rollback() {
    use linguist_application::{
        restore::{plan_group_rollback, rollback_group},
        split::{SplitApplyRequest, apply_group},
    };
    let mut s = Scenario::new(
        "grammar-split",
        "japanese_grammar",
        Some("Japanese::Grammar"),
    );
    let grammar = model::grammar();
    let mut fields: BTreeMap<String, String> = grammar
        .fields
        .iter()
        .map(|f| (f.clone(), String::new()))
        .collect();
    fields.insert("Pattern".into(), "〜ても / 〜てもいい".into());
    fields.insert("Meaning".into(), "dù / được phép".into());
    fields.insert("Formation".into(), "V-て + も".into());
    fields.insert("Language".into(), "ja".into());
    let note_id = s
        .lab
        .call(
            "labTest.addNote",
            json!({"model": grammar.name, "deck": "Japanese::Grammar", "fields": fields, "tags": ["legacy"]}),
        )
        .as_i64()
        .unwrap();
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    s.lab.call("labTest.study", json!({"note_id": note_id}));
    let before = s.lab.note(note_id).unwrap();
    let mapping = format!(
        "purposes.japanese_grammar.fields={}",
        json!({"pattern": "Pattern", "meaning": "Meaning", "formation": "Formation", "language": "Language"})
    );
    let model_setting = format!("purposes.japanese_grammar.source_model={}", grammar.name);
    let (code, drafted, stderr) = s.cli(&[
        "--set",
        &mapping,
        "--set",
        &model_setting,
        "grammar",
        "revamp",
        "--note-id",
        &note_id.to_string(),
    ]);
    assert_eq!(code, 4, "{stderr}");
    let (plan, _, _) = plan_of(&drafted);
    let draft = s.store().revision(plan, 1).unwrap().documents[0].clone();
    assert_eq!(draft.archives[0].original_fields, before.fields);
    // Observed digest of the managed grammar type as the capture recorded it.
    s.lab.call(
        "labTest.registerDigest",
        json!({"model_id": before.model_id, "digest": draft.sources[0].model_manifest, "kind": "observed"}),
    );
    // Harness stand-in for the unresolvable native-history review: the
    // reviewed combined document over the captured source.
    let mut base_doc = LearningDocument::from_json(
        &serde_json::to_vec(&json!({
            "schema_version": 2, "id": Uuid::new_v4(), "target_language": "ja",
            "explanation_language": "vi",
            "content": {"kind": "grammar", "body": grammar_unit("〜ても / 〜てもいい", "combined", "dù / được phép")},
            "requested_tasks": ["recognition"], "tags": ["release"],
        }))
        .unwrap(),
    )
    .unwrap();
    base_doc.sources.push(draft.sources[0].clone());
    base_doc.archives.push(draft.archives[0].clone());
    base_doc.issues = linguist_core::validation::validate(&base_doc);
    let (base_revision, base_digest) = s.bind(plan, Some(vec![base_doc.clone()]));
    s.log
        .push("harness: reviewed combined grammar document from the CLI capture".into());
    // CLI split into an anchor (first unit keeps the note) and a sibling.
    let request = json!({
        "schema_version": 2, "base_revision": base_revision, "base_digest": base_digest,
        "document_id": base_doc.id, "input_digest": base_doc.semantic_digest().unwrap(),
        "actor": "release-check", "anchor_index": 0,
        "units": [grammar_unit("〜ても", "concession", "dù"), grammar_unit("〜てもいい", "permission", "được phép")],
    });
    let request_path = s.root.join("split.json");
    std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let (code, split, stderr) = s.cli(&[
        "plans",
        "split-grammar",
        &plan.to_string(),
        "--request",
        request_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{stderr} {split}");
    let group: Uuid = split["grammar_groups"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let anchor: Uuid = split["grammar_groups"][0]["anchor_document"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // CLI review: each unit accepts the native split plan naming the anchor.
    loop {
        let store = s.store();
        let latest = store.latest_revision(plan).unwrap();
        let revision = store.revision(plan, latest).unwrap();
        drop(store);
        let pending = revision.documents.iter().find_map(|doc| {
            linguist_core::validation::validate(doc)
                .into_iter()
                .find(|i| i.code == "GRAMMAR_SPLIT_NATIVE_REVIEW")
                .map(|issue| (doc.clone(), issue))
        });
        let Some((doc, issue)) = pending else { break };
        let decision = json!({
            "schema_version": 2, "base_revision": latest, "base_digest": revision.approval_digest().unwrap(),
            "document_id": doc.id, "issue_id": issue.id, "input_digest": doc.semantic_digest().unwrap(),
            "actor": "release-check", "choice": linguist_core::records::ReviewChoice::Anchor(anchor),
        });
        let path = s.root.join(format!("decision-{latest}.json"));
        std::fs::write(&path, serde_json::to_vec(&decision).unwrap()).unwrap();
        let (code, _, stderr) = s.cli(&[
            "plans",
            "resolve",
            &plan.to_string(),
            &issue.id,
            "--decision",
            path.to_str().unwrap(),
        ]);
        assert!(code == 0 || code == 4, "{stderr}");
    }
    let (revision, digest) = s.bind(plan, None);
    let (code, validated, stderr) = s.cli(&[
        "plans",
        "validate",
        &plan.to_string(),
        "--revision",
        &revision.to_string(),
    ]);
    assert_eq!(code, 0, "{stderr} {validated}");
    let mut warnings: Vec<String> = validated["evidence"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|i| i["issues"].as_array().unwrap().iter())
        .filter(|i| i["severity"] == "warning")
        .map(|i| i["code"].as_str().unwrap().to_owned())
        .collect();
    warnings.sort();
    warnings.dedup();
    let warnings: Vec<&str> = warnings.iter().map(String::as_str).collect();
    let approval = s.approve(plan, revision, &digest, &warnings);
    let rev = revision.to_string();
    let group_text = group.to_string();
    let preview = s.ok(&[
        "apply",
        &plan.to_string(),
        "--revision",
        &rev,
        "--split-group",
        &group_text,
    ]);
    assert_eq!(preview["collection_writes_enabled"], false, "{preview}");
    let (code, _, stderr) = s.cli(&[
        "apply",
        &plan.to_string(),
        "--revision",
        &rev,
        "--split-group",
        &group_text,
        "--apply",
    ]);
    assert_eq!(code, 3, "{stderr}");
    // Harness transport: ALG-SPLIT over the real collection.
    let mut store = s.store();
    let token = s.writer(&mut store);
    let checkpoint = s.checkpoint(&mut store, &token);
    let outcome = apply_group(
        &mut store,
        &token,
        &mut LabPort {
            lab: &s.lab,
            state: s.state(),
        },
        &SplitApplyRequest {
            apply: true,
            plan_id: plan,
            revision,
            digest: &digest,
            grammar_group: group,
            approval_id: approval,
            checkpoint_id: checkpoint,
            protected_manifest_digest: PROTECTED,
            reuse_max_age_seconds: 600,
            max_package_bytes: 256 << 20,
            max_media_bytes: 64 << 20,
            accept_schema_change: false,
            now_ms: now_ms(),
            new_execution_id: None,
        },
    )
    .unwrap();
    assert_eq!(outcome.state, "complete", "{outcome:?}");
    let dispatched: Vec<String> =
        serde_json::from_value(s.lab.call("labTest.dispatched", json!({}))).unwrap();
    assert_eq!(
        dispatched,
        ["create_note", "update_note"],
        "sibling first, anchor last"
    );
    let anchor_after = s.lab.note(note_id).unwrap();
    assert!(anchor_after.fields["Pattern"].contains("〜ても"));
    assert!(!anchor_after.fields["Pattern"].contains("〜てもいい"));
    assert_eq!(anchor_after.cards[0].id, before.cards[0].id);
    assert_eq!(
        anchor_after.cards[0].history_digest,
        before.cards[0].history_digest
    );
    assert_eq!(anchor_after.cards[0].review_count, 2);
    let sibling = outcome
        .units
        .iter()
        .find(|u| u.role != "anchor")
        .unwrap()
        .note_id
        .unwrap();
    let sibling_note = s.lab.note(sibling).unwrap();
    assert!(sibling_note.fields["Pattern"].contains("〜てもいい"));
    assert_eq!(sibling_note.cards[0].review_count, 0);
    s.log.push(format!(
        "harness: split group {group} complete ({dispatched:?})"
    ));
    // Rollback: the anchor's content returns with its history; the unstudied
    // sibling is removed only because the decision lists it.
    let mut port = LabPort {
        lab: &s.lab,
        state: s.state(),
    };
    let items = plan_group_rollback(&store, &mut port, outcome.execution_id).unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
    let checkpoint = s.checkpoint(&mut store, &token);
    let requests: Vec<RestoreRequest> = items
        .iter()
        .map(|item| {
            let plan = item.plan.clone().unwrap();
            RestoreRequest {
                apply: true,
                snapshot_id: item.snapshot_id,
                decision: Some(RestoreDecision {
                    schema_version: 1,
                    snapshot_id: item.snapshot_id,
                    observed_state_digest: plan.observed_state_digest.clone(),
                    actor: "release-check".into(),
                    fields: BTreeMap::new(),
                    decks: BTreeMap::new(),
                    remove_unstudied_cards: vec![],
                    delete_created_notes: plan.created_notes.iter().map(|n| n.note_id).collect(),
                    accept_missing_media: vec![],
                    accept_schema_change: false,
                }),
                checkpoint_id: checkpoint,
                group_id: None,
                protected_manifest_digest: PROTECTED,
                reuse_max_age_seconds: 600,
                max_package_bytes: 256 << 20,
                max_media_bytes: 64 << 20,
                now_ms: now_ms(),
            }
        })
        .collect();
    let mut port = LabPort {
        lab: &s.lab,
        state: s.state(),
    };
    let results = rollback_group(
        &mut store,
        &token,
        &mut port,
        outcome.execution_id,
        &requests,
    )
    .unwrap();
    for result in &results {
        let outcome = result.as_ref().unwrap();
        assert_eq!(
            outcome.target_state,
            OperationState::Restored,
            "{outcome:?}"
        );
    }
    store.release_lease(&token).ok();
    let restored = s.lab.note(note_id).unwrap();
    assert_eq!(restored.fields, before.fields);
    assert_eq!(restored.cards[0].id, before.cards[0].id);
    assert_eq!(restored.cards[0].review_count, 2);
    assert!(s.lab.note(sibling).is_none());
    s.log.push(
        "harness: group rollback restored the anchor and removed the unstudied sibling".into(),
    );
    s.report("grammar_split");
}

#[test]
fn base64_round_trips() {
    for bytes in [&b""[..], b"a", b"ab", b"abc", "食べる".as_bytes()] {
        assert_eq!(base64_decode(&base64_encode(bytes)), bytes);
    }
    assert_eq!(base64_encode(b"abc"), "YWJj");
    let _ = canonical::FORMAT;
}
