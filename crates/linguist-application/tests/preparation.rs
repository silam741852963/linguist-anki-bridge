use linguist_application::*;
use linguist_config::*;
use std::collections::BTreeMap;
struct Fixture {
    root: std::path::PathBuf,
    environment: BTreeMap<String, String>,
    settings: Effective,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-prepare-{}", uuid::Uuid::new_v4()));
        let environment = BTreeMap::from([("HOME".into(), root.to_str().unwrap().into())]);
        let registry = Registry::builtin();
        let flags = BTreeMap::from([
            ("llm.enabled".into(), serde_json::json!(false)),
            ("dictionary.provider".into(), serde_json::json!("authored")),
            (
                "images.search_when_missing".into(),
                serde_json::json!(false),
            ),
            ("kanji.enabled".into(), serde_json::json!(false)),
        ]);
        let settings = resolve(
            &registry,
            &ConfigFile::default(),
            &ResolveOptions {
                environment: environment.clone(),
                flags,
                ..Default::default()
            },
        )
        .unwrap();
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
fn input(kind: Kind) -> serde_json::Value {
    let fixture = match kind {
        Kind::Vocabulary => include_str!("../../../contracts/v2/fixtures/vocabulary.json"),
        Kind::Grammar => include_str!("../../../contracts/v2/fixtures/grammar.json"),
    };
    let doc: serde_json::Value = serde_json::from_str(fixture).unwrap();
    serde_json::json!({"schema_version":2,"kind":doc["content"]["kind"],"body":doc["content"]["body"],"target_language":doc["target_language"],"explanation_language":doc["explanation_language"]})
}
#[test]
fn both_authored_workflows_preserve_raw_input_and_freeze_settings() {
    for kind in [Kind::Vocabulary, Kind::Grammar] {
        let f = Fixture::new();
        let bytes = serde_json::to_vec_pretty(&input(kind)).unwrap();
        let result = prepare_authored(&bytes, kind, &f.settings, &f.environment).unwrap();
        assert!(result.ready, "{:?}", result.issues);
        assert!(!result.apply_eligible && !result.duplicate_check_performed);
        let store = linguist_store::Store::read_only(&f.state()).unwrap();
        assert_eq!(
            store
                .asset(&result.original_input_digest, 1024 * 1024)
                .unwrap(),
            bytes
        );
        let plan = store.revision(result.plan_id, 1).unwrap();
        assert!(plan.binding.is_none());
        assert_eq!(
            plan.documents[0].archives[0].asset_digests,
            vec![result.original_input_digest]
        );
        assert_eq!(
            plan.documents[0].requested_tasks,
            vec![match kind {
                Kind::Vocabulary => linguist_core::Task::Comprehension,
                Kind::Grammar => linguist_core::Task::Recognition,
            }]
        );
        assert_eq!(
            plan.settings.values["storage.state_dir"],
            f.state().to_str().unwrap()
        );
        assert_ne!(plan.settings.fingerprint, f.settings.fingerprint);
        assert_eq!(
            plan.settings.fingerprint,
            linguist_core::canonical::digest("resolved-settings", &plan.settings.values).unwrap()
        );
    }
}
#[test]
fn jsonl_batch_preserves_order_sources_and_reports_exact_duplicates() {
    for kind in [Kind::Vocabulary, Kind::Grammar] {
        let f = Fixture::new();
        let first = serde_json::to_vec(&input(kind)).unwrap();
        let mut bytes = first.clone();
        bytes.push(b'\n');
        bytes.extend_from_slice(&first);
        let prepared = prepare_authored_jsonl(&bytes, kind, &f.settings, &f.environment).unwrap();
        assert_eq!(prepared.items.len(), 2);
        assert!(!prepared.ready);
        assert!(prepared.items[0].ready);
        assert!(!prepared.items[1].ready);
        assert!(
            prepared.items[1]
                .issues
                .iter()
                .any(|issue| issue.code == "DUPLICATE_BATCH_ITEM")
        );
        let store = linguist_store::Store::read_only(&f.state()).unwrap();
        let plan = store.revision(prepared.plan_id, 1).unwrap();
        assert_eq!(plan.documents.len(), 2);
        assert_eq!(
            plan.documents[0].sources[0].fields["authored_input"].as_bytes(),
            &[first.as_slice(), b"\n"].concat()
        );
        assert_eq!(
            plan.documents[1].sources[0].fields["authored_input"].as_bytes(),
            first
        );
        for document in &plan.documents {
            let digest = &document.archives[0].asset_digests[0];
            assert_eq!(
                store.asset(digest, 1024 * 1024).unwrap(),
                document.sources[0].fields["authored_input"].as_bytes()
            );
        }
    }
}
#[test]
fn jsonl_invalid_later_record_leaves_no_state() {
    let f = Fixture::new();
    let first = serde_json::to_vec(&input(Kind::Vocabulary)).unwrap();
    let bytes = [
        first.as_slice(),
        b"\n{\"schema_version\":2,\"kind\":\"grammar\"}",
    ]
    .concat();
    let error =
        prepare_authored_jsonl(&bytes, Kind::Vocabulary, &f.settings, &f.environment).unwrap_err();
    assert!(error.starts_with("INPUT_RECORD_2:"), "{error}");
    assert!(!f.state().exists());
}
#[test]
fn jsonl_framing_and_configured_bounds_fail_before_state_creation() {
    let mut f = Fixture::new();
    let first = serde_json::to_vec(&input(Kind::Vocabulary)).unwrap();
    assert!(
        prepare_authored_jsonl(b"{broken}\n", Kind::Vocabulary, &f.settings, &f.environment)
            .unwrap_err()
            .starts_with("INPUT_RECORD_1:")
    );
    assert_eq!(
        prepare_authored_jsonl(b"", Kind::Vocabulary, &f.settings, &f.environment).unwrap_err(),
        "INPUT_EMPTY_BATCH"
    );
    assert_eq!(
        prepare_authored_jsonl(
            &[first.as_slice(), b"\n\n"].concat(),
            Kind::Vocabulary,
            &f.settings,
            &f.environment
        )
        .unwrap_err(),
        "INPUT_EMPTY_RECORD: line 2"
    );
    f.settings
        .values
        .insert("selection.max_notes".into(), serde_json::json!(1));
    assert!(
        prepare_authored_jsonl(
            &[first.as_slice(), b"\n", first.as_slice()].concat(),
            Kind::Vocabulary,
            &f.settings,
            &f.environment
        )
        .unwrap_err()
        .starts_with("INPUT_BATCH_TOO_LARGE")
    );
    f.settings
        .values
        .insert("selection.max_notes".into(), serde_json::json!(10));
    f.settings.values.insert(
        "input.max_record_chars".into(),
        serde_json::json!(std::str::from_utf8(&first).unwrap().chars().count() - 1),
    );
    assert!(
        prepare_authored_jsonl(&first, Kind::Vocabulary, &f.settings, &f.environment)
            .unwrap_err()
            .contains("INPUT_RECORD_TOO_LARGE")
    );
    assert!(!f.state().exists());
}
#[test]
fn managed_duplicate_candidates_remain_review_evidence_even_when_fields_match() {
    use linguist_application::duplicate_candidates::{CandidateReader, inspect};
    struct OverflowReader;
    impl CandidateReader for OverflowReader {
        fn find_notes(&self, _query: &str) -> Result<Vec<String>> {
            Ok(vec!["123".into(), "124".into()])
        }
        fn notes_info(&self, _ids: &[String]) -> Result<Vec<serde_json::Value>> {
            panic!("oversized candidate sets must not fetch note fields")
        }
    }
    struct Reader {
        query: String,
        rows: Vec<serde_json::Value>,
    }
    impl CandidateReader for Reader {
        fn find_notes(&self, query: &str) -> Result<Vec<String>> {
            assert_eq!(query, self.query);
            Ok(vec!["123".into()])
        }
        fn notes_info(&self, ids: &[String]) -> Result<Vec<serde_json::Value>> {
            assert_eq!(ids, &["123"]);
            Ok(self.rows.clone())
        }
    }
    for kind in [Kind::Vocabulary, Kind::Grammar] {
        let f = Fixture::new();
        let prepared = prepare_authored(
            &serde_json::to_vec(&input(kind)).unwrap(),
            kind,
            &f.settings,
            &f.environment,
        )
        .unwrap();
        let store = linguist_store::Store::read_only(&f.state()).unwrap();
        let plan = store.revision(prepared.plan_id, 1).unwrap();
        let rendered = &plan.rendered[0];
        let fields = rendered
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::json!({"value":value})))
            .collect::<serde_json::Map<_, _>>();
        let (name, field, term) = match kind {
            Kind::Vocabulary => ("Linguist Vocabulary v2", "Expression", "食べる"),
            Kind::Grammar => ("Linguist Grammar v2", "Pattern", "〜ても"),
        };
        let reader = Reader {
            query: format!("note:\"{name}\" {field}:\"{term}\""),
            rows: vec![serde_json::json!({"noteId":123,"modelName":name,"fields":fields})],
        };
        let report = inspect(&plan, prepared.document_id, &reader, 1, 10000).unwrap();
        assert_eq!(report.candidates.len(), 1);
        assert!(report.candidates[0].compared_fields_match);
        assert!(!report.collection_duplicate_check_complete);
        assert!(!report.semantic_identity_verified);
        assert!(!report.apply_eligible);
        assert!(inspect(&plan, prepared.document_id, &reader, 0, 10000).is_err());
        assert!(
            inspect(&plan, prepared.document_id, &OverflowReader, 1, 10000)
                .unwrap_err()
                .starts_with("DUPLICATE_CANDIDATES_TOO_MANY")
        );
    }
}
#[test]
fn invalid_content_is_persisted_for_review_without_a_render() {
    let f = Fixture::new();
    let mut value = input(Kind::Vocabulary);
    value["body"]["expression"] = serde_json::json!("");
    let result = prepare_authored(
        &serde_json::to_vec(&value).unwrap(),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    assert!(!result.ready);
    assert!(!result.issues.is_empty());
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    assert!(
        store
            .revision(result.plan_id, 1)
            .unwrap()
            .rendered
            .is_empty()
    );
}
#[test]
fn schema_kind_and_provider_failures_do_not_initialize_state() {
    let mut f = Fixture::new();
    let bytes = serde_json::to_vec(&input(Kind::Vocabulary)).unwrap();
    assert!(prepare_authored(&bytes, Kind::Grammar, &f.settings, &f.environment).is_err());
    let mut value = input(Kind::Vocabulary);
    value["unknown"] = serde_json::json!(true);
    assert!(
        prepare_authored(
            &serde_json::to_vec(&value).unwrap(),
            Kind::Vocabulary,
            &f.settings,
            &f.environment
        )
        .is_err()
    );
    f.settings
        .values
        .insert("llm.enabled".into(), serde_json::json!(true));
    assert!(
        prepare_authored(&bytes, Kind::Vocabulary, &f.settings, &f.environment)
            .unwrap_err()
            .contains("UNAVAILABLE")
    );
    assert!(!f.root.exists());
}
#[test]
fn explicit_tasks_win_over_config_and_credentials_stay_references() {
    let mut f = Fixture::new();
    f.settings.values.insert(
        "learning.vocabulary.production".into(),
        serde_json::json!(true),
    );
    f.settings.values.insert(
        "llm.api_key_env".into(),
        serde_json::json!("LAB_TEST_SECRET"),
    );
    f.environment
        .insert("LAB_TEST_SECRET".into(), "must never be stored".into());
    let mut value = input(Kind::Vocabulary);
    value["requested_tasks"] = serde_json::json!(["comprehension"]);
    let result = prepare_authored(
        &serde_json::to_vec(&value).unwrap(),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    assert!(result.ready);
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    let plan = store.revision(result.plan_id, 1).unwrap();
    assert_eq!(
        plan.settings.secret_refs["llm.api_key_env"],
        "LAB_TEST_SECRET"
    );
    assert!(
        !serde_json::to_string(&plan)
            .unwrap()
            .contains("must never be stored")
    );
}

#[test]
fn export_controls_private_archive_disclosure_and_checksums_every_asset() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let f = Fixture::new();
    let bytes = serde_json::to_vec_pretty(&input(Kind::Vocabulary)).unwrap();
    let result = prepare_authored(&bytes, Kind::Vocabulary, &f.settings, &f.environment).unwrap();
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    let plan = store.revision(result.plan_id, 1).unwrap();
    for private in [false, true] {
        let path = f.root.join(if private {
            "private.json"
        } else {
            "content.json"
        });
        let receipt = export::export_plan(&store, &plan, &path, private).unwrap();
        let exported = std::fs::read(&path).unwrap();
        assert_eq!(
            receipt.checksum,
            linguist_core::canonical::asset_digest(&exported)
        );
        assert_eq!(receipt.size_bytes, exported.len() as u64);
        assert!(!receipt.apply_authorized);
        let bundle: serde_json::Value = linguist_core::canonical::parse(&exported).unwrap();
        assert_eq!(
            bundle["manifest_digest"],
            linguist_core::canonical::digest("plan-export-manifest", &bundle["manifest"]).unwrap()
        );
        assert_eq!(bundle["manifest"]["source_plan_digest"], result.digest);
        assert_eq!(bundle["manifest"]["includes_private_archives"], private);
        if private {
            assert_eq!(
                bundle["manifest"]["private_plan"]["id"],
                result.plan_id.to_string()
            );
            let asset = &bundle["asset_data"][0];
            let decoded = STANDARD.decode(asset["data"].as_str().unwrap()).unwrap();
            assert_eq!(decoded, bytes);
            assert_eq!(
                asset["digest"],
                linguist_core::canonical::asset_digest(&decoded)
            );
        } else {
            assert!(bundle["manifest"]["private_plan"].is_null());
            assert!(bundle["asset_data"].as_array().unwrap().is_empty());
            assert!(
                !String::from_utf8(exported.clone())
                    .unwrap()
                    .contains("authored_input")
            );
        }
        assert!(
            export::export_plan(&store, &plan, &path, private)
                .unwrap_err()
                .contains("CONFLICT")
        );
        assert_eq!(std::fs::read(&path).unwrap(), exported);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    assert_eq!(store.revision(result.plan_id, 1).unwrap(), plan);
}

struct DictionaryFixture;
impl DictionaryPort for DictionaryFixture {
    fn lookup(
        &self,
        query: &str,
        target: &linguist_core::Language,
    ) -> std::result::Result<linguist_dictionary::JishoPage, String> {
        linguist_dictionary::parse_jisho(query,target,r#"{"meta":{"status":200},"data":[{"slug":"eat","japanese":[{"word":"食べる","reading":"たべる"}],"senses":[{"english_definitions":["to eat"]},{"english_definitions":["consume a meal"]}]}]}"#.as_bytes(),1024,10).map_err(|error|error.to_string())
    }
}
#[test]
fn dictionary_preparation_archives_all_senses_and_requires_explicit_selection() {
    use linguist_core::{LearningContent, records::ReviewChoice, review::*};
    let mut f = Fixture::new();
    f.settings
        .values
        .insert("dictionary.provider".into(), serde_json::json!("jisho"));
    let bytes=r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","body":{"expression":"食べる"}}"#.as_bytes();
    let result = prepare_with_dictionary(
        bytes,
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        Some(&DictionaryFixture),
    )
    .unwrap();
    assert!(!result.ready);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "DICTIONARY_SENSE_REVIEW")
    );
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let base = store.revision(result.plan_id, 1).unwrap();
    let doc = &base.documents[0];
    assert_eq!(doc.archives.len(), 2);
    assert_eq!(doc.evidence.len(), 2);
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    assert!(vocab.meaning.is_empty());
    assert!(vocab.sense_key.is_empty());
    assert_eq!(vocab.dictionary[0].senses.len(), 2);
    let mut seen = Vec::new();
    let mut cursor = 0;
    loop {
        let page =
            linguist_application::review::inspection::page(&base, Some(doc.id), cursor, 1).unwrap();
        assert!(page["issues"].as_array().unwrap().len() <= 1);
        assert_eq!(page["archives_included"], false);
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("provider_response")
        );
        for issue in page["issues"].as_array().unwrap() {
            seen.push(issue.clone());
        }
        let Some(next) = page["next_index"].as_u64() else {
            break;
        };
        cursor = next as u32;
    }
    let selected = seen
        .iter()
        .find(|issue| issue["issue"]["code"] == "DICTIONARY_SENSE_REVIEW")
        .unwrap();
    assert_eq!(selected["actor_required"], true);
    assert_eq!(selected["request_identity"]["base_digest"], result.digest);
    assert_eq!(selected["templates"].as_array().unwrap().len(), 2);
    assert_eq!(
        selected["templates"][0]["choice"]["decision"],
        "sense_with_reading"
    );
    assert!(
        seen.iter()
            .any(|issue| issue["issue"]["code"] == "REQUIRED_CONTENT"
                && issue["resolution_available"] == false)
    );
    let raw = store
        .asset(&doc.archives[1].asset_digests[0], 1024)
        .unwrap();
    assert!(String::from_utf8(raw).unwrap().contains("consume a meal"));
    let request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: result.digest,
        document_id: doc.id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Sense(vocab.dictionary[0].senses[1].key.clone()),
    };
    let resolved = resolve(&base, &request, "unix-seconds:1".into()).unwrap();
    assert!(
        resolved.ready,
        "{:?}",
        resolved.revision.documents[0].issues
    );
    let LearningContent::Vocabulary(selected) = &resolved.revision.documents[0].content else {
        panic!()
    };
    assert_eq!(selected.meaning, "consume a meal");
    assert_eq!(selected.reading, "たべる");
    assert_ne!(
        resolved.revision.documents[0].semantic_digest().unwrap(),
        request.input_digest
    );
    store.publish_revision(&resolved.revision).unwrap();
    assert_eq!(store.revision(result.plan_id, 1).unwrap(), base);
    assert!(
        store
            .validate_revision(result.plan_id, 2)
            .unwrap()
            .evidence
            .content_ready
    );
    let mut changed = resolved.revision.documents[0].clone();
    changed.personal_notes = "changed content".into();
    assert!(
        linguist_core::validate(&changed)
            .iter()
            .any(|issue| issue.code == "DICTIONARY_SENSE_REVIEW")
    );
}
#[test]
fn dictionary_port_cannot_fabricate_facts_unrelated_to_archived_bytes() {
    struct Forged;
    impl DictionaryPort for Forged {
        fn lookup(
            &self,
            query: &str,
            target: &linguist_core::Language,
        ) -> std::result::Result<linguist_dictionary::JishoPage, String> {
            let mut page = DictionaryFixture.lookup(query, target)?;
            page.entries[0].senses[0].definitions = vec!["invented fact".into()];
            Ok(page)
        }
    }
    let mut f = Fixture::new();
    f.settings
        .values
        .insert("dictionary.provider".into(), serde_json::json!("jisho"));
    let bytes = serde_json::to_vec(&input(Kind::Vocabulary)).unwrap();
    assert!(
        prepare_with_dictionary(
            &bytes,
            Kind::Vocabulary,
            &f.settings,
            &f.environment,
            Some(&Forged)
        )
        .unwrap_err()
        .contains("CONFLICT")
    );
    assert!(!f.root.exists());
}

struct EnglishDictionaryFixture;
impl DictionaryPort for EnglishDictionaryFixture {
    fn lookup(
        &self,
        query: &str,
        target: &linguist_core::Language,
    ) -> std::result::Result<linguist_dictionary::DictionaryPage, String> {
        linguist_dictionary::wiktionary::parse_definition(query,target,
            br#"{"en":[{"language":"English","partOfSpeech":"Verb","definitions":[{"definition":"Consume food","parsedExamples":[{"example":"We eat food."}]}]}]}"#,1024,10).map_err(|error|error.to_string())
    }
}
#[test]
fn english_dictionary_preparation_resolves_sense_without_inventing_pronunciation() {
    use linguist_core::{LearningContent, records::ReviewChoice, review::*};
    let mut f = Fixture::new();
    f.settings.values.insert(
        "dictionary.provider".into(),
        serde_json::json!("wiktionary"),
    );
    let input=br#"{"schema_version":2,"kind":"vocabulary","target_language":"en","body":{"expression":"eat","examples":[{"sentence":"I eat.","translation":"","provenance":"user"}]}}"#;
    let result = prepare_with_dictionary(
        input,
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        Some(&EnglishDictionaryFixture),
    )
    .unwrap();
    assert!(!result.ready);
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let base = store.revision(result.plan_id, 1).unwrap();
    let doc = &base.documents[0];
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    let request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: result.digest,
        document_id: doc.id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Sense(vocab.dictionary[0].senses[0].key.clone()),
    };
    let resolved = resolve(&base, &request, "unix-seconds:1".into()).unwrap();
    assert!(
        resolved.ready,
        "{:?}",
        resolved.revision.documents[0].issues
    );
    let LearningContent::Vocabulary(vocab) = &resolved.revision.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.meaning, "Consume food");

    let reference = &resolved.revision.rendered[0].fields["Meaning"];
    assert!(reference.starts_with("<p>Consume food</p>"));
    for value in [
        "Forms",
        "eat",
        "Wiktionary contributors",
        "We eat food.",
        "en.wiktionary.org",
    ] {
        assert!(
            reference.contains(value),
            "missing dictionary reference {value}"
        );
    }

    assert!(vocab.reading.is_empty() && vocab.pronunciation.is_empty());
    assert_eq!(vocab.examples.len(), 2);
    assert_eq!(
        vocab.examples[0].provenance,
        linguist_core::Provenance::User
    );
    assert_eq!(
        vocab.examples[1].provenance,
        linguist_core::Provenance::Dictionary
    );
    store.publish_revision(&resolved.revision).unwrap();
    assert!(
        store
            .validate_revision(result.plan_id, 2)
            .unwrap()
            .evidence
            .content_ready
    );
    assert_eq!(store.revision(result.plan_id, 1).unwrap(), base);
}

#[test]
fn japanese_sense_review_requires_a_reading_for_the_selected_written_form() {
    use linguist_core::{LearningContent, records::ReviewChoice, review::*};
    struct MultipleReadings;
    impl DictionaryPort for MultipleReadings {
        fn lookup(
            &self,
            query: &str,
            target: &linguist_core::Language,
        ) -> std::result::Result<linguist_dictionary::DictionaryPage, String> {
            linguist_dictionary::parse_jisho(query, target,
                r#"{"meta":{"status":200},"data":[{"slug":"life","japanese":[{"word":"生","reading":"せい"},{"word":"生","reading":"なま"},{"word":"生活","reading":"せいかつ"}],"senses":[{"english_definitions":["life"]}]}]}"#.as_bytes(), 4096, 10)
                .map_err(|error| error.to_string())
        }
    }
    let mut f = Fixture::new();
    f.settings
        .values
        .insert("dictionary.provider".into(), serde_json::json!("jisho"));
    let input = r#"{"schema_version":2,"kind":"vocabulary","target_language":"ja","explanation_language":"en","body":{"expression":"生"}}"#;
    let prepared = prepare_with_dictionary(
        input.as_bytes(),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
        Some(&MultipleReadings),
    )
    .unwrap();
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &base.documents[0];
    let LearningContent::Vocabulary(vocab) = &doc.content else {
        panic!()
    };
    let key = vocab.dictionary[0].senses[0].key.clone();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: prepared.digest,
        document_id: doc.id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Sense(key.clone()),
    };
    assert!(
        resolve(&base, &request, "now".into())
            .unwrap_err()
            .to_string()
            .contains("multiple dictionary readings")
    );
    for reading in ["せいかつ", "invented", ""] {
        request.choice = ReviewChoice::SenseWithReading {
            key: key.clone(),
            reading: reading.into(),
        };
        assert!(
            resolve(&base, &request, "now".into())
                .unwrap_err()
                .to_string()
                .contains("READING_CONFLICT")
        );
    }
    request.choice = ReviewChoice::SenseWithReading {
        key,
        reading: "なま".into(),
    };
    let encoded = serde_json::to_vec(&request).unwrap();
    let decoded: ResolutionRequest = linguist_core::canonical::parse(&encoded).unwrap();
    let result = resolve(&base, &decoded, "now".into()).unwrap();
    let LearningContent::Vocabulary(vocab) = &result.revision.documents[0].content else {
        panic!()
    };
    assert_eq!(vocab.expression, "生");
    assert_eq!(vocab.reading, "なま");
    assert_eq!(vocab.meaning, "life");
    assert!(
        !linguist_core::validate(&result.revision.documents[0])
            .iter()
            .any(|i| i.code == "DICTIONARY_SENSE_REVIEW")
    );
    store.publish_revision(&result.revision).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    let mut changed = result.revision.documents[0].clone();
    let LearningContent::Vocabulary(vocab) = &mut changed.content else {
        panic!()
    };
    vocab.reading = "せい".into();
    // Even another valid reading needs its own explicit decision.
    let digest = changed.semantic_digest().unwrap();
    changed.reviews[0].input_digest = digest;
    assert!(
        linguist_core::validate(&changed)
            .iter()
            .any(|i| i.code == "DICTIONARY_SENSE_REVIEW")
    );
}

#[test]
fn typed_cue_repairs_preserve_tasks_reject_leaks_and_publish_recoverable_children() {
    use linguist_core::{Task, records::ReviewChoice, review::ResolutionRequest};
    let f = Fixture::new();
    let mut authored = input(Kind::Vocabulary);
    authored["requested_tasks"] = serde_json::json!(["comprehension", "production", "spelling"]);
    let prepared = prepare_authored(
        &serde_json::to_vec(&authored).unwrap(),
        Kind::Vocabulary,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &base.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|issue| {
            issue.code == "MISSING_CUE" && issue.field.as_deref() == Some("production_prompt")
        })
        .unwrap();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: prepared.digest,
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "author".into(),
        choice: ReviewChoice::Cue {
            task: Task::Production,
            text: "食べる".into(),
        },
    };
    assert!(
        linguist_application::review::resolve(&store, &base, &request, "now".into())
            .unwrap_err()
            .contains("CUE_CONTENT_INVALID")
    );
    request.choice = ReviewChoice::Cue {
        task: Task::Spelling,
        text: "Write the verb for consuming food.".into(),
    };
    assert!(linguist_application::review::resolve(&store, &base, &request, "now".into()).is_err());
    request.choice = ReviewChoice::Cue {
        task: Task::Production,
        text: "x".repeat(
            f.settings.values["input.max_record_chars"]
                .as_u64()
                .unwrap() as usize
                + 1,
        ),
    };
    assert!(
        linguist_application::review::resolve(&store, &base, &request, "now".into())
            .unwrap_err()
            .contains("REVIEW_INPUT_LIMIT")
    );
    assert_eq!(store.latest_revision(base.id).unwrap(), 1);
    request.choice = ReviewChoice::Cue {
        task: Task::Production,
        text: "Say the verb for consuming food.".into(),
    };
    let result =
        linguist_application::review::resolve(&store, &base, &request, "now".into()).unwrap();
    assert!(!result.ready);
    assert_eq!(
        result.revision.documents[0].requested_tasks,
        base.documents[0].requested_tasks
    );
    store.publish_revision(&result.revision).unwrap();
    let child = result.revision;
    let doc = &child.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|issue| {
            issue.code == "MISSING_CUE" && issue.field.as_deref() == Some("spelling_prompt")
        })
        .unwrap();
    request.base_revision = 2;
    request.base_digest = child.approval_digest().unwrap();
    request.input_digest = doc.semantic_digest().unwrap();
    request.issue_id = issue.id.clone();
    request.choice = ReviewChoice::Cue {
        task: Task::Spelling,
        text: "Write the verb for consuming food.".into(),
    };
    let result =
        linguist_application::review::resolve(&store, &child, &request, "now".into()).unwrap();
    assert!(result.ready, "{:?}", result.revision.documents[0].issues);
    assert_eq!(
        result.revision.documents[0].sources,
        base.documents[0].sources
    );
    assert_eq!(
        result.revision.documents[0].archives,
        base.documents[0].archives
    );
    assert_eq!(result.revision.rendered[0].fields["EnableProduction"], "1");
    assert_eq!(result.revision.rendered[0].fields["EnableSpelling"], "1");
    store.publish_revision(&result.revision).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    drop(store);
    assert_eq!(
        linguist_store::Store::read_only(&f.state())
            .unwrap()
            .revision(base.id, 3)
            .unwrap(),
        result.revision
    );
}

#[test]
fn grammar_prompt_and_exercise_repairs_require_complete_nonleaking_answers() {
    use linguist_core::{Task, records::ReviewChoice, review::ResolutionRequest};
    let f = Fixture::new();
    let mut authored = input(Kind::Grammar);
    authored["requested_tasks"] = serde_json::json!(["recognition", "application"]);
    authored["body"]["recognition_prompt"] = serde_json::json!("");
    let prepared = prepare_authored(
        &serde_json::to_vec(&authored).unwrap(),
        Kind::Grammar,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    let mut store = linguist_store::Store::open(&f.state()).unwrap();
    let base = store.revision(prepared.plan_id, 1).unwrap();
    let doc = &base.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|issue| {
            issue.code == "REQUIRED_CONTENT" && issue.field.as_deref() == Some("recognition_prompt")
        })
        .unwrap();
    let request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: prepared.digest,
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "author".into(),
        choice: ReviewChoice::Cue {
            task: Task::Recognition,
            text: "Mẫu này thể hiện quan hệ gì?".into(),
        },
    };
    let result =
        linguist_application::review::resolve(&store, &base, &request, "now".into()).unwrap();
    store.publish_revision(&result.revision).unwrap();
    let child = result.revision;
    let doc = &child.documents[0];
    let issue = doc
        .issues
        .iter()
        .find(|issue| issue.code == "MISSING_EXERCISE")
        .unwrap();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 2,
        base_digest: child.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "author".into(),
        choice: ReviewChoice::Exercise {
            prompt: "雨が降っても行きます。".into(),
            answer: "行きます".into(),
        },
    };
    assert!(
        linguist_application::review::resolve(&store, &child, &request, "now".into())
            .unwrap_err()
            .contains("CUE_CONTENT_INVALID")
    );
    request.choice = ReviewChoice::Exercise {
        prompt: "雨が降っても___。".into(),
        answer: "".into(),
    };
    assert!(linguist_application::review::resolve(&store, &child, &request, "now".into()).is_err());
    request.choice = ReviewChoice::Exercise {
        prompt: "雨が降っても___。".into(),
        answer: "行きます".into(),
    };
    let result =
        linguist_application::review::resolve(&store, &child, &request, "now".into()).unwrap();
    assert!(result.ready, "{:?}", result.revision.documents[0].issues);
    assert_eq!(
        result.revision.documents[0].requested_tasks,
        base.documents[0].requested_tasks
    );
    store.publish_revision(&result.revision).unwrap();
    assert!(
        !store
            .validate_revision(base.id, 3)
            .unwrap()
            .evidence
            .apply_eligible
    );
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
}
