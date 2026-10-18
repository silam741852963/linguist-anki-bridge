//! WP-09: regeneration ownership, snapshot export, editor drafts, summaries.
use linguist_application::{
    DictionaryPort, Kind, editor,
    export::export_snapshot,
    prepare_with_providers,
    regenerate::{Stage, prepare_item, preview, regenerate},
    review::summarize,
    vocab::{KanjiPort, Providers},
};
use linguist_config::{ConfigFile, Effective, Registry, ResolveOptions, expand_path};
use linguist_core::{
    FieldIntent, Language, LearningContent, Provenance, canonical,
    records::{Evidence, PlanRevision, ReviewChoice, SourceArchive, SourceRecord},
    review::{ResolutionRequest, resolve},
};
use linguist_dictionary::kanji::KanjiEntry;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

struct Fixture {
    root: std::path::PathBuf,
    environment: BTreeMap<String, String>,
    settings: Effective,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-review-{}", uuid::Uuid::new_v4()));
        let environment = BTreeMap::from([("HOME".into(), root.to_str().unwrap().into())]);
        let mut settings = linguist_config::resolve(
            &Registry::builtin(),
            &ConfigFile::default(),
            &ResolveOptions {
                environment: environment.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        for (key, value) in [
            ("llm.enabled", json!(false)),
            ("dictionary.provider", json!("authored")),
            ("images.search_when_missing", json!(false)),
            ("kanji.enabled", json!(true)),
            ("audio.provider", json!("preserve")),
        ] {
            settings.values.insert(key.into(), value);
        }
        Self {
            root,
            environment,
            settings,
        }
    }
    fn state(&self) -> std::path::PathBuf {
        expand_path(
            self.settings.values["storage.state_dir"].as_str().unwrap(),
            &self.environment,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Kanji(&'static str);
impl KanjiPort for Kanji {
    fn lookup(&self, character: char) -> Result<Option<KanjiEntry>, String> {
        let raw = format!("<p>{character}{}</p>", self.0).into_bytes();
        Ok(Some(KanjiEntry {
            character: character.to_string(),
            meanings: vec![self.0.into()],
            kun_readings: vec![],
            on_readings: vec![],
            strokes: None,
            radical: None,
            parts: vec![],
            grade: None,
            jlpt: None,
            frequency: None,
            schema: linguist_dictionary::kanji::SCHEMA,
            source_url: "https://jisho.org/search/x".into(),
            raw_digest: String::new(),
            fetched_at: 1,
            from_cache: false,
            raw_bytes: raw,
        }))
    }
    fn stroke_order(&self, character: char) -> Result<Option<Vec<u8>>, String> {
        Ok(Some(format!("GIF89a{character}{}", self.0).into_bytes()))
    }
}

fn japanese() -> Vec<u8> {
    serde_json::to_vec(&json!({"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en",
        "requested_tasks":["comprehension"],
        "body":{"expression":"食","meaning":"food","sense_key":"food","reading":"しょく"}}))
    .unwrap()
}

fn after_count(f: &Fixture, id: uuid::Uuid) -> u32 {
    linguist_store::Store::read_only(&f.state())
        .unwrap()
        .latest_revision(id)
        .unwrap()
}

fn latest(f: &Fixture, id: uuid::Uuid) -> PlanRevision {
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    store
        .revision(id, store.latest_revision(id).unwrap())
        .unwrap()
}

#[test]
fn enrichment_regeneration_replaces_stage_output_but_protects_edits() {
    let f = Fixture::new();
    let result = prepare_with_providers(
        &japanese(),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        Providers {
            kanji: Some(&Kanji("old")),
            ..Default::default()
        },
    )
    .unwrap();
    let base = latest(&f, result.plan_id);
    let LearningContent::Vocabulary(vocab) = &base.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.kanji_details[0].meanings, ["old"]);
    let old_stroke = vocab.kanji_details[0].stroke_digest.clone().unwrap();
    assert!(
        base.documents[0]
            .media
            .iter()
            .any(|m| m.digest == old_stroke
                && m.role == linguist_core::records::MediaRole::KanjiStroke
                && m.mime == "image/gif")
    );
    assert!(base.rendered[0].fields["Kanji"].contains(&format!("lab_stroke_{old_stroke}.gif")));
    let none = BTreeSet::new();
    let preview_only = preview(&base, &[], Stage::Enrichment, &none).unwrap();
    assert_eq!(preview_only.items[0].cleared, ["kanji"]);
    assert!(!preview_only.writes_enabled);
    // Preview never writes.
    assert_eq!(latest(&f, result.plan_id).revision, 1);

    let mut store = linguist_store::Store::open_existing(&f.state()).unwrap();
    let digest = base.approval_digest().unwrap();
    assert!(
        regenerate(
            &mut store,
            &base,
            "stale",
            &[],
            Stage::Enrichment,
            &none,
            &f.settings,
            &f.environment,
            Providers::default()
        )
        .unwrap_err()
        .contains("BASE_CONFLICT")
    );
    let (child, report) = regenerate(
        &mut store,
        &base,
        &digest,
        &[],
        Stage::Enrichment,
        &none,
        &f.settings,
        &f.environment,
        Providers {
            kanji: Some(&Kanji("new")),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.items[0].cleared, ["kanji"]);
    assert_eq!(child.revision, 2);
    let LearningContent::Vocabulary(vocab) = &child.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.kanji_details[0].meanings, ["new"]);
    let strokes: Vec<_> = child.documents[0]
        .media
        .iter()
        .filter(|m| m.role == linguist_core::records::MediaRole::KanjiStroke)
        .collect();
    assert_eq!(strokes.len(), 1);
    assert_ne!(strokes[0].digest, old_stroke);
    assert_eq!(
        child.documents[0]
            .sources
            .iter()
            .filter(|s| s.kind == "jisho_kanji_pages_v2")
            .count(),
        1
    );
    // The parent remains exactly as published.
    assert_eq!(store.revision(base.id, 1).unwrap(), base);

    // A user-edited Kanji field is protected unless explicitly overwritten.
    let mut edited = child.documents[0].clone();
    edited
        .edits
        .insert("Kanji".into(), FieldIntent::Set("my own note".into()));
    let (_, item) = prepare_item(&edited, Stage::Enrichment, &none).unwrap();
    assert!(item.protected.contains(&"kanji".to_owned()));
    let (prepared, item) = prepare_item(
        &edited,
        Stage::Enrichment,
        &BTreeSet::from(["kanji".to_owned()]),
    )
    .unwrap();
    assert!(item.overwritten.contains(&"kanji".to_owned()));
    assert!(!prepared.edits.contains_key("Kanji"));
    assert!(
        prepare_item(
            &edited,
            Stage::Enrichment,
            &BTreeSet::from(["meaning".to_owned()])
        )
        .unwrap_err()
        .contains("OVERWRITE_FIELD_INVALID")
    );
}

#[test]
fn generation_preview_only_clears_generated_content() {
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings.values.insert("kanji.enabled".into(), json!(false));
    let result = prepare_with_providers(
        &japanese(),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers::default(),
    )
    .unwrap();
    let mut document = latest(&f, result.plan_id).documents[0].clone();
    if let LearningContent::Vocabulary(v) = &mut document.content {
        v.usage = "Generated usage.".into();
        v.nuance = vec![linguist_core::document::Contrast {
            expression: "食べ物".into(),
            difference: "Authored contrast.".into(),
        }];
        v.examples.push(linguist_core::Example {
            sentence: "食を楽しむ。".into(),
            translation: "Enjoy food.".into(),
            provenance: Provenance::Generated,
            evidence_ids: vec![],
        });
    }
    let source = document.sources[0].id;
    document.evidence.push(Evidence {
        id: uuid::Uuid::new_v4(),
        field: "usage".into(),
        provenance: Provenance::Generated,
        source_id: Some(source),
        region_id: None,
        target: None,
        source_span: None,
        language: "en".to_owned().try_into().unwrap(),
        claim: "Generated usage.".into(),
        source_url: None,
        ambiguous: false,
    });
    let (prepared, item) = prepare_item(&document, Stage::Generation, &BTreeSet::new()).unwrap();
    assert_eq!(item.cleared, ["usage", "examples"]);
    assert_eq!(item.protected, ["nuance"]);
    let LearningContent::Vocabulary(v) = &prepared.content else {
        panic!()
    };
    assert!(v.usage.is_empty() && v.examples.is_empty());
    assert_eq!(v.nuance.len(), 1);
    let (prepared, item) = prepare_item(
        &document,
        Stage::Generation,
        &BTreeSet::from(["nuance".to_owned()]),
    )
    .unwrap();
    assert_eq!(item.overwritten, ["nuance"]);
    let LearningContent::Vocabulary(v) = &prepared.content else {
        panic!()
    };
    assert!(v.nuance.is_empty());
    assert!(
        prepare_item(
            &document,
            Stage::Dictionary,
            &BTreeSet::from(["usage".to_owned()])
        )
        .is_err()
    );
}

#[test]
fn dictionary_regeneration_replaces_entries_and_keeps_the_reviewed_meaning() {
    struct Dictionary(&'static str);
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            let body = format!(
                r#"{{"meta":{{"status":200}},"data":[{{"slug":"eat","japanese":[{{"word":"食べる","reading":"たべる"}}],"senses":[{{"english_definitions":["to eat"]}},{{"english_definitions":["{}"]}}]}}]}}"#,
                self.0
            );
            linguist_dictionary::parse_jisho(query, target, body.as_bytes(), 4096, 10)
                .map_err(|e| e.to_string())
        }
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("jisho"));
    settings.values.insert("kanji.enabled".into(), json!(false));
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension"],"body":{"expression":"食べる"}}"#.as_bytes();
    let result = prepare_with_providers(
        bytes,
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            dictionary: Some(&Dictionary("to live on")),
            ..Default::default()
        },
    )
    .unwrap();
    let base = latest(&f, result.plan_id);
    let LearningContent::Vocabulary(vocab) = &base.documents[0].content else {
        panic!()
    };
    let key = vocab.dictionary[0].senses[0].key.clone();
    let chosen = resolve(
        &base,
        &ResolutionRequest {
            schema_version: 2,
            base_revision: 1,
            base_digest: base.approval_digest().unwrap(),
            document_id: base.documents[0].id,
            issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[0].id),
            input_digest: base.documents[0].semantic_digest().unwrap(),
            actor: "reviewer".into(),
            choice: ReviewChoice::Sense(key),
        },
        "unix-seconds:1".into(),
    )
    .unwrap()
    .revision;
    let mut store = linguist_store::Store::open_existing(&f.state()).unwrap();
    store.publish_revision(&chosen).unwrap();
    let (child, report) = regenerate(
        &mut store,
        &chosen,
        &chosen.approval_digest().unwrap(),
        &[],
        Stage::Dictionary,
        &BTreeSet::new(),
        &settings,
        &f.environment,
        Providers {
            dictionary: Some(&Dictionary("to subsist on")),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.items[0].protected.contains(&"meaning".to_owned()));
    assert!(
        report.items[0]
            .invalidated_decisions
            .iter()
            .any(|id| id.starts_with("DICTIONARY_SENSE_REVIEW"))
    );
    let LearningContent::Vocabulary(vocab) = &child.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.meaning, "to eat");
    assert!(
        vocab.dictionary[0]
            .senses
            .iter()
            .any(|s| s.definitions == ["to subsist on"])
    );
    assert_eq!(
        child.documents[0]
            .sources
            .iter()
            .filter(|s| s.kind == "jisho_api_v1")
            .count(),
        1
    );
    // The selection must be confirmed again against the new entries.
    assert!(
        linguist_core::validation::validate(&child.documents[0])
            .iter()
            .any(|i| i.code == "DICTIONARY_SENSE_REVIEW")
    );
    assert_eq!(summarize(&child).unwrap()["status"], "needs_review");
}

#[test]
fn snapshot_export_bundles_originals_without_claiming_a_backup() {
    let f = Fixture::new();
    std::fs::create_dir_all(&f.root).unwrap();
    let state = f.root.join("snapshot-state");
    let mut store = linguist_store::Store::open(&state).unwrap();
    let digest = store.publish_asset(b"original note fields", 1024).unwrap();
    let source_id = uuid::Uuid::new_v4();
    let mut snapshot = linguist_core::records::Snapshot {
        id: uuid::Uuid::new_v4(),
        operation_id: uuid::Uuid::new_v4(),
        originals: vec![SourceRecord {
            id: source_id,
            kind: "anki_read_capture_v2".into(),
            location: "anki_note:1".into(),
            digest: digest.clone(),
            text: None,
            fields: Default::default(),
            model_manifest: "model".into(),
            template_manifest: None,
            captured_at_unix_seconds: None,
            tags: vec![],
            cards: vec![],
            media_refs: vec![],
        }],
        archives: vec![SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id,
            digest: digest.clone(),
            original_text: None,
            original_fields: Default::default(),
            asset_digests: vec![digest.clone()],
        }],
        media: vec![],
        before_digest: String::new(),
    };
    snapshot.before_digest = canonical::digest(
        "snapshot-original",
        &(&snapshot.originals, &snapshot.archives, &snapshot.media),
    )
    .unwrap();
    store.publish_snapshot(&snapshot).unwrap();
    let out = f.root.join("snapshot.json");
    let receipt = export_snapshot(&store, snapshot.id, &out).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(receipt.checksum, linguist_provider::sha256_hex(&bytes));
    let bundle: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bundle["manifest"]["format"], "lab-snapshot-bundle-v2");
    assert_eq!(bundle["manifest"]["full_collection_backup"], false);
    assert_eq!(bundle["manifest"]["contains_private_note_content"], true);
    assert_eq!(bundle["asset_data"][0]["digest"], digest);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // No overwrite, and unknown snapshots fail.
    assert!(
        export_snapshot(&store, snapshot.id, &out)
            .unwrap_err()
            .contains("DESTINATION_CONFLICT")
    );
    assert!(export_snapshot(&store, uuid::Uuid::new_v4(), &f.root.join("x.json")).is_err());
}

#[test]
fn editor_drafts_are_typed_and_commands_split_without_a_shell() {
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings.values.insert("kanji.enabled".into(), json!(false));
    let result = prepare_with_providers(
        &japanese(),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers::default(),
    )
    .unwrap();
    let plan = latest(&f, result.plan_id);
    let draft = editor::draft(&plan).unwrap();
    let patch = editor::parse(&draft).unwrap();
    assert_eq!(patch.base_digest, plan.approval_digest().unwrap());
    assert!(patch.items[0].fields.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&draft).unwrap();
    assert_eq!(
        value["current_values"][plan.documents[0].id.to_string()]["Meaning"],
        "food"
    );
    assert!(editor::parse(br#"{"patch":{},"extra":1}"#).is_err());
    assert_eq!(
        editor::split_command(r#"code --wait 'my dir/x' "a \"b\"" c\ d"#).unwrap(),
        ["code", "--wait", "my dir/x", "a \"b\"", "c d"]
    );
    assert!(editor::split_command("vim 'open").is_err());
    let env = BTreeMap::from([
        ("EDITOR".into(), "nano -w".into()),
        ("VISUAL".into(), " ".into()),
    ]);
    assert_eq!(editor::editor_argv(&[], &env).unwrap(), ["nano", "-w"]);
    assert_eq!(editor::editor_argv(&["ed".into()], &env).unwrap(), ["ed"]);
    assert!(editor::editor_argv(&[], &BTreeMap::new()).is_err());
}

#[test]
fn summaries_classify_status_and_workflow_without_payloads() {
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings.values.insert("kanji.enabled".into(), json!(false));
    let result = prepare_with_providers(
        &japanese(),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers::default(),
    )
    .unwrap();
    let summary = summarize(&latest(&f, result.plan_id)).unwrap();
    assert_eq!(summary["status"], "ready");
    assert_eq!(summary["workflow"], "vocab_add");
    assert!(summary.get("documents").is_none());
}

/// A two-document plan where each document has an open dictionary sense review.
fn two_sense_plan(f: &Fixture) -> PlanRevision {
    struct Dictionary;
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            let body = r#"{"meta":{"status":200},"data":[{"slug":"eat","japanese":[{"word":"食べる","reading":"たべる"}],"senses":[{"english_definitions":["to eat"]},{"english_definitions":["to live on"]}]}]}"#;
            linguist_dictionary::parse_jisho(query, target, body.as_bytes(), 4096, 10)
                .map_err(|e| e.to_string())
        }
    }
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("jisho"));
    settings.values.insert("kanji.enabled".into(), json!(false));
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension"],"body":{"expression":"食べる"}}"#.as_bytes();
    let result = prepare_with_providers(
        bytes,
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            dictionary: Some(&Dictionary),
            ..Default::default()
        },
    )
    .unwrap();
    let first = latest(f, result.plan_id);
    let mut second = first.documents[0].clone();
    second.id = uuid::Uuid::new_v4();
    let mut plan = first.clone();
    plan.documents.push(second);
    plan.revision = 2;
    plan.parent_digest = Some(first.approval_digest().unwrap());
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    store.publish_revision(&plan).unwrap();
    latest(f, result.plan_id)
}

fn sense_key(plan: &PlanRevision, index: usize, nth: usize) -> String {
    let LearningContent::Vocabulary(vocab) = &plan.documents[index].content else {
        panic!()
    };
    vocab.dictionary[0].senses[nth].key.clone()
}

#[test]
fn resolve_batch_applies_ordered_digest_bound_decisions_and_stops_at_a_conflict() {
    use linguist_application::review::batch::{
        BatchDecision, ResolutionBatch, resolve_batch, template,
    };
    let f = Fixture::new();
    let base = two_sense_plan(&f);
    // The template lists both open sense reviews and decides nothing.
    let skeleton = template(&base, "reviewer").unwrap();
    let listed: Vec<_> = skeleton["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| {
            d["issue_id"]
                .as_str()
                .unwrap()
                .starts_with("DICTIONARY_SENSE_REVIEW")
        })
        .collect();
    assert_eq!(listed.len(), 2, "{skeleton}");
    assert!(listed.iter().all(|d| d["choice"].is_null()));
    assert!(
        listed
            .iter()
            .all(|d| d["options"]["templates"].as_array().unwrap().len() >= 2)
    );
    // A null choice is refused before anything is published.
    let unfilled: ResolutionBatch = serde_json::from_value(skeleton.clone()).unwrap();
    let mut no_history = |_: &PlanRevision, _: uuid::Uuid, _: &[_], _: &str| -> Result<_, String> {
        panic!("no history decisions here")
    };
    let clock = || Ok("unix-seconds:1".to_string());
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let error = resolve_batch(&mut store, &base, &unfilled, &clock, &mut no_history).unwrap_err();
    assert!(
        error.starts_with("REVIEW_BATCH_DECISION_INVALID"),
        "{error}"
    );
    assert_eq!(latest(&f, base.id).revision, base.revision);

    let decision = |index: usize, choice: ReviewChoice| BatchDecision {
        document_id: base.documents[index].id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[index].id),
        input_digest: base.documents[index].semantic_digest().unwrap(),
        choice: Some(choice),
        history_map: None,
        expect_resolved: false,
        options: None,
    };
    let batch = ResolutionBatch {
        schema_version: 2,
        base_revision: base.revision,
        base_digest: base.approval_digest().unwrap(),
        actor: "reviewer".into(),
        decisions: vec![
            decision(0, ReviewChoice::Sense(sense_key(&base, 0, 1))),
            decision(1, ReviewChoice::Sense(sense_key(&base, 1, 0))),
        ],
    };
    // A batch bound to another revision is refused outright.
    let mut stale = batch.clone();
    stale.base_digest = "0".repeat(64);
    assert_eq!(
        resolve_batch(&mut store, &base, &stale, &clock, &mut no_history).unwrap_err(),
        "REVIEW_BASE_CONFLICT"
    );
    // An assertion listed first is still checked last, after the decision
    // that closes its issue.
    let mut batch = batch;
    let mut assertion = decision(0, ReviewChoice::Sense(String::new()));
    assertion.choice = None;
    assertion.expect_resolved = true;
    batch.decisions.insert(0, assertion);
    let outcome = resolve_batch(&mut store, &base, &batch, &clock, &mut no_history).unwrap();
    assert!(outcome.conflict.is_none());
    assert_eq!(outcome.applied.len(), 2);
    assert_eq!(outcome.verified_closed.len(), 1);
    assert_eq!(outcome.verified_closed[0].index, 0);
    // One pass, one published revision (WP-24), with both decisions in it.
    assert_eq!(outcome.revision, base.revision + 1);
    assert!(
        outcome
            .applied
            .iter()
            .all(|a| a.revision == outcome.revision)
    );
    assert_eq!(after_count(&f, base.id), base.revision + 1);
    let after = latest(&f, base.id);
    assert_eq!(after.revision, outcome.revision);
    assert_eq!(after.approval_digest().unwrap(), outcome.digest);
    for (index, nth) in [(0, 1), (1, 0)] {
        let LearningContent::Vocabulary(vocab) = &after.documents[index].content else {
            panic!()
        };
        assert_eq!(vocab.sense_key, sense_key(&base, index, nth));
    }

    // Stop at the first conflict: the earlier decision is published, the
    // conflicting one and every later one are not.
    let base = two_sense_plan(&f);
    let mut conflicting = ResolutionBatch {
        schema_version: 2,
        base_revision: base.revision,
        base_digest: base.approval_digest().unwrap(),
        actor: "reviewer".into(),
        decisions: vec![
            BatchDecision {
                document_id: base.documents[0].id,
                issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[0].id),
                input_digest: base.documents[0].semantic_digest().unwrap(),
                choice: Some(ReviewChoice::Sense(sense_key(&base, 0, 0))),
                history_map: None,
                expect_resolved: false,
                options: None,
            },
            BatchDecision {
                document_id: base.documents[1].id,
                issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[1].id),
                input_digest: "0".repeat(64),
                choice: Some(ReviewChoice::Sense(sense_key(&base, 1, 0))),
                history_map: None,
                expect_resolved: false,
                options: None,
            },
        ],
    };
    // Generated-fact decisions sort after sense decisions, whatever the file order.
    conflicting.decisions.insert(
        0,
        BatchDecision {
            document_id: base.documents[0].id,
            issue_id: "GENERATED_FACT_REVIEW:usage".into(),
            input_digest: base.documents[0].semantic_digest().unwrap(),
            choice: Some(ReviewChoice::ContentVerified {
                evidence_ids: vec![uuid::Uuid::new_v4()],
            }),
            history_map: None,
            expect_resolved: false,
            options: None,
        },
    );
    let outcome = resolve_batch(&mut store, &base, &conflicting, &clock, &mut no_history).unwrap();
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(outcome.applied[0].index, 1);
    let conflict = outcome.conflict.unwrap();
    assert_eq!(conflict.index, 2);
    assert_eq!(conflict.error, "REVIEW_INPUT_CONFLICT");
    assert_eq!(outcome.not_attempted, 1);
    assert_eq!(latest(&f, base.id).revision, base.revision + 1);

    // An issue the batch does not close fails its assertion.
    let base = latest(&f, base.id);
    let open = ResolutionBatch {
        schema_version: 2,
        base_revision: base.revision,
        base_digest: base.approval_digest().unwrap(),
        actor: "reviewer".into(),
        decisions: vec![BatchDecision {
            document_id: base.documents[1].id,
            issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[1].id),
            input_digest: base.documents[1].semantic_digest().unwrap(),
            choice: None,
            history_map: None,
            expect_resolved: true,
            options: None,
        }],
    };
    let outcome = resolve_batch(&mut store, &base, &open, &clock, &mut no_history).unwrap();
    assert_eq!(outcome.conflict.unwrap().error, "REVIEW_ISSUE_STILL_OPEN");
    assert_eq!(latest(&f, base.id).revision, base.revision);
}

/// WP-24: the campaign policy picks the sense whose reading is the note's,
/// leaves an unmatched choice to the agent, keeps illustrations, archives
/// dictionary pages and asks about uncertain images.
#[test]
fn campaign_policy_decides_only_what_the_user_preferences_settle() {
    use linguist_application::review::{batch::template, policy};
    use linguist_core::records::{EvidenceTarget, MediaAsset, MediaOwner, MediaRole};
    let f = Fixture::new();
    let mut plan = two_sense_plan(&f);
    let LearningContent::Vocabulary(vocab) = &mut plan.documents[0].content else {
        panic!()
    };
    vocab.reading = "たべる".into();
    let policy = policy::Policy::builtin();
    policy.validate().unwrap();
    let filled = policy::fill(&policy, &plan, &template(&plan, "x").unwrap()).unwrap();
    assert_eq!(filled.decided, 1, "{filled:?}");
    let batch = filled.batch.unwrap();
    assert_eq!(batch["actor"], "campaign-policy");
    assert_eq!(
        batch["decisions"][0]["document_id"],
        json!(plan.documents[0].id)
    );
    assert_eq!(
        batch["decisions"][0]["choice"]["value"]["key"],
        json!(sense_key(&plan, 0, 0)),
        "the first (most common) sense with the note's reading"
    );
    assert_eq!(filled.undecided.len(), 1);
    assert_eq!(filled.undecided[0].document_id, plan.documents[1].id);
    assert!(
        filled.undecided[0]
            .reason
            .starts_with("SENSE_READING_UNMATCHED")
    );

    // Source images: the decision follows the OCR classification.
    let document = &mut plan.documents[0];
    let source = uuid::Uuid::new_v4();
    let mut entries = Vec::new();
    for (name, class) in [
        ("cat.jpg", Some("illustration")),
        ("Screenshot (1).png", Some("dictionary")),
        ("paste-1.jpg", Some("uncertain")),
        ("never.png", None),
        ("6f1a.gif", Some("illustration")),
    ] {
        let digest = format!("{:064x}", entries.len() + 1);
        document.media.push(MediaAsset {
            digest: digest.clone(),
            filename: name.into(),
            original_filename: Some(name.into()),
            size_bytes: 1,
            mime: if name.ends_with(".gif") {
                "image/gif"
            } else {
                "image/png"
            }
            .into(),
            owner: MediaOwner::Source,
            role: MediaRole::Archive,
            source_id: Some(source),
            attribution: String::new(),
            license: None,
        });
        let evidence = |field: &str, claim: String| Evidence {
            id: uuid::Uuid::new_v4(),
            field: field.into(),
            provenance: Provenance::Source,
            source_id: Some(source),
            region_id: None,
            target: Some(EvidenceTarget::MediaAsset {
                digest: digest.clone(),
            }),
            source_span: None,
            language: Language::try_from("ja".to_owned()).unwrap(),
            claim,
            source_url: None,
            ambiguous: false,
        };
        document
            .evidence
            .push(evidence("media_format", "{}".into()));
        if let Some(class) = class {
            document.evidence.push(evidence(
                &format!("ocr:{digest}"),
                json!({"classification": {"class": class}}).to_string(),
            ));
        }
        entries.push(json!({"document_id": document.id, "issue_id": format!("SOURCE_MEDIA_CONTENT_REVIEW:{name}"),
            "input_digest": "d", "choice": null,
            "options": {"issue": {"code": "SOURCE_MEDIA_CONTENT_REVIEW", "field": name, "source_refs": [source]}, "templates": []}}));
    }
    entries.push(json!({"document_id": document.id, "issue_id": "IMAGE_CANDIDATE_REVIEW:x", "input_digest": "d", "choice": null,
        "options": {"issue": {"code": "IMAGE_CANDIDATE_REVIEW", "source_refs": ["abc"]}, "templates": []}}));
    entries.push(json!({"document_id": document.id, "issue_id": "IMAGE_CLASSIFICATION_REVIEW:x", "input_digest": "d", "choice": null,
        "options": {"issue": {"code": "IMAGE_CLASSIFICATION_REVIEW", "source_refs": [source]}, "templates": []}}));
    let template =
        json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": entries});
    let filled = policy::fill(&policy, &plan, &template).unwrap();
    let roles: Vec<_> = filled.batch.as_ref().unwrap()["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["issue_id"].as_str().unwrap().to_owned(),
                d["choice"]["value"].clone(),
            )
        })
        .collect();
    let role = |name: &str| {
        roles
            .iter()
            .find(|(id, _)| id.ends_with(name))
            .map(|(_, v)| v["role"].clone())
    };
    assert_eq!(role("cat.jpg"), Some(json!("picture")));
    assert_eq!(role("Screenshot (1).png"), Some(json!("archive")));
    assert_eq!(
        role("6f1a.gif"),
        Some(json!("archive")),
        "stroke GIFs are never pictures"
    );
    assert_eq!(role("paste-1.jpg"), None);
    // A kept illustration declines the searched candidate.
    assert_eq!(
        roles
            .iter()
            .find(|(id, _)| id.starts_with("IMAGE_CANDIDATE"))
            .unwrap()
            .1,
        json!("")
    );
    let open: BTreeSet<_> = filled
        .undecided
        .iter()
        .map(|u| u.reason.split(':').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        open,
        BTreeSet::from([
            "AGENT_CODE".into(),
            "IMAGE_CLASS_UNCERTAIN".into(),
            "IMAGE_NOT_CLASSIFIED".into()
        ])
    );
}

/// WP-24: a generated collocation list that names the word itself is
/// rejected by the policy; a proper one is verified.
#[test]
fn campaign_policy_rejects_a_collocation_that_is_the_word() {
    use linguist_application::review::policy;
    let f = Fixture::new();
    let mut plan = two_sense_plan(&f);
    let document = &mut plan.documents[0];
    let mut entries = Vec::new();
    for (index, claim) in [
        r#"[{"phrase":"食べる","gloss":"eat"}]"#,
        r#"[{"phrase":"ご飯を食べる","gloss":"eat a meal"},{"phrase":"食べ物","gloss":"food"}]"#,
        r#"[{"phrase":"取っ手","gloss":"handle"}]"#,
        r#"[{"phrase":"ご飯を食べる","gloss":"eat"},{"phrase":"ご飯を食べる","gloss":"eat rice"}]"#,
    ]
    .iter()
    .enumerate()
    {
        let id = uuid::Uuid::new_v4();
        document.evidence.push(Evidence {
            id,
            field: "collocations".into(),
            provenance: Provenance::Generated,
            source_id: None,
            region_id: None,
            target: None,
            source_span: None,
            language: Language::try_from("en".to_owned()).unwrap(),
            claim: (*claim).into(),
            source_url: None,
            ambiguous: false,
        });
        entries.push(json!({"document_id": document.id, "issue_id": format!("GENERATED_FACT_REVIEW:{index}"), "input_digest": "d", "choice": null,
            "options": {"issue": {"code": "GENERATED_FACT_REVIEW", "source_refs": [id]}, "templates": [
                {"choice": {"decision": "content_verified", "value": {"evidence_ids": [id]}}},
                {"choice": {"decision": "content_rejected", "value": {"evidence_id": id}}}]}}));
    }
    let template =
        json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": entries});
    let filled = policy::fill(&policy::Policy::builtin(), &plan, &template).unwrap();
    let decisions = filled.batch.unwrap()["decisions"].clone();
    assert_eq!(
        decisions[0]["choice"]["decision"], "content_rejected",
        "the word itself"
    );
    assert_eq!(
        decisions[1]["choice"]["decision"], "content_verified",
        "word and stem"
    );
    assert_eq!(
        decisions[2]["choice"]["decision"], "content_rejected",
        "a phrase without the word"
    );
    assert_eq!(
        decisions[3]["choice"]["decision"], "content_rejected",
        "the same phrase twice"
    );
}

/// WP-24 agent findings, reinforced in the policy: a sense is chosen only by
/// an equal reading (台所 だいどころ is not だいどこ), a collocation must use
/// the word itself, not a compound sharing its kanji (体温 for 体 read からだ),
/// and another spelling of the word is no nuance contrast (躰 for 体).
/// WP-24 (得意, 一覧, 再生): among senses of the same reading, the one the
/// learner's own dictionary screenshot names is chosen, not the first.
#[test]
fn the_sense_a_screenshot_names_is_chosen_among_equal_readings() {
    use linguist_application::review::policy;
    let f = Fixture::new();
    let mut plan = two_sense_plan(&f);
    let (first, second) = (sense_key(&plan, 0, 0), sense_key(&plan, 0, 1));
    let document = &mut plan.documents[0];
    let LearningContent::Vocabulary(vocab) = &mut document.content else {
        panic!()
    };
    vocab.reading = "たべる".into();
    let sense = |key: &str| json!({"decision": "sense_with_reading", "value": {"key": key, "reading": "たべる"}});
    let batch = |document: &linguist_core::LearningDocument| {
        json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": [
        {"document_id": document.id, "issue_id": "DICTIONARY_SENSE_REVIEW:x", "input_digest": "d", "choice": null,
         "options": {"issue": {"code": "DICTIONARY_SENSE_REVIEW"}, "templates": [
            {"reading": "たべる", "choice": sense(&first)},
            {"reading": "たべる", "choice": sense(&second)}]}}]})
    };
    let chosen = |plan: &PlanRevision| {
        policy::fill(&policy::Policy::builtin(), plan, &batch(&plan.documents[0]))
            .unwrap()
            .batch
            .unwrap()["decisions"][0]["choice"]["value"]["key"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // Without a screenshot: dictionary order.
    assert_eq!(chosen(&plan), first);
    // The learner's screenshot names the second sense.
    let source_id = plan.documents[0].sources[0].id;
    plan.documents[0]
        .regions
        .push(linguist_core::records::SourceRegion {
            id: uuid::Uuid::new_v4(),
            source_id,
            image_digest: "a".repeat(64),
            bounds: [0, 0, 10, 10],
            reading_order: 0,
            engine: "tesseract".into(),
            settings_digest: "b".repeat(64),
            text: "1. to live on (e.g. a salary)".into(),
            confidence: Some(0.9),
            language: Language::try_from("en".to_owned()).unwrap(),
        });
    assert_eq!(chosen(&plan), second);
    // WP-24 (こらえる, やがて): a kana word with no reading or pronunciation
    // is its own reading.
    let LearningContent::Vocabulary(vocab) = &mut plan.documents[0].content else {
        panic!()
    };
    vocab.expression = "たべる".into();
    vocab.reading.clear();
    vocab.pronunciation.clear();
    plan.documents[0].regions.clear();
    assert_eq!(chosen(&plan), first);
}

#[test]
fn campaign_policy_reinforces_the_agents_findings() {
    use linguist_application::review::policy;
    let f = Fixture::new();
    let mut plan = two_sense_plan(&f);
    let document = &mut plan.documents[0];
    let LearningContent::Vocabulary(vocab) = &mut document.content else {
        panic!()
    };
    vocab.expression = "体".into();
    vocab.reading = "から だ".into();
    vocab.dictionary[0].forms = vec!["体".into(), "躰".into()];
    vocab.dictionary[0].readings = vec!["からだ".into()];
    let sense = |key: &str| json!({"decision": "sense_with_reading", "value": {"key": key}});
    let mut entries = vec![
        json!({"document_id": document.id, "issue_id": "DICTIONARY_SENSE_REVIEW:x", "input_digest": "d", "choice": null,
        "options": {"issue": {"code": "DICTIONARY_SENSE_REVIEW"}, "templates": [
            {"reading": "から", "choice": sense("prefix")},
            {"reading": "カラダ", "choice": sense("equal")}]}}),
    ];
    for (index, (field, claim)) in [
        (
            "collocations",
            r#"[{"phrase":"体を動かす","gloss":"move one's body"}]"#,
        ),
        (
            "collocations",
            r#"[{"phrase":"体温を測る","gloss":"take one's temperature"}]"#,
        ),
        (
            "nuance",
            r#"[{"expression":"躰","difference":"older spelling"}]"#,
        ),
        (
            "nuance",
            r#"[{"expression":"身体","difference":"more formal"}]"#,
        ),
        (
            "nuance",
            r#"[{"expression":"体の","difference":"its own form"}]"#,
        ),
        (
            "collocations",
            r#"[{"phrase":"体を洗う","gloss":"wash oneself"},{"phrase":"体の","gloss":"of the body"}]"#,
        ),
    ]
    .iter()
    .enumerate()
    {
        let id = uuid::Uuid::new_v4();
        document.evidence.push(Evidence {
            id,
            field: (*field).into(),
            provenance: Provenance::Generated,
            source_id: None,
            region_id: None,
            target: None,
            source_span: None,
            language: Language::try_from("en".to_owned()).unwrap(),
            claim: (*claim).into(),
            source_url: None,
            ambiguous: false,
        });
        entries.push(json!({"document_id": document.id, "issue_id": format!("GENERATED_FACT_REVIEW:{index}"), "input_digest": "d", "choice": null,
            "options": {"issue": {"code": "GENERATED_FACT_REVIEW", "source_refs": [id]}, "templates": [
                {"choice": {"decision": "content_verified", "value": {"evidence_ids": [id]}}},
                {"choice": {"decision": "content_rejected", "value": {"evidence_id": id}}}]}}));
    }
    let template =
        json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": entries});
    let filled = policy::fill(&policy::Policy::builtin(), &plan, &template).unwrap();
    let decisions = filled.batch.unwrap()["decisions"].clone();
    assert_eq!(
        decisions[0]["choice"]["value"]["key"], "equal",
        "{decisions}"
    );
    // Spaced syllables with separated readings (二十七: に じゅう なな; に じゅう しち).
    let mut numbers = plan.clone();
    let LearningContent::Vocabulary(v) = &mut numbers.documents[0].content else {
        panic!()
    };
    v.reading.clear();
    v.pronunciation = "に じゅう なな; に じゅう しち".into();
    let only_sense = json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": [
        {"document_id": numbers.documents[0].id, "issue_id": "DICTIONARY_SENSE_REVIEW:x", "input_digest": "d", "choice": null,
         "options": {"issue": {"code": "DICTIONARY_SENSE_REVIEW"}, "templates": [
            {"reading": "に", "choice": sense("syllable")},
            {"reading": "にじゅうしち", "choice": sense("second")},
            {"reading": "にじゅうなな", "choice": sense("first")}]}}]});
    let filled = policy::fill(&policy::Policy::builtin(), &numbers, &only_sense).unwrap();
    assert_eq!(
        filled.batch.unwrap()["decisions"][0]["choice"]["value"]["key"],
        "second",
        "a separated reading, never a syllable; dictionary order among equals"
    );
    // Readings on separate lines (意見: い けん / ご い けん).
    let LearningContent::Vocabulary(v) = &mut numbers.documents[0].content else {
        panic!()
    };
    v.pronunciation = "い けん\n\nご い けん".into();
    let lines = json!({"schema_version": 2, "base_revision": 1, "base_digest": "x", "decisions": [
        {"document_id": numbers.documents[0].id, "issue_id": "DICTIONARY_SENSE_REVIEW:x", "input_digest": "d", "choice": null,
         "options": {"issue": {"code": "DICTIONARY_SENSE_REVIEW"}, "templates": [
            {"reading": "いけん", "choice": sense("opinion")}]}}]});
    let filled = policy::fill(&policy::Policy::builtin(), &numbers, &lines).unwrap();
    assert_eq!(
        filled.batch.unwrap()["decisions"][0]["choice"]["value"]["key"],
        "opinion"
    );
    let decision = |i: usize| decisions[i]["choice"]["decision"].clone();
    assert_eq!(decision(1), "content_verified", "the word itself");
    assert_eq!(
        decision(2),
        "content_rejected",
        "a compound sharing its kanji"
    );
    assert_eq!(decision(3), "content_rejected", "another spelling");
    assert_eq!(decision(4), "content_verified", "a different word");
    assert_eq!(
        decision(5),
        "content_rejected",
        "one of its own forms (節約する)"
    );
    assert_eq!(
        decision(6),
        "content_rejected",
        "a collocation that is one of its own forms (遠慮する)"
    );
}
