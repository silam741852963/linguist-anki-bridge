//! EV-11 deterministic semantic corpus: 120 annotated fixtures, 30 per
//! workflow, Japanese/English balanced. Each fixture runs through the same
//! library path as the CLI (authored add, or read capture -> field mapping ->
//! staged revamp draft) with no network, LLM or OCR. Dictionary fixtures use
//! canned provider bytes that the real parsers re-read.
//!
//! Every fixture must meet its own annotation (readiness, required and absent
//! issue codes, rendered and staged values) and the corpus-wide policy:
//! zero source loss, recoverable provenance for every staged value and
//! example, no unsupported generated claim in ready content, no task-answer
//! leakage in ready content and no live markup in rendered fields.
//! The corpus is written by `scripts/generate-semantic-corpus.py`.
use linguist_application::{
    AddInput, DictionaryPort, Kind, mapping::map_purpose_fields, prepare_with_providers,
    revamp::stage_document, source_archive::RevampCapture, source_archive::archive_read_capture,
    source_archive::media::attach_original_media, vocab,
};
use linguist_config::{ConfigFile, Registry, ResolveOptions, resolve};
use linguist_core::{
    LearningContent, LearningDocument, Provenance, canonical,
    validation::{Issue, Severity},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Corpus {
    schema_version: u16,
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    id: String,
    workflow: String,
    purpose: String,
    language: String,
    explanation_language: String,
    categories: Vec<String>,
    rationale: String,
    input: Value,
    dictionary: Option<Value>,
    settings: BTreeMap<String, Value>,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    ready: bool,
    issues: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
    #[serde(default)]
    rendered_contains: BTreeMap<String, String>,
    #[serde(default)]
    rendered_excludes: BTreeMap<String, String>,
    #[serde(default)]
    staged: BTreeMap<String, String>,
    #[serde(default)]
    staged_contains: BTreeMap<String, String>,
    #[serde(default)]
    staged_excludes: BTreeMap<String, String>,
    #[serde(default)]
    media_archived: Vec<String>,
    #[serde(default)]
    documents: Option<usize>,
}

fn corpus() -> Corpus {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/semantic/corpus-v1.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// Canned provider response re-parsed by the real Jisho/Wiktionary parsers.
struct Canned {
    provider: String,
    body: Vec<u8>,
}
impl DictionaryPort for Canned {
    fn lookup(
        &self,
        query: &str,
        target: &linguist_core::Language,
    ) -> Result<linguist_dictionary::JishoPage, String> {
        match self.provider.as_str() {
            "jisho" => linguist_dictionary::parse_jisho(query, target, &self.body, 1 << 20, 100),
            _ => linguist_dictionary::wiktionary::parse_definition(
                query,
                target,
                &self.body,
                1 << 20,
                100,
            ),
        }
        .map_err(|error| error.to_string())
    }
}

struct Run {
    root: PathBuf,
    environment: BTreeMap<String, String>,
}
impl Run {
    fn new(id: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lab-semantic-{id}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let environment = BTreeMap::from([("HOME".into(), root.to_str().unwrap().to_owned())]);
        Self { root, environment }
    }
    fn settings(
        &self,
        fixture: &Fixture,
        flags: Vec<(String, Value)>,
    ) -> linguist_config::Effective {
        let mut options = ResolveOptions {
            purpose: Some(fixture.purpose.clone()),
            environment: self.environment.clone(),
            ..Default::default()
        };
        let provider = fixture
            .dictionary
            .as_ref()
            .map(|d| d["provider"].clone())
            .unwrap_or(json!("authored"));
        for (key, value) in [
            ("llm.enabled", json!(false)),
            ("dictionary.provider", provider),
            ("images.search_when_missing", json!(false)),
            ("kanji.enabled", json!(false)),
            (
                "storage.state_dir",
                json!(self.root.join("state").to_str().unwrap()),
            ),
        ] {
            options.flags.insert(key.into(), value);
        }
        for (key, value) in flags.into_iter().chain(fixture.settings.clone()) {
            options.flags.insert(key, value);
        }
        resolve(&Registry::builtin(), &ConfigFile::default(), &options)
            .unwrap_or_else(|e| panic!("{}: settings: {e}", fixture.id))
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Outcome {
    document: LearningDocument,
    rendered: Option<BTreeMap<String, String>>,
    issues: Vec<Issue>,
}

fn ready(issues: &[Issue]) -> bool {
    issues
        .iter()
        .all(|issue| issue.severity == Severity::Warning)
}

fn run_add(fixture: &Fixture, run: &Run) -> Outcome {
    let settings = run.settings(fixture, vec![]);
    let bytes = serde_json::to_vec_pretty(&fixture.input).unwrap();
    // The fixture must be a valid public AddInput.
    let _: AddInput = serde_json::from_slice(&bytes).unwrap();
    let canned = fixture.dictionary.as_ref().map(|d| Canned {
        provider: d["provider"].as_str().unwrap().to_owned(),
        body: serde_json::to_vec(&d["body"]).unwrap(),
    });
    let kind = if fixture.workflow == "vocab_add" {
        Kind::Vocabulary
    } else {
        Kind::Grammar
    };
    let prepared = prepare_with_providers(
        &bytes,
        kind,
        &settings,
        &run.environment,
        vocab::Providers {
            dictionary: canned.as_ref().map(|c| c as &dyn DictionaryPort),
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{}: prepare: {e}", fixture.id));
    let store = linguist_store::Store::read_only(&run.root.join("state")).unwrap();
    // Source preservation: the exact input bytes are the archived original.
    assert_eq!(
        store
            .asset(&prepared.original_input_digest, 1 << 20)
            .unwrap(),
        bytes,
        "{}: original input bytes",
        fixture.id
    );
    let plan = store.revision(prepared.plan_id, 1).unwrap();
    assert_eq!(plan.documents.len(), 1, "{}", fixture.id);
    let document = plan.documents[0].clone();
    for archive in &document.archives {
        for digest in &archive.asset_digests {
            assert!(
                store.asset(digest, 1 << 22).is_ok(),
                "{}: archived asset {digest} is stored",
                fixture.id
            );
        }
    }
    assert_eq!(prepared.ready, ready(&document.issues), "{}", fixture.id);
    let rendered = plan
        .rendered
        .first()
        .map(|note| note.fields.clone())
        .or_else(|| {
            linguist_core::render::render(&document, &BTreeMap::new())
                .ok()
                .map(|note| note.fields)
        });
    Outcome {
        issues: document.issues.clone(),
        document,
        rendered,
    }
}

fn media_bytes(spec: &Value) -> Option<Vec<u8>> {
    if spec.is_null() {
        return None;
    }
    if let Some([width, height]) = spec["png"]
        .as_array()
        .map(|a| [a[0].as_u64().unwrap(), a[1].as_u64().unwrap()])
    {
        let image =
            image::RgbImage::from_pixel(width as u32, height as u32, image::Rgb([200, 30, 30]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        return Some(bytes.into_inner());
    }
    if let Some(name) = spec["fixture"].as_str() {
        return Some(
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(name),
            )
            .unwrap(),
        );
    }
    Some(spec["text"].as_str().unwrap().as_bytes().to_vec())
}

fn run_revamp(fixture: &Fixture, run: &Run) -> Outcome {
    let input = &fixture.input;
    let model = input["model"].as_str().unwrap();
    let pairs: Vec<(String, String)> = input["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let fields: BTreeMap<String, Value> = pairs
        .iter()
        .enumerate()
        .map(|(order, (name, value))| (name.clone(), json!({"value": value, "order": order})))
        .collect();
    let note = json!({"noteId": "1700000000001", "modelName": model, "fields": fields,
        "cards": ["1700000000101"], "tags": input["tags"]});
    let model_json = json!({"model": {"name": model, "id": "1600000000001"},
        "fields": pairs.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "templates": {"Card 1": {"Front": "{{Front}}", "Back": "{{Back}}"}}, "css": ".card {}"});
    let cards =
        json!([{"cardId": "1700000000101", "note": "1700000000001", "reps": input["reps"]}]);
    let mut captured = archive_read_capture(
        &serde_json::to_vec(&note).unwrap(),
        &serde_json::to_vec(&model_json).unwrap(),
        &serde_json::to_vec(&cards).unwrap(),
        1 << 24,
    )
    .unwrap_or_else(|e| panic!("{}: capture: {e}", fixture.id));
    let media: BTreeMap<String, Option<Vec<u8>>> = input["media"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, spec)| (name.clone(), media_bytes(spec)))
        .collect();
    if !captured.source.media_refs.is_empty() || !media.is_empty() {
        attach_original_media(&mut captured, media.clone(), 1 << 24, 1 << 25)
            .unwrap_or_else(|e| panic!("{}: media: {e}", fixture.id));
    }
    let mapping_flags = vec![
        (
            format!("purposes.{}.fields", fixture.purpose),
            input["mapping"].clone(),
        ),
        (
            format!("purposes.{}.source_model", fixture.purpose),
            json!(model),
        ),
    ];
    let settings = run.settings(fixture, mapping_flags);
    let mapping = map_purpose_fields(&settings, &fixture.purpose, model, &captured.source.fields)
        .unwrap_or_else(|e| panic!("{}: mapping: {e}", fixture.id));
    let capture = RevampCapture { captured, mapping };
    let document = stage_document(&capture, &settings, &fixture.purpose)
        .unwrap_or_else(|e| panic!("{}: stage: {e}", fixture.id));
    // Source preservation: every original field is archived byte-exactly, and
    // every readable media file keeps its exact bytes.
    let original: BTreeMap<String, String> = pairs.iter().cloned().collect();
    assert_eq!(document.archives.len(), 1, "{}", fixture.id);
    assert_eq!(
        document.archives[0].original_fields, original,
        "{}",
        fixture.id
    );
    assert_eq!(document.sources[0].fields, original, "{}", fixture.id);
    for (name, bytes) in &media {
        if let Some(bytes) = bytes {
            let digest = canonical::asset_digest(bytes);
            assert_eq!(
                capture.captured.assets.get(&digest),
                Some(bytes),
                "{}: {name}",
                fixture.id
            );
            assert!(
                document.archives[0].asset_digests.contains(&digest),
                "{}: {name} archived",
                fixture.id
            );
        }
    }
    let staged = serde_json::to_value(&document.content).unwrap()["body"].clone();
    for (role, value) in staged.as_object().unwrap() {
        let Some(text) = value.as_str().filter(|text| !text.is_empty()) else {
            continue;
        };
        // Recoverable provenance: each staged scalar is a source claim.
        assert!(
            document
                .evidence
                .iter()
                .any(|evidence| evidence.field == *role
                    && evidence.provenance == Provenance::Source
                    && evidence.claim == text
                    && evidence.source_id == Some(document.sources[0].id)),
            "{}: staged {role} lacks source evidence",
            fixture.id
        );
    }
    Outcome {
        issues: document.issues.clone(),
        rendered: linguist_core::render::render(&document, &original)
            .ok()
            .map(|note| note.fields),
        document,
    }
}

fn body(document: &LearningDocument) -> Value {
    serde_json::to_value(&document.content).unwrap()["body"].clone()
}

/// Corpus-wide policy checks that hold for every fixture.
fn policy(fixture: &Fixture, outcome: &Outcome) {
    let id = &fixture.id;
    let document = &outcome.document;
    // Provenance: every source has its archive, evidence points at a source,
    // and every example's evidence exists.
    for source in &document.sources {
        assert!(
            document
                .archives
                .iter()
                .any(|archive| archive.source_id == source.id && archive.digest == source.digest),
            "{id}: source {} has no matching archive",
            source.id
        );
    }
    let evidence: BTreeSet<_> = document.evidence.iter().map(|e| e.id).collect();
    for item in &document.evidence {
        if let Some(source) = item.source_id {
            assert!(
                document.sources.iter().any(|s| s.id == source),
                "{id}: evidence source missing"
            );
        }
    }
    let examples = match &document.content {
        LearningContent::Vocabulary(v) => &v.examples,
        LearningContent::Grammar(g) => &g.examples,
    };
    for example in examples {
        assert!(
            example.evidence_ids.iter().all(|e| evidence.contains(e)),
            "{id}: example evidence missing"
        );
    }
    let is_ready = ready(&outcome.issues);
    if is_ready {
        // No unsupported generated claim in ready content.
        assert!(
            examples
                .iter()
                .all(|e| e.provenance != Provenance::Generated || !e.evidence_ids.is_empty()),
            "{id}: ready content has an unsupported generated example"
        );
        // Independent task-leakage check on ready content.
        let normalize = |text: &str| text.to_lowercase().split_whitespace().collect::<String>();
        match &document.content {
            LearningContent::Vocabulary(v) => {
                let answer = normalize(&v.expression);
                for prompt in [&v.production_prompt, &v.spelling_prompt] {
                    assert!(
                        answer.is_empty() || !normalize(prompt).contains(&answer),
                        "{id}: ready cue leaks the answer"
                    );
                }
            }
            LearningContent::Grammar(g) => {
                let answer = normalize(&g.exercise_answer);
                assert!(
                    answer.is_empty() || !normalize(&g.exercise_prompt).contains(&answer),
                    "{id}: ready exercise leaks the answer"
                );
            }
        }
        let rendered = outcome.rendered.as_ref().expect("ready content renders");
        for (field, value) in rendered {
            let lower = value.to_ascii_lowercase();
            for tag in [
                "<script",
                "<iframe",
                "<img src=x",
                "<a href",
                "<span onclick",
            ] {
                assert!(!lower.contains(tag), "{id}: {field} contains live {tag}");
            }
        }
    } else {
        assert!(
            outcome.rendered.is_none(),
            "{id}: content that is not ready must not render"
        );
    }
}

fn check(fixture: &Fixture, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    let expect = &fixture.expect;
    let codes: BTreeSet<&str> = outcome.issues.iter().map(|i| i.code.as_str()).collect();
    let is_ready = ready(&outcome.issues);
    if is_ready != expect.ready {
        failures.push(format!(
            "ready={is_ready}, expected {}; issues {codes:?}",
            expect.ready
        ));
    }
    for code in &expect.issues {
        if !codes.contains(code.as_str()) {
            failures.push(format!("missing issue {code}; got {codes:?}"));
        }
    }
    for code in &expect.absent {
        if codes.contains(code.as_str()) {
            failures.push(format!("unexpected issue {code}"));
        }
    }
    for (field, text) in &expect.rendered_contains {
        match outcome.rendered.as_ref().and_then(|r| r.get(field)) {
            Some(value) if value.contains(text) => {}
            other => failures.push(format!("rendered {field} lacks {text:?}: {other:?}")),
        }
    }
    for (field, text) in &expect.rendered_excludes {
        if let Some(value) = outcome.rendered.as_ref().and_then(|r| r.get(field))
            && value.contains(text)
        {
            failures.push(format!("rendered {field} contains {text:?}: {value:?}"));
        }
    }
    let staged = body(&outcome.document);
    let staged_text = |role: &str| staged[role].as_str().unwrap_or_default().to_owned();
    for (role, text) in &expect.staged {
        if staged_text(role) != *text {
            failures.push(format!(
                "staged {role} = {:?}, expected {text:?}",
                staged_text(role)
            ));
        }
    }
    for (role, text) in &expect.staged_contains {
        if !staged_text(role).contains(text) {
            failures.push(format!("staged {role} lacks {text:?}"));
        }
    }
    for (role, text) in &expect.staged_excludes {
        if staged_text(role).contains(text) {
            failures.push(format!("staged {role} contains {text:?}"));
        }
    }
    for name in &expect.media_archived {
        if !outcome.document.media.iter().any(|m| m.filename == *name) {
            failures.push(format!("media {name} not archived"));
        }
    }
    if let Some(count) = expect.documents
        && count != 1
    {
        failures.push("revamp stages exactly one document per source note".into());
    }
    failures
}

#[test]
fn corpus_shape_is_balanced_and_covers_every_required_category() {
    let corpus = corpus();
    assert_eq!(corpus.schema_version, 1);
    assert_eq!(corpus.fixtures.len(), 120);
    let ids: BTreeSet<_> = corpus.fixtures.iter().map(|f| &f.id).collect();
    assert_eq!(ids.len(), 120);
    for workflow in ["vocab_add", "vocab_revamp", "grammar_add", "grammar_revamp"] {
        for language in ["ja", "en"] {
            let count = corpus
                .fixtures
                .iter()
                .filter(|f| f.workflow == workflow && f.language == language)
                .count();
            assert_eq!(count, 15, "{workflow}/{language}");
        }
    }
    for category in [
        "vietnamese_explanation",
        "kana",
        "kanji",
        "homograph",
        "mixed_model",
        "mixed_image",
        "multi_pattern",
        "no_dictionary_match",
        "shared_media",
        "task_leakage",
        "adversarial",
    ] {
        let count = corpus
            .fixtures
            .iter()
            .filter(|f| f.categories.iter().any(|c| c == category))
            .count();
        assert!(count >= 2, "{category}: {count}");
    }
    for fixture in &corpus.fixtures {
        assert!(!fixture.rationale.trim().is_empty(), "{}", fixture.id);
        assert!(["en", "vi"].contains(&fixture.explanation_language.as_str()));
    }
}

#[test]
fn every_fixture_meets_its_annotation_and_the_corpus_policy() {
    let corpus = corpus();
    let mut failures = Vec::new();
    let mut summary: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for fixture in &corpus.fixtures {
        let run = Run::new(&fixture.id);
        let outcome = if fixture.workflow.ends_with("_add") {
            run_add(fixture, &run)
        } else {
            run_revamp(fixture, &run)
        };
        policy(fixture, &outcome);
        let problems = check(fixture, &outcome);
        let entry = summary.entry(fixture.workflow.clone()).or_default();
        entry.0 += 1;
        if problems.is_empty() {
            entry.1 += 1;
        } else {
            failures.push(format!("{}: {}", fixture.id, problems.join("; ")));
        }
    }
    println!("semantic corpus: {summary:?}");
    assert!(failures.is_empty(), "{failures:#?}");
}
