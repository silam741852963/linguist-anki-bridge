//! WP-07: vocabulary add with enrichment, reviewed cue/media/duplicate choices.
use linguist_application::{
    DictionaryPort, Kind,
    images::{ImageCandidate, ImageSearch, RejectedCandidate},
    prepare_with_providers,
    speech::Synthesis,
    vocab::{ImagePort, KanjiPort, Providers, SpeechPort},
};
use linguist_config::{ConfigFile, Effective, Registry, ResolveOptions, expand_path};
use linguist_core::{
    Language, LearningContent, Task,
    records::{MediaRole, PlanRevision, ReviewChoice},
    review::{ResolutionRequest, decision_templates, resolve},
    validation::{self, Severity},
};
use linguist_dictionary::kanji::KanjiEntry;
use serde_json::json;
use std::collections::BTreeMap;

struct Fixture {
    root: std::path::PathBuf,
    environment: BTreeMap<String, String>,
    settings: Effective,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-vocab-{}", uuid::Uuid::new_v4()));
        let environment = BTreeMap::from([("HOME".into(), root.to_str().unwrap().into())]);
        let mut settings = resolve_settings(&environment);
        for (key, value) in [
            ("llm.enabled", json!(false)),
            ("dictionary.provider", json!("authored")),
            ("kanji.enabled", json!(true)),
            ("images.search_when_missing", json!(true)),
            ("images.provider", json!("wikimedia")),
            ("audio.provider", json!("piper")),
        ] {
            settings.values.insert(key.into(), value);
        }
        Self {
            root,
            environment,
            settings,
        }
    }
    fn store(&self) -> linguist_store::Store {
        linguist_store::Store::read_only(
            &expand_path(
                self.settings.values["storage.state_dir"].as_str().unwrap(),
                &self.environment,
            )
            .unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn resolve_settings(environment: &BTreeMap<String, String>) -> Effective {
    linguist_config::resolve(
        &Registry::builtin(),
        &ConfigFile::default(),
        &ResolveOptions {
            environment: environment.clone(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn png(shade: u8) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(4, 3, image::Luma([shade])))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
fn wav() -> Vec<u8> {
    let size = 4000u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(32000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.resize(44 + size as usize, 0);
    bytes
}

struct Kanji {
    fail: bool,
}
impl KanjiPort for Kanji {
    fn lookup(&self, character: char) -> Result<Option<KanjiEntry>, String> {
        if self.fail {
            return Err("PROVIDER_READ_Offline".into());
        }
        if character != '食' {
            return Ok(None);
        }
        let raw = "<div class=\"kanji details\">食</div>".as_bytes().to_vec();
        Ok(Some(KanjiEntry {
            character: "食".into(),
            meanings: vec!["eat".into(), "food".into()],
            kun_readings: vec!["た.べる".into()],
            on_readings: vec!["ショク".into()],
            strokes: Some(9),
            radical: None,
            parts: vec![],
            grade: None,
            jlpt: Some("N5".into()),
            frequency: None,
            schema: linguist_dictionary::kanji::SCHEMA,
            source_url: "https://jisho.org/search/%E9%A3%9F%23kanji".into(),
            raw_digest: linguist_provider::sha256_hex(&raw),
            fetched_at: 1,
            from_cache: false,
            raw_bytes: raw,
        }))
    }
    fn stroke_order(&self, _character: char) -> Result<Option<Vec<u8>>, String> {
        Ok(Some(b"GIF89a stroke".to_vec()))
    }
}
struct Images {
    fail: bool,
}
fn candidate(title: &str, shade: u8) -> ImageCandidate {
    let bytes = png(shade);
    ImageCandidate {
        provider: "wikimedia_commons",
        title: title.into(),
        page_url: format!("https://commons.wikimedia.org/wiki/{title}"),
        original_url: "https://upload.wikimedia.org/x.png".into(),
        source_url: "https://upload.wikimedia.org/x.png".into(),
        mime: "image/png".into(),
        width: 4,
        height: 3,
        sha256: linguist_provider::sha256_hex(&bytes),
        size_bytes: bytes.len() as u64,
        license: Some("CC BY 4.0".into()),
        license_url: None,
        artist: Some("Ann".into()),
        credit: None,
        attribution_required: Some(true),
        usage_terms: None,
        description: None,
        fetched_at: 1,
        from_cache: false,
        review_required: true,
        bytes,
    }
}
impl ImagePort for Images {
    fn search(&self, expression: &str) -> Result<ImageSearch, String> {
        if self.fail {
            return Err("PROVIDER_READ_Deadline".into());
        }
        Ok(ImageSearch {
            query: expression.into(),
            request_url: "https://commons.wikimedia.org/w/api.php?x".into(),
            response_sha256: String::new(),
            candidates: vec![
                candidate("File:Meal.png", 10),
                candidate("File:Bowl.png", 200),
            ],
            rejected: vec![RejectedCandidate {
                title: "File:Page.html".into(),
                code: "PROVIDER_READ_ContentType".into(),
            }],
            response: b"{\"query\":{}}".to_vec(),
        })
    }
}
struct Speech {
    fail: bool,
}
impl SpeechPort for Speech {
    fn synthesize(&self, text: &str, target: &Language) -> Result<Synthesis, String> {
        if self.fail {
            return Err("SPEECH_PROCESS_TIMEOUT".into());
        }
        assert_eq!((text, target.as_str()), ("たべる", "ja"));
        let bytes = wav();
        Ok(Synthesis {
            provider: "piper",
            engine_version: "piper 1.2.0".into(),
            executable_sha256: String::new(),
            voice_sha256: String::new(),
            voice_config_sha256: String::new(),
            voice_language: "ja_JP".into(),
            voice_dataset: Some("test".into()),
            speaker: None,
            length_scale: 1.0,
            text_sha256: String::new(),
            mime: "audio/wav".into(),
            sample_rate: 16000,
            sha256: linguist_provider::sha256_hex(&bytes),
            size_bytes: bytes.len() as u64,
            review_required: true,
            bytes,
        })
    }
}

fn japanese(tasks: &[&str], production: &str, spelling: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en",
        "requested_tasks": tasks,
        "body":{"expression":"食べる","meaning":"to eat","sense_key":"eat","reading":"たべる",
            "production_prompt":production,"spelling_prompt":spelling}}))
    .unwrap()
}

fn decide(plan: &PlanRevision, issue_id: &str, choice: ReviewChoice) -> PlanRevision {
    let document = &plan.documents[0];
    resolve(
        plan,
        &ResolutionRequest {
            schema_version: 2,
            base_revision: plan.revision,
            base_digest: plan.approval_digest().unwrap(),
            document_id: document.id,
            issue_id: issue_id.into(),
            input_digest: document.semantic_digest().unwrap(),
            actor: "reviewer".into(),
            choice,
        },
        "unix-seconds:1".into(),
    )
    .unwrap_or_else(|e| panic!("{issue_id}: {e:?}"))
    .revision
}

fn issue_id(plan: &PlanRevision, code: &str, field: &str) -> String {
    validation::validate(&plan.documents[0])
        .into_iter()
        .find(|i| i.code == code && i.field.as_deref() == Some(field))
        .unwrap_or_else(|| panic!("{code} {field}"))
        .id
}

#[test]
fn japanese_add_enriches_kanji_and_stages_reviewed_cues_picture_and_audio() {
    let f = Fixture::new();
    let providers = Providers {
        kanji: Some(&Kanji { fail: false }),
        images: Some(&Images { fail: false }),
        speech: Some(&Speech { fail: false }),
        ..Default::default()
    };
    let result = prepare_with_providers(
        &japanese(&["comprehension", "production", "spelling"], "", ""),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        providers,
    )
    .unwrap();
    assert!(!result.ready);
    assert!(
        result
            .next_commands
            .iter()
            .any(|c| c.contains("plans resolve") && c.contains("IMAGE_CANDIDATE_REVIEW"))
    );
    let store = f.store();
    let plan = store.revision(result.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    assert!(vocab.kanji.is_empty());
    assert_eq!(vocab.kanji_details[0].character, "食");
    assert_eq!(vocab.kanji_details[0].meanings, ["eat", "food"]);
    assert_eq!(vocab.kanji_details[0].on_readings, ["ショク"]);
    let kanji_source = doc
        .sources
        .iter()
        .find(|s| s.kind == "jisho_kanji_pages_v2")
        .unwrap();
    let archive = doc
        .archives
        .iter()
        .find(|a| a.source_id == kanji_source.id)
        .unwrap();
    assert!(store.asset(&archive.asset_digests[0], 1 << 20).is_ok());
    // Media candidates are archived but render nowhere until chosen.
    // The stroke GIF renders directly; it does not stop the picture search.
    assert_eq!(doc.media.len(), 4);
    assert_eq!(
        doc.media
            .iter()
            .filter(|m| m.role == MediaRole::KanjiStroke)
            .count(),
        1
    );
    assert!(
        doc.media
            .iter()
            .filter(|m| m.role != MediaRole::KanjiStroke)
            .all(|m| m.role == MediaRole::Archive)
    );
    for media in &doc.media {
        assert_eq!(
            store.asset(&media.digest, 1 << 20).unwrap().len() as u64,
            media.size_bytes
        );
    }
    let issues = validation::validate(doc);
    let codes: Vec<_> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.severity))
        .collect();
    assert!(codes.contains(&("IMAGE_CANDIDATE_REVIEW", Severity::Review)));
    assert!(codes.contains(&("AUDIO_CANDIDATE_REVIEW", Severity::Review)));
    assert!(codes.contains(&("IMAGE_CANDIDATE_REJECTED", Severity::Warning)));
    // v3 fronts show fields; no text cue is requested.
    assert!(!issues.iter().any(|i| i.code == "MISSING_CUE"));
    let image_issue = issues
        .iter()
        .find(|i| i.code == "IMAGE_CANDIDATE_REVIEW")
        .unwrap();
    assert_eq!(decision_templates(doc, image_issue).len(), 3);

    let mut current = plan.clone();
    let picked = image_issue.source_refs[1].clone();
    current = decide(
        &current,
        &image_issue.id,
        ReviewChoice::Media(picked.clone()),
    );
    let audio = issue_id(&current, "AUDIO_CANDIDATE_REVIEW", "audio");
    let audio_digest = current.documents[0]
        .media
        .iter()
        .find(|m| m.mime == "audio/wav")
        .unwrap()
        .digest
        .clone();
    current = decide(&current, &audio, ReviewChoice::Media(audio_digest.clone()));
    let final_issues = validation::validate(&current.documents[0]);
    assert!(
        final_issues.iter().all(|i| i.severity == Severity::Warning),
        "{final_issues:?}"
    );
    let rendered = &current.rendered[0];
    assert!(rendered.fields["Picture"].contains(&format!("lab_{picked}.png")));
    assert!(rendered.fields["Audio"].contains(&format!("[sound:lab_{audio_digest}.wav]")));
    assert!(rendered.fields["Kanji"].contains("ショク"));
    let roles: Vec<_> = current.documents[0].media.iter().map(|m| m.role).collect();
    assert_eq!(
        roles.iter().filter(|r| **r == MediaRole::Picture).count(),
        1
    );

    // Declining every picture candidate is also a valid explicit decision.
    let declined = decide(&plan, &image_issue.id, ReviewChoice::Media(String::new()));
    assert!(
        !validation::validate(&declined.documents[0])
            .iter()
            .any(|i| i.code == "IMAGE_CANDIDATE_REVIEW")
    );
    assert!(
        declined.documents[0]
            .media
            .iter()
            .all(|m| matches!(m.role, MediaRole::Archive | MediaRole::KanjiStroke))
    );
    // Unknown candidates are rejected; a later role change reopens the review.
    let document = &plan.documents[0];
    let bad = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: document.id,
        issue_id: image_issue.id.clone(),
        input_digest: document.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Media("f".repeat(64)),
    };
    assert!(resolve(&plan, &bad, "unix-seconds:1".into()).is_err());
    let mut tampered = current.documents[0].clone();
    tampered
        .media
        .iter_mut()
        .find(|m| m.digest == picked)
        .unwrap()
        .role = MediaRole::Archive;
    assert!(
        validation::validate(&tampered)
            .iter()
            .any(|i| i.code == "IMAGE_CANDIDATE_REVIEW")
    );
}

#[test]
fn optional_enrichment_failures_are_warnings_and_authored_cues_are_kept() {
    let f = Fixture::new();
    let providers = Providers {
        kanji: Some(&Kanji { fail: true }),
        images: Some(&Images { fail: true }),
        speech: Some(&Speech { fail: true }),
        ..Default::default()
    };
    let result = prepare_with_providers(
        &japanese(
            &["comprehension", "production", "spelling"],
            "What do you do with food?",
            "た＿る",
        ),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        providers,
    )
    .unwrap();
    assert!(result.ready, "{:?}", result.issues);
    let codes: Vec<_> = result
        .issues
        .iter()
        .map(|i| (i.code.as_str(), i.severity))
        .collect();
    for code in [
        "KANJI_ENRICHMENT_FAILED",
        "IMAGE_SEARCH_FAILED",
        "AUDIO_SYNTHESIS_FAILED",
    ] {
        assert!(codes.contains(&(code, Severity::Warning)), "{code}");
    }
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    let LearningContent::Vocabulary(vocab) = &plan.documents[0].content else {
        panic!()
    };
    assert!(vocab.kanji.is_empty());
    assert_eq!(vocab.production_prompt, "What do you do with food?");
    assert_eq!(vocab.spelling_prompt, "た＿る");
    assert!(plan.documents[0].media.is_empty());
    assert!(
        result
            .next_commands
            .iter()
            .any(|c| c.contains("plans approve"))
    );
}

#[test]
fn disabled_enrichment_and_unavailable_adapters_are_explicit() {
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    for (key, value) in [
        ("kanji.enabled", json!(false)),
        ("images.provider", json!("disabled")),
        ("audio.provider", json!("preserve")),
    ] {
        settings.values.insert(key.into(), value);
    }
    // No port is consulted when the setting disables it.
    struct Never;
    impl KanjiPort for Never {
        fn lookup(&self, _: char) -> Result<Option<KanjiEntry>, String> {
            panic!("kanji consulted")
        }
    }
    impl ImagePort for Never {
        fn search(&self, _: &str) -> Result<ImageSearch, String> {
            panic!("images consulted")
        }
    }
    impl SpeechPort for Never {
        fn synthesize(&self, _: &str, _: &Language) -> Result<Synthesis, String> {
            panic!("speech consulted")
        }
    }
    let result = prepare_with_providers(
        &japanese(&["comprehension"], "", ""),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            kanji: Some(&Never),
            images: Some(&Never),
            speech: Some(&Never),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.ready);
    assert!(result.issues.is_empty());
    for (key, value) in [
        ("audio.provider", json!("custom")),
        ("kanji.schema", json!("/opt/kanji.json")),
    ] {
        let mut changed = f.settings.clone();
        changed.values.insert(key.into(), value);
        let error = prepare_with_providers(
            &japanese(&["comprehension"], "", ""),
            Kind::Vocabulary,
            &changed,
            &f.environment,
            Providers::default(),
        )
        .unwrap_err();
        assert!(
            error.starts_with("CAPABILITY_UNAVAILABLE"),
            "{key}: {error}"
        );
    }
}

#[test]
fn spelling_suggestions_cover_kana_words_english_pronunciation_and_leaks() {
    let doc = |target: &str,
               expression: &str,
               reading: &str,
               pronunciation: &str,
               meaning: &str| {
        serde_json::from_value::<linguist_core::LearningDocument>(json!({
            "schema_version":2,"id":uuid::Uuid::nil(),"target_language":target,"explanation_language":"en",
            "requested_tasks":["comprehension","production","spelling"],
            "content":{"kind":"vocabulary","body":{"expression":expression,"meaning":meaning,"sense_key":"k","reading":reading,
                "pronunciation":pronunciation,"usage":"","examples":[],"dictionary":[],"kanji":"","production_prompt":"","spelling_prompt":""}}
        }))
        .unwrap()
    };
    use linguist_core::cues::suggest;
    // A kana-only word cannot use its own reading as a cue.
    assert_eq!(
        suggest(
            &doc("ja", "すごい", "すごい", "", "amazing"),
            Task::Spelling
        )
        .as_deref(),
        Some("amazing")
    );
    assert_eq!(
        suggest(
            &doc("en", "colonel", "", "/ˈkɜːnəl/", "army officer"),
            Task::Spelling
        )
        .as_deref(),
        Some("/ˈkɜːnəl/ — army officer")
    );
    // Meanings that contain the answer are never suggested.
    let leaky = doc("en", "run", "", "/rʌn/", "to run fast");
    assert_eq!(suggest(&leaky, Task::Production), None);
    assert_eq!(suggest(&leaky, Task::Spelling), None);
    assert_eq!(
        suggest(&doc("en", "eat", "", "", ""), Task::Production),
        None
    );
}

#[test]
fn dictionary_sense_selection_unlocks_kana_spelling_suggestion_without_ollama() {
    struct Dictionary;
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            linguist_dictionary::parse_jisho(query, target, r#"{"meta":{"status":200},"data":[{"slug":"eat","japanese":[{"word":"食べる","reading":"たべる"}],"senses":[{"english_definitions":["to eat"]},{"english_definitions":["to live on"]}]}]}"#.as_bytes(), 1024, 10)
                .map_err(|e| e.to_string())
        }
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("jisho"));
    settings.values.insert("kanji.enabled".into(), json!(false));
    settings
        .values
        .insert("images.provider".into(), json!("disabled"));
    settings
        .values
        .insert("audio.provider".into(), json!("preserve"));
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension","spelling"],"body":{"expression":"食べる"}}"#.as_bytes();
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
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    // Before sense review the Spelling front has nothing to say aloud.
    assert!(
        validation::validate(&plan.documents[0])
            .iter()
            .any(|i| i.code == "SPELLING_CUE_MISSING")
    );
    let LearningContent::Vocabulary(vocab) = &plan.documents[0].content else {
        panic!()
    };
    let key = vocab.dictionary[0].senses[1].key.clone();
    let ready = decide(
        &plan,
        &format!("DICTIONARY_SENSE_REVIEW:{}", plan.documents[0].id),
        ReviewChoice::Sense(key),
    );
    // The selected reading becomes the spoken Spelling front.
    assert!(
        validation::ready(&ready.documents[0]),
        "{:?}",
        validation::validate(&ready.documents[0])
    );
    assert_eq!(ready.rendered[0].fields["Pronunciation"], "たべる");
    assert!(ready.rendered[0].fields["Meaning"].contains("to live on"));
}

#[test]
fn collection_duplicate_candidates_become_a_recorded_decision() {
    use linguist_application::duplicate_candidates::{Candidate, Report, record};
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings.values.insert("kanji.enabled".into(), json!(false));
    settings
        .values
        .insert("images.provider".into(), json!("disabled"));
    settings
        .values
        .insert("audio.provider".into(), json!("preserve"));
    let result = prepare_with_providers(
        &japanese(&["comprehension"], "", ""),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers::default(),
    )
    .unwrap();
    assert!(result.ready);
    let root = expand_path(
        settings.values["storage.state_dir"].as_str().unwrap(),
        &f.environment,
    )
    .unwrap();
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let base = store.revision(result.plan_id, 1).unwrap();
    let report = |candidates: Vec<Candidate>| Report {
        schema_version: 2,
        plan_id: base.id,
        revision: 1,
        plan_digest: result.digest.clone(),
        document_id: result.document_id,
        observed_at_unix_seconds: 1,
        candidates,
        search_scope: "managed_v2_primary_field",
        collection_duplicate_check_complete: false,
        semantic_identity_verified: false,
        apply_eligible: false,
    };
    assert!(
        record(&mut store, &base, &result.digest, &report(vec![]), 2)
            .unwrap()
            .is_none()
    );
    let found = report(vec![Candidate {
        note_id: "1700000000001".into(),
        model_matches: true,
        compared_fields_match: true,
        relation: "same_fields",
    }]);
    assert!(record(&mut store, &base, "stale", &found, 2).is_err());
    let child = record(&mut store, &base, &result.digest, &found, 2)
        .unwrap()
        .unwrap();
    assert_eq!(child.revision, 2);
    let issue = validation::validate(&child.documents[0])
        .into_iter()
        .find(|i| i.code == "COLLECTION_DUPLICATE_REVIEW")
        .unwrap();
    assert_eq!(issue.severity, Severity::Review);
    assert_eq!(decision_templates(&child.documents[0], &issue).len(), 2);
    let note_id: linguist_core::AnkiId = "1700000000001".to_owned().try_into().unwrap();
    let request = |action: &str| ResolutionRequest {
        schema_version: 2,
        base_revision: 2,
        base_digest: child.approval_digest().unwrap(),
        document_id: child.documents[0].id,
        issue_id: issue.id.clone(),
        input_digest: child.documents[0].semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Duplicate {
            note_id: note_id.clone(),
            action: action.into(),
        },
    };
    assert!(resolve(&child, &request("merge"), "unix-seconds:1".into()).is_err());
    let decided = resolve(&child, &request("skip"), "unix-seconds:1".into()).unwrap();
    assert!(decided.ready);
    assert!(
        decided.revision.documents[0].reviews.iter().any(
            |r| matches!(&r.choice, ReviewChoice::Duplicate { action, .. } if action == "skip")
        )
    );
}

#[test]
fn dictionary_recordings_become_reviewed_audio_candidates() {
    use linguist_application::vocab::RecordingPort;
    use linguist_dictionary::recording::Recording;
    struct Recordings(Option<Vec<u8>>);
    impl RecordingPort for Recordings {
        fn japanese(&self, expression: &str, kana: &str) -> Result<Option<Recording>, String> {
            assert_eq!((expression, kana), ("食べる", "たべる"));
            Ok(self.0.clone().map(|bytes| Recording {
                provider: "japanesepod101",
                source_url: "https://cdn.innovativelanguage.com/x.mp3".into(),
                sha256: linguist_provider::sha256_hex(&bytes),
                fetched_at: 1,
                from_cache: false,
                bytes,
            }))
        }
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings.values.insert("kanji.enabled".into(), json!(false));
    settings
        .values
        .insert("images.provider".into(), json!("disabled"));
    settings
        .values
        .insert("audio.provider".into(), json!("dictionary"));
    settings
        .values
        .insert("audio.synthesis_fallback".into(), json!("none"));
    let tone = include_bytes!("fixtures/audio/tone.mp3").to_vec();
    let result = prepare_with_providers(
        &japanese(&["comprehension"], "", ""),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            recordings: Some(&Recordings(Some(tone.clone()))),
            ..Default::default()
        },
    )
    .unwrap();
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    let digest = linguist_core::canonical::asset_digest(&tone);
    let asset = plan.documents[0]
        .media
        .iter()
        .find(|m| m.digest == digest)
        .unwrap();
    assert_eq!(
        (asset.mime.as_str(), asset.role),
        ("audio/mpeg", MediaRole::Archive)
    );
    let issue = issue_id(&plan, "AUDIO_CANDIDATE_REVIEW", "audio");
    let chosen = decide(&plan, &issue, ReviewChoice::Media(digest.clone()));
    assert!(chosen.rendered[0].fields["Audio"].contains(&format!("[sound:lab_{digest}.mp3]")));
    // No published recording: a warning only, nothing staged.
    let result = prepare_with_providers(
        &japanese(&["comprehension"], "", ""),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            recordings: Some(&Recordings(None)),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.ready, "{:?}", result.issues);
    assert!(result.issues.iter().any(|i| i.code == "AUDIO_NOT_FOUND"));
    // With a synthesis fallback, the missing recording is synthesized (VOICEVOX
    // credit in the attribution) and reviewed like any audio candidate.
    struct Voicevox;
    impl SpeechPort for Voicevox {
        fn synthesize(&self, text: &str, target: &Language) -> Result<Synthesis, String> {
            assert_eq!((text, target.as_str()), ("たべる", "ja"));
            let bytes = wav();
            Ok(Synthesis {
                provider: "voicevox",
                engine_version: "0.24.1".into(),
                executable_sha256: String::new(),
                voice_sha256: String::new(),
                voice_config_sha256: String::new(),
                voice_language: "ja".into(),
                voice_dataset: Some("VOICEVOX:春日部つむぎ".into()),
                speaker: Some("8".into()),
                length_scale: 1.0,
                text_sha256: String::new(),
                mime: "audio/wav".into(),
                sample_rate: 24000,
                sha256: linguist_provider::sha256_hex(&bytes),
                size_bytes: bytes.len() as u64,
                review_required: true,
                bytes,
            })
        }
    }
    settings
        .values
        .insert("audio.synthesis_fallback".into(), json!("voicevox"));
    let result = prepare_with_providers(
        &japanese(&["comprehension"], "", ""),
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            recordings: Some(&Recordings(None)),
            speech: Some(&Voicevox),
            ..Default::default()
        },
    )
    .unwrap();
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    let synthesized = plan.documents[0]
        .media
        .iter()
        .find(|m| m.mime == "audio/wav")
        .unwrap();
    assert!(
        synthesized.attribution.starts_with("VOICEVOX:春日部つむぎ"),
        "{}",
        synthesized.attribution
    );
    issue_id(&plan, "AUDIO_CANDIDATE_REVIEW", "audio");
}

#[test]
fn japanese_pictures_search_irasutoya_first_and_commons_by_the_selected_sense() {
    use linguist_core::records::MediaOwner;
    use std::sync::Mutex;
    struct Dictionary;
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            linguist_dictionary::parse_jisho(query, target, r#"{"meta":{"status":200},"data":[{"slug":"eat","japanese":[{"word":"食べる","reading":"たべる"}],"senses":[{"english_definitions":["to eat"]},{"english_definitions":["to live on"]}]}]}"#.as_bytes(), 1024, 10)
                .map_err(|e| e.to_string())
        }
    }
    struct Recorder(Mutex<Vec<String>>, &'static str);
    impl ImagePort for Recorder {
        fn search(&self, expression: &str) -> Result<ImageSearch, String> {
            self.0.lock().unwrap().push(expression.into());
            let mut found = candidate(self.1, if self.1.starts_with("File:") { 10 } else { 90 });
            found.provider = if self.1.starts_with("File:") {
                "wikimedia_commons"
            } else {
                "irasutoya"
            };
            Ok(ImageSearch {
                query: expression.into(),
                request_url: "https://example.invalid/search".into(),
                response_sha256: String::new(),
                candidates: vec![found],
                rejected: vec![],
                response: format!("{{\"{}\":1}}", self.1).into_bytes(),
            })
        }
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("jisho"));
    settings.values.insert("kanji.enabled".into(), json!(false));
    settings
        .values
        .insert("audio.provider".into(), json!("preserve"));
    let commons = Recorder(Mutex::default(), "File:Meal.png");
    let irasutoya = Recorder(Mutex::default(), "食事のイラスト");
    let providers = Providers {
        dictionary: Some(&Dictionary),
        images: Some(&commons),
        illustrations: Some(&irasutoya),
        ..Default::default()
    };
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension"],"body":{"expression":"食べる"}}"#.as_bytes();
    let result = prepare_with_providers(
        bytes,
        Kind::Vocabulary,
        &settings,
        &f.environment,
        providers,
    )
    .unwrap();
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    // いらすとや is searched by the word and staged before Commons, which is
    // searched by the first sense's gloss until a sense is chosen.
    assert_eq!(*irasutoya.0.lock().unwrap(), ["食べる"]);
    assert_eq!(*commons.0.lock().unwrap(), ["to eat"]);
    let staged: Vec<_> = doc
        .media
        .iter()
        .filter_map(|m| m.original_filename.as_deref())
        .collect();
    assert_eq!(staged, ["食事のイラスト", "File:Meal.png"]);
    let review = doc
        .issues
        .iter()
        .find(|i| i.code == "IMAGE_CANDIDATE_REVIEW")
        .unwrap();
    assert_eq!(review.source_refs[0], doc.media[0].digest);
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    let key = vocab.dictionary[0].senses[1].key.clone();
    let chosen = decide(
        &plan,
        &format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        ReviewChoice::Sense(key),
    );
    let mut again = chosen.documents[0].clone();
    again.issues.retain(|i| i.code != "IMAGE_CANDIDATE_REVIEW");
    again.media.retain(|m| m.owner != MediaOwner::External);
    // No post title names the word, so generated collocation nouns are tried.
    if let LearningContent::Vocabulary(v) = &mut again.content {
        v.collocations = serde_json::from_value(json!([
            {"phrase": "ご飯を食べる", "gloss": "eat a meal"},
            {"phrase": "食べる量", "gloss": "amount eaten"},
            {"phrase": "朝ご飯", "gloss": "breakfast"},
            {"phrase": "食べるのが好き", "gloss": "like eating"},
            {"phrase": "食べるの時間", "gloss": "time to eat"}
        ]))
        .unwrap();
    }
    // Reviewer-chosen terms are always searched after the word; a title naming
    // one of them (食事のイラスト) skips the collocation nouns.
    settings
        .values
        .insert("images.search_terms".into(), json!(["朝食", "食事"]));
    linguist_application::vocab::enrich_document(&again, &settings, &f.environment, providers)
        .unwrap();
    assert_eq!(*commons.0.lock().unwrap(), ["to eat", "to live on"]);
    assert_eq!(
        *irasutoya.0.lock().unwrap(),
        ["食べる", "食べる", "朝食", "食事"]
    );
    // Without terms, no title names the word, so the collocation nouns follow.
    settings
        .values
        .insert("images.search_terms".into(), json!([]));
    linguist_application::vocab::enrich_document(&again, &settings, &f.environment, providers)
        .unwrap();
    assert_eq!(irasutoya.0.lock().unwrap()[4..], ["食べる", "ご飯", "時間"]);
}

#[test]
fn a_kana_word_matches_its_reading_only_dictionary_entry() {
    use std::sync::Mutex;
    struct Dictionary;
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            linguist_dictionary::parse_jisho(query, target, r#"{"meta":{"status":200},"data":[{"slug":"x","japanese":[{"reading":"ビタミン"}],"senses":[{"english_definitions":["vitamin"]}]}]}"#.as_bytes(), 1024, 10)
                .map_err(|e| e.to_string())
        }
    }
    struct Recorder(Mutex<Vec<String>>);
    impl ImagePort for Recorder {
        fn search(&self, expression: &str) -> Result<ImageSearch, String> {
            self.0.lock().unwrap().push(expression.into());
            Err("PROVIDER_READ_Deadline".into())
        }
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("jisho"));
    settings.values.insert("kanji.enabled".into(), json!(false));
    settings
        .values
        .insert("audio.provider".into(), json!("preserve"));
    let commons = Recorder(Mutex::default());
    let illustrations = Recorder(Mutex::default());
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension"],"body":{"expression":"ビタミン"}}"#.as_bytes();
    prepare_with_providers(
        bytes,
        Kind::Vocabulary,
        &settings,
        &f.environment,
        Providers {
            dictionary: Some(&Dictionary),
            images: Some(&commons),
            illustrations: Some(&illustrations),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(*commons.0.lock().unwrap(), ["vitamin"]);
}

#[test]
fn a_word_the_dictionary_lacks_gets_a_generated_entry_in_its_style() {
    use std::os::unix::fs::PermissionsExt;
    struct Dictionary;
    impl DictionaryPort for Dictionary {
        fn lookup(
            &self,
            query: &str,
            target: &Language,
        ) -> Result<linguist_dictionary::JishoPage, String> {
            linguist_dictionary::parse_jisho(query, target, r#"{"meta":{"status":200},"data":[{"slug":"fuku","japanese":[{"word":"副","reading":"ふく"}],"senses":[{"english_definitions":["vice-"]}]}]}"#.as_bytes(), 1024, 10)
                .map_err(|e| e.to_string())
        }
    }
    let f = Fixture::new();
    let bin = std::env::temp_dir().join(format!("lab-dict-agent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&bin).unwrap();
    let reply = json!({"type":"result","subtype":"success","is_error":false,
        "structured_output":{"readings":["ふくいいんちょう"],"senses":[{"definitions":["vice-chairperson","deputy chair"],"parts_of_speech":["Noun"]}]},
        "modelUsage":{"fake-model":{}}});
    std::fs::write(
        bin.join("claude"),
        format!("#!/bin/sh\n[ \"$1\" = --version ] && {{ echo '9.9.9'; exit 0; }}\ncat > /dev/null\nprintf '%s' '{reply}'\n"),
    )
    .unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = f.environment.clone();
    environment.insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
    let mut settings = f.settings.clone();
    for (key, value) in [
        ("dictionary.provider", json!("jisho")),
        ("kanji.enabled", json!(false)),
        ("images.provider", json!("disabled")),
        ("audio.provider", json!("preserve")),
        ("llm.enabled", json!(true)),
        ("llm.provider", json!("claude_code")),
        ("llm.fallback", json!([])),
    ] {
        settings.values.insert(key.into(), value);
    }
    let bytes = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","requested_tasks":["comprehension"],"body":{"expression":"副委員長"}}"#.as_bytes();
    let result = prepare_with_providers(
        bytes,
        Kind::Vocabulary,
        &settings,
        &environment,
        Providers {
            dictionary: Some(&Dictionary),
            ..Default::default()
        },
    )
    .unwrap();
    let plan = f.store().revision(result.plan_id, 1).unwrap();
    let doc = &plan.documents[0];
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    let generated = vocab
        .dictionary
        .iter()
        .find(|e| e.provider == "generated-dictionary-v1")
        .unwrap();
    assert_eq!(generated.forms, ["副委員長"]);
    assert_eq!(generated.readings, ["ふくいいんちょう"]);
    assert_eq!(generated.senses[0].labels, ["Noun"]);
    let issues = validation::validate(doc);
    assert!(
        issues
            .iter()
            .any(|i| i.code == "DICTIONARY_ENTRY_GENERATED")
    );
    // It is reviewed like any generated fact, and offers its sense.
    assert!(
        issues
            .iter()
            .any(|i| i.code == "GENERATED_FACT_REVIEW" && i.field.as_deref() == Some("dictionary"))
    );
    let sense = issues
        .iter()
        .find(|i| i.code == "DICTIONARY_SENSE_REVIEW")
        .unwrap();
    let choices = decision_templates(doc, sense);
    assert!(choices.iter().any(|c| matches!(c, ReviewChoice::SenseWithReading { reading, .. } if reading == "ふくいいんちょう")), "{choices:?}");
    let chosen = choices
        .iter()
        .find(|c| matches!(c, ReviewChoice::SenseWithReading { reading, .. } if reading == "ふくいいんちょう"))
        .unwrap()
        .clone();
    let picked = decide(&plan, &sense.id, chosen);
    let LearningContent::Vocabulary(v) = &picked.documents[0].content else {
        panic!()
    };
    assert_eq!(v.meaning, "vice-chairperson; deputy chair");
    // Rejecting the generated entry removes it and the sense chosen from it.
    let fact = validation::validate(&picked.documents[0])
        .into_iter()
        .find(|i| i.code == "GENERATED_FACT_REVIEW" && i.field.as_deref() == Some("dictionary"))
        .unwrap();
    let reject = decision_templates(&picked.documents[0], &fact)
        .into_iter()
        .find(|c| matches!(c, ReviewChoice::ContentRejected { .. }))
        .unwrap();
    let rejected = decide(&picked, &fact.id, reject);
    let LearningContent::Vocabulary(v) = &rejected.documents[0].content else {
        panic!()
    };
    assert!(
        v.dictionary
            .iter()
            .all(|e| e.provider != "generated-dictionary-v1")
    );
    assert!(v.meaning.is_empty() && v.sense_key.is_empty());
    let _ = std::fs::remove_dir_all(&bin);
}
