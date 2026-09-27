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
    let fields = values
        .iter()
        .enumerate()
        .map(|(index, (name, value))| ((*name).to_owned(), json!({"value":value,"order":index})))
        .collect::<BTreeMap<_, _>>();
    let note = json!({"noteId":"123","modelName":"Legacy","fields":fields,"cards":["456"],"tags":["preserved"]});
    let model = json!({"model":{"name":"Legacy","id":"12"},"fields":values.iter().map(|(name,_)|name).collect::<Vec<_>>(),"templates":{"Card":{"Front":"front","Back":"back"}},"css":"style"});
    let cards = json!([{"cardId":"456","note":"123","reps":5}]);
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
        g.pattern.is_empty()
            && g.use_key.is_empty()
            && g.meaning.is_empty()
            && g.examples.is_empty()
    );
    assert_eq!(g.formation, "V + なら");
    for code in [
        "SOURCE_COMBINED_FIELD_REVIEW",
        "SOURCE_RICH_FIELD_REVIEW",
        "SOURCE_STRUCTURED_ROLE_REVIEW",
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
