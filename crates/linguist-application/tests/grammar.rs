//! WP-08: grammar preparation from screenshots and authored input.
use linguist_application::{grammar::*, mapping::*, ocr_inspection, revamp::*, source_archive::*};
use linguist_config::{ConfigFile, Effective, Registry, ResolveOptions};
use linguist_core::{
    Example, Grammar, LearningContent, Provenance, Task,
    records::{Evidence, PlanRevision, ReviewChoice},
    review::{ResolutionRequest, decision_templates, resolve},
    validation::{self, Severity},
};
use serde_json::json;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::PathBuf};

const HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

struct Fixture {
    root: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    /// Fake OCR engine printing one TSV line per entry of `lines` (text, confidence).
    fn new(lines: &[(&str, u32)], exit: Option<u32>) -> Self {
        let root = std::env::temp_dir().join(format!("lab-grammar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("tessdata")).unwrap();
        std::fs::write(root.join("tessdata/eng.traineddata"), "eng").unwrap();
        let mut tsv = format!("{HEADER}\n");
        for (index, (text, confidence)) in lines.iter().enumerate() {
            tsv.push_str(&format!(
                "5\t1\t1\t1\t{}\t1\t3\t{}\t90\t6\t{confidence}\t{text}\n",
                index + 1,
                index * 9 + 3
            ));
        }
        let body = match exit {
            Some(code) => format!("exit {code}"),
            None => format!("cat <<'EOF'\n{tsv}EOF"),
        };
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 'tesseract 5.9.9-fake'; exit 0; }}\n\
             for a in \"$@\"; do [ \"$a\" = --list-langs ] && {{ \
             echo 'List of available languages in \"{}/tessdata/\" (1):'; echo eng; exit 0; }}; done\n{body}\n",
            root.display()
        );
        let path = root.join("tesseract");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        while let Err(error) = std::process::Command::new(&path).arg("--version").output() {
            assert_eq!(error.raw_os_error(), Some(libc::ETXTBSY));
        }
        Self { root }
    }

    /// A Japanese grammar note whose front is only a screenshot and whose back
    /// is a Vietnamese explanation.
    fn capture(&self, pattern: &str) -> (RevampCapture, Effective) {
        let values = [
            ("Front", pattern),
            ("Back", "Diễn tả sự nhượng bộ"),
            ("Media", "<img src=\"page.png\">"),
        ];
        let fields = values
            .iter()
            .enumerate()
            .map(|(i, (n, v))| ((*n).to_owned(), json!({"value":v,"order":i})))
            .collect::<BTreeMap<_, _>>();
        let note = json!({"noteId":"91","modelName":"Legacy","fields":fields,"cards":["501"],"tags":["grammar"]});
        let model = json!({"model":{"name":"Legacy","id":"12"},"fields":["Front","Back","Media"],"templates":{"Card":{"Front":"f","Back":"b"}},"css":""});
        let cards = json!([{"cardId":"501","note":"91","reps":40}]);
        let mut captured = archive_read_capture(
            &serde_json::to_vec(&note).unwrap(),
            &serde_json::to_vec(&model).unwrap(),
            &serde_json::to_vec(&cards).unwrap(),
            10000,
        )
        .unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(120, 60, image::Luma([250])))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        linguist_application::source_archive::media::attach_original_media(
            &mut captured,
            BTreeMap::from([("page.png".into(), Some(png.into_inner()))]),
            10 * 1024 * 1024,
            10 * 1024 * 1024,
        )
        .unwrap();
        let purpose = "japanese_grammar";
        let mut options = ResolveOptions {
            purpose: Some(purpose.into()),
            ..Default::default()
        };
        options.flags.insert(
            format!("purposes.{purpose}.fields"),
            json!({"pattern":"Front","meaning":"Back"}),
        );
        options
            .flags
            .insert(format!("purposes.{purpose}.source_model"), json!("Legacy"));
        let mut settings =
            linguist_config::resolve(&Registry::builtin(), &ConfigFile::default(), &options)
                .unwrap();
        for (key, value) in [
            ("learning.explanation_language", json!("vi")),
            ("images.existing_policy", json!("inspect")),
            ("ocr.executable", json!(self.root.join("tesseract"))),
            ("ocr.languages", json!(["eng"])),
            ("storage.temp_dir", json!(self.root.join("tmp"))),
            ("storage.cache_dir", json!(self.root.join("cache"))),
            ("storage.state_dir", json!(self.root.join("state"))),
            ("cache.ttl_hours", json!(0)),
        ] {
            settings.values.insert(key.into(), value);
        }
        let mapping =
            map_purpose_fields(&settings, purpose, "Legacy", &captured.source.fields).unwrap();
        (RevampCapture { captured, mapping }, settings)
    }

    fn publish(&self, pattern: &str) -> (PlanRevision, Effective) {
        let (capture, settings) = self.capture(pattern);
        let env = BTreeMap::from([("HOME".into(), "/tmp/lab-grammar".into())]);
        let prepared =
            publish_capture_draft(&capture, &settings, "japanese_grammar", &env).unwrap();
        let store = linguist_store::Store::read_only(&self.root.join("state")).unwrap();
        (store.revision(prepared.plan_id, 1).unwrap(), settings)
    }
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

const FIVE: [(&str, u32); 6] = [
    ("〜ても", 96),
    ("Diễn tả sự nhượng bộ", 96),
    ("〜なくてもいい", 96),
    ("〜ば〜ほど", 96),
    ("〜ないで / 〜ずに", 96),
    ("〜ようにする", 96),
];

#[test]
fn five_pattern_screenshot_becomes_five_reviewed_units_with_fresh_siblings() {
    let fixture = Fixture::new(&FIVE, None);
    let (plan, mut settings) = fixture.publish("");
    let doc = &plan.documents[0];
    assert_eq!(doc.explanation_language.as_str(), "vi");
    let issue = validation::validate(doc)
        .into_iter()
        .find(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
        .unwrap();
    // Negation and operators survive verbatim; the explanation line is not a unit.
    let texts: Vec<String> = issue
        .source_refs
        .iter()
        .map(|id| {
            doc.regions
                .iter()
                .find(|r| r.id.to_string() == *id)
                .unwrap()
                .text
                .clone()
        })
        .collect();
    assert_eq!(
        texts,
        [
            "〜ても",
            "〜なくてもいい",
            "〜ば〜ほど",
            "〜ないで / 〜ずに",
            "〜ようにする"
        ]
    );
    let templates = decision_templates(doc, &issue);
    assert_eq!(templates.len(), 6);
    let ReviewChoice::Segmentation(all) = templates[0].clone() else {
        panic!()
    };
    assert_eq!(all.len(), 5);
    // Out-of-order or unknown regions are rejected.
    let reversed: Vec<_> = all.iter().rev().copied().collect();
    let request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: doc.id,
        issue_id: issue.id.clone(),
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::Segmentation(reversed),
    };
    assert!(resolve(&plan, &request, "unix-seconds:1".into()).is_err());

    let segmented = decide(&plan, &issue.id, ReviewChoice::Segmentation(all.clone()));
    let pending = validation::validate(&segmented.documents[0]);
    assert!(
        pending
            .iter()
            .any(|i| i.code == "GRAMMAR_SPLIT_PENDING" && i.severity == Severity::Error)
    );
    assert!(
        !pending
            .iter()
            .any(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
    );
    settings.values.insert("llm.enabled".into(), json!(true));
    assert!(
        ocr_inspection::generation_ready(&segmented.documents[0], &settings)
            .unwrap_err()
            .starts_with("GENERATION_SEGMENTATION_REQUIRED")
    );

    // Live captures do not yet carry native card state (WP-03); inject a mature
    // card so the split must show it stays on the anchor only.
    let mut segmented = segmented;
    let capture_index = segmented.documents[0]
        .sources
        .iter()
        .position(|s| s.kind == "anki_read_capture_v2")
        .unwrap();
    segmented.documents[0].sources[capture_index].cards = vec![linguist_core::records::CardState {
        id: "501".to_owned().try_into().unwrap(),
        task: Task::Recognition,
        deck_id: "1".to_owned().try_into().unwrap(),
        home_deck_id: "1".to_owned().try_into().unwrap(),
        scheduler: BTreeMap::from([("interval_days".into(), "120".into())]),
        history_digest: "a".repeat(64),
    }];
    let mut store = linguist_store::Store::open_existing(&fixture.root.join("state")).unwrap();
    store.publish_revision(&segmented).unwrap();
    let mut template = split_template(&segmented, doc.id).unwrap();
    assert_eq!(
        template
            .units
            .iter()
            .map(|u| u.pattern.as_str())
            .collect::<Vec<_>>(),
        texts.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(
        template
            .units
            .iter()
            .all(|u| u.use_key.is_empty() && u.meaning.is_empty())
    );
    for (index, unit) in template.units.iter_mut().enumerate() {
        unit.use_key = format!("use-{index}");
        unit.meaning = format!("ý nghĩa {index}");
        unit.formation = "V + mẫu".into();
        unit.examples = vec![Example {
            sentence: format!("例文{index}。"),
            translation: format!("Câu ví dụ {index}."),
            provenance: Provenance::User,
            evidence_ids: vec![],
        }];
    }
    template.actor = "reviewer".into();
    let raw = serde_json::to_vec(&template).unwrap();
    let child = split(&mut store, &segmented, &template, &raw).unwrap();
    assert_eq!(child.documents.len(), 5);
    let group = &child.grammar_groups[0];
    assert_eq!(group.anchor_document, doc.id);
    for document in &child.documents {
        let issues = validation::validate(document);
        assert!(!issues.iter().any(|i| matches!(
            i.code.as_str(),
            "GRAMMAR_SPLIT_PENDING" | "GRAMMAR_SEGMENTATION_REVIEW"
        )));
        let capture = document
            .sources
            .iter()
            .find(|s| s.kind == "anki_read_capture_v2")
            .unwrap();
        if document.id == doc.id {
            // Only the anchor keeps the captured card state and task maps.
            assert_eq!(capture.cards.len(), 1);
            assert_eq!(capture.cards[0].scheduler["interval_days"], "120");
            assert_eq!(document.task_maps, doc.task_maps);
        } else {
            assert!(capture.cards.is_empty());
            assert!(document.task_maps.is_empty());
            assert_eq!(document.requested_tasks, [Task::Recognition]);
            // The source archive remains linked as a reference.
            assert!(document.archives.iter().any(|a| a.source_id == capture.id));
        }
    }
}

#[test]
fn single_unit_choice_fills_an_empty_pattern_verbatim() {
    let fixture = Fixture::new(&FIVE, None);
    let (plan, _) = fixture.publish("");
    let issue = validation::validate(&plan.documents[0])
        .into_iter()
        .find(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
        .unwrap();
    let second = uuid::Uuid::parse_str(&issue.source_refs[1]).unwrap();
    let chosen = decide(&plan, &issue.id, ReviewChoice::Segmentation(vec![second]));
    let LearningContent::Grammar(grammar) = &chosen.documents[0].content else {
        panic!()
    };
    assert_eq!(grammar.pattern, "〜なくてもいい");
    let issues = validation::validate(&chosen.documents[0]);
    assert!(
        !issues
            .iter()
            .any(|i| i.code == "GRAMMAR_SPLIT_PENDING" || i.code == "GRAMMAR_SEGMENTATION_REVIEW")
    );
    // An existing source pattern is kept; the decision then only confirms scope.
    let (kept, _) = Fixture::new(&FIVE, None).publish("〜ても");
    let issue = validation::validate(&kept.documents[0])
        .into_iter()
        .find(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
        .unwrap();
    let first = uuid::Uuid::parse_str(&issue.source_refs[0]).unwrap();
    let confirmed = decide(&kept, &issue.id, ReviewChoice::Segmentation(vec![first]));
    let LearningContent::Grammar(grammar) = &confirmed.documents[0].content else {
        panic!()
    };
    assert_eq!(grammar.pattern, "〜ても");
}

#[test]
fn missing_low_confidence_and_failed_ocr_stay_reviewable() {
    let fixture = Fixture::new(&[("〜ても", 30)], None);
    let (plan, _) = fixture.publish("");
    let codes: Vec<_> = validation::validate(&plan.documents[0])
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(codes.contains(&"OCR_TEXT_REVIEW".to_owned()));
    assert!(codes.contains(&"GRAMMAR_SEGMENTATION_REVIEW".to_owned()));
    let failed = Fixture::new(&[], Some(3));
    let (plan, _) = failed.publish("");
    let issues = validation::validate(&plan.documents[0]);
    assert!(issues.iter().any(|i| i.code == "OCR_FAILED_REVIEW"));
    assert!(
        !issues
            .iter()
            .any(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
    );
    // A missing pattern is still blocking and needs an explicit decision or edit.
    assert!(
        issues
            .iter()
            .any(|i| i.code == "REQUIRED_CONTENT" && i.field.as_deref() == Some("pattern"))
    );
    let empty = Fixture::new(&[], None);
    let (plan, _) = empty.publish("");
    let issues = validation::validate(&plan.documents[0]);
    assert!(issues.iter().any(|i| i.code == "OCR_TEXT_REVIEW"));
    assert!(
        !issues
            .iter()
            .any(|i| i.code == "GRAMMAR_SEGMENTATION_REVIEW")
    );
}

fn grammar_doc(
    target: &str,
    explanation: &str,
    grammar: Grammar,
    tasks: &[&str],
) -> linguist_core::LearningDocument {
    serde_json::from_value(json!({
        "schema_version": 2, "id": uuid::Uuid::new_v4(), "target_language": target,
        "explanation_language": explanation, "requested_tasks": tasks,
        "content": {"kind": "grammar", "body": grammar}
    }))
    .unwrap()
}
fn unit(pattern: &str, meaning: &str, sentence: &str, translation: &str) -> Grammar {
    Grammar {
        pattern: pattern.into(),
        use_key: "k".into(),
        meaning: meaning.into(),
        formation: "f".into(),
        recognition_prompt: String::new(),
        examples: vec![Example {
            sentence: sentence.into(),
            translation: translation.into(),
            provenance: Provenance::User,
            evidence_ids: vec![],
        }],
        usage: String::new(),
        exercise_prompt: String::new(),
        exercise_answer: String::new(),
    }
}

#[test]
fn recognition_and_application_suggestions_are_focused_and_leak_free() {
    use linguist_core::cues::{suggest_exercise, suggest_recognition};
    let english = grammar_doc(
        "en",
        "en",
        unit("used to", "past habit", "I used to swim every day.", ""),
        &["recognition", "application"],
    );
    assert_eq!(
        suggest_recognition(&english).as_deref(),
        Some("What does “used to” express here?")
    );
    assert_eq!(
        suggest_exercise(&english),
        Some((
            "I ___ swim every day.".to_owned(),
            "used to — past habit".to_owned()
        ))
    );
    let vietnamese = grammar_doc(
        "ja",
        "vi",
        unit(
            "〜ても",
            "dù cho",
            "雨が降っても行きます。",
            "Dù trời mưa tôi vẫn đi.",
        ),
        &["recognition", "application"],
    );
    assert_eq!(
        suggest_recognition(&vietnamese).as_deref(),
        Some("Mẫu “〜ても” diễn đạt ý gì ở đây?")
    );
    assert_eq!(
        suggest_exercise(&vietnamese),
        Some((
            "Dù trời mưa tôi vẫn đi.\n雨が降っ＿＿行きます。".to_owned(),
            "ても — dù cho".to_owned()
        ))
    );
    // Discontinuous patterns, ambiguous (repeated) occurrences and absent
    // examples get no exercise suggestion.
    let discontinuous = grammar_doc(
        "en",
        "en",
        unit(
            "not only ... but also",
            "addition",
            "Not only red but also blue.",
            "",
        ),
        &["application"],
    );
    assert_eq!(suggest_exercise(&discontinuous), None);
    let repeated = grammar_doc(
        "en",
        "en",
        unit("so", "result", "It was so so cold, so I left.", ""),
        &["application"],
    );
    assert_eq!(suggest_exercise(&repeated), None);
    // A meaning that appears in the recognition prompt is reported as a leak.
    let mut leaky = english.clone();
    if let LearningContent::Grammar(g) = &mut leaky.content {
        g.recognition_prompt = "Which pattern marks a past habit?".into();
        g.exercise_prompt = "I ___ swim.".into();
        g.exercise_answer = "used to / would — past habit (both accepted)".into();
    }
    let issues = validation::validate(&leaky);
    assert!(
        issues
            .iter()
            .any(|i| i.code == "ANSWER_LEAK" && i.field.as_deref() == Some("recognition_prompt"))
    );
    // Multiple acknowledged answers in ExerciseAnswer are valid content.
    assert!(
        !issues
            .iter()
            .any(|i| i.field.as_deref() == Some("exercise_prompt"))
    );
}

#[test]
fn exercise_and_recognition_templates_resolve_through_typed_decisions() {
    let f = AuthoredFixture::new();
    let bytes = serde_json::to_vec(&json!({"schema_version":2,"kind":"grammar","target_language":"en","explanation_language":"en",
        "requested_tasks":["recognition","application"],
        "body":{"pattern":"used to","use_key":"past-habit","meaning":"past habit","formation":"used to + base verb","recognition_prompt":"","usage":"","exercise_prompt":"","exercise_answer":"",
            "examples":[{"sentence":"I used to swim every day.","translation":"","provenance":"user"}]}}))
    .unwrap();
    let result = linguist_application::prepare_authored(
        &bytes,
        linguist_application::Kind::Grammar,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    assert!(!result.ready);
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    let mut plan = store.revision(result.plan_id, 1).unwrap();
    for (field, expected) in [
        (
            "recognition_prompt",
            ReviewChoice::Cue {
                task: Task::Recognition,
                text: "What does “used to” express here?".into(),
            },
        ),
        (
            "exercise_prompt",
            ReviewChoice::Exercise {
                prompt: "I ___ swim every day.".into(),
                answer: "used to — past habit".into(),
            },
        ),
    ] {
        let issue = validation::validate(&plan.documents[0])
            .into_iter()
            .find(|i| i.field.as_deref() == Some(field))
            .unwrap();
        assert_eq!(
            decision_templates(&plan.documents[0], &issue),
            vec![expected.clone()]
        );
        plan = decide(&plan, &issue.id, expected);
    }
    // Acknowledged alternatives are accepted by the typed exercise decision too.
    assert!(
        validation::ready(&plan.documents[0]),
        "{:?}",
        validation::validate(&plan.documents[0])
    );
}

#[test]
fn conflicting_source_claims_need_evidence_exact_review() {
    let f = AuthoredFixture::new();
    let bytes = serde_json::to_vec(&json!({"schema_version":2,"kind":"grammar","target_language":"ja","explanation_language":"en",
        "body":{"pattern":"〜ても","use_key":"concession","meaning":"even if","formation":"V-te + も","recognition_prompt":"What relation is expressed?","usage":"","exercise_prompt":"","exercise_answer":"",
            "examples":[{"sentence":"雨が降っても行きます。","translation":"I will go even if it rains.","provenance":"user"}]}}))
    .unwrap();
    let result = linguist_application::prepare_authored(
        &bytes,
        linguist_application::Kind::Grammar,
        &f.settings,
        &f.environment,
    )
    .unwrap();
    assert!(result.ready, "{:?}", result.issues);
    let store = linguist_store::Store::read_only(&f.state()).unwrap();
    let mut plan = store.revision(result.plan_id, 1).unwrap();
    let source = plan.documents[0].sources[0].id;
    for claim in ["V-te + も", "V-ta + も"] {
        plan.documents[0].evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field: "formation".into(),
            provenance: Provenance::Source,
            source_id: Some(source),
            region_id: None,
            target: None,
            source_span: None,
            language: "ja".to_owned().try_into().unwrap(),
            claim: claim.into(),
            source_url: None,
            ambiguous: false,
        });
    }
    let issue = validation::validate(&plan.documents[0])
        .into_iter()
        .find(|i| i.code == "SOURCE_CLAIM_CONFLICT")
        .unwrap();
    assert_eq!(issue.severity, Severity::Review);
    assert_eq!(issue.source_refs.len(), 2);
    let ids: Vec<uuid::Uuid> = issue
        .source_refs
        .iter()
        .map(|id| id.parse().unwrap())
        .collect();
    let partial = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: plan.approval_digest().unwrap(),
        document_id: plan.documents[0].id,
        issue_id: issue.id.clone(),
        input_digest: plan.documents[0].semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::ContentVerified {
            evidence_ids: ids[..1].to_vec(),
        },
    };
    assert!(resolve(&plan, &partial, "unix-seconds:1".into()).is_err());
    let resolved = decide(
        &plan,
        &issue.id,
        ReviewChoice::ContentVerified { evidence_ids: ids },
    );
    assert!(
        validation::ready(&resolved.documents[0]),
        "{:?}",
        validation::validate(&resolved.documents[0])
    );
}

#[test]
fn grammar_add_works_with_default_dictionary_routing_but_rejects_explicit_lookup() {
    let f = AuthoredFixture::new();
    let mut settings = f.settings.clone();
    settings
        .values
        .insert("dictionary.provider".into(), json!("auto"));
    let bytes = serde_json::to_vec(&json!({"schema_version":2,"kind":"grammar","target_language":"en","explanation_language":"en",
        "body":{"pattern":"be used to","use_key":"familiarity","meaning":"be accustomed to","formation":"be used to + noun/V-ing","recognition_prompt":"What does this pattern express?","usage":"","exercise_prompt":"","exercise_answer":"",
            "examples":[{"sentence":"I am used to noise.","translation":"","provenance":"user"}]}}))
    .unwrap();
    let result = linguist_application::prepare_authored(
        &bytes,
        linguist_application::Kind::Grammar,
        &settings,
        &f.environment,
    )
    .unwrap();
    assert!(result.ready, "{:?}", result.issues);
    settings
        .values
        .insert("dictionary.provider".into(), json!("wiktionary"));
    assert!(
        linguist_application::prepare_authored(
            &bytes,
            linguist_application::Kind::Grammar,
            &settings,
            &f.environment
        )
        .unwrap_err()
        .contains("CAPABILITY_UNAVAILABLE")
    );
}

struct AuthoredFixture {
    root: PathBuf,
    environment: BTreeMap<String, String>,
    settings: Effective,
}
impl AuthoredFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-grammar-add-{}", uuid::Uuid::new_v4()));
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
            ("kanji.enabled", json!(false)),
        ] {
            settings.values.insert(key.into(), value);
        }
        Self {
            root,
            environment,
            settings,
        }
    }
    fn state(&self) -> PathBuf {
        linguist_config::expand_path(
            self.settings.values["storage.state_dir"].as_str().unwrap(),
            &self.environment,
        )
        .unwrap()
    }
}
impl Drop for AuthoredFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
