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
    assert!(error.starts_with("REVIEW_BATCH_DECISION_INVALID"), "{error}");
    assert_eq!(latest(&f, base.id).revision, base.revision);

    let decision = |index: usize, choice: ReviewChoice| BatchDecision {
        document_id: base.documents[index].id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[index].id),
        input_digest: base.documents[index].semantic_digest().unwrap(),
        choice: Some(choice),
        history_map: None,
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
    let outcome = resolve_batch(&mut store, &base, &batch, &clock, &mut no_history).unwrap();
    assert!(outcome.conflict.is_none());
    assert_eq!(outcome.applied.len(), 2);
    assert_eq!(outcome.revision, base.revision + 2);
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
                options: None,
            },
            BatchDecision {
                document_id: base.documents[1].id,
                issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[1].id),
                input_digest: "0".repeat(64),
                choice: Some(ReviewChoice::Sense(sense_key(&base, 1, 0))),
                history_map: None,
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
            options: None,
        },
    );
    let outcome =
        resolve_batch(&mut store, &base, &conflicting, &clock, &mut no_history).unwrap();
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(outcome.applied[0].index, 1);
    let conflict = outcome.conflict.unwrap();
    assert_eq!(conflict.index, 2);
    assert_eq!(conflict.error, "REVIEW_INPUT_CONFLICT");
    assert_eq!(outcome.not_attempted, 1);
    assert_eq!(latest(&f, base.id).revision, base.revision + 1);
}
