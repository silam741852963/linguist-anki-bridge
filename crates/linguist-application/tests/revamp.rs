use linguist_application::{mapping::*, revamp::*, source_archive::*};
use linguist_config::*;
use linguist_core::{LearningContent, Provenance, Task, validation};
use serde_json::json;
use std::collections::BTreeMap;
fn setup(
    purpose: &str,
    values: &[(&str, &str)],
    mapping: &[(&str, &str)],
) -> (RevampCapture, Effective) {
    setup_id("123", purpose, values, mapping)
}
fn setup_id(
    note_id: &str,
    purpose: &str,
    values: &[(&str, &str)],
    mapping: &[(&str, &str)],
) -> (RevampCapture, Effective) {
    let card_id = (note_id.parse::<u64>().unwrap() + 333).to_string();
    let fields = values
        .iter()
        .enumerate()
        .map(|(index, (name, value))| ((*name).to_owned(), json!({"value":value,"order":index})))
        .collect::<BTreeMap<_, _>>();
    let note = json!({"noteId":note_id,"modelName":"Legacy","fields":fields,"cards":[card_id],"tags":["preserved"]});
    let model = json!({"model":{"name":"Legacy","id":"12"},"fields":values.iter().map(|(name,_)|name).collect::<Vec<_>>(),"templates":{"Card":{"Front":"front","Back":"back"}},"css":"style"});
    let cards = json!([{"cardId":card_id,"note":note_id,"reps":5}]);
    let captured = archive_read_capture(
        &serde_json::to_vec(&note).unwrap(),
        &serde_json::to_vec(&model).unwrap(),
        &serde_json::to_vec(&cards).unwrap(),
        10000,
    )
    .unwrap();
    let mut options = ResolveOptions {
        purpose: Some(purpose.into()),
        ..Default::default()
    };
    options.flags.insert(
        format!("purposes.{purpose}.fields"),
        serde_json::to_value(
            mapping
                .iter()
                .map(|(role, name)| (role.to_string(), name.to_string()))
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap(),
    );
    options
        .flags
        .insert(format!("purposes.{purpose}.source_model"), json!("Legacy"));
    let settings = resolve(&Registry::builtin(), &ConfigFile::default(), &options).unwrap();
    let mapping =
        map_purpose_fields(&settings, purpose, "Legacy", &captured.source.fields).unwrap();
    (RevampCapture { captured, mapping }, settings)
}
#[test]
fn plain_vocabulary_roles_become_source_candidates_with_complete_archive_and_review() {
    let (capture, settings) = setup(
        "japanese_vocab",
        &[
            ("Word", " 猫 "),
            ("Definition", "cat"),
            ("Key", "cat-animal"),
            ("Unused", "  untouched\n"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Definition"),
            ("sense_key", "Key"),
        ],
    );
    let doc = stage_document(&capture, &settings, "japanese_vocab").unwrap();
    let LearningContent::Vocabulary(v) = &doc.content else {
        panic!()
    };
    assert_eq!(v.expression, "猫");
    assert_eq!(v.meaning, "cat");
    assert_eq!(doc.requested_tasks, vec![Task::Comprehension]);
    assert_eq!(doc.sources[0].fields["Unused"], "  untouched\n");
    assert!(doc.evidence.iter().any(|e| e.field == "expression"
        && e.claim == "猫"
        && e.provenance == Provenance::Source
        && e.source_id == Some(doc.sources[0].id)));
    assert!(!validation::ready(&doc));
    assert!(
        doc.issues
            .iter()
            .any(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW" && i.stage == "capture")
    );
}
#[test]
fn grammar_defaults_and_combined_rich_examples_and_language_conflicts_stay_reviewable() {
    let (capture, settings) = setup(
        "japanese_grammar",
        &[
            ("Combined", "なら explanation"),
            ("Formation", "V + なら"),
            ("Examples", "<ul><li>sentence</li></ul>"),
            ("Meaning", "<b>if</b>"),
            ("Lang", "vi"),
        ],
        &[
            ("pattern", "Combined"),
            ("use_key", "Combined"),
            ("formation", "Formation"),
            ("meaning", "Meaning"),
            ("examples", "Examples"),
            ("language", "Lang"),
        ],
    );
    let doc = stage_document(&capture, &settings, "japanese_grammar").unwrap();
    assert_eq!(doc.target_language.as_str(), "ja");
    assert_eq!(doc.explanation_language.as_str(), "vi");
    let LearningContent::Grammar(g) = &doc.content else {
        panic!()
    };
    assert!(
        g.pattern.is_empty() && g.use_key.is_empty() && g.meaning == "if" && g.examples.is_empty()
    );
    assert_eq!(g.formation, "V + なら");
    for code in [
        "SOURCE_COMBINED_FIELD_REVIEW",
        "SOURCE_HTML_TEXT_REVIEW",
        "SOURCE_EXAMPLES_SCHEMA_REVIEW",
        "SOURCE_LANGUAGE_CONFLICT",
    ] {
        assert!(doc.issues.iter().any(|i| i.code == code), "{code}");
    }
    assert_eq!(doc.sources[0].fields, capture.captured.source.fields);
}
#[test]
fn tampered_assets_fields_and_changed_model_constraints_are_rejected() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    settings
        .values
        .insert("purposes.english_vocab.source_model".into(), json!("Other"));
    assert!(stage_document(&capture, &settings, "english_vocab").is_err());
    settings.values.insert(
        "purposes.english_vocab.source_model".into(),
        json!("Legacy"),
    );
    capture
        .captured
        .source
        .fields
        .insert("Word".into(), "tampered".into());
    assert!(stage_document(&capture, &settings, "english_vocab").is_err());
    let (mut capture, settings) = setup(
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    capture
        .captured
        .assets
        .values_mut()
        .next()
        .unwrap()
        .push(b' ');
    assert!(stage_document(&capture, &settings, "english_vocab").is_err());
}

#[test]
fn published_revamp_draft_recovers_full_sources_and_remains_unbound_and_unapproved() {
    let (capture, mut settings) = setup(
        "english_vocab",
        &[
            ("Word", "cat"),
            ("Meaning", "a small feline"),
            ("Sense", "cat-animal"),
            ("Unused", "  preserved\n"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Meaning"),
            ("sense_key", "Sense"),
        ],
    );
    let root = std::env::temp_dir().join(format!("lab-revamp-plan-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root.to_str().unwrap()));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    let prepared =
        publish_capture_draft(&capture, &settings, "english_vocab", &environment).unwrap();
    assert!(!prepared.ready && !prepared.apply_eligible && !prepared.duplicate_check_performed);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    assert!(plan.binding.is_none() && plan.review_decisions.is_empty() && plan.rendered.is_empty());
    assert_eq!(plan.documents[0].id, prepared.document_id);
    assert_eq!(
        plan.documents[0].semantic_digest().unwrap(),
        prepared.input_digest
    );
    assert_eq!(
        plan.documents[0].sources[0].fields["Unused"],
        "  preserved\n"
    );
    for (digest, bytes) in &capture.captured.assets {
        assert_eq!(store.asset(digest, 100000).unwrap(), *bytes);
    }
    assert_eq!(
        plan.settings.values["storage.state_dir"],
        root.to_str().unwrap()
    );
    assert!(!validation::ready(&plan.documents[0]));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn invalid_capture_cannot_initialize_draft_state() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    let root =
        std::env::temp_dir().join(format!("lab-invalid-revamp-plan-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root.to_str().unwrap()));
    capture
        .captured
        .assets
        .remove(&capture.captured.source.model_manifest);
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    assert!(publish_capture_draft(&capture, &settings, "english_vocab", &environment).is_err());
    assert!(!root.exists());
}

#[test]
fn batch_capture_publishes_one_revision_and_recovers_every_source_asset() {
    let (first, mut settings) = setup_id(
        "124",
        "english_vocab",
        &[("Word", "dog")],
        &[("expression", "Word")],
    );
    let (second, _) = setup_id(
        "123",
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    let captures = [first, second];
    let root = std::env::temp_dir().join(format!("lab-revamp-batch-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    let results =
        publish_capture_drafts(&captures, &settings, "english_vocab", &environment).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].plan_id, results[1].plan_id);
    assert_eq!(results[0].digest, results[1].digest);
    assert_ne!(results[0].document_id, results[1].document_id);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(results[0].plan_id, 1).unwrap();
    assert_eq!(plan.documents.len(), 2);
    for (index, word) in ["dog", "cat"].iter().enumerate() {
        assert_eq!(plan.documents[index].sources[0].fields["Word"], *word);
        assert_eq!(plan.documents[index].id, results[index].document_id);
        assert!(!results[index].ready && !results[index].apply_eligible);
        for (digest, bytes) in &captures[index].captured.assets {
            assert_eq!(store.asset(digest, 100000).unwrap(), *bytes);
        }
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_limit_duplicate_or_invalid_later_source_leaves_no_state() {
    let (first, mut settings) = setup_id(
        "123",
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    let (mut second, _) = setup_id(
        "124",
        "english_vocab",
        &[("Word", "dog")],
        &[("expression", "Word")],
    );
    let root =
        std::env::temp_dir().join(format!("lab-revamp-batch-reject-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    second.captured.assets.clear();
    let captures = [first, second];
    assert!(publish_capture_drafts(&captures, &settings, "english_vocab", &environment).is_err());
    assert!(!root.exists());
    settings
        .values
        .insert("selection.max_notes".into(), json!(1));
    assert_eq!(
        publish_capture_drafts(&captures, &settings, "english_vocab", &environment).unwrap_err(),
        "REVAMP_SELECTION_LIMIT"
    );
    assert!(!root.exists());
    settings
        .values
        .insert("selection.max_notes".into(), json!(2));
    let (same, _) = setup_id(
        "123",
        "english_vocab",
        &[("Word", "duplicate")],
        &[("expression", "Word")],
    );
    assert_eq!(
        publish_capture_drafts(
            &[captures.into_iter().next().unwrap(), same],
            &settings,
            "english_vocab",
            &environment
        )
        .unwrap_err(),
        "REVAMP_SELECTION_DUPLICATE"
    );
    assert!(!root.exists());
}

#[test]
fn aggregate_capture_archive_limit_fails_before_state_creation() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    let root =
        std::env::temp_dir().join(format!("lab-revamp-batch-bytes-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    settings.values.insert("input.max_file_mb".into(), json!(1));
    let bytes = vec![b'x'; 1024 * 1024];
    capture
        .captured
        .assets
        .insert(linguist_core::canonical::asset_digest(&bytes), bytes);
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    assert_eq!(
        publish_capture_drafts(&[capture], &settings, "english_vocab", &environment).unwrap_err(),
        "REVAMP_BATCH_ARCHIVE_LIMIT"
    );
    assert!(!root.exists());
}

#[test]
fn html_candidates_preserve_text_boundaries_and_entities_without_scripts_or_source_rewrites() {
    let raw = "<script>invented answer</script><div>first &amp; second<br>line</div><p>next</p><img src='picture.png'>";
    let (capture, settings) = setup(
        "english_vocab",
        &[
            ("Word", "<b>cat</b>"),
            ("Meaning", raw),
            ("Reading", "[sound:cat.mp3]"),
            ("Pronunciation", "<ruby>猫<rt>ねこ</rt></ruby>"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Meaning"),
            ("reading", "Reading"),
            ("pronunciation", "Pronunciation"),
        ],
    );
    let doc = stage_document(&capture, &settings, "english_vocab").unwrap();
    let LearningContent::Vocabulary(v) = &doc.content else {
        panic!()
    };
    assert_eq!(v.expression, "cat");
    assert_eq!(v.meaning, "first & second\nline\n\nnext");
    assert!(v.reading.is_empty() && v.pronunciation.is_empty());
    assert!(
        !doc.evidence
            .iter()
            .any(|e| e.claim.contains("invented answer"))
    );
    assert_eq!(doc.sources[0].fields["Meaning"], raw);
    assert!(
        doc.issues
            .iter()
            .any(|i| i.code == "SOURCE_HTML_TEXT_REVIEW")
    );
    assert!(
        doc.issues
            .iter()
            .any(|i| i.code == "SOURCE_RICH_FIELD_REVIEW")
    );
    assert!(!validation::ready(&doc));
}

#[test]
fn explicit_example_pairs_preserve_order_repeats_and_application_assigned_source_evidence() {
    let raw = r#"[{"sentence":"猫です。","translation":"Là mèo."},{"sentence":"猫です。","translation":"Là mèo."}]"#;
    let (capture, settings) = setup(
        "japanese_grammar",
        &[("Examples", raw)],
        &[("examples", "Examples")],
    );
    let doc = stage_document(&capture, &settings, "japanese_grammar").unwrap();
    let LearningContent::Grammar(g) = &doc.content else {
        panic!()
    };
    assert_eq!(g.examples.len(), 2);
    assert_eq!(g.examples[0].sentence, "猫です。");
    assert_eq!(g.examples[0].translation, "Là mèo.");
    assert_ne!(g.examples[0].evidence_ids, g.examples[1].evidence_ids);
    for example in &g.examples {
        assert_eq!(example.provenance, Provenance::Source);
        assert!(doc.evidence.iter().any(
            |e| example.evidence_ids.contains(&e.id) && e.source_id == Some(doc.sources[0].id)
        ));
    }
    assert_eq!(doc.archives[0].original_fields["Examples"], raw);
    assert!(
        doc.issues
            .iter()
            .any(|i| i.code == "SOURCE_EXAMPLES_REVIEW")
    );
    assert!(!validation::ready(&doc));
}
#[test]
fn malformed_or_unpaired_examples_remain_archived_without_partial_acceptance() {
    for raw in [
        r#"[{"sentence":"猫","translation":""}]"#,
        r#"[{"sentence":"猫","translation":"cat","provenance":"source"}]"#,
        r#"[{"sentence":"<b>猫</b>","translation":"cat"}]"#,
        r#"[{"sentence":"猫","translation":"cat"},{"sentence":"","translation":"bad"}]"#,
    ] {
        let (capture, settings) = setup(
            "japanese_grammar",
            &[("Examples", raw)],
            &[("examples", "Examples")],
        );
        let doc = stage_document(&capture, &settings, "japanese_grammar").unwrap();
        let LearningContent::Grammar(g) = &doc.content else {
            panic!()
        };
        assert!(g.examples.is_empty());
        assert_eq!(doc.sources[0].fields["Examples"], raw);
        assert!(!doc.evidence.iter().any(|e| e.field == "examples"));
    }
    let (capture, settings) = setup(
        "english_vocab",
        &[("Examples", r#"[{"sentence":"A cat.","translation":""}]"#)],
        &[("examples", "Examples")],
    );
    let doc = stage_document(&capture, &settings, "english_vocab").unwrap();
    let LearningContent::Vocabulary(v) = &doc.content else {
        panic!()
    };
    assert_eq!(v.examples.len(), 1);
}

#[test]
fn source_content_review_is_evidence_exact_and_cannot_waive_native_history() {
    use linguist_core::{
        records::ReviewChoice,
        review::{ResolutionRequest, resolve},
    };
    let (capture, mut settings) = setup(
        "english_vocab",
        &[
            ("Word", "<b>cat</b>"),
            ("Meaning", "feline"),
            ("Key", "cat-animal"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Meaning"),
            ("sense_key", "Key"),
        ],
    );
    let root = std::env::temp_dir().join(format!("lab-source-review-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root.to_str().unwrap()));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-source-review".into())]);
    let prepared =
        publish_capture_draft(&capture, &settings, "english_vocab", &environment).unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|i| i.code == "SOURCE_HTML_TEXT_REVIEW")
        .unwrap();
    let evidence = doc
        .evidence
        .iter()
        .find(|e| e.field == "expression")
        .unwrap();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::SourceContentVerified {
            source_id: doc.sources[0].id,
            evidence_ids: vec![evidence.id],
        },
    };
    let resolved = resolve(&plan, &request, "unix-seconds:1".into()).unwrap();
    assert!(!resolved.ready);
    assert!(
        !validation::validate(&resolved.revision.documents[0])
            .iter()
            .any(|i| i.id == issue.id)
    );
    assert!(
        validation::validate(&resolved.revision.documents[0])
            .iter()
            .any(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW")
    );
    assert_eq!(plan.documents[0].reviews.len(), 0);
    for ids in [
        vec![],
        vec![evidence.id, evidence.id],
        vec![uuid::Uuid::new_v4()],
    ] {
        request.choice = ReviewChoice::SourceContentVerified {
            source_id: doc.sources[0].id,
            evidence_ids: ids,
        };
        assert!(resolve(&plan, &request, "unix-seconds:1".into()).is_err());
    }
    request.choice = ReviewChoice::SourceContentVerified {
        source_id: doc.sources[0].id,
        evidence_ids: vec![evidence.id],
    };
    request.issue_id = doc
        .issues
        .iter()
        .find(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW")
        .unwrap()
        .id
        .clone();
    assert!(resolve(&plan, &request, "unix-seconds:1".into()).is_err());
    let mut changed = resolved.revision.documents[0].clone();
    changed.context.push_str("new context");
    assert!(
        validation::validate(&changed)
            .iter()
            .any(|i| i.code == "SOURCE_HTML_TEXT_REVIEW")
    );
    drop(store);
    let mut store = linguist_store::Store::open(&root).unwrap();
    store.publish_revision(&resolved.revision).unwrap();
    drop(store);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let restored = store.revision(prepared.plan_id, 2).unwrap();
    assert_eq!(
        restored.documents[0].reviews,
        resolved.revision.documents[0].reviews
    );
    let mut stale = restored.documents[0].clone();
    stale.context.push_str("after restart");
    assert!(
        validation::validate(&stale)
            .iter()
            .any(|i| i.code == "SOURCE_HTML_TEXT_REVIEW")
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
