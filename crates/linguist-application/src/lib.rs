//! CLI use cases. Preparation stages local evidence and never writes to Anki.
use linguist_core::{records::*, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VocabularyInput {
    pub expression: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub sense_key: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub pronunciation: String,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub examples: Vec<Example>,
    #[serde(default)]
    pub dictionary: Vec<DictionaryEntry>,
    #[serde(default)]
    pub kanji: String,
    #[serde(default)]
    pub production_prompt: String,
    #[serde(default)]
    pub spelling_prompt: String,
}
impl From<VocabularyInput> for Vocabulary {
    fn from(input: VocabularyInput) -> Self {
        Self {
            expression: input.expression,
            meaning: input.meaning,
            sense_key: input.sense_key,
            reading: input.reading,
            pronunciation: input.pronunciation,
            usage: input.usage,
            examples: input.examples,
            dictionary: input.dictionary,
            kanji: input.kanji,
            production_prompt: input.production_prompt,
            spelling_prompt: input.spelling_prompt,
            nuance: vec![],
            collocations: vec![],
            kanji_details: vec![],
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AddInput {
    Vocabulary {
        schema_version: u16,
        target_language: Language,
        #[serde(default)]
        explanation_language: Option<Language>,
        body: VocabularyInput,
        #[serde(default)]
        requested_tasks: Option<Vec<Task>>,
        #[serde(default)]
        context: String,
        #[serde(default)]
        personal_notes: String,
        #[serde(default)]
        source_summary: String,
        #[serde(default)]
        tags: Vec<String>,
    },
    Grammar {
        schema_version: u16,
        target_language: Language,
        #[serde(default)]
        explanation_language: Option<Language>,
        body: Grammar,
        #[serde(default)]
        requested_tasks: Option<Vec<Task>>,
        #[serde(default)]
        context: String,
        #[serde(default)]
        personal_notes: String,
        #[serde(default)]
        source_summary: String,
        #[serde(default)]
        tags: Vec<String>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Vocabulary,
    Grammar,
}
#[derive(Debug, Serialize)]
pub struct Prepared {
    pub document_id: uuid::Uuid,
    pub input_digest: String,
    pub schema_version: u16,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub digest: String,
    pub ready: bool,
    pub issues: Vec<Issue>,
    pub original_input_digest: String,
    pub apply_eligible: bool,
    pub duplicate_check_performed: bool,
    /// Exact follow-up commands for unresolved issues; preparation never applies.
    pub next_commands: Vec<String>,
}
impl Prepared {
    /// Recompute follow-up commands from the item's current issues.
    pub fn refresh_next_commands(&mut self, llm_enabled: bool) {
        let plan = self.plan_id;
        let revision = self.revision;
        let digest = &self.digest;
        let mut commands = Vec::new();
        for issue in &self.issues {
            match issue.severity {
                Severity::Review => commands.push(format!(
                    "linguist-anki-bridge plans resolve {plan} {} --decision FILE",
                    issue.id
                )),
                Severity::Error if issue.code == "MISSING_CUE" && llm_enabled => commands.push(format!(
                    "linguist-anki-bridge plans generate {plan} --item-id {} --base-revision {revision} --digest {digest} --use-current-settings",
                    self.document_id
                )),
                Severity::Error => commands.push(format!(
                    "linguist-anki-bridge plans edit {plan} --base-revision {revision} --patch FILE"
                )),
                Severity::Warning => {}
            }
        }
        commands.dedup();
        commands.push(format!(
            "linguist-anki-bridge plans show {plan} --revision {revision}"
        ));
        if self.ready {
            commands.push(format!(
                "linguist-anki-bridge plans approve {plan} --revision {revision} --digest {digest} --actor NAME"
            ));
        }
        self.next_commands = commands;
    }
}
#[derive(Debug, Serialize)]
pub struct PreparedBatch {
    pub schema_version: u16,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub digest: String,
    pub ready: bool,
    pub items: Vec<Prepared>,
    pub apply_eligible: bool,
    pub duplicate_check_performed: bool,
}
struct PreparedRecord<'a> {
    structured: Cow<'a, [u8]>,
    archive: &'a [u8],
    source_kind: &'static str,
    location: String,
    model_manifest: &'static str,
    fields: Option<BTreeMap<String, String>>,
}
impl<'a> PreparedRecord<'a> {
    fn json(bytes: &'a [u8]) -> Self {
        Self {
            structured: Cow::Borrowed(bytes),
            archive: bytes,
            source_kind: "authored_json_v2",
            location: "local_input".into(),
            model_manifest: "authored-input-v2",
            fields: None,
        }
    }
}
/// Requested adapters must never be silently skipped.
pub(crate) fn authored_capabilities(settings: &linguist_config::Effective) -> Result<(), String> {
    // Selected adapters without an implementation fail before any read or state.
    vocab::preflight(settings)
}
/// Freeze filesystem values at preparation time. Credentials stay environment references.
pub fn freeze_settings(
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<ResolvedSettings, String> {
    let registry = linguist_config::Registry::builtin();
    let mut values = settings.values.clone();
    let mut secret_refs = BTreeMap::new();
    for (key, value) in &mut values {
        let entry = registry.lookup(key)?;
        if entry
            .constraints
            .get("format")
            .and_then(serde_json::Value::as_str)
            == Some("path")
            && let Some(text) = value.as_str()
        {
            let path = linguist_config::expand_path(text, environment)?;
            if !path.is_absolute() {
                return Err(format!("FROZEN_PATH_MUST_BE_ABSOLUTE:{key}"));
            }
            *value = serde_json::json!(path.to_str().ok_or("FROZEN_PATH_ENCODING")?);
        }
        if key.ends_with(".api_key_env")
            && let Some(name) = value.as_str()
        {
            secret_refs.insert(key.clone(), name.into());
        }
    }
    let mut resource_hashes = BTreeMap::new();
    if values["llm.enabled"] == true {
        for (key, reference, prompt) in [
            (
                "llm.prompts.vocabulary",
                "builtin:vocabulary-v3",
                generation::VOCABULARY_PROMPT_V3,
            ),
            (
                "llm.prompts.grammar",
                "builtin:grammar-v3",
                generation::GRAMMAR_PROMPT_V3,
            ),
        ] {
            if values[key] == reference {
                resource_hashes
                    .insert(reference.into(), canonical::asset_digest(prompt.as_bytes()));
            }
        }
    }
    let fingerprint = canonical::digest("resolved-settings", &values).map_err(|e| e.to_string())?;
    let (semantic_fingerprint, execution_fingerprint) =
        linguist_config::setting_fingerprints(&values)?;
    Ok(ResolvedSettings {
        version: 2,
        values,
        provenance: settings.provenance.clone(),
        resource_hashes,
        secret_refs,
        fingerprint,
        semantic_fingerprint,
        execution_fingerprint,
    })
}
pub fn prepare_authored(
    bytes: &[u8],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Prepared, String> {
    prepare_with_dictionary(bytes, expected_kind, settings, environment, None)
}
/// Stage one explicitly authored CLI record. The canonical normalized values
/// are the recoverable source asset; shell quoting/argv spelling is not claimed.
pub fn prepare_authored_inline(
    input: AddInput,
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Prepared, String> {
    let bytes = canonical::bytes(&input).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| "INPUT_ENCODING")?;
    let record = PreparedRecord {
        structured: Cow::Borrowed(&bytes),
        archive: &bytes,
        source_kind: "authored_inline_v1",
        location: "cli:inline".into(),
        model_manifest: "authored-inline-v1",
        fields: Some(BTreeMap::from([("inline_input".into(), text)])),
    };
    let mut batch = publish_authored_records(
        &[record],
        expected_kind,
        settings,
        environment,
        vocab::Providers::default(),
        false,
    )?;
    Ok(batch.items.remove(0))
}
pub trait DictionaryPort {
    fn lookup(
        &self,
        query: &str,
        target: &Language,
    ) -> Result<linguist_dictionary::JishoPage, String>;
}
pub fn prepare_with_dictionary(
    bytes: &[u8],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    dictionary: Option<&dyn DictionaryPort>,
) -> Result<Prepared, String> {
    prepare_with_providers(
        bytes,
        expected_kind,
        settings,
        environment,
        vocab::Providers {
            dictionary,
            ..Default::default()
        },
    )
}
/// Authored JSON with injected provider ports (tests and alternative adapters).
pub fn prepare_with_providers(
    bytes: &[u8],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    providers: vocab::Providers<'_>,
) -> Result<Prepared, String> {
    let mut batch = publish_authored_records(
        &[PreparedRecord::json(bytes)],
        expected_kind,
        settings,
        environment,
        providers,
        false,
    )?;
    Ok(batch.items.remove(0))
}

/// JSONL is opt-in. Each physical line is one complete v2 record; whitespace-only
/// lines fail with their one-based position instead of silently shifting items.
pub fn prepare_authored_jsonl(
    bytes: &[u8],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<PreparedBatch, String> {
    let max_bytes = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    if bytes.len() as u64 > max_bytes {
        return Err("INPUT_TOO_LARGE".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "INPUT_ENCODING")?;
    if text.is_empty() {
        return Err("INPUT_EMPTY_BATCH".into());
    }
    let records: Vec<&[u8]> = text.split_inclusive('\n').map(str::as_bytes).collect();
    if records.len() as u64 > settings.values["selection.max_notes"].as_u64().unwrap() {
        return Err("INPUT_BATCH_TOO_LARGE: exceeds selection.max_notes".into());
    }
    for (index, record) in records.iter().enumerate() {
        if record.iter().all(u8::is_ascii_whitespace) {
            return Err(format!("INPUT_EMPTY_RECORD: line {}", index + 1));
        }
    }
    let records: Vec<_> = records.into_iter().map(PreparedRecord::json).collect();
    publish_authored_records(
        &records,
        expected_kind,
        settings,
        environment,
        vocab::Providers::default(),
        true,
    )
}

/// Explicit simple authored-card CSV; the whole original file is retained as
/// shared source evidence, while each record keeps decoded column evidence.
pub fn prepare_authored_csv(
    bytes: &[u8],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<PreparedBatch, String> {
    if bytes.len() as u64 > settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024 {
        return Err("INPUT_TOO_LARGE".into());
    }
    std::str::from_utf8(bytes).map_err(|_| "INPUT_ENCODING")?;
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .from_reader(content);
    let headers = reader
        .headers()
        .map_err(|_| "INPUT_CSV_HEADER_INVALID")?
        .clone();
    let required: &[&str] = match expected_kind {
        Kind::Vocabulary => &["expression", "meaning", "target_language", "sense_key"],
        Kind::Grammar => &[
            "pattern",
            "meaning",
            "formation",
            "use_key",
            "target_language",
        ],
    };
    let optional: &[&str] = match expected_kind {
        Kind::Vocabulary => &[
            "reading",
            "pronunciation",
            "usage",
            "context",
            "personal_notes",
            "source_summary",
            "explanation_language",
            "production_prompt",
            "spelling_prompt",
            "example_sentence",
            "example_translation",
        ],
        Kind::Grammar => &[
            "recognition_prompt",
            "usage",
            "exercise_prompt",
            "exercise_answer",
            "context",
            "personal_notes",
            "source_summary",
            "explanation_language",
            "example_sentence",
            "example_translation",
        ],
    };
    let names: std::collections::BTreeSet<_> = headers.iter().collect();
    if headers.is_empty()
        || names.len() != headers.len()
        || required.iter().any(|name| !names.contains(name))
        || names
            .iter()
            .any(|name| !required.contains(name) && !optional.contains(name))
        || names.contains("example_sentence") != names.contains("example_translation")
    {
        return Err("INPUT_CSV_HEADER_INVALID: require unique known columns for the selected card kind and paired example columns".into());
    }
    let header_json =
        serde_json::to_string(&headers.iter().collect::<Vec<_>>()).map_err(|e| e.to_string())?;
    let mut records = Vec::new();
    let max_notes = settings.values["selection.max_notes"].as_u64().unwrap() as usize;
    for (index, row) in reader.records().enumerate() {
        let row = row.map_err(|_| format!("INPUT_CSV_RECORD_INVALID: record {}", index + 1))?;
        if records.len() >= max_notes {
            return Err("INPUT_BATCH_TOO_LARGE: exceeds selection.max_notes".into());
        }
        let cells: BTreeMap<_, _> = headers.iter().zip(row.iter()).collect();
        let value = |name: &str| cells.get(name).copied().unwrap_or_default();
        let mut body = match expected_kind {
            Kind::Vocabulary => {
                serde_json::json!({"expression":value("expression"),"meaning":value("meaning"),"sense_key":value("sense_key")})
            }
            Kind::Grammar => {
                serde_json::json!({"pattern":value("pattern"),"meaning":value("meaning"),"formation":value("formation"),"use_key":value("use_key"),"examples":[]})
            }
        };
        let fields: &[&str] = match expected_kind {
            Kind::Vocabulary => &[
                "reading",
                "pronunciation",
                "usage",
                "production_prompt",
                "spelling_prompt",
            ],
            Kind::Grammar => &[
                "recognition_prompt",
                "usage",
                "exercise_prompt",
                "exercise_answer",
            ],
        };
        for &field in fields {
            if names.contains(field) {
                body[field] = serde_json::json!(value(field));
            }
        }
        if names.contains("example_sentence") {
            let sentence = value("example_sentence");
            let translation = value("example_translation");
            if sentence.is_empty() != translation.is_empty() {
                return Err(format!(
                    "INPUT_CSV_EXAMPLE_PAIR_INVALID: record {}",
                    index + 1
                ));
            }
            if !sentence.is_empty() {
                body["examples"] = serde_json::json!([{"sentence":sentence,"translation":translation,"provenance":"user"}]);
            }
        }
        let kind = match expected_kind {
            Kind::Vocabulary => "vocabulary",
            Kind::Grammar => "grammar",
        };
        let mut input = serde_json::json!({"schema_version":2,"kind":kind,"target_language":value("target_language"),"body":body});
        for field in [
            "context",
            "personal_notes",
            "source_summary",
            "explanation_language",
        ] {
            if names.contains(field) && !value(field).is_empty() {
                input[field] = serde_json::json!(value(field));
            }
        }
        let structured = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
        let fields = BTreeMap::from([
            ("csv_header".into(), header_json.clone()),
            (
                "csv_record".into(),
                serde_json::to_string(&row.iter().collect::<Vec<_>>())
                    .map_err(|e| e.to_string())?,
            ),
            ("csv_record_index".into(), (index + 1).to_string()),
        ]);
        records.push(PreparedRecord {
            structured: Cow::Owned(structured),
            archive: bytes,
            source_kind: "authored_csv_v1",
            location: format!("local_input:csv-record:{}", index + 1),
            model_manifest: "authored-csv-v1",
            fields: Some(fields),
        });
    }
    publish_authored_records(
        &records,
        expected_kind,
        settings,
        environment,
        vocab::Providers::default(),
        true,
    )
}

fn build_authored_document(
    record: &PreparedRecord<'_>,
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    providers: vocab::Providers<'_>,
) -> Result<(LearningDocument, Vec<Vec<u8>>), String> {
    let bytes = record.structured.as_ref();
    if bytes.len() as u64 > settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024 {
        return Err("INPUT_TOO_LARGE".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "INPUT_ENCODING")?;
    if text.chars().count() as u64 > settings.values["input.max_record_chars"].as_u64().unwrap() {
        return Err("INPUT_RECORD_TOO_LARGE".into());
    }
    let input: AddInput = canonical::parse(bytes).map_err(|e| e.to_string())?;
    authored_capabilities(settings)?;
    let (
        version,
        target_language,
        explanation_language,
        content,
        tasks,
        context,
        personal_notes,
        source_summary,
        tags,
    ) = match input {
        AddInput::Vocabulary {
            schema_version,
            target_language,
            explanation_language,
            body,
            requested_tasks,
            context,
            personal_notes,
            source_summary,
            tags,
        } => {
            if expected_kind != Kind::Vocabulary {
                return Err("INPUT_KIND_CONFLICT".into());
            }
            let tasks = requested_tasks.unwrap_or_else(|| {
                let mut tasks = vec![Task::Comprehension];
                if settings.values["learning.vocabulary.production"] == true {
                    tasks.push(Task::Production);
                }
                if settings.values["learning.vocabulary.spelling"] == true {
                    tasks.push(Task::Spelling);
                }
                tasks
            });
            (
                schema_version,
                target_language,
                explanation_language,
                LearningContent::Vocabulary(body.into()),
                tasks,
                context,
                personal_notes,
                source_summary,
                tags,
            )
        }
        AddInput::Grammar {
            schema_version,
            target_language,
            explanation_language,
            body,
            requested_tasks,
            context,
            personal_notes,
            source_summary,
            tags,
        } => {
            if expected_kind != Kind::Grammar {
                return Err("INPUT_KIND_CONFLICT".into());
            }
            let tasks = requested_tasks.unwrap_or_else(|| {
                let mut tasks = vec![Task::Recognition];
                if settings.values["learning.grammar.application"] == true {
                    tasks.push(Task::Application);
                }
                tasks
            });
            (
                schema_version,
                target_language,
                explanation_language,
                LearningContent::Grammar(body),
                tasks,
                context,
                personal_notes,
                source_summary,
                tags,
            )
        }
    };
    if version != 2 {
        return Err("UNSUPPORTED_ADD_INPUT_VERSION".into());
    }
    let explanation_language = explanation_language.unwrap_or(Language::try_from(
        settings.values["learning.explanation_language"]
            .as_str()
            .unwrap()
            .to_owned(),
    )?);
    let original_input_digest = canonical::asset_digest(record.archive);
    let original_text = std::str::from_utf8(record.archive)
        .map_err(|_| "INPUT_ENCODING")?
        .to_owned();
    let source_id = uuid::Uuid::new_v4();
    let fields = record
        .fields
        .clone()
        .unwrap_or_else(|| BTreeMap::from([("authored_input".into(), text.into())]));
    let mut document = LearningDocument {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        target_language,
        explanation_language,
        content,
        requested_tasks: tasks,
        task_maps: vec![],
        tags,
        context,
        personal_notes,
        source_summary,
        sources: vec![SourceRecord {
            id: source_id,
            kind: record.source_kind.into(),
            location: record.location.clone(),
            digest: original_input_digest.clone(),
            text: Some(original_text.clone()),
            fields: fields.clone(),
            model_manifest: record.model_manifest.into(),
            template_manifest: None,
            captured_at_unix_seconds: None,
            tags: vec![],
            cards: vec![],
            media_refs: vec![],
        }],
        archives: vec![SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id,
            digest: original_input_digest.clone(),
            original_text: Some(original_text),
            original_fields: fields,
            asset_digests: vec![original_input_digest.clone()],
        }],
        regions: vec![],
        evidence: vec![],
        media: vec![],
        edits: BTreeMap::new(),
        reviews: vec![],
        issues: vec![],
    };
    let (enriched, mut provider_assets) =
        dictionary::enrich_document(&document, settings, environment, providers.dictionary)?;
    // Optional enrichment runs after dictionary lookup on the same staged item.
    let (enriched, enrichment_assets) =
        vocab::enrich_document(&enriched, settings, environment, providers)?;
    provider_assets.extend(enrichment_assets);
    document = enriched;
    document.issues = validation::validate(&document);
    Ok((document, provider_assets))
}

fn publish_authored_records(
    records: &[PreparedRecord<'_>],
    expected_kind: Kind,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    providers: vocab::Providers<'_>,
    line_numbered: bool,
) -> Result<PreparedBatch, String> {
    if records.is_empty() {
        return Err("INPUT_EMPTY_BATCH".into());
    }
    let frozen = freeze_settings(settings, environment)?;
    let mut documents = Vec::with_capacity(records.len());
    let mut assets = Vec::new();
    let mut rendered = Vec::new();
    // Complete all parsing, enrichment and validation before opening state.
    for (index, record) in records.iter().enumerate() {
        let (document, provider_assets) =
            build_authored_document(record, expected_kind, settings, environment, providers)
                .map_err(|error| {
                    if line_numbered {
                        format!("INPUT_RECORD_{}: {error}", index + 1)
                    } else {
                        error
                    }
                })?;
        if let Ok(card) = render::render(&document, &document.sources[0].fields) {
            rendered.push(card);
        }
        assets.extend(provider_assets);
        documents.push(document);
    }
    let plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: frozen,
        binding: None,
        source_digest: canonical::digest(
            "source-capture",
            &documents
                .iter()
                .flat_map(|document| document.sources.iter())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?,
        selection: None,
        documents,
        rendered,
        review_decisions: vec![],
    };
    let validation = linguist_core::plan_validation::inspect(&plan).map_err(|e| e.to_string())?;
    let ready = validation.content_ready;
    let root = std::path::Path::new(plan.settings.values["storage.state_dir"].as_str().unwrap());
    let mut store = linguist_store::Store::open(root)?;
    let mut seen = std::collections::BTreeSet::new();
    for record in records {
        if seen.insert(canonical::asset_digest(record.archive)) {
            store.publish_asset(
                record.archive,
                settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024,
            )?;
        }
    }
    let asset_cap = settings.values["network.max_response_mb"]
        .as_u64()
        .unwrap()
        .max(settings.values["media.max_asset_mb"].as_u64().unwrap())
        * 1024
        * 1024;
    for asset in assets {
        store.publish_asset(&asset, asset_cap)?;
    }
    let digest = store.publish_revision(&plan)?;
    let items = plan
        .documents
        .iter()
        .zip(validation.items.iter())
        .map(|(document, checked)| {
            Ok(Prepared {
                document_id: document.id,
                input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
                schema_version: 2,
                plan_id: plan.id,
                revision: 1,
                digest: digest.clone(),
                ready: checked.content_ready,
                issues: checked.issues.clone(),
                original_input_digest: document.sources[0].digest.clone(),
                apply_eligible: false,
                duplicate_check_performed: false,
                next_commands: vec![],
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let llm = settings.values["llm.enabled"] == true;
    let mut items = items;
    for item in &mut items {
        item.refresh_next_commands(llm);
    }
    Ok(PreparedBatch {
        schema_version: 2,
        plan_id: plan.id,
        revision: 1,
        digest,
        ready,
        items,
        apply_eligible: false,
        duplicate_check_performed: false,
    })
}

pub mod agents;
pub mod apply;
pub mod audio;
pub mod backup;
pub mod cache;
pub mod capture;
pub mod checkpoint;
pub mod dictionary;
pub mod dictionary_entry;
pub mod duplicate_candidates;
pub mod editor;
pub mod export;
pub mod generation;
pub mod grammar;
pub mod illustrations;
pub mod images;
pub mod job_executor;
pub mod jobs;
pub mod legacy_jobs;
pub mod live_validation;
pub mod mapping;
pub mod media;
pub mod model_install;
pub mod native_port;
pub mod ocr;
pub mod ocr_inspection;
pub mod ollama;
pub mod plan_diff;
pub mod regenerate;
pub mod resources;
pub mod restore;
pub mod revamp;
pub mod review;
pub mod selector;
pub mod source_archive;
pub mod speech;
pub mod split;
pub mod vocab;
pub mod vocab_split;
pub mod voicevox;
