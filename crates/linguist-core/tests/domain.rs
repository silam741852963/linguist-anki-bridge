use linguist_core::{canonical, document::*, legacy, render, validation};
use std::collections::BTreeMap;
fn vocabulary() -> LearningDocument {
    LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap()
}
#[test]
fn canonical_objects_sort_but_arrays_keep_order() {
    let a: serde_json::Value = canonical::parse(br#"{"b":2,"a":1}"#).unwrap();
    let b: serde_json::Value = canonical::parse(br#"{"a":1,"b":2}"#).unwrap();
    assert_eq!(canonical::bytes(&a).unwrap(), canonical::bytes(&b).unwrap());
    assert_ne!(
        canonical::digest("test", &vec![1, 2]).unwrap(),
        canonical::digest("test", &vec![2, 1]).unwrap()
    );
    let value = serde_json::json!({"\u{e000}":1,"\u{1f600}":2});
    assert_eq!(
        String::from_utf8(canonical::bytes(&value).unwrap()).unwrap(),
        "{\"😀\":2,\"\u{e000}\":1}"
    );
}
#[test]
fn strict_json_rejects_duplicate_unsafe_and_trailing_input() {
    for input in [r#"{"a":1,"a":2}"#, r#"{"id":9007199254740992}"#, r#"{} {}"#] {
        assert!(canonical::parse::<serde_json::Value>(input.as_bytes()).is_err());
    }
    assert!(canonical::bytes(&f64::NAN).is_err());
    assert!(canonical::parse::<AnkiId>(b"123").is_err());
    assert!(canonical::parse::<AnkiId>(br#""0123""#).is_err());
}
#[test]
fn field_intents_preserve_set_and_clear_distinctly() {
    let source = "old".to_owned();
    assert_eq!(
        FieldIntent::Keep.resolve(Some(&source)).unwrap(),
        Some(source)
    );
    assert!(FieldIntent::<String>::Keep.resolve(None).is_err());
    assert_eq!(
        FieldIntent::Set(String::new()).resolve(None).unwrap(),
        Some(String::new())
    );
    assert_eq!(FieldIntent::<String>::Clear.resolve(None).unwrap(), None);
}
#[test]
fn fixtures_render_fixed_models_and_roundtrip() {
    for bytes in [
        include_bytes!("../../../contracts/v2/fixtures/vocabulary.json").as_slice(),
        include_bytes!("../../../contracts/v2/fixtures/grammar.json").as_slice(),
    ] {
        let doc = LearningDocument::from_json(bytes).unwrap();
        assert!(validation::ready(&doc), "{:?}", validation::validate(&doc));
        let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
        assert_eq!(
            rendered.fields.len(),
            if matches!(doc.content, LearningContent::Vocabulary(_)) {
                18
            } else {
                15
            }
        );
        assert_eq!(
            doc,
            LearningDocument::from_json(&canonical::bytes(&doc).unwrap()).unwrap()
        );
        assert!(legacy::export_v1(&doc).is_err());
    }
}
#[test]
fn task_cues_reject_answer_leakage_and_invalid_task_kinds() {
    let mut doc = vocabulary();
    doc.requested_tasks.push(Task::Production);
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.production_prompt = "食べる means what?".into();
    }
    assert!(
        validation::validate(&doc)
            .iter()
            .any(|i| i.code == "ANSWER_LEAK")
    );
    doc.requested_tasks = vec![Task::Application];
    assert!(!validation::ready(&doc));
}
#[test]
fn html_and_media_cannot_bypass_typed_controls() {
    let clean = render::sanitize_reference(
        "<script>alert(1)</script><img src=x onerror=alert(2)><p onclick='x'>safe</p>",
    );
    assert_eq!(clean, "<p>safe</p>");
    let mut doc = vocabulary();
    doc.edits
        .insert("Audio".into(), FieldIntent::Set("[sound:evil.mp3]".into()));
    assert!(render::render(&doc, &BTreeMap::new()).is_err());
    for name in ["../x", "a[sound].mp3", "x\n.png", "x\".png"] {
        assert!(!validation::safe_media_name(name));
    }
}
#[test]
fn legacy_archive_is_lossless_without_promoting_readiness() {
    let bytes = include_bytes!("../../../legacy/contracts/fixtures/card-document.v1.json");
    let archive = legacy::LegacyArchive::from_json(bytes).unwrap();
    assert_eq!(
        *archive.original(),
        canonical::parse::<serde_json::Value>(&archive.to_v1_json().unwrap()).unwrap()
    );
}
#[test]
fn effective_html_empty_field_blocks_rendering() {
    let mut doc = vocabulary();
    doc.edits.insert(
        "Meaning".into(),
        FieldIntent::Set("<p> </p><script>evil()</script>".into()),
    );
    assert!(render::render(&doc, &BTreeMap::new()).is_err());
}
#[test]
fn review_is_bound_to_content_and_cannot_waive_errors() {
    use linguist_core::records::*;
    let mut doc = vocabulary();
    let id = uuid::Uuid::new_v4();
    doc.evidence.push(Evidence {
        id,
        field: "meaning".into(),
        provenance: Provenance::Generated,
        source_id: None,
        region_id: None,
        language: doc.explanation_language.clone(),
        claim: "to eat".into(),
        source_url: None,
        ambiguous: false,
    });
    assert!(!validation::ready(&doc));
    doc.reviews.push(ReviewDecision {
        id: uuid::Uuid::new_v4(),
        issue_id: format!("GENERATED_FACT_REVIEW:{id}"),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        created_at: "2026-09-26T00:00:00Z".into(),
        choice: ReviewChoice::ContentVerified {
            evidence_ids: vec![id],
        },
    });
    assert!(validation::ready(&doc));
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.meaning = "different content".into();
    }
    assert!(!validation::ready(&doc));
}
#[test]
fn approval_binds_content_but_excludes_review_timestamp_and_epoch() {
    use linguist_core::records::*;
    let doc = vocabulary();
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    let mut plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "settings".into(),
        },
        binding: Some(CollectionBinding {
            endpoint: "http://127.0.0.1:8765".into(),
            profile_fingerprint: "profile".into(),
            path_fingerprint: "path".into(),
            bridge_id: uuid::Uuid::new_v4(),
            lineage_id: uuid::Uuid::new_v4(),
            session_epoch: uuid::Uuid::new_v4(),
            capability_digest: "capabilities".into(),
        }),
        source_digest: "source".into(),
        selection: None,
        documents: vec![doc],
        rendered: vec![rendered],
        review_decisions: vec![ReviewDecision {
            id: uuid::Uuid::new_v4(),
            issue_id: "issue".into(),
            input_digest: "input".into(),
            actor: "reviewer".into(),
            created_at: "earlier".into(),
            choice: ReviewChoice::Sense("sense".into()),
        }],
    };
    let before = plan.approval_digest().unwrap();
    assert!(
        serde_json::to_value(&plan)
            .unwrap()
            .get("selection")
            .is_none()
    );
    plan.binding.as_mut().unwrap().session_epoch = uuid::Uuid::new_v4();
    plan.review_decisions[0].created_at = "later".into();
    assert_eq!(before, plan.approval_digest().unwrap());
    plan.documents[0].personal_notes = "created_at is literal user content".into();
    assert_ne!(before, plan.approval_digest().unwrap());
}

#[test]
fn example_translation_is_required_only_across_languages() {
    let mut doc = vocabulary();
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.examples[0].translation.clear();
    }
    let incomplete = |doc: &LearningDocument| {
        validation::validate(doc)
            .iter()
            .any(|i| i.code == "INCOMPLETE_EXAMPLE")
    };
    assert!(incomplete(&doc));
    doc.target_language = Language::try_from("en-GB".to_owned()).unwrap();
    doc.explanation_language = Language::try_from("en-US".to_owned()).unwrap();
    assert!(!incomplete(&doc));
    doc.explanation_language = Language::try_from("vi".to_owned()).unwrap();
    assert!(incomplete(&doc));
    doc.explanation_language = Language::try_from("en".to_owned()).unwrap();
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.examples[0].sentence.clear();
    }
    assert!(incomplete(&doc));
}

#[test]
fn model_comparison_reports_exact_content_without_claiming_task_ordinals() {
    let target = linguist_core::model::vocabulary();
    let templates = target
        .templates
        .iter()
        .map(|t| (t.name.clone(), (t.front.clone(), t.back.clone())))
        .collect();
    let exact = linguist_core::model::compare(
        &target.name,
        &target.fields,
        &templates,
        &target.css,
        &target,
    );
    assert!(exact.exact_content_match);
    assert!(exact.name_matches);
    assert!(!exact.source_template_ordinals_verified);
    assert!(!exact.apply_authorized);
    let renamed = linguist_core::model::compare(
        "User copy",
        &target.fields,
        &templates,
        &target.css,
        &target,
    );
    assert!(renamed.exact_content_match);
    assert!(!renamed.name_matches);
    let unrelated = linguist_core::model::compare(
        &target.name,
        &["Front".into(), "Back".into()],
        &BTreeMap::new(),
        "",
        &target,
    );
    assert!(!unrelated.exact_content_match);
    assert_eq!(unrelated.unexpected_fields, vec!["Front", "Back"]);
    assert!(
        unrelated
            .fields
            .iter()
            .all(|f| f.source_ordinals.is_empty())
    );
}

#[test]
fn model_comparison_retains_duplicate_fields_and_exposes_template_differences() {
    let target = linguist_core::model::vocabulary();
    let mut fields = target.fields.clone();
    fields.swap(0, 1);
    fields.push("Expression".into());
    fields.push("Audio and IPA".into());
    let mut templates: BTreeMap<_, _> = target
        .templates
        .iter()
        .map(|t| (t.name.clone(), (t.front.clone(), t.back.clone())))
        .collect();
    templates.get_mut("Comprehension").unwrap().0 = "Changed prompt".into();
    templates.remove("Spelling");
    templates.insert("User task".into(), ("Front".into(), "Back".into()));
    let report =
        linguist_core::model::compare("Picture Words", &fields, &templates, "changed CSS", &target);
    assert!(!report.exact_content_match);
    assert!(!report.field_order_matches);
    assert!(!report.css_matches);
    assert_eq!(report.duplicate_source_fields, vec!["Expression"]);
    assert_eq!(report.fields[0].source_ordinals, vec![1, 18]);
    assert_eq!(report.unexpected_fields, vec!["Audio and IPA"]);
    assert_eq!(report.unexpected_templates, vec!["User task"]);
    assert!(!report.templates[0].front_matches);
    assert!(report.templates[0].back_matches);
    assert!(!report.templates[2].present);
    assert_eq!(report.templates[2].target_ordinal, 2);
}
