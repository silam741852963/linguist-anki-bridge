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
fn canonical_bytes_and_domain_hash_match_ecmascript_vectors() {
    let vectors: Vec<serde_json::Value> = canonical::parse(include_bytes!(
        "../../../contracts/v2/fixtures/jcs-vectors.json"
    ))
    .unwrap();
    for vector in vectors {
        let value: serde_json::Value =
            canonical::parse(vector["input"].as_str().unwrap().as_bytes()).unwrap();
        assert_eq!(
            String::from_utf8(canonical::bytes(&value).unwrap()).unwrap(),
            vector["canonical"],
            "{}",
            vector["name"]
        );
        assert_eq!(
            canonical::digest("test-vector", &value).unwrap(),
            vector["digest"],
            "{}",
            vector["name"]
        );
    }
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
fn anki_ids_are_canonical_and_fit_the_wire_safe_integer_range() {
    for id in ["1", "123456789", "9007199254740991"] {
        assert_eq!(
            String::from(canonical::parse::<AnkiId>(format!("\"{id}\"").as_bytes()).unwrap()),
            id
        );
    }
    for id in [
        "0",
        "01",
        "-1",
        "+1",
        "1.0",
        "9007199254740992",
        "9223372036854775807",
    ] {
        assert!(
            canonical::parse::<AnkiId>(format!("\"{id}\"").as_bytes()).is_err(),
            "{id}"
        );
    }
    let schema = schemars::schema_for!(LearningDocument);
    let schema = serde_json::to_value(schema).unwrap();
    let id_schema = &schema["$defs"]["AnkiId"];
    assert!(id_schema["pattern"].as_str().unwrap().starts_with("^(?:"));
    assert_eq!(id_schema["minLength"], 1);
    assert_eq!(id_schema["maxLength"], 16);
}
#[test]
fn generated_contract_versions_match_the_supported_wire_versions() {
    fn version<T: schemars::JsonSchema>() -> serde_json::Value {
        let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        schema["properties"]["schema_version"].clone()
    }
    for schema in [
        version::<LearningDocument>(),
        version::<linguist_core::records::PlanRevision>(),
        version::<linguist_core::editing::PlanPatch>(),
        version::<linguist_core::review::ResolutionRequest>(),
        version::<linguist_core::plan_validation::ValidationEvidence>(),
    ] {
        assert_eq!(schema["minimum"], 2);
        assert_eq!(schema["maximum"], 2);
    }
    let schema = serde_json::to_value(schemars::schema_for!(
        linguist_core::records::SelectionReceipt
    ))
    .unwrap();
    assert_eq!(schema["properties"]["schema_version"]["minimum"], 1);
    assert_eq!(schema["properties"]["schema_version"]["maximum"], 1);
    for field in ["matched_note_ids", "selected_note_ids"] {
        assert_eq!(
            schema["properties"][field]["items"]["$ref"],
            "#/$defs/AnkiId"
        );
    }
    let selector = serde_json::to_value(schemars::schema_for!(
        linguist_core::records::SelectionInput
    ))
    .unwrap();
    assert_eq!(
        selector["oneOf"][0]["properties"]["input"]["items"]["$ref"],
        "#/$defs/AnkiId"
    );
}
#[test]
fn native_receipt_requires_readback_for_verified_effects() {
    use linguist_core::records::{NativeOperationReceipt, NativeReceiptState};
    let fixture: NativeOperationReceipt = canonical::parse(include_bytes!(
        "../../../contracts/v2/fixtures/native-operation-receipt.json"
    ))
    .unwrap();
    fixture.validate().unwrap();
    assert_eq!(
        fixture,
        canonical::parse(&canonical::bytes(&fixture).unwrap()).unwrap()
    );
    let mut value = serde_json::json!({
        "schema_version":1,
        "lineage_id":uuid::Uuid::new_v4(),
        "operation_id":uuid::Uuid::new_v4(),
        "session_epoch":uuid::Uuid::new_v4(),
        "payload_digest":"a".repeat(64),
        "approved_digest":format!("lab-jcs-v1:plan:{}", "b".repeat(64)),
        "state":"unknown",
        "readback":null,
        "evidence_digest":"c".repeat(64)
    });
    let receipt: NativeOperationReceipt =
        canonical::parse(&canonical::bytes(&value).unwrap()).unwrap();
    receipt.validate().unwrap();
    value["state"] = serde_json::json!("verified");
    let missing: NativeOperationReceipt =
        canonical::parse(&canonical::bytes(&value).unwrap()).unwrap();
    assert!(missing.validate().is_err());
    value["readback"] = serde_json::json!({
        "observed_state_digest":"d".repeat(64),
        "note_ids":["1"],
        "card_ids":["2"],
        "history_digest":null,
        "manifest_digests":["e".repeat(64)]
    });
    let verified: NativeOperationReceipt =
        canonical::parse(&canonical::bytes(&value).unwrap()).unwrap();
    assert_eq!(verified.state, NativeReceiptState::Verified);
    verified.validate().unwrap();
    value["readback"]["card_ids"] = serde_json::json!(["2", "2"]);
    let duplicate: NativeOperationReceipt =
        canonical::parse(&canonical::bytes(&value).unwrap()).unwrap();
    assert!(duplicate.validate().is_err());
    value["readback"]["card_ids"] = serde_json::json!(["9007199254740992"]);
    assert!(
        canonical::parse::<NativeOperationReceipt>(&canonical::bytes(&value).unwrap()).is_err()
    );
}
#[test]
fn resume_binding_decision_binds_one_operation_and_changed_epoch() {
    use linguist_core::records::ResumeBindingDecision;
    let bytes = include_bytes!("../../../contracts/v2/fixtures/resume-binding-decision.json");
    let decision: ResumeBindingDecision = canonical::parse(bytes).unwrap();
    decision.validate().unwrap();
    assert_eq!(
        decision,
        canonical::parse(&canonical::bytes(&decision).unwrap()).unwrap()
    );
    let mut changed = serde_json::to_value(&decision).unwrap();
    changed["new_binding"]["session_epoch"] = changed["old_binding"]["session_epoch"].clone();
    assert!(
        canonical::parse::<ResumeBindingDecision>(&canonical::bytes(&changed).unwrap())
            .unwrap()
            .validate()
            .is_err()
    );
    changed = serde_json::to_value(&decision).unwrap();
    changed["new_binding"]["lineage_id"] = serde_json::json!(uuid::Uuid::new_v4());
    assert!(
        canonical::parse::<ResumeBindingDecision>(&canonical::bytes(&changed).unwrap())
            .unwrap()
            .validate()
            .is_err()
    );
    changed = serde_json::to_value(&decision).unwrap();
    changed["observed_state_digest"] = serde_json::json!("not a digest");
    assert!(
        canonical::parse::<ResumeBindingDecision>(&canonical::bytes(&changed).unwrap())
            .unwrap()
            .validate()
            .is_err()
    );
}
#[test]
fn gate_and_capability_records_cannot_promote_declarations_to_tested() {
    use linguist_core::records::{CapabilityReport, GateEvidence};
    let gate: GateEvidence = canonical::parse(include_bytes!(
        "../../../contracts/v2/fixtures/gate-evidence-not-run.json"
    ))
    .unwrap();
    gate.validate().unwrap();
    assert_eq!(
        gate,
        canonical::parse(&canonical::bytes(&gate).unwrap()).unwrap()
    );
    let mut promoted = serde_json::to_value(&gate).unwrap();
    promoted["status"] = serde_json::json!("pass");
    let forged: GateEvidence = canonical::parse(&canonical::bytes(&promoted).unwrap()).unwrap();
    assert!(forged.validate().is_err());

    let report: CapabilityReport = canonical::parse(include_bytes!(
        "../../../contracts/v2/fixtures/capability-report.json"
    ))
    .unwrap();
    report.validate().unwrap();
    assert_eq!(
        report,
        canonical::parse(&canonical::bytes(&report).unwrap()).unwrap()
    );
    let mut promoted = serde_json::to_value(&report).unwrap();
    promoted["actions"][0]["state"] = serde_json::json!("gate_tested");
    let forged: CapabilityReport = canonical::parse(&canonical::bytes(&promoted).unwrap()).unwrap();
    assert!(forged.validate().is_err());
    promoted["actions"][0]["gate_id"] = serde_json::json!("EV-03");
    promoted["actions"][0]["gate_evidence_digest"] = serde_json::json!("b".repeat(64));
    let structurally_tested: CapabilityReport =
        canonical::parse(&canonical::bytes(&promoted).unwrap()).unwrap();
    structurally_tested.validate().unwrap();
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
        // Vocabulary v3 has nine fields, grammar v4 six.
        let expected = match doc.content {
            LearningContent::Vocabulary(_) => 9,
            LearningContent::Grammar(_) => 6,
        };
        assert_eq!(rendered.fields.len(), expected);
        assert_eq!(
            doc,
            LearningDocument::from_json(&canonical::bytes(&doc).unwrap()).unwrap()
        );
        assert!(legacy::export_v1(&doc).is_err());
    }
}
#[test]
fn rich_dictionary_roundtrip_keeps_all_senses_and_renders_inert_reference() {
    use linguist_core::records::{ReviewChoice, ReviewDecision};
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary-rich-dictionary.json"
    ))
    .unwrap();
    assert!(
        validation::validate(&doc)
            .iter()
            .any(|issue| issue.code == "DICTIONARY_SENSE_REVIEW")
    );
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!("expected vocabulary")
    };
    assert_eq!(vocab.dictionary[0].senses.len(), 2);
    assert_eq!(
        vocab.dictionary[0].senses[0].examples[0].provenance,
        Provenance::Dictionary
    );
    assert_eq!(vocab.dictionary[0].related_entries, ["edible", "meal"]);
    assert_eq!(
        doc,
        LearningDocument::from_json(&canonical::bytes(&doc).unwrap()).unwrap()
    );
    let input_digest = doc.semantic_digest().unwrap();
    doc.reviews.push(ReviewDecision {
        id: uuid::Uuid::new_v4(),
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest,
        actor: "reviewer".into(),
        created_at: "2026-10-01T00:00:00Z".into(),
        choice: ReviewChoice::Sense("consume-food".into()),
    });
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    let meaning = &rendered.fields["Meaning"];
    // WP-19: senses only, the selection marked by class, no provenance text.
    assert!(meaning.starts_with("<ol class=\"lab-senses\">"));
    assert_eq!(meaning.matches("lab-selected").count(), 1);
    assert!(meaning.contains("wear away"));
    for hidden in [
        "Sense:",
        "(selected)",
        "Wiktionary contributors",
        "We eat rice.",
        "consume-food",
        "https://",
    ] {
        assert!(!meaning.contains(hidden), "shows {hidden}: {meaning}");
    }
    for active in ["<script", "<img", "href=", "src=\"x"] {
        assert!(
            !meaning.contains(active),
            "active provider markup: {meaning}"
        );
    }
    let selected = meaning.find("lab-selected").unwrap();
    assert!(selected < meaning.find("wear away").unwrap());
    let tags = render::tags(&doc);
    assert!(tags.contains(&"lab::lang::en".to_string()), "{tags:?}");
    assert!(
        tags.contains(&"lab::kind::vocabulary".to_string()),
        "{tags:?}"
    );
}
#[test]
fn task_cues_reject_answer_leakage_and_invalid_task_kinds() {
    // A kana word's reading is the answer: it is not shown, so the Spelling
    // front needs the recording.
    let mut doc = vocabulary();
    doc.requested_tasks.push(Task::Spelling);
    doc.media
        .retain(|m| m.role != linguist_core::records::MediaRole::Audio);
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.expression = "たべる".into();
        v.pronunciation = "た べる".into();
        assert_eq!(render::spoken_cue(v), "");
    }
    let codes: Vec<_> = validation::validate(&doc)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(
        codes.contains(&"SPELLING_CUE_MISSING".to_owned()),
        "{codes:?}"
    );
    assert!(!codes.contains(&"ANSWER_LEAK".to_owned()), "{codes:?}");
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
    for bytes in [
        include_bytes!("../../../legacy/contracts/fixtures/card-document.v1.json").as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/dictionary-preserve-card.v1.json")
            .as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/grammar-card.v1.json").as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/injection-card.v1.json").as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/media-replacement-card.v1.json")
            .as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/modernization-card.v1.json").as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/shared-fields-card.v1.json").as_slice(),
        include_bytes!("../../../legacy/contracts/fixtures/validation-issues-card.v1.json")
            .as_slice(),
    ] {
        let archive = legacy::LegacyArchive::from_json(bytes).unwrap();
        assert_eq!(archive.to_v1_json().unwrap(), bytes);
        assert_eq!(archive.raw_digest, canonical::asset_digest(bytes));
        assert_eq!(
            *archive.original(),
            canonical::parse::<serde_json::Value>(bytes).unwrap()
        );
    }
    let compact = br#"{"schema_version":1,"extension":{"b":2,"a":1}}"#;
    let spaced = b"{ \"extension\": { \"a\": 1, \"b\": 2 }, \"schema_version\": 1 }\n";
    let a = legacy::LegacyArchive::from_json(compact).unwrap();
    let b = legacy::LegacyArchive::from_json(spaced).unwrap();
    assert_eq!(a.digest, b.digest);
    assert_ne!(a.raw_digest, b.raw_digest);
    assert_eq!(b.to_v1_json().unwrap(), spaced);
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
        target: None,
        source_span: None,
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
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
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
fn new_approval_binding_excludes_execution_settings_but_legacy_binding_is_stable() {
    use linguist_core::records::*;
    let mut plan = PlanRevision {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            version: 2,
            values: BTreeMap::from([
                ("llm.temperature".into(), serde_json::json!(0.0)),
                ("output.format".into(), serde_json::json!("text")),
                ("retry.read_attempts".into(), serde_json::json!(3)),
            ]),
            provenance: BTreeMap::from([
                ("llm.temperature".into(), "builtin".into()),
                ("output.format".into(), "builtin".into()),
                ("retry.read_attempts".into(), "builtin".into()),
            ]),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "all-a".into(),
            semantic_fingerprint: "semantic-a".into(),
            execution_fingerprint: "execution-a".into(),
        },
        binding: None,
        source_digest: "source".into(),
        selection: None,
        grammar_groups: vec![],
        documents: vec![vocabulary()],
        rendered: vec![],
        review_decisions: vec![],
    };
    let (semantic, execution) = setting_fingerprints(&plan.settings.values).unwrap();
    plan.settings.semantic_fingerprint = semantic;
    plan.settings.execution_fingerprint = execution;
    plan.settings.fingerprint =
        linguist_core::canonical::digest("resolved-settings", &plan.settings.values).unwrap();
    let approved = plan.approval_digest().unwrap();
    plan.settings
        .values
        .insert("output.format".into(), serde_json::json!("json"));
    plan.settings
        .values
        .insert("retry.read_attempts".into(), serde_json::json!(5));
    plan.settings
        .provenance
        .insert("output.format".into(), "flag".into());
    let (semantic, execution) = setting_fingerprints(&plan.settings.values).unwrap();
    plan.settings.semantic_fingerprint = semantic;
    plan.settings.execution_fingerprint = execution;
    plan.settings.fingerprint =
        linguist_core::canonical::digest("resolved-settings", &plan.settings.values).unwrap();
    assert_eq!(approved, plan.approval_digest().unwrap());
    plan.settings
        .values
        .insert("llm.temperature".into(), serde_json::json!(0.4));
    assert!(plan.approval_digest().is_err());
    let (semantic, execution) = setting_fingerprints(&plan.settings.values).unwrap();
    plan.settings.semantic_fingerprint = semantic;
    plan.settings.execution_fingerprint = execution;
    plan.settings.fingerprint =
        linguist_core::canonical::digest("resolved-settings", &plan.settings.values).unwrap();
    assert_ne!(approved, plan.approval_digest().unwrap());
    plan.settings
        .values
        .insert("llm.temperature".into(), serde_json::json!(0.0));
    plan.settings.semantic_fingerprint.clear();
    plan.settings.execution_fingerprint.clear();
    let legacy = plan.approval_digest().unwrap();
    plan.settings
        .values
        .insert("output.format".into(), serde_json::json!("text"));
    assert_ne!(legacy, plan.approval_digest().unwrap());
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
fn source_media_requires_a_matching_source_archive_asset() {
    use linguist_core::records::{MediaAsset, MediaOwner, MediaRole, SourceArchive, SourceRecord};

    let mut doc = vocabulary();
    let source_id = uuid::Uuid::new_v4();
    let digest = "a".repeat(64);
    doc.sources.push(SourceRecord {
        id: source_id,
        kind: "test".into(),
        location: "fixture".into(),
        digest: "b".repeat(64),
        text: None,
        fields: BTreeMap::new(),
        model_manifest: String::new(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec!["original.png".into()],
    });
    doc.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id,
        digest: "b".repeat(64),
        original_text: None,
        original_fields: BTreeMap::new(),
        asset_digests: vec![digest.clone()],
    });
    doc.media.push(MediaAsset {
        digest: digest.clone(),
        filename: "original.png".into(),
        original_filename: Some("original.png".into()),
        size_bytes: 1,
        mime: "image/png".into(),
        owner: MediaOwner::Source,
        role: MediaRole::Archive,
        source_id: Some(source_id),
        attribution: "source".into(),
        license: None,
    });
    let has_issue = |doc: &LearningDocument, code: &str| {
        validation::validate(doc)
            .iter()
            .any(|issue| issue.code == code)
    };
    assert!(!has_issue(&doc, "SOURCE_MEDIA_ARCHIVE_REQUIRED"));
    assert!(!has_issue(&doc, "MEDIA_SOURCE_MISSING"));
    assert!(
        serde_json::to_value(&doc.sources[0])
            .unwrap()
            .get("template_manifest")
            .is_none()
    );
    doc.sources[0].template_manifest = Some(digest.clone());
    doc.sources[0].captured_at_unix_seconds = Some(1);
    assert!(!has_issue(&doc, "SOURCE_TEMPLATE_ARCHIVE_REQUIRED"));
    doc.sources[0].captured_at_unix_seconds = Some(0);
    assert!(has_issue(&doc, "SOURCE_CAPTURE_TIME_INVALID"));
    doc.sources[0].captured_at_unix_seconds = Some(1);
    doc.sources[0].text = Some("raw source text".into());
    doc.archives[0].original_text = doc.sources[0].text.clone();
    assert!(has_issue(&doc, "SOURCE_TEXT_ASSET_REQUIRED"));
    doc.archives[0]
        .asset_digests
        .push(canonical::asset_digest(b"raw source text"));
    assert!(!has_issue(&doc, "SOURCE_TEXT_ASSET_REQUIRED"));
    doc.archives[0].original_text = Some("different".into());
    assert!(has_issue(&doc, "SOURCE_ARCHIVE_REQUIRED"));
    doc.archives[0].original_text = doc.sources[0].text.clone();

    doc.archives[0].asset_digests.clear();
    assert!(has_issue(&doc, "SOURCE_MEDIA_ARCHIVE_REQUIRED"));
    assert!(has_issue(&doc, "SOURCE_TEMPLATE_ARCHIVE_REQUIRED"));
    doc.archives[0].asset_digests.push(digest);
    doc.media[0].source_id = Some(uuid::Uuid::new_v4());
    assert!(has_issue(&doc, "MEDIA_SOURCE_MISSING"));
    assert!(has_issue(&doc, "SOURCE_MEDIA_ARCHIVE_REQUIRED"));
    doc.media[0].source_id = None;
    assert!(has_issue(&doc, "SOURCE_MEDIA_ARCHIVE_REQUIRED"));
}

#[test]
fn declared_task_maps_bind_source_model_and_fixed_target_ordinals() {
    use linguist_core::records::{SourceArchive, SourceRecord, SourceTaskMap};

    let mut map: SourceTaskMap = canonical::parse(include_bytes!(
        "../../../contracts/v2/fixtures/source-task-map.json"
    ))
    .unwrap();
    map.validate().unwrap();
    let mut invalid = map.clone();
    invalid.entries[1].source_ordinal = 0;
    assert!(invalid.validate().is_err());
    invalid = map.clone();
    invalid.entries[1].target_ordinal = 2;
    assert!(invalid.validate().is_err());
    invalid = map.clone();
    invalid.entries[1].target_task = Task::Recognition;
    assert!(invalid.validate().is_err());

    map.entries.truncate(1);
    let mut doc = vocabulary();
    doc.sources.push(SourceRecord {
        id: map.source_id,
        kind: "anki_read_capture_v2".into(),
        location: "anki_note:123".into(),
        digest: "b".repeat(64),
        text: None,
        fields: BTreeMap::new(),
        model_manifest: map.source_model_digest.clone(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    doc.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id: map.source_id,
        digest: "b".repeat(64),
        original_text: None,
        original_fields: BTreeMap::new(),
        asset_digests: vec!["b".repeat(64), map.source_model_digest.clone()],
    });
    doc.task_maps.push(map);
    let mapped_digest = doc.semantic_digest().unwrap();
    let mapped_issue = |doc: &LearningDocument| {
        validation::validate(doc)
            .iter()
            .any(|issue| issue.code == "SOURCE_TASK_MAP_INVALID")
    };
    assert!(!mapped_issue(&doc));
    doc.task_maps[0].source_model_digest = "d".repeat(64);
    assert_ne!(doc.semantic_digest().unwrap(), mapped_digest);
    assert!(mapped_issue(&doc));
    doc.task_maps[0].source_model_digest = "c".repeat(64);
    doc.task_maps[0].entries[0].target_task = Task::Production;
    assert!(mapped_issue(&doc));
}

#[test]
fn evidence_targets_and_source_spans_reject_stale_or_split_references() {
    use linguist_core::records::{
        Evidence, EvidenceTarget, SourceArchive, SourceRecord, SourceTextSpan,
    };

    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary-rich-dictionary.json"
    ))
    .unwrap();
    let source_id = uuid::Uuid::new_v4();
    let text = "猫abc";
    let digest = canonical::asset_digest(text.as_bytes());
    doc.sources.push(SourceRecord {
        id: source_id,
        kind: "test".into(),
        location: "fixture".into(),
        digest: digest.clone(),
        text: Some(text.into()),
        fields: BTreeMap::new(),
        model_manifest: String::new(),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    doc.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: Some(text.into()),
        original_fields: BTreeMap::new(),
        asset_digests: vec![digest],
    });
    let evidence_id = uuid::Uuid::new_v4();
    let evidence_index = doc.evidence.len();
    doc.evidence.push(Evidence {
        id: evidence_id,
        field: "meaning".into(),
        provenance: Provenance::Dictionary,
        source_id: Some(source_id),
        region_id: None,
        target: Some(EvidenceTarget::DictionarySense {
            entry_index: 0,
            sense_index: 0,
        }),
        source_span: Some(SourceTextSpan {
            start_byte: 0,
            end_byte: 3,
        }),
        language: doc.explanation_language.clone(),
        claim: "consume food".into(),
        source_url: None,
        ambiguous: false,
    });
    let has_issue = |doc: &LearningDocument, code: &str| {
        validation::validate(doc)
            .iter()
            .any(|issue| issue.code == code)
    };
    assert!(!has_issue(&doc, "EVIDENCE_TARGET_INVALID"));
    assert!(!has_issue(&doc, "EVIDENCE_SOURCE_SPAN_INVALID"));
    doc.evidence[evidence_index]
        .source_span
        .as_mut()
        .unwrap()
        .end_byte = 2;
    assert!(has_issue(&doc, "EVIDENCE_SOURCE_SPAN_INVALID"));
    doc.evidence[evidence_index]
        .source_span
        .as_mut()
        .unwrap()
        .end_byte = 3;
    doc.evidence[evidence_index].target = Some(EvidenceTarget::DictionarySense {
        entry_index: 0,
        sense_index: 99,
    });
    assert!(has_issue(&doc, "EVIDENCE_TARGET_INVALID"));
    doc.evidence[evidence_index].target = Some(EvidenceTarget::MediaAsset {
        digest: "a".repeat(64),
    });
    assert!(has_issue(&doc, "EVIDENCE_TARGET_INVALID"));
    doc.evidence[evidence_index].target = Some(EvidenceTarget::Example { index: 0 });
    assert!(has_issue(&doc, "EVIDENCE_TARGET_INVALID"));
    doc.evidence[evidence_index].provenance = Provenance::User;
    if let LearningContent::Vocabulary(vocab) = &mut doc.content {
        vocab.examples[0].evidence_ids.push(evidence_id);
    }
    assert!(!has_issue(&doc, "EVIDENCE_TARGET_INVALID"));
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
    assert_eq!(report.fields[0].source_ordinals, vec![1, 9]);
    assert_eq!(report.unexpected_fields, vec!["Audio and IPA"]);
    assert_eq!(report.unexpected_templates, vec!["User task"]);
    assert!(!report.templates[0].front_matches);
    assert!(report.templates[0].back_matches);
    assert!(!report.templates[2].present);
    assert_eq!(report.templates[2].target_ordinal, 2);
}
#[test]
fn v3_vocabulary_renders_fixed_sections_kanji_strokes_and_tags() {
    use linguist_core::records::{MediaAsset, MediaOwner, MediaRole};
    let mut doc = vocabulary();
    let stroke = "c".repeat(64);
    doc.requested_tasks = vec![Task::Comprehension, Task::Production, Task::Spelling];
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.pronunciation = String::new();
        v.reading = "たべる".into();
        v.usage = "Neutral; <any> meal.".into();
        v.nuance = vec![Contrast {
            expression: "召し上がる".into(),
            difference: "Honorific; use for a superior's eating.".into(),
        }];
        v.collocations = vec![Collocation {
            phrase: "ご飯を食べる".into(),
            gloss: "eat a meal".into(),
        }];
        v.examples = vec![Example {
            sentence: "朝ご飯を食べる。".into(),
            translation: "I eat breakfast.".into(),
            provenance: Provenance::User,
            evidence_ids: vec![],
        }];
        v.kanji = "legacy text is dropped".into();
        v.kanji_details = vec![KanjiDetail {
            character: "食".into(),
            meanings: vec!["eat".into(), "food".into()],
            on_readings: vec!["ショク".into()],
            kun_readings: vec!["た.べる".into()],
            strokes: Some(9),
            radical: Some("食 — eat".into()),
            parts: vec!["人".into(), "良".into()],
            jlpt: Some("N5".into()),
            stroke_digest: Some(stroke.clone()),
        }];
    }
    doc.media.push(MediaAsset {
        digest: stroke.clone(),
        filename: format!("lab_{stroke}.gif"),
        original_filename: Some("98df.gif".into()),
        size_bytes: 10,
        mime: "image/gif".into(),
        owner: MediaOwner::App,
        role: MediaRole::KanjiStroke,
        source_id: None,
        attribution: "KanjiVG".into(),
        license: Some("CC-BY-SA-3.0".into()),
    });
    assert!(validation::ready(&doc), "{:?}", validation::validate(&doc));
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    assert_eq!(rendered.model.name, "Linguist Vocabulary v3");
    assert_eq!(
        rendered.fields.keys().cloned().collect::<Vec<_>>(),
        [
            "Audio",
            "EnableProduction",
            "EnableSpelling",
            "Expression",
            "Kanji",
            "Meaning",
            "Picture",
            "Pronunciation",
            "UsageExamples"
        ]
    );
    assert_eq!(rendered.fields["Pronunciation"], "たべる");
    // Spaces older notes put at kanji boundaries are dropped from kana only.
    for (authored, shown) in [("た\u{a0}べ る", "たべる"), ("ta be ru", "ta be ru")] {
        let mut spaced = doc.clone();
        if let LearningContent::Vocabulary(v) = &mut spaced.content {
            v.pronunciation = authored.into();
        }
        let fields = render::render(&spaced, &BTreeMap::new()).unwrap().fields;
        assert_eq!(fields["Pronunciation"], shown);
    }
    // The same name with the same bytes is one file; other bytes collide.
    let mut twice = doc.clone();
    let mut copy = twice.media.last().unwrap().clone();
    copy.filename = copy.filename.to_uppercase().replace(".GIF", ".gif");
    twice.media.push(copy.clone());
    let collides = |d: &LearningDocument| {
        validation::validate(d)
            .iter()
            .any(|i| i.code == "MEDIA_NAME_COLLISION")
    };
    assert!(!collides(&twice));
    twice.media.last_mut().unwrap().digest = "0".repeat(64);
    assert!(collides(&twice));
    assert_eq!(rendered.fields["EnableSpelling"], "1");
    let usage = &rendered.fields["UsageExamples"];
    for text in [
        "<h4>Usage</h4><p>Neutral; &lt;any&gt; meal.</p>",
        "<dt>召し上がる</dt><dd>Honorific; use for a superior's eating.</dd>",
        "ご飯を<b class=\"lab-hl\">食べる</b>",
        "<div class=\"lab-tr\">I eat breakfast.</div>",
    ] {
        assert!(usage.contains(text), "missing {text}: {usage}");
    }
    let kanji = &rendered.fields["Kanji"];
    assert!(kanji.contains(&format!("<img src=\"lab_{stroke}.gif\">")));
    assert!(kanji.contains("<b>ON</b>ショク"));
    assert!(kanji.contains("9 strokes · radical 食 — eat · parts 人 良 · N5"));
    assert!(!kanji.contains("legacy text"));
    assert!(rendered.media_digests.contains(&stroke));
    let tags = render::tags(&doc);
    for tag in [
        "lab::lang::ja",
        "lab::kind::vocabulary",
        "lab::task::spelling",
        "lab::has::kanji",
    ] {
        assert!(tags.contains(&tag.to_owned()), "{tags:?}");
    }
    // A stroke asset no kanji detail points to is not a valid render input.
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.kanji_details[0].stroke_digest = None;
    }
    assert!(
        validation::validate(&doc)
            .iter()
            .any(|issue| issue.code == "INVALID_MEDIA_TYPE")
    );
}
#[test]
fn escaped_text_matches_the_html_text_serializer() {
    assert_eq!(
        render::escape("a & b <i> \"q\" 'q' \u{a0}x"),
        "a &amp; b &lt;i&gt; \"q\" 'q' &nbsp;x"
    );
}
#[test]
fn v3_related_dictionary_words_render_on_the_back_only() {
    let mut doc = vocabulary();
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.sense_key = "eat".into();
        v.meaning = "to eat".into();
        v.dictionary = serde_json::from_value(serde_json::json!([
            {"provider":"jisho","source_url":"https://jisho.org/x","language":"ja",
             "forms":["食べる","喰べる"],"readings":["たべる"],
             "senses":[{"key":"eat","definitions":["to eat"],"labels":["Ichidan verb"]}],
             "related_entries":["召し上がる"]},
            {"provider":"jisho","source_url":"https://jisho.org/y","language":"ja",
             "forms":["食べ物"],"readings":["たべもの"],
             "senses":[{"key":"food","definitions":["food","<b>provisions</b>"],"labels":[]}]}
        ]))
        .unwrap();
    }
    let input_digest = doc.semantic_digest().unwrap();
    doc.reviews.push(linguist_core::records::ReviewDecision {
        id: uuid::Uuid::new_v4(),
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest,
        actor: "reviewer".into(),
        created_at: "2026-10-08T00:00:00Z".into(),
        choice: linguist_core::records::ReviewChoice::Sense("eat".into()),
    });
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    let back = &rendered.fields["UsageExamples"];
    for text in [
        "<h4>Also written</h4><ul class=\"lab-collocations\"><li><span class=\"lab-target\">喰べる</span></li></ul>",
        "<h4>Related words</h4>",
        "<div class=\"lab-target\"><b>食べ物</b> <span class=\"lab-tr\">たべもの</span></div>",
        "food; &lt;b&gt;provisions&lt;/b&gt;",
        "<h4>See also</h4><ul class=\"lab-collocations\"><li><span class=\"lab-target\">召し上がる</span></li></ul>",
    ] {
        assert!(back.contains(text), "missing {text}: {back}");
    }
    // Fronts show Meaning, which never lists neighbours.
    assert!(!rendered.fields["Meaning"].contains("食べ物"));
    assert!(!rendered.fields["Meaning"].contains("召し上がる"));
}

#[test]
fn english_vocabulary_renders_exactly_the_english_model_fields() {
    let mut doc = vocabulary();
    doc.target_language = "en".to_owned().try_into().unwrap();
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.expression = "eat".into();
        v.reading.clear();
        v.pronunciation = "/iːt/".into();
    }
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    assert_eq!(rendered.model.name, "Linguist English Vocabulary v1");
    assert_eq!(
        rendered.fields.keys().collect::<Vec<_>>(),
        rendered
            .model
            .fields
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert!(!rendered.fields.contains_key("Kanji"));
    assert!(render::tags(&doc).contains(&"lab::lang::en".to_string()));
}

#[test]
fn only_an_exact_dictionary_entry_asks_for_a_sense() {
    let mut doc = vocabulary();
    let entry = |forms: &[&str], readings: &[&str]| DictionaryEntry {
        provider: "jisho-api-v1".into(),
        source_url: "https://jisho.org/word/x".into(),
        language: doc.target_language.clone(),
        forms: forms.iter().map(|s| s.to_string()).collect(),
        readings: readings.iter().map(|s| s.to_string()).collect(),
        senses: vec![Sense {
            key: "lab-jcs-v1:jisho-sense:1".into(),
            definitions: vec!["vice".into()],
            labels: vec![],
            examples: vec![],
        }],
        metadata: Default::default(),
        related_entries: vec![],
    };
    let asks = |doc: &LearningDocument| {
        validation::validate(doc)
            .iter()
            .any(|i| i.code == "DICTIONARY_SENSE_REVIEW")
    };
    // A partial match (副 for 副委員長) leaves the meaning to the author.
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.expression = "副委員長".into();
        v.dictionary = vec![entry(&["副"], &["ふく"])];
    }
    assert!(!asks(&doc));
    // A written form, or a reading for a kana word, is exact.
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.dictionary
            .push(entry(&["副委員長"], &["ふくいいんちょう"]));
    }
    assert!(asks(&doc));
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.expression = "ビタミン".into();
        v.dictionary = vec![entry(&[], &["ビタミン"])];
    }
    assert!(asks(&doc));
}

fn grammar(pattern: &str) -> Grammar {
    Grammar {
        pattern: pattern.into(),
        ..Default::default()
    }
}

#[test]
fn grammar_forms_drop_slots_and_expand_optional_parts() {
    assert_eq!(
        render::grammar_forms(&grammar("〜に加え（て）")),
        ["に加えて", "に加え"]
    );
    assert_eq!(render::grammar_forms(&grammar("N + だらけ")), ["だらけ"]);
    // A leading な that is part of the pattern is kept; Aな/Aい markers are not.
    assert_eq!(
        render::grammar_forms(&grammar("〜ないでください")),
        ["ないでください"]
    );
    assert_eq!(
        render::grammar_forms(&grammar("Aい／Aな／Vる＋ほど")),
        ["ほど"]
    );
    assert_eq!(
        render::grammar_forms(&grammar("V-て + もいい")),
        ["てもいい"]
    );
    // Reviewed forms join the derived ones; the longest match wins.
    let mut joined = grammar("〜に加え（て）");
    joined.forms = vec!["に加え".into(), "加えて".into()];
    assert_eq!(
        render::grammar_forms(&joined),
        ["に加えて", "に加え", "加えて"]
    );
    let mut reviewed = grammar("〜ために");
    reviewed.forms = vec!["〜ために".into(), "ための".into()];
    assert_eq!(render::grammar_forms(&reviewed), ["ために", "ための"]);
}

#[test]
fn grammar_v4_renders_every_example_with_its_own_audio() {
    use linguist_core::records::{Evidence, EvidenceTarget, MediaAsset, MediaOwner, MediaRole};
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/grammar.json"
    ))
    .unwrap();
    let LearningContent::Grammar(g) = &mut doc.content else {
        panic!()
    };
    g.pattern = "〜ても".into();
    g.source_meaning = "Dù ~ cũng".into();
    g.usage = "Concession.".into();
    g.jlpt = "n4".into();
    g.lesson = "Minna 25".into();
    g.nuance = vec![Contrast {
        expression: "〜のに".into(),
        difference: "Adds surprise or regret.".into(),
    }];
    let mut second = g.examples[0].clone();
    second.sentence = "高くても買います。".into();
    g.examples.push(second);
    // One reading of each example, plus one of a sentence no longer listed.
    for (digest, text) in [
        ("a".repeat(64), "雨が降っても行きます。"),
        ("b".repeat(64), "高くても買います。"),
        ("c".repeat(64), "消えた例文。"),
    ] {
        doc.media.push(MediaAsset {
            digest: digest.clone(),
            filename: format!("lab_{digest}.wav"),
            original_filename: None,
            size_bytes: 10,
            mime: "audio/wav".into(),
            owner: MediaOwner::App,
            role: MediaRole::Audio,
            source_id: None,
            attribution: "test".into(),
            license: None,
        });
        doc.evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field: "audio".into(),
            provenance: Provenance::Provider,
            source_id: None,
            region_id: None,
            target: Some(EvidenceTarget::MediaAsset { digest }),
            source_span: None,
            language: doc.target_language.clone(),
            claim: serde_json::json!({ "text": text }).to_string(),
            source_url: None,
            ambiguous: true,
        });
    }
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    assert_eq!(rendered.model.name, "Linguist Grammar v4");
    let example = &rendered.fields["Example"];
    assert!(example.contains(&format!(
        "[sound:lab_{}.wav]<span class=\"lab-target\">雨が降っ<b class=\"lab-hl\">ても</b>行きます。</span>",
        "a".repeat(64)
    )));
    assert!(example.contains(&format!("[sound:lab_{}.wav]<span", "b".repeat(64))));
    assert!(!example.contains(&"c".repeat(64)));
    assert!(!rendered.media_digests.contains(&"c".repeat(64)));
    assert_eq!(
        rendered.fields["Meaning"],
        "<div class=\"lab-gloss\">Ngay cả khi</div>"
    );
    assert_eq!(rendered.fields["Usage"], "<p>Concession.</p>");
    assert!(rendered.fields["Nuance"].contains("<dt>〜のに</dt>"));
    for gone in [
        "UsageExamples",
        "ExercisePrompt",
        "ExerciseAnswer",
        "EnableApplication",
        "Audio",
    ] {
        assert!(!rendered.fields.contains_key(gone), "{gone}");
    }
    doc.requested_tasks.push(Task::Application);
    assert!(
        validation::validate(&doc)
            .iter()
            .any(|i| i.code == "GRAMMAR_APPLICATION_RETIRED")
    );
    doc.requested_tasks.pop();
    let tags = render::tags(&doc);
    for tag in [
        "lab::jlpt::n4",
        "lab::lesson::minna-25",
        "lab::kind::grammar",
        "lab::explain::vi",
    ] {
        assert!(tags.contains(&tag.to_string()), "{tags:?}");
    }
}
