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
    let model = linguist_application::source_archive::fixture_model_manifest(
        json!({"model":{"name":"Legacy","id":"12"},"fields":values.iter().map(|(name,_)|name).collect::<Vec<_>>(),"templates":{"Card":{"Front":"front","Back":"back"}},"css":"style"}),
    );
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
fn archive_disposition_resolves_format_review_without_rendering_invalid_bytes() {
    use linguist_core::{
        records::{MediaRole, ReviewChoice},
        review::ResolutionRequest,
    };
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Media", "<img src=\"broken.png\">")],
        &[("expression", "Word")],
    );
    let bytes = vec![1, 2, 3];
    let digest = linguist_core::canonical::asset_digest(&bytes);
    linguist_application::source_archive::media::attach_original_media(
        &mut capture.captured,
        BTreeMap::from([("broken.png".into(), Some(bytes.clone()))]),
        10 * 1024 * 1024,
        10 * 1024 * 1024,
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!("lab-archive-review-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_vocab",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-archive-review".into())]),
    )
    .unwrap();
    let mut store = linguist_store::Store::open(&root).unwrap();
    let mut plan = store.revision(prepared.plan_id, 1).unwrap();
    for code in ["SOURCE_MEDIA_CONTENT_REVIEW", "SOURCE_MEDIA_FORMAT_REVIEW"] {
        let doc = &plan.documents[0];
        let issue = doc.issues.iter().find(|i| i.code == code).unwrap();
        let evidence = doc
            .evidence
            .iter()
            .find(|e| e.field == "media_format")
            .unwrap();
        let mut request = ResolutionRequest {
            schema_version: 2,
            base_revision: plan.revision,
            base_digest: plan.approval_digest().unwrap(),
            document_id: doc.id,
            issue_id: issue.id.clone(),
            input_digest: doc.semantic_digest().unwrap(),
            actor: "source owner".into(),
            choice: ReviewChoice::SourceMediaRole {
                source_id: doc.sources[0].id,
                asset_digest: digest.clone(),
                original_filename: "broken.png".into(),
                evidence_id: evidence.id,
                role: MediaRole::Picture,
                attribution: "Retain original bytes for recovery; omit from cards".into(),
                license: None,
            },
        };
        assert!(
            linguist_application::review::resolve(&store, &plan, &request, "now".into()).is_err()
        );
        if let ReviewChoice::SourceMediaRole { role, .. } = &mut request.choice {
            *role = MediaRole::Archive;
        }
        let result =
            linguist_application::review::resolve(&store, &plan, &request, "now".into()).unwrap();
        assert!(!result.ready);
        store.publish_revision(&result.revision).unwrap();
        plan = result.revision;
    }
    assert_eq!(plan.documents[0].media[0].role, MediaRole::Archive);
    assert!(
        !validation::validate(&plan.documents[0])
            .iter()
            .any(|i| i.code == "SOURCE_MEDIA_CONTENT_REVIEW"
                || i.code == "SOURCE_MEDIA_FORMAT_REVIEW")
    );
    assert_eq!(store.asset(&digest, 100000).unwrap(), bytes);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_audio_receipt_and_archive_role_survive_restart() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Media", "[sound:misnamed.png]")],
        &[("expression", "Word")],
    );
    let bytes = include_bytes!("fixtures/audio/tone.ogg").to_vec();
    let digest = linguist_core::canonical::asset_digest(&bytes);
    linguist_application::source_archive::media::attach_original_media(
        &mut capture.captured,
        BTreeMap::from([("misnamed.png".into(), Some(bytes.clone()))]),
        10 * 1024 * 1024,
        10 * 1024 * 1024,
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!("lab-source-audio-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-audio-test".into())]);
    let prepared =
        publish_capture_draft(&capture, &settings, "english_vocab", &environment).unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    assert_eq!(doc.media[0].mime, "audio/ogg");
    assert_eq!(
        doc.media[0].role,
        linguist_core::records::MediaRole::Archive
    );
    assert!(
        !doc.issues
            .iter()
            .any(|i| i.code == "SOURCE_AUDIO_COMPLETENESS_REVIEW")
    );
    let evidence = doc
        .evidence
        .iter()
        .find(|e| e.field == "media_format")
        .unwrap();
    let receipt: serde_json::Value = serde_json::from_str(&evidence.claim).unwrap();
    assert_eq!(receipt["asset_digest"], digest);
    assert_eq!(receipt["inspection"]["container_extent_verified"], true);
    assert_eq!(receipt["inspection"]["stream_end_observed"], true);
    assert_eq!(store.asset(&digest, 100000).unwrap(), bytes);
    let issue = doc
        .issues
        .iter()
        .find(|i| i.code == "SOURCE_MEDIA_CONTENT_REVIEW")
        .unwrap();
    let mut request = linguist_core::review::ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "recording owner".into(),
        choice: linguist_core::records::ReviewChoice::SourceMediaRole {
            source_id: doc.sources[0].id,
            asset_digest: digest.clone(),
            original_filename: "misnamed.png".into(),
            evidence_id: evidence.id,
            role: linguist_core::records::MediaRole::Audio,
            attribution: "Existing recording reviewed for reuse by its owner".into(),
            license: None,
        },
    };
    let result =
        linguist_application::review::resolve(&store, &plan, &request, "now".into()).unwrap();
    assert!(!result.ready); // Native identity/history remains unresolved.
    assert_eq!(
        result.revision.documents[0].media[0].role,
        linguist_core::records::MediaRole::Audio
    );
    assert_eq!(
        result.revision.documents[0].media[0].filename,
        format!("lab_{digest}.ogg")
    );
    assert_eq!(
        result.revision.documents[0].media[0]
            .original_filename
            .as_deref(),
        Some("misnamed.png")
    );
    assert_eq!(
        plan.documents[0].media[0].role,
        linguist_core::records::MediaRole::Archive
    );
    assert_eq!(result.revision.documents[0].archives, doc.archives);
    assert!(
        !validation::validate(&result.revision.documents[0])
            .iter()
            .any(|i| i.id == issue.id)
    );
    let mut forged = plan.clone();
    let fake = forged.documents[0]
        .evidence
        .iter_mut()
        .find(|e| e.id == evidence.id)
        .unwrap();
    let mut receipt: serde_json::Value = serde_json::from_str(&fake.claim).unwrap();
    receipt["inspection"]["sample_rate"] = json!(999);
    fake.claim = receipt.to_string();
    request.base_digest = forged.approval_digest().unwrap();
    request.input_digest = forged.documents[0].semantic_digest().unwrap();
    assert_eq!(
        linguist_application::review::resolve(&store, &forged, &request, "now".into()).unwrap_err(),
        "REVIEW_MEDIA_INSPECTION_CONFLICT"
    );
    let mut disabled = plan.clone();
    disabled
        .settings
        .values
        .insert("audio.provider".into(), json!("disabled"));
    (
        disabled.settings.semantic_fingerprint,
        disabled.settings.execution_fingerprint,
    ) = setting_fingerprints(&disabled.settings.values).unwrap();
    disabled.settings.fingerprint =
        linguist_core::canonical::digest("resolved-settings", &disabled.settings.values).unwrap();
    request.base_digest = disabled.approval_digest().unwrap();
    request.input_digest = doc.semantic_digest().unwrap();
    assert_eq!(
        linguist_application::review::resolve(&store, &disabled, &request, "now".into())
            .unwrap_err(),
        "REVIEW_MEDIA_FROZEN_POLICY_CONFLICT"
    );
    drop(store);
    let mut store = linguist_store::Store::open(&root).unwrap();
    store.publish_revision(&result.revision).unwrap();
    drop(store);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let reopened = store.revision(prepared.plan_id, 2).unwrap();
    assert_eq!(
        reopened.documents[0].media[0].role,
        linguist_core::records::MediaRole::Audio
    );
    assert_eq!(store.asset(&digest, 100000).unwrap(), bytes);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn decoded_source_image_has_digest_linked_evidence_but_remains_archive_only() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Media", "<img src=\"misnamed.mp3\">")],
        &[("expression", "Word")],
    );
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(2, 3))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    let bytes = output.into_inner();
    let digest = linguist_core::canonical::asset_digest(&bytes);
    linguist_application::source_archive::media::attach_original_media(
        &mut capture.captured,
        BTreeMap::from([("misnamed.mp3".into(), Some(bytes.clone()))]),
        10 * 1024 * 1024,
        10 * 1024 * 1024,
    )
    .unwrap();
    let document = stage_document(&capture, &settings, "english_vocab").unwrap();
    assert_eq!(document.media[0].mime, "image/png");
    assert_eq!(
        document.media[0].role,
        linguist_core::records::MediaRole::Archive
    );
    let evidence = document
        .evidence
        .iter()
        .find(|e| e.field == "media_format")
        .unwrap();
    assert_eq!(
        evidence.target,
        Some(linguist_core::records::EvidenceTarget::MediaAsset {
            digest: digest.clone(),
        })
    );
    let receipt: serde_json::Value = serde_json::from_str(&evidence.claim).unwrap();
    assert_eq!(receipt["asset_digest"], digest);
    assert_eq!(receipt["inspection"]["height"], 3);
    assert_eq!(capture.captured.assets[&digest], bytes);
    let root = std::env::temp_dir().join(format!("lab-picture-review-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_vocab",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-picture-review".into())]),
    )
    .unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    let request = linguist_core::review::ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: doc
            .issues
            .iter()
            .find(|i| i.code == "SOURCE_MEDIA_CONTENT_REVIEW")
            .unwrap()
            .id
            .clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "image owner".into(),
        choice: linguist_core::records::ReviewChoice::SourceMediaRole {
            source_id: doc.sources[0].id,
            asset_digest: digest.clone(),
            original_filename: "misnamed.mp3".into(),
            evidence_id: doc
                .evidence
                .iter()
                .find(|e| e.field == "media_format")
                .unwrap()
                .id,
            role: linguist_core::records::MediaRole::Picture,
            attribution: "Image reviewed for reuse by its owner".into(),
            license: None,
        },
    };
    let resolved =
        linguist_application::review::resolve(&store, &plan, &request, "now".into()).unwrap();
    assert_eq!(
        resolved.revision.documents[0].media[0].filename,
        format!("lab_{digest}.png")
    );
    assert!(!resolved.ready);
    let mut omitted = plan.clone();
    omitted
        .settings
        .values
        .insert("images.existing_policy".into(), json!("omit_reference"));
    (
        omitted.settings.semantic_fingerprint,
        omitted.settings.execution_fingerprint,
    ) = setting_fingerprints(&omitted.settings.values).unwrap();
    omitted.settings.fingerprint =
        linguist_core::canonical::digest("resolved-settings", &omitted.settings.values).unwrap();
    let mut conflict = request;
    conflict.base_digest = omitted.approval_digest().unwrap();
    assert_eq!(
        linguist_application::review::resolve(&store, &omitted, &conflict, "now".into())
            .unwrap_err(),
        "REVIEW_MEDIA_FROZEN_POLICY_CONFLICT"
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        document
            .issues
            .iter()
            .any(|i| i.code == "SOURCE_MEDIA_CONTENT_REVIEW")
    );
    settings
        .values
        .insert("media.allowed_image_types".into(), json!(["image/jpeg"]));
    let document = stage_document(&capture, &settings, "english_vocab").unwrap();
    assert_eq!(document.media[0].mime, "application/octet-stream");
    let evidence = document
        .evidence
        .iter()
        .find(|e| e.field == "media_format")
        .unwrap();
    assert!(evidence.ambiguous);
    let receipt: serde_json::Value = serde_json::from_str(&evidence.claim).unwrap();
    assert_eq!(receipt["asset_digest"], digest);
    assert_eq!(receipt["failure"]["code"], "IMAGE_FORMAT_DISALLOWED");
    assert!(receipt.get("inspection").is_none());
    assert!(
        document
            .issues
            .iter()
            .find(|i| i.code == "SOURCE_MEDIA_FORMAT_REVIEW")
            .unwrap()
            .message
            .contains("media.allowed_image_types")
    );
    assert!(
        document
            .issues
            .iter()
            .any(|i| i.code == "SOURCE_MEDIA_FORMAT_REVIEW")
    );
    settings
        .values
        .insert("media.max_asset_mb".into(), json!(0));
    assert!(stage_document(&capture, &settings, "english_vocab").is_err());
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
fn invalid_query_and_purpose_fail_before_search_or_state_creation() {
    let (_, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat")],
        &[("expression", "Word")],
    );
    let root =
        std::env::temp_dir().join(format!("lab-revamp-query-reject-{}", uuid::Uuid::new_v4()));
    for (key, value) in [
        ("llm.enabled", json!(false)),
        ("dictionary.provider", json!("authored")),
        ("images.search_when_missing", json!(false)),
        ("anki.endpoint", json!("http://127.0.0.1:1")),
        ("storage.state_dir", json!(root)),
    ] {
        settings.values.insert(key.into(), value);
    }
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-home".into())]);
    let client = linguist_anki::Client::from_settings(&settings, &environment).unwrap();
    for (query, purpose, expected) in [
        (" ".to_owned(), "english_vocab", "REVAMP_QUERY_EMPTY"),
        (
            "x".repeat(settings.values["input.max_record_chars"].as_u64().unwrap() as usize + 1),
            "english_vocab",
            "REVAMP_QUERY_LIMIT",
        ),
        (
            "tag:source".to_owned(),
            "unsupported",
            "SOURCE_MAPPING_PURPOSE_UNSUPPORTED",
        ),
    ] {
        assert_eq!(
            prepare_source_selection(
                &client,
                &settings,
                purpose,
                SourceSelector::Query(query),
                &environment
            )
            .unwrap_err(),
            expected
        );
        assert!(!root.exists());
    }
    assert_eq!(
        prepare_source_selection_limited(
            &client,
            &settings,
            "english_vocab",
            SourceSelector::NoteIds(vec!["123".into()]),
            &environment,
            Some(1)
        )
        .unwrap_err(),
        "REVAMP_EXPLICIT_IDS_LIMIT_CONFLICT"
    );
    assert_eq!(
        prepare_source_selection_limited(
            &client,
            &settings,
            "english_vocab",
            SourceSelector::Query("tag:source".into()),
            &environment,
            Some(0)
        )
        .unwrap_err(),
        "REVAMP_SELECTION_LIMIT_INVALID"
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
    let choices = linguist_core::review::decision_templates(doc, issue);
    assert_eq!(choices, vec![request.choice.clone()]);
    let page = linguist_application::review::inspection::page(&plan, Some(doc.id), 0, 100).unwrap();
    assert!(
        page["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["issue"]["code"] == "SOURCE_HTML_TEXT_REVIEW"
                && entry["templates"][0]["choice"]["decision"] == "source_content_verified")
    );
    assert!(page["issues"].as_array().unwrap().iter().any(|entry| {
        entry["issue"]["code"] == "SOURCE_NATIVE_HISTORY_REVIEW"
            && entry["resolution_available"] == true
            && entry["templates"] == json!([])
            && entry["resolution_command"]
                .as_str()
                .is_some_and(|c| c.contains("plans resolve-history"))
    }));
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

#[test]
fn dictionary_enrichment_retains_source_revision_and_requires_sense_review() {
    struct Dictionary;
    impl linguist_application::DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &linguist_core::Language,
        ) -> std::result::Result<linguist_dictionary::DictionaryPage, String> {
            linguist_dictionary::wiktionary::parse_definition(query, target,
                br#"{"en":[{"language":"English","partOfSpeech":"Verb","definitions":[{"definition":"Consume food"},{"definition":"Wear away"}]}]}"#,
                1024, 10).map_err(|error| error.to_string())
        }
    }
    struct Failure;
    impl linguist_application::DictionaryPort for Failure {
        fn lookup(
            &self,
            _: &str,
            _: &linguist_core::Language,
        ) -> std::result::Result<linguist_dictionary::DictionaryPage, String> {
            Err("DICTIONARY_PROVIDER_FAILED: fixture unavailable".into())
        }
    }
    let (capture, mut settings) = setup(
        "english_vocab",
        &[
            ("Word", "eat"),
            ("Meaning", "My original meaning"),
            ("Private", "Keep this"),
        ],
        &[("expression", "Word"), ("meaning", "Meaning")],
    );
    let root = std::env::temp_dir().join(format!("lab-dictionary-revamp-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    settings
        .values
        .insert("dictionary.provider".into(), json!("wiktionary"));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_vocab",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-dictionary-revamp".into())]),
    )
    .unwrap();
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    assert!(
        linguist_application::dictionary::enrich_revision(&mut store, &base, Some(&Failure))
            .is_err()
    );
    assert_eq!(store.latest_revision(base.id).unwrap(), 1);
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    let child =
        linguist_application::dictionary::enrich_revision(&mut store, &base, Some(&Dictionary))
            .unwrap();
    assert_eq!(child.revision, 2);
    assert_eq!(child.parent_digest, Some(base.approval_digest().unwrap()));
    assert_eq!(child.documents[0].sources[0], base.documents[0].sources[0]);
    assert_eq!(
        child.documents[0].archives[0],
        base.documents[0].archives[0]
    );
    assert_eq!(
        child.documents[0].requested_tasks,
        base.documents[0].requested_tasks
    );
    let LearningContent::Vocabulary(vocab) = &child.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.expression, "eat");
    assert_eq!(vocab.meaning, "My original meaning");
    assert_eq!(vocab.dictionary[0].senses.len(), 2);
    assert!(
        child.documents[0]
            .issues
            .iter()
            .any(|issue| issue.code == "DICTIONARY_SENSE_REVIEW")
    );
    assert!(
        child.documents[0]
            .evidence
            .iter()
            .any(|e| e.provenance == Provenance::Source && e.claim == "My original meaning")
    );
    assert!(
        child.documents[0]
            .evidence
            .iter()
            .any(|e| e.provenance == Provenance::Dictionary
                && e.claim == "Consume food"
                && e.target
                    == Some(linguist_core::records::EvidenceTarget::DictionarySense {
                        entry_index: 0,
                        sense_index: 0,
                    }))
    );
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    assert!(
        linguist_application::dictionary::enrich_revision(&mut store, &base, Some(&Dictionary))
            .unwrap_err()
            .contains("BASE_CONFLICT")
    );
    assert!(
        linguist_application::dictionary::enrich_revision(&mut store, &child, Some(&Dictionary))
            .unwrap_err()
            .contains("ALREADY_ENRICHED")
    );
    drop(store);
    let reopened = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(reopened.revision(base.id, 2).unwrap(), child);
    let raw = reopened
        .asset(&child.documents[0].archives[1].asset_digests[0], 1024)
        .unwrap();
    assert!(String::from_utf8(raw).unwrap().contains("Wear away"));
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mapped_enable_fields_retain_task_candidates_without_claiming_native_mapping() {
    let (capture, settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Production", "1"), ("Spelling", "0")],
        &[
            ("expression", "Word"),
            ("enable_production", "Production"),
            ("enable_spelling", "Spelling"),
        ],
    );
    let document = stage_document(&capture, &settings, "english_vocab").unwrap();
    // Nonempty markers remain candidates; "0" is not silently treated as disabled.
    assert_eq!(
        document.requested_tasks,
        vec![Task::Comprehension, Task::Production, Task::Spelling]
    );
    assert_eq!(document.sources[0], capture.captured.source);
    assert!(
        document
            .issues
            .iter()
            .any(|i| i.code == "SOURCE_TASK_MAPPING_REVIEW"
                && i.field.as_deref() == Some("enable_spelling"))
    );
    assert!(
        document
            .issues
            .iter()
            .any(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW")
    );
    assert!(
        document
            .evidence
            .iter()
            .any(|e| e.field == "enable_spelling"
                && e.claim == "0"
                && e.provenance == Provenance::Source)
    );
    assert!(linguist_core::render::render(&document, &capture.captured.source.fields).is_err());
    // v3 fronts show fields: no text cue is requested for any task.
    assert!(
        !document
            .issues
            .iter()
            .any(|issue| issue.code == "MISSING_CUE")
    );
    let (capture, settings) = setup(
        "english_grammar",
        &[
            ("Pattern", "used to"),
            ("Application", "yes"),
            ("Cue", "Complete: I ___ walk there."),
            ("Answer", "used to"),
        ],
        &[
            ("pattern", "Pattern"),
            ("enable_application", "Application"),
            ("exercise_prompt", "Cue"),
            ("exercise_answer", "Answer"),
        ],
    );
    let document = stage_document(&capture, &settings, "english_grammar").unwrap();
    assert_eq!(
        document.requested_tasks,
        vec![Task::Recognition, Task::Application]
    );
    let LearningContent::Grammar(grammar) = &document.content else {
        panic!()
    };
    assert_eq!(grammar.exercise_prompt, "Complete: I ___ walk there.");
    assert_eq!(grammar.exercise_answer, "used to");
    let (capture, settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Production", ""), ("Spelling", "   ")],
        &[
            ("expression", "Word"),
            ("enable_production", "Production"),
            ("enable_spelling", "Spelling"),
        ],
    );
    let document = stage_document(&capture, &settings, "english_vocab").unwrap();
    assert_eq!(document.requested_tasks, vec![Task::Comprehension]);
    assert_eq!(document.archives[0].original_fields["Spelling"], "   ");
}

#[test]
fn grammar_split_records_one_anchor_and_fresh_siblings_with_recoverable_archives() {
    use linguist_application::grammar::*;
    let (capture, mut settings) = setup(
        "english_grammar",
        &[
            ("Pattern", "used to / be used to"),
            ("Meaning", "Original lesson"),
        ],
        &[("pattern", "Pattern"), ("meaning", "Meaning")],
    );
    let root = std::env::temp_dir().join(format!("lab-grammar-split-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_grammar",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-grammar-split".into())]),
    )
    .unwrap();
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    let original = &base.documents[0];
    let LearningContent::Grammar(grammar) = &original.content else {
        panic!()
    };
    let mut first = grammar.clone();
    first.pattern = "used to".into();
    first.use_key = "past-habit".into();
    let mut second = grammar.clone();
    second.pattern = "be used to".into();
    second.use_key = "familiarity".into();
    let mut request = SplitRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: base.approval_digest().unwrap(),
        document_id: original.id,
        input_digest: original.semantic_digest().unwrap(),
        actor: "source owner".into(),
        anchor_index: 1,
        units: vec![first.clone(), second.clone()],
    };
    request.anchor_index = 2;
    let raw = serde_json::to_vec(&request).unwrap();
    assert!(split(&mut store, &base, &request, &raw).is_err());
    assert_eq!(store.latest_revision(base.id).unwrap(), 1);
    request.anchor_index = 1;
    request.units = vec![first.clone(), first.clone()];
    let raw = serde_json::to_vec(&request).unwrap();
    assert!(
        split(&mut store, &base, &request, &raw)
            .unwrap_err()
            .contains("UNIT_INVALID")
    );
    request.units = vec![first, second];
    let saved_pattern = request.units[0].pattern.clone();
    request.units[0].pattern = "x".repeat(100001);
    let oversized = serde_json::to_vec(&request).unwrap();
    assert!(
        split(&mut store, &base, &request, &oversized)
            .unwrap_err()
            .contains("INPUT_LIMIT")
    );
    assert_eq!(store.latest_revision(base.id).unwrap(), 1);
    request.units[0].pattern = saved_pattern;
    let raw = serde_json::to_vec(&request).unwrap();
    let child = split(&mut store, &base, &request, &raw).unwrap();
    assert_eq!(child.revision, 2);
    assert_eq!(child.parent_digest, Some(base.approval_digest().unwrap()));
    assert_eq!(child.grammar_groups.len(), 1);
    let group = &child.grammar_groups[0];
    assert_eq!(group.anchor_document, original.id);
    assert_eq!(
        group.units,
        child
            .documents
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(child.documents[1].id, original.id);
    assert_ne!(child.documents[0].id, original.id);
    assert!(child.binding.is_none());
    assert!(child.rendered.is_empty());
    for document in &child.documents {
        assert_eq!(document.sources[0], original.sources[0]);
        assert_eq!(document.archives[0], original.archives[0]);
        assert!(
            document
                .issues
                .iter()
                .any(|issue| issue.code == "GRAMMAR_SPLIT_NATIVE_REVIEW")
        );
        assert!(document.reviews.is_empty());
    }
    assert_eq!(
        store.asset(&group.request_asset_digest, 100000).unwrap(),
        raw
    );
    let mut forged = child.clone();
    forged.grammar_groups[0].anchor_document = uuid::Uuid::new_v4();
    assert!(forged.approval_digest().is_err());
    let mut forged = child.clone();
    forged.grammar_groups[0].units[0] = forged.grammar_groups[0].units[1];
    assert!(forged.approval_digest().is_err());
    let mut changed = child.clone();
    changed.grammar_groups[0].actor = "another reviewer".into();
    assert!(changed.approval_digest().is_err());
    assert!(
        split(&mut store, &base, &request, &raw)
            .unwrap_err()
            .contains("BASE_CONFLICT")
    );
    drop(store);
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    assert_eq!(store.revision(base.id, 2).unwrap(), child);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn revamp_enrichment_child_preserves_source_tasks_cards_and_parent() {
    use linguist_application::vocab::{KanjiPort, Providers, enrich_revision, requested};
    struct Kanji;
    impl KanjiPort for Kanji {
        fn lookup(
            &self,
            character: char,
        ) -> std::result::Result<Option<linguist_dictionary::kanji::KanjiEntry>, String> {
            let raw = format!("<h1>{character}</h1>").into_bytes();
            Ok(Some(linguist_dictionary::kanji::KanjiEntry {
                character: character.to_string(),
                meanings: vec!["eat".into()],
                kun_readings: vec![],
                on_readings: vec!["ショク".into()],
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
    }
    let (capture, mut settings) = setup(
        "japanese_vocab",
        &[
            ("Word", "食べる"),
            ("Meaning", "to eat"),
            ("Reading", "たべる"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Meaning"),
            ("reading", "Reading"),
        ],
    );
    let root = std::env::temp_dir().join(format!("lab-revamp-enrich-{}", uuid::Uuid::new_v4()));
    for (key, value) in [
        ("storage.state_dir", json!(root)),
        ("kanji.enabled", json!(true)),
        ("images.search_when_missing", json!(false)),
        ("audio.provider", json!("preserve")),
    ] {
        settings.values.insert(key.into(), value);
    }
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-revamp-enrich".into())]);
    let prepared =
        publish_capture_draft(&capture, &settings, "japanese_vocab", &environment).unwrap();
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    assert!(requested(&settings, &base));
    let child = enrich_revision(
        &mut store,
        &base,
        &environment,
        Providers {
            kanji: Some(&Kanji),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(child.revision, 2);
    assert_eq!(
        child.parent_digest.as_deref(),
        Some(prepared.digest.as_str())
    );
    let (before, after) = (&base.documents[0], &child.documents[0]);
    assert_eq!(after.requested_tasks, before.requested_tasks);
    assert_eq!(after.task_maps, before.task_maps);
    assert_eq!(after.sources[0], before.sources[0]);
    assert_eq!(after.sources[0].cards, before.sources[0].cards);
    let LearningContent::Vocabulary(vocab) = &after.content else {
        panic!()
    };
    assert!(vocab.kanji.is_empty());
    assert_eq!(vocab.kanji_details[0].character, "食");
    assert_eq!(vocab.kanji_details[0].meanings[0], "eat");
    assert_eq!(vocab.meaning, "to eat");
    // The source-only parent remains intact and the enrichment ran once.
    assert_eq!(store.revision(prepared.plan_id, 1).unwrap(), base);
    let mut frozen = settings.clone();
    frozen.values = child.settings.values.clone();
    assert!(!requested(&frozen, &child));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

/// Companion `note_evidence` shaped like `inspection.py` for the fixture note.
fn note_evidence(
    doc: &linguist_core::LearningDocument,
    field_order: &[&str],
    reviews: usize,
) -> serde_json::Value {
    let source = &doc.sources[0];
    let note_id = source.location.strip_prefix("anki_note:").unwrap();
    let card_id = (note_id.parse::<u64>().unwrap() + 333).to_string();
    let rows: Vec<_> = (0..reviews)
        .map(|i| {
            json!({"id": 1700000000000u64 + i as u64, "card_id": card_id, "usn": -1,
                        "ease": 3, "interval": 1, "previous_interval": 0, "factor": 2500,
                        "time": 1000, "type": 1})
        })
        .collect();
    json!({
        "schema_version": 1, "note_id": note_id,
        "note": {"guid": "g", "model_id": "12", "fields": source.fields, "tags": source.tags},
        "model": {"id": "12", "name": "Legacy",
                  "fields": field_order,
                  "templates": [{"ordinal": 0, "name": "Card", "front": "front", "back": "back"}],
                  "css": "style"},
        "cards": [{"id": card_id, "ordinal": 0, "deck_id": "1", "original_deck_id": "0",
                   "repetitions": reviews, "reviews": rows}],
        "repeated_reads_matched": true, "review_rows_untruncated": true,
    })
}

#[test]
fn native_history_evidence_resolves_review_and_maps_every_card() {
    use linguist_core::{records::ReviewChoice, review::resolve};
    let (capture, mut settings) = setup(
        "english_vocab",
        &[
            ("Word", "cat"),
            ("Meaning", "feline"),
            ("Key", "cat-animal"),
        ],
        &[
            ("expression", "Word"),
            ("meaning", "Meaning"),
            ("sense_key", "Key"),
        ],
    );
    let root = std::env::temp_dir().join(format!("lab-native-history-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root.to_str().unwrap()));
    let environment = BTreeMap::from([("HOME".into(), "/tmp/lab-native-history".into())]);
    let prepared =
        publish_capture_draft(&capture, &settings, "english_vocab", &environment).unwrap();
    let store = linguist_store::Store::read_only(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let doc = plan.documents[0].clone();
    let mut evidence = note_evidence(&doc, &["Word", "Meaning", "Key"], 2);
    // An unmapped card ordinal is refused: no card or history is dropped.
    let error = native_history_request(
        &plan,
        doc.id,
        &evidence,
        &[(1, Task::Comprehension)],
        "reviewer",
    )
    .unwrap_err();
    assert!(error.starts_with("NATIVE_HISTORY_CARD_UNMAPPED"), "{error}");
    // A live note that changed since capture is refused.
    let mut drifted = evidence.clone();
    drifted["note"]["fields"]["Meaning"] = json!("changed");
    let error = native_history_request(
        &plan,
        doc.id,
        &drifted,
        &[(0, Task::Comprehension)],
        "reviewer",
    )
    .unwrap_err();
    assert!(
        error.starts_with("NATIVE_HISTORY_SOURCE_CONFLICT"),
        "{error}"
    );
    let request = native_history_request(
        &plan,
        doc.id,
        &evidence,
        &[(0, Task::Comprehension)],
        "reviewer",
    )
    .unwrap();
    let ReviewChoice::NativeHistory {
        cards, task_map, ..
    } = &request.choice
    else {
        panic!("{:?}", request.choice)
    };
    assert_eq!(cards[0].review_count, 2);
    assert_eq!(cards[0].repetitions, 2);
    assert_eq!(task_map.entries[0].target_ordinal, 0);
    let resolved = resolve(&plan, &request, "unix-seconds:1".into()).unwrap();
    let after = &resolved.revision.documents[0];
    assert_eq!(after.task_maps, vec![task_map.clone()]);
    assert!(
        !validation::validate(after)
            .iter()
            .any(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW")
    );
    // Tampered evidence no longer resolves the issue.
    let mut tampered = after.clone();
    if let ReviewChoice::NativeHistory { cards, .. } = &mut tampered.reviews[0].choice {
        cards[0].review_count = 0;
    }
    assert!(
        validation::validate(&tampered)
            .iter()
            .any(|i| i.code == "SOURCE_NATIVE_HISTORY_REVIEW")
    );
    // A task the plan does not request cannot be mapped.
    evidence["cards"][0]["ordinal"] = json!(0);
    assert!(
        native_history_request(
            &plan,
            doc.id,
            &evidence,
            &[(0, Task::Recognition)],
            "reviewer"
        )
        .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmapped_legacy_field_is_dropped_by_a_typed_decision_and_stays_archived() {
    use linguist_core::{records::ReviewChoice, review::ResolutionRequest};
    let (capture, mut settings) = setup(
        "english_vocab",
        &[("Word", "cat"), ("Cue", "Say the animal")],
        &[("expression", "Word")],
    );
    let root = std::env::temp_dir().join(format!("lab-drop-field-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_vocab",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-drop-field".into())]),
    )
    .unwrap();
    let store = linguist_store::Store::open(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|i| i.code == "SOURCE_UNMAPPED_FIELD_REVIEW")
        .unwrap();
    let templates = linguist_core::review::decision_templates(doc, issue);
    let source_id = doc.sources[0].id;
    assert!(templates.contains(&ReviewChoice::SourceFieldDropped {
        source_id,
        field: "Cue".into()
    }));
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "owner".into(),
        choice: ReviewChoice::SourceFieldDropped {
            source_id,
            field: "Word".into(),
        },
    };
    // Only the issue's own field can be dropped.
    assert!(linguist_application::review::resolve(&store, &plan, &request, "now".into()).is_err());
    request.choice = ReviewChoice::SourceFieldDropped {
        source_id,
        field: "Cue".into(),
    };
    let result =
        linguist_application::review::resolve(&store, &plan, &request, "now".into()).unwrap();
    let after = &result.revision.documents[0];
    assert!(
        !validation::validate(after)
            .iter()
            .any(|i| i.code == "SOURCE_UNMAPPED_FIELD_REVIEW"
                && i.severity != validation::Severity::Warning)
    );
    assert_eq!(after.archives[0].original_fields["Cue"], "Say the animal");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn source_media_keep_the_order_the_fields_reference_them_in() {
    let (mut capture, mut settings) = setup(
        "english_vocab",
        &[
            ("Word", "secretary"),
            ("Media", "<img src=\"秘书.jpg\"><img src=\"book-box.jpg\">"),
        ],
        &[("expression", "Word")],
    );
    let png = |n: u8| {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(2, 2, image::Rgb([n, 0, 0]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    };
    assert_eq!(
        capture.captured.source.media_refs,
        ["book-box.jpg", "秘书.jpg"]
    );
    linguist_application::source_archive::media::attach_original_media(
        &mut capture.captured,
        BTreeMap::from([
            ("book-box.jpg".into(), Some(png(1))),
            ("秘书.jpg".into(), Some(png(2))),
        ]),
        10 * 1024 * 1024,
        10 * 1024 * 1024,
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!("lab-media-order-{}", uuid::Uuid::new_v4()));
    settings
        .values
        .insert("storage.state_dir".into(), json!(root));
    let prepared = publish_capture_draft(
        &capture,
        &settings,
        "english_vocab",
        &BTreeMap::from([("HOME".into(), "/tmp/lab-media-order".into())]),
    )
    .unwrap();
    let store = linguist_store::Store::open(&root).unwrap();
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    let names: Vec<_> = plan.documents[0]
        .media
        .iter()
        .map(|m| m.original_filename.as_deref().unwrap_or(&m.filename))
        .collect();
    assert_eq!(names, ["秘书.jpg", "book-box.jpg"]);
}
