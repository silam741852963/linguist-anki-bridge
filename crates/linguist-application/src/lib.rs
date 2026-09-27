//! CLI use cases. Preparation stages local evidence and never writes to Anki.
use linguist_core::{records::*, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
}
/// Initial authored path; requested adapters must never be silently skipped.
fn authored_capabilities(settings: &linguist_config::Effective) -> Result<(), String> {
    if settings.values["llm.enabled"] != false {
        return Err("CAPABILITY_UNAVAILABLE: generation is not implemented; authored preparation requires llm.enabled=false".into());
    }
    if settings.values["images.search_when_missing"] == true
        && settings.values["images.provider"] != "disabled"
    {
        return Err("CAPABILITY_UNAVAILABLE: image search is not implemented; disable images.search_when_missing or images.provider".into());
    }
    if !matches!(
        settings.values["audio.provider"].as_str(),
        Some("preserve" | "disabled")
    ) {
        return Err("CAPABILITY_UNAVAILABLE: audio generation is not implemented; choose preserve or disabled".into());
    }
    Ok(())
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
    let fingerprint = canonical::digest("resolved-settings", &values).map_err(|e| e.to_string())?;
    Ok(ResolvedSettings {
        version: 2,
        values,
        provenance: settings.provenance.clone(),
        resource_hashes: BTreeMap::new(),
        secret_refs,
        fingerprint,
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
    if bytes.len() as u64 > settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024 {
        return Err("INPUT_TOO_LARGE".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "INPUT_ENCODING")?;
    if text.chars().count() as u64 > settings.values["input.max_record_chars"].as_u64().unwrap() {
        return Err("INPUT_RECORD_TOO_LARGE".into());
    }
    let input: AddInput = canonical::parse(bytes).map_err(|e| e.to_string())?;
    authored_capabilities(settings)?;
    let frozen = freeze_settings(settings, environment)?;
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
    if let LearningContent::Vocabulary(vocab) = &content
        && target_language.as_str().starts_with("ja")
        && settings.values["kanji.enabled"] == true
        && vocab
            .expression
            .chars()
            .any(|c| matches!(c as u32, 0x3400..=0x9fff | 0x20000..=0x323af))
    {
        return Err("CAPABILITY_UNAVAILABLE: kanji enrichment is not implemented; authored preparation requires kanji.enabled=false for kanji expressions".into());
    }
    if version != 2 {
        return Err("UNSUPPORTED_ADD_INPUT_VERSION".into());
    }
    let explanation_language = explanation_language.unwrap_or(Language::try_from(
        settings.values["learning.explanation_language"]
            .as_str()
            .unwrap()
            .to_owned(),
    )?);
    let original_input_digest = canonical::asset_digest(bytes);
    let source_id = uuid::Uuid::new_v4();
    let fields = BTreeMap::from([("authored_input".into(), text.into())]);
    let mut document = LearningDocument {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        target_language,
        explanation_language,
        content,
        requested_tasks: tasks,
        tags,
        context,
        personal_notes,
        source_summary,
        sources: vec![SourceRecord {
            id: source_id,
            kind: "authored_json_v2".into(),
            location: "local_input".into(),
            digest: original_input_digest.clone(),
            fields: fields.clone(),
            model_manifest: "authored-input-v2".into(),
            tags: vec![],
            cards: vec![],
            media_refs: vec![],
        }],
        archives: vec![SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id,
            digest: original_input_digest.clone(),
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
    let mut provider_assets = Vec::new();
    if settings.values["dictionary.provider"] != "authored" {
        if let LearningContent::Vocabulary(vocab) = &mut document.content {
            let japanese = document.target_language.as_str().split('-').next() == Some("ja");
            let english = document.target_language.as_str().split('-').next() == Some("en");
            if !(japanese
                && matches!(
                    settings.values["dictionary.provider"].as_str(),
                    Some("jisho" | "auto")
                )
                || english
                    && matches!(
                        settings.values["dictionary.provider"].as_str(),
                        Some("wiktionary" | "auto")
                    ))
            {
                return Err(
                    "CAPABILITY_UNAVAILABLE: this dictionary/language adapter is not implemented"
                        .into(),
                );
            }
            if !document.explanation_language.as_str().starts_with("en") {
                return Err(
                    "CAPABILITY_UNAVAILABLE: dictionary definition translation is not implemented"
                        .into(),
                );
            }
            if vocab.expression.trim().is_empty() {
                return Err("VOCAB_EXPRESSION_REQUIRED".into());
            }
            let page = if let Some(provider) = dictionary {
                provider.lookup(&vocab.expression, &document.target_language)?
            } else {
                let client = linguist_dictionary::transport::DictionaryClient::for_target(
                    settings,
                    &document.target_language,
                )
                .map_err(|e| format!("CAPABILITY_UNAVAILABLE: {e}"))?;
                client
                    .lookup(&vocab.expression, &document.target_language, 1000)
                    .map_err(|e| format!("DICTIONARY_PROVIDER_FAILED: {e}"))?
            };
            if page.query != vocab.expression
                || canonical::asset_digest(&page.raw_bytes) != page.raw_digest
            {
                return Err("DICTIONARY_RESPONSE_CONFLICT".into());
            }
            // Reparse port output: callers cannot fabricate rich facts unrelated to saved bytes.
            let maximum =
                settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024;
            let verified = if japanese {
                linguist_dictionary::parse_jisho(
                    &page.query,
                    &document.target_language,
                    &page.raw_bytes,
                    maximum,
                    1000,
                )
            } else {
                linguist_dictionary::wiktionary::parse_definition(
                    &page.query,
                    &document.target_language,
                    &page.raw_bytes,
                    maximum,
                    1000,
                )
            }
            .map_err(|e| format!("DICTIONARY_PROVIDER_FAILED: {e}"))?;
            if verified.entries != page.entries || verified.request_url != page.request_url {
                return Err("DICTIONARY_RESPONSE_CONFLICT".into());
            }
            vocab.dictionary = page.entries;
            let source_id = uuid::Uuid::new_v4();
            let fields = BTreeMap::from([(
                "provider_response".into(),
                String::from_utf8(page.raw_bytes.clone()).map_err(|_| "INPUT_ENCODING")?,
            )]);
            document.sources.push(SourceRecord {
                id: source_id,
                kind: if japanese {
                    "jisho_api_v1"
                } else {
                    "wiktionary_definition_v0.8"
                }
                .into(),
                location: page.request_url.clone(),
                digest: page.raw_digest.clone(),
                fields: fields.clone(),
                model_manifest: if japanese {
                    "jisho-api-v1"
                } else {
                    "wiktionary-definition-v0.8"
                }
                .into(),
                tags: vec![],
                cards: vec![],
                media_refs: vec![],
            });
            document.archives.push(SourceArchive {
                id: uuid::Uuid::new_v4(),
                source_id,
                digest: page.raw_digest.clone(),
                original_fields: fields,
                asset_digests: vec![page.raw_digest.clone()],
            });
            for entry in &vocab.dictionary {
                for sense in &entry.senses {
                    document.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4(),
                        field: "meaning".into(),
                        provenance: Provenance::Dictionary,
                        source_id: Some(source_id),
                        region_id: None,
                        language: "en".to_owned().try_into()?,
                        claim: sense.definitions.join("; "),
                        source_url: Some(entry.source_url.clone()),
                        ambiguous: vocab.dictionary.len() > 1 || entry.senses.len() > 1,
                    });
                }
            }
            if vocab.dictionary.is_empty() {
                let mut issue = Issue::new(
                    "DICTIONARY_NOT_FOUND",
                    Severity::Warning,
                    Some("dictionary"),
                    "No dictionary entry was found; authored content remains distinct from dictionary facts.",
                );
                issue.stage = "dictionary".into();
                document.issues.push(issue);
            }
            provider_assets.push(page.raw_bytes);
        } else {
            return Err("CAPABILITY_UNAVAILABLE: authored grammar preparation requires dictionary.provider=authored".into());
        }
    }
    document.issues = validation::validate(&document);
    let rendered = render::render(&document, &document.sources[0].fields).ok();
    let ready = rendered.is_some()
        && document
            .issues
            .iter()
            .all(|issue| issue.severity == Severity::Warning);
    let plan = PlanRevision {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: frozen,
        binding: None,
        source_digest: canonical::digest("source-capture", &document.sources)
            .map_err(|e| e.to_string())?,
        selection: None,
        documents: vec![document],
        rendered: rendered.into_iter().collect(),
        review_decisions: vec![],
    };
    let root = std::path::Path::new(plan.settings.values["storage.state_dir"].as_str().unwrap());
    let mut store = linguist_store::Store::open(root)?;
    store.publish_asset(
        bytes,
        settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024,
    )?;
    for asset in provider_assets {
        store.publish_asset(
            &asset,
            settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
        )?;
    }
    let digest = store.publish_revision(&plan)?;
    Ok(Prepared {
        document_id: plan.documents[0].id,
        input_digest: plan.documents[0]
            .semantic_digest()
            .map_err(|e| e.to_string())?,
        schema_version: 2,
        plan_id: plan.id,
        revision: 1,
        digest,
        ready,
        issues: plan.documents[0].issues.clone(),
        original_input_digest,
        apply_eligible: false,
        duplicate_check_performed: false,
    })
}

pub mod audio;
pub mod capture;
pub mod export;
pub mod generation;
pub mod jobs;
pub mod mapping;
pub mod media;
pub mod ollama;
pub mod revamp;
pub mod review;
pub mod source_archive;
