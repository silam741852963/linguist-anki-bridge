//! Generation contracts. No provider requests or document mutation occur in this module.
use linguist_config::Effective;
use linguist_core::{
    Example, LearningContent, LearningDocument, Provenance, Task, canonical, validation,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub mod repair;

pub const VOCABULARY_PROMPT_V2: &str = include_str!("../../../resources/prompts/vocabulary-v2.txt");
pub const GRAMMAR_PROMPT_V2: &str = include_str!("../../../resources/prompts/grammar-v2.txt");

#[derive(Debug, Serialize)]
pub struct ModelIdentity {
    pub name: String,
    pub digest: String,
}
pub struct GeneratedDraft {
    pub document: LearningDocument,
    /// Publish these exact bytes before publishing any plan that references the draft.
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Publish one transport-validated candidate as a blocked review revision.
/// Inference is never retried here and no native collection effect is possible.
pub fn publish_candidate(
    store: &mut linguist_store::Store,
    base: &linguist_core::records::PlanRevision,
    document_id: uuid::Uuid,
    expected_digest: &str,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    client: &crate::ollama::transport::Client,
) -> Result<Value, String> {
    if base.approval_digest().map_err(|e| e.to_string())? != expected_digest
        || store.latest_revision(base.id)? != base.revision
    {
        return Err("GENERATION_BASE_CONFLICT".into());
    }
    let parent = base
        .documents
        .iter()
        .find(|document| document.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?
        .clone();
    publish_candidate_from(
        store,
        base,
        &parent,
        expected_digest,
        settings,
        environment,
        client,
    )
}

/// As [`publish_candidate`], generating from a caller-prepared copy of the item
/// (for example with regenerable fields cleared). The base revision and digest
/// are still checked against the store before any provider call.
pub fn publish_candidate_from(
    store: &mut linguist_store::Store,
    base: &linguist_core::records::PlanRevision,
    parent: &LearningDocument,
    expected_digest: &str,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    client: &crate::ollama::transport::Client,
) -> Result<Value, String> {
    if base.approval_digest().map_err(|e| e.to_string())? != expected_digest
        || store.latest_revision(base.id)? != base.revision
        || !base
            .documents
            .iter()
            .any(|document| document.id == parent.id)
    {
        return Err("GENERATION_BASE_CONFLICT".into());
    }
    let document_id = parent.id;
    let frozen = crate::freeze_settings(settings, environment)?;
    if frozen.values["storage.state_dir"] != base.settings.values["storage.state_dir"] {
        return Err("GENERATION_STORAGE_CONFLICT".into());
    }
    if settings.values["llm.enabled"] != true {
        return Err("GENERATION_DISABLED: enable llm.enabled in current settings".into());
    }
    // Check request and revised configuration before any provider call.
    build_request(parent, settings)?;
    let mut child = base.clone();
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("GENERATION_REVISION_LIMIT")?;
    child.parent_digest = Some(expected_digest.into());
    child.settings = frozen;
    child.binding = None;
    child.approval_digest().map_err(|e| e.to_string())?;
    let mut draft = client.generate_draft(parent)?;
    if draft.document.id != document_id
        || draft.document.target_language != parent.target_language
        || draft.document.explanation_language != parent.explanation_language
        || !draft.document.issues.iter().any(|issue| {
            issue.code == "GENERATION_ENGINE_UNVERIFIED"
                && issue.severity == validation::Severity::Error
        })
    {
        return Err("GENERATION_DRAFT_CONFLICT".into());
    }
    draft.document.reviews.clear();
    let max_bytes = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    let mut total = 0u64;
    for (digest, bytes) in &draft.assets {
        if canonical::asset_digest(bytes) != *digest {
            return Err("GENERATION_ASSET_DIGEST_CONFLICT".into());
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or("GENERATION_ARCHIVE_LIMIT")?;
        if total > max_bytes {
            return Err("GENERATION_ARCHIVE_LIMIT".into());
        }
    }
    let index = child
        .documents
        .iter()
        .position(|document| document.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    child.documents[index] = draft.document;
    child
        .rendered
        .retain(|rendered| rendered.document_id != document_id);
    child.review_decisions.clear();
    let sources: Vec<_> = child
        .documents
        .iter()
        .flat_map(|document| &document.sources)
        .collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    for (digest, bytes) in draft.assets {
        if store.publish_asset(&bytes, max_bytes)? != digest {
            return Err("GENERATION_ASSET_DIGEST_CONFLICT".into());
        }
    }
    let digest = store.publish_revision(&child)?;
    Ok(
        json!({"schema_version":2,"plan_id":child.id,"revision":child.revision,"digest":digest,"document_id":document_id,"ready":false,"generation_engine_verified":false,"apply_eligible":false,"issues":child.documents[index].issues,"assets_archived":true}),
    )
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedExample {
    pub sentence: String,
    pub translation: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VocabularySupplement {
    pub usage: String,
    pub examples: Vec<GeneratedExample>,
    pub production_prompt: String,
    pub spelling_prompt: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrammarSupplement {
    pub meaning: String,
    pub formation: String,
    pub usage: String,
    pub examples: Vec<GeneratedExample>,
    pub recognition_prompt: String,
    pub exercise_prompt: String,
    pub exercise_answer: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Supplement {
    Vocabulary(VocabularySupplement),
    Grammar(GrammarSupplement),
}
#[derive(Debug, Serialize)]
pub struct GenerationRequest {
    pub system_prompt: String,
    pub user_json: String,
    pub output_schema: Value,
    pub prompt_digest: String,
    pub schema_digest: String,
    pub input_digest: String,
    pub allowed_fields: Vec<String>,
    pub examples_requested: usize,
}

pub fn build_request(
    doc: &LearningDocument,
    settings: &Effective,
) -> Result<GenerationRequest, String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "llm.enabled",
        "llm.prompts.vocabulary",
        "llm.prompts.grammar",
        "learning.examples_min",
        "learning.generated_examples_max",
    ] {
        registry.validate_value(
            key,
            settings
                .values
                .get(key)
                .ok_or("GENERATION_SETTING_MISSING")?,
        )?;
    }
    if settings.values["llm.enabled"] != true {
        return Err("GENERATION_DISABLED".into());
    }
    if validation::validate(doc)
        .iter()
        .any(|issue| issue.code == "DICTIONARY_SENSE_REVIEW")
    {
        return Err("GENERATION_SENSE_SELECTION_REQUIRED".into());
    }
    if !doc.target_language.is_target_supported() {
        return Err("GENERATION_TARGET_UNSUPPORTED".into());
    }
    // ALG-OCR: required OCR and its reviews finish before dependent generation.
    crate::ocr_inspection::generation_ready(doc, settings)?;
    let mut allowed = Vec::new();
    let mut field = |name: &str, value: &str| {
        let managed_name = match name {
            "meaning" => "Meaning",
            "formation" => "Formation",
            "usage" => "Usage",
            "production_prompt" => "ProductionPrompt",
            "spelling_prompt" => "SpellingPrompt",
            "recognition_prompt" => "RecognitionPrompt",
            "exercise_prompt" => "ExercisePrompt",
            "exercise_answer" => "ExerciseAnswer",
            _ => name,
        };
        if value.trim().is_empty() && !doc.edits.contains_key(managed_name) {
            allowed.push(name.to_owned());
        }
    };
    let (kind, prompt_key, prompt_ref, prompt, example_count) = match &doc.content {
        LearningContent::Vocabulary(v) => {
            if v.expression.trim().is_empty() || v.meaning.trim().is_empty() {
                return Err("GENERATION_ACCEPTED_ANSWER_REQUIRED".into());
            }
            field("usage", &v.usage);
            if doc.requested_tasks.contains(&Task::Production) {
                field("production_prompt", &v.production_prompt);
            }
            if doc.requested_tasks.contains(&Task::Spelling) {
                field("spelling_prompt", &v.spelling_prompt);
            }
            (
                "vocabulary",
                "llm.prompts.vocabulary",
                "builtin:vocabulary-v2",
                VOCABULARY_PROMPT_V2,
                v.examples.len(),
            )
        }
        LearningContent::Grammar(g) => {
            if g.pattern.trim().is_empty() || g.use_key.trim().is_empty() {
                return Err("GENERATION_ACCEPTED_UNIT_REQUIRED".into());
            }
            field("meaning", &g.meaning);
            field("formation", &g.formation);
            field("usage", &g.usage);
            field("recognition_prompt", &g.recognition_prompt);
            if doc.requested_tasks.contains(&Task::Application) {
                field("exercise_prompt", &g.exercise_prompt);
                field("exercise_answer", &g.exercise_answer);
            }
            (
                "grammar",
                "llm.prompts.grammar",
                "builtin:grammar-v2",
                GRAMMAR_PROMPT_V2,
                g.examples.len(),
            )
        }
    };
    if settings.values[prompt_key] != prompt_ref {
        return Err(
            "CAPABILITY_UNAVAILABLE: pinned custom prompt loading is not implemented".into(),
        );
    }
    let examples_requested = (settings.values["learning.examples_min"].as_u64().unwrap() as usize)
        .saturating_sub(example_count)
        .min(
            settings.values["learning.generated_examples_max"]
                .as_u64()
                .unwrap() as usize,
        );
    if examples_requested > 0 {
        allowed.push("examples".into());
    }
    // Scheduling/history and approval actors are not learning material sent to the model.
    let sources: Vec<_> = doc.sources.iter().filter(|source| source.kind != "generated_supplement_v2").map(|source| json!({"id":source.id,"kind":source.kind,"fields":source.fields,"digest":source.digest})).collect();
    let user = json!({"schema_version":2,"kind":kind,"target_language":doc.target_language,"explanation_language":doc.explanation_language,
        "accepted_content":doc.content,"context":doc.context,"sources":sources,"regions":doc.regions,"evidence":doc.evidence,
        "allowed_fields":allowed,"examples_requested":examples_requested});
    let output_schema = constrained_schema(kind, &allowed, examples_requested)?;
    Ok(GenerationRequest {
        system_prompt: prompt.into(),
        user_json: String::from_utf8(canonical::bytes(&user).map_err(|e| e.to_string())?)
            .map_err(|_| "GENERATION_ENCODING")?,
        prompt_digest: canonical::asset_digest(prompt.as_bytes()),
        schema_digest: canonical::digest("generation-schema-v2", &output_schema)
            .map_err(|e| e.to_string())?,
        input_digest: doc.semantic_digest().map_err(|e| e.to_string())?,
        output_schema,
        allowed_fields: allowed,
        examples_requested,
    })
}

/// The supplement schema narrowed to this request: only the document's kind,
/// fields that are not allowed must be empty, and at most `examples` new
/// examples. Constrained decoding then cannot produce output that
/// `validate_output` must reject; validation still runs on every reply.
fn constrained_schema(kind: &str, allowed: &[String], examples: usize) -> Result<Value, String> {
    let mut schema =
        serde_json::to_value(schema_for!(Supplement)).map_err(|_| "GENERATION_SCHEMA_INVALID")?;
    let branch = schema["oneOf"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|b| b["properties"]["kind"]["const"] == kind)
                .cloned()
        })
        .ok_or("GENERATION_SCHEMA_INVALID")?;
    schema["oneOf"] = json!([branch]);
    let definition = if kind == "vocabulary" {
        "VocabularySupplement"
    } else {
        "GrammarSupplement"
    };
    let properties = schema["$defs"][definition]["properties"]
        .as_object_mut()
        .ok_or("GENERATION_SCHEMA_INVALID")?;
    for (name, property) in properties.iter_mut() {
        if name == "examples" {
            property["maxItems"] = json!(examples);
        } else if !allowed.iter().any(|field| field == name) {
            property["enum"] = json!([""]);
        }
    }
    Ok(schema)
}

pub fn parse_output(bytes: &[u8], max_bytes: u64, max_chars: usize) -> Result<Supplement, String> {
    if !(1..=100 * 1024 * 1024).contains(&max_bytes) || !(1..=1000000).contains(&max_chars) {
        return Err("GENERATION_INVALID_LIMITS".into());
    }
    if bytes.len() as u64 > max_bytes {
        return Err("GENERATION_OUTPUT_LIMIT".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "GENERATION_OUTPUT_ENCODING")?;
    if text.chars().take(max_chars + 1).count() > max_chars {
        return Err("GENERATION_OUTPUT_LIMIT".into());
    }
    canonical::parse(bytes).map_err(|_| "GENERATION_OUTPUT_SCHEMA_INVALID".into())
}

/// Validate a supplement against the exact document/request before any future merge.
pub fn validate_output(
    doc: &LearningDocument,
    request: &GenerationRequest,
    bytes: &[u8],
    max_bytes: u64,
    max_chars: usize,
) -> Result<Supplement, String> {
    if doc.semantic_digest().map_err(|e| e.to_string())? != request.input_digest {
        return Err("GENERATION_INPUT_CONFLICT".into());
    }
    let output = parse_output(bytes, max_bytes, max_chars)?;
    let (fields, examples): (Vec<(&str, &str)>, _) = match (&doc.content, &output) {
        (LearningContent::Vocabulary(_), Supplement::Vocabulary(v)) => (
            vec![
                ("usage", &v.usage),
                ("production_prompt", &v.production_prompt),
                ("spelling_prompt", &v.spelling_prompt),
            ],
            &v.examples,
        ),
        (LearningContent::Grammar(_), Supplement::Grammar(g)) => (
            vec![
                ("meaning", &g.meaning),
                ("formation", &g.formation),
                ("usage", &g.usage),
                ("recognition_prompt", &g.recognition_prompt),
                ("exercise_prompt", &g.exercise_prompt),
                ("exercise_answer", &g.exercise_answer),
            ],
            &g.examples,
        ),
        _ => return Err("GENERATION_KIND_CONFLICT".into()),
    };
    for (name, value) in fields {
        if !value.is_empty() && !request.allowed_fields.iter().any(|field| field == name) {
            return Err("GENERATION_FIELD_NOT_ALLOWED".into());
        }
    }
    if examples.len() > request.examples_requested
        || (!examples.is_empty()
            && !request
                .allowed_fields
                .iter()
                .any(|field| field == "examples"))
    {
        return Err("GENERATION_EXAMPLE_LIMIT".into());
    }
    let translation_required = doc.target_language.as_str().split('-').next()
        != doc.explanation_language.as_str().split('-').next();
    if examples.iter().any(|example| {
        example.sentence.trim().is_empty()
            || (translation_required && example.translation.trim().is_empty())
    }) {
        return Err("GENERATION_INCOMPLETE_EXAMPLE".into());
    }
    Ok(output)
}

/// Stage an immutable child document; callers must prove installed model capabilities separately.
/// Never mutates the parent or publishes assets/state/Anki effects.
pub fn merge_output(
    doc: &LearningDocument,
    settings: &Effective,
    request: &GenerationRequest,
    bytes: &[u8],
    model: &ModelIdentity,
) -> Result<GeneratedDraft, String> {
    merge_checked_output(doc, settings, request, bytes, model, None)
}

/// Parse the provider envelope again at the trust boundary, then validate its supplement.
/// This creates a reviewable draft, not proof of prompt fit or permission to apply.
pub fn merge_ollama_response(
    doc: &LearningDocument,
    settings: &Effective,
    request: &GenerationRequest,
    response: &[u8],
    model: &ModelIdentity,
) -> Result<GeneratedDraft, String> {
    let completion = crate::ollama::parse_completion(response, settings)?;
    merge_checked_output(
        doc,
        settings,
        request,
        completion.content.as_bytes(),
        model,
        Some(&completion),
    )
}

fn merge_checked_output(
    doc: &LearningDocument,
    settings: &Effective,
    request: &GenerationRequest,
    bytes: &[u8],
    model: &ModelIdentity,
    completion: Option<&crate::ollama::Completion>,
) -> Result<GeneratedDraft, String> {
    let expected = build_request(doc, settings)?;
    if canonical::bytes(&expected).map_err(|e| e.to_string())?
        != canonical::bytes(request).map_err(|e| e.to_string())?
    {
        return Err("GENERATION_REQUEST_CONFLICT".into());
    }
    if settings.values.get("llm.model").and_then(Value::as_str) != Some(model.name.as_str())
        || model.digest.len() != 71
        || !model.digest.starts_with("sha256:")
        || !model.digest[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("GENERATION_MODEL_IDENTITY_INVALID".into());
    }
    let registry = linguist_config::Registry::builtin();
    for key in ["network.max_response_mb", "input.max_record_chars"] {
        registry.validate_value(
            key,
            settings
                .values
                .get(key)
                .ok_or("GENERATION_SETTING_MISSING")?,
        )?;
    }
    let output = validate_output(
        doc,
        request,
        bytes,
        settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
        settings.values["input.max_record_chars"].as_u64().unwrap() as usize,
    )?;
    let mut child = doc.clone();
    let source_id = uuid::Uuid::new_v4();
    let mut claims = Vec::new();
    let mut set = |name: &str, current: &mut String, value: String| -> Result<(), String> {
        if !value.is_empty() {
            if !current.trim().is_empty() {
                return Err("GENERATION_AUTHORED_FIELD_CONFLICT".into());
            }
            *current = value.clone();
            claims.push((name.to_owned(), value));
        }
        Ok(())
    };
    let new_examples = match (&mut child.content, output) {
        (LearningContent::Vocabulary(v), Supplement::Vocabulary(s)) => {
            set("usage", &mut v.usage, s.usage)?;
            set(
                "production_prompt",
                &mut v.production_prompt,
                s.production_prompt,
            )?;
            set("spelling_prompt", &mut v.spelling_prompt, s.spelling_prompt)?;
            s.examples
        }
        (LearningContent::Grammar(g), Supplement::Grammar(s)) => {
            set("meaning", &mut g.meaning, s.meaning)?;
            set("formation", &mut g.formation, s.formation)?;
            set("usage", &mut g.usage, s.usage)?;
            set(
                "recognition_prompt",
                &mut g.recognition_prompt,
                s.recognition_prompt,
            )?;
            set("exercise_prompt", &mut g.exercise_prompt, s.exercise_prompt)?;
            set("exercise_answer", &mut g.exercise_answer, s.exercise_answer)?;
            s.examples
        }
        _ => return Err("GENERATION_KIND_CONFLICT".into()),
    };
    let examples = match &mut child.content {
        LearningContent::Vocabulary(v) => &mut v.examples,
        LearningContent::Grammar(g) => &mut g.examples,
    };
    for example in new_examples {
        if examples
            .iter()
            .any(|old| old.sentence == example.sentence && old.translation == example.translation)
        {
            continue;
        }
        let id = uuid::Uuid::new_v4();
        child.evidence.push(linguist_core::records::Evidence {
            id,
            field: "examples".into(),
            provenance: Provenance::Generated,
            source_id: Some(source_id),
            region_id: None,
            target: Some(linguist_core::records::EvidenceTarget::Example {
                index: examples.len(),
            }),
            source_span: None,
            language: child.target_language.clone(),
            claim: serde_json::to_string(&example).map_err(|_| "GENERATION_ENCODING")?,
            source_url: None,
            ambiguous: false,
        });
        examples.push(Example {
            sentence: example.sentence,
            translation: example.translation,
            provenance: Provenance::Generated,
            evidence_ids: vec![id],
        });
    }
    for (field, claim) in claims {
        let target = (field == "formation")
            .then_some(linguist_core::records::EvidenceTarget::GrammarFormation);
        child.evidence.push(linguist_core::records::Evidence {
            id: uuid::Uuid::new_v4(),
            field,
            provenance: Provenance::Generated,
            source_id: Some(source_id),
            region_id: None,
            target,
            source_span: None,
            language: child.explanation_language.clone(),
            claim,
            source_url: None,
            ambiguous: false,
        });
    }
    let raw = std::str::from_utf8(bytes).map_err(|_| "GENERATION_OUTPUT_ENCODING")?;
    let parameters: BTreeMap<_, _> = settings
        .values
        .iter()
        .filter(|(key, _)| key.starts_with("llm.") || key.starts_with("learning."))
        .collect();
    let mut fields = BTreeMap::from([
        ("provider_output".into(), raw.into()),
        (
            "request".into(),
            String::from_utf8(canonical::bytes(request).map_err(|e| e.to_string())?)
                .map_err(|_| "GENERATION_ENCODING")?,
        ),
        (
            "generation_parameters".into(),
            String::from_utf8(canonical::bytes(&parameters).map_err(|e| e.to_string())?)
                .map_err(|_| "GENERATION_ENCODING")?,
        ),
        (
            "model_identity".into(),
            serde_json::to_string(model).map_err(|_| "GENERATION_ENCODING")?,
        ),
        (
            "prior_reviews".into(),
            String::from_utf8(canonical::bytes(&doc.reviews).map_err(|e| e.to_string())?)
                .map_err(|_| "GENERATION_ENCODING")?,
        ),
    ]);
    if let Some(completion) = completion {
        fields.insert(
            "provider_response".into(),
            String::from_utf8(completion.raw.clone()).map_err(|_| "GENERATION_ENCODING")?,
        );
        fields.insert(
            "response_receipt".into(),
            String::from_utf8(canonical::bytes(completion).map_err(|e| e.to_string())?)
                .map_err(|_| "GENERATION_ENCODING")?,
        );
    }
    let manifest = canonical::bytes(&fields).map_err(|e| e.to_string())?;
    if manifest.len() > 100 * 1024 * 1024 {
        return Err("GENERATION_ARCHIVE_LIMIT".into());
    }
    let digest = canonical::asset_digest(&manifest);
    let output_digest = canonical::asset_digest(bytes);
    let output_text = String::from_utf8(bytes.to_vec()).map_err(|_| "GENERATION_ENCODING")?;
    let mut assets = BTreeMap::from([
        (digest.clone(), manifest),
        (output_digest.clone(), bytes.to_vec()),
    ]);
    if let Some(completion) = completion {
        assets.insert(completion.raw_digest.clone(), completion.raw.clone());
    }
    child.sources.push(linguist_core::records::SourceRecord {
        id: source_id,
        kind: "generated_supplement_v2".into(),
        location: "local_generation_artifact".into(),
        digest: digest.clone(),
        text: Some(output_text.clone()),
        fields: fields.clone(),
        model_manifest: format!("{}@{}", model.name, model.digest),
        template_manifest: None,
        captured_at_unix_seconds: None,
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    child.archives.push(linguist_core::records::SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: Some(output_text),
        original_fields: fields,
        asset_digests: assets.keys().cloned().collect(),
    });
    // Conservative whole-document invalidation until dependency-specific rebinding exists.
    child.reviews.clear();
    child.issues.retain(|issue| issue.stage != "validation");
    child.issues.extend(validation::validate(&child));
    Ok(GeneratedDraft {
        document: child,
        assets,
    })
}

/// ALG-VOCAB step 5 after preparation: generate supplements for every item whose
/// request is currently buildable, one child revision per item. Ineligible items
/// (pending sense review, OCR review, nothing to fill) are reported, not forced.
/// A failure for one item leaves earlier revisions and other items intact.
pub fn generate_pending(
    plan_id: uuid::Uuid,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    client: &crate::ollama::transport::Client,
) -> Result<Vec<Value>, String> {
    let frozen = crate::freeze_settings(settings, environment)?;
    let root = std::path::PathBuf::from(
        frozen.values["storage.state_dir"]
            .as_str()
            .ok_or("GENERATION_STATE_PATH_MISSING")?,
    );
    let ids: Vec<uuid::Uuid> = {
        let store = linguist_store::Store::read_only(&root)?;
        let latest = store.latest_revision(plan_id)?;
        store
            .revision(plan_id, latest)?
            .documents
            .iter()
            .map(|document| document.id)
            .collect()
    };
    let mut results = Vec::new();
    for id in ids {
        let mut store = linguist_store::Store::open_existing(&root)?;
        let latest = store.latest_revision(plan_id)?;
        let base = store.revision(plan_id, latest)?;
        let document = base
            .documents
            .iter()
            .find(|document| document.id == id)
            .ok_or("PLAN_ITEM_NOT_FOUND")?;
        match build_request(document, settings) {
            Err(reason) => {
                results.push(json!({"document_id":id,"generated":false,"skipped":reason}));
                continue;
            }
            Ok(request) if request.allowed_fields.is_empty() => {
                results.push(json!({"document_id":id,"generated":false,"skipped":"GENERATION_NOTHING_TO_FILL"}));
                continue;
            }
            Ok(_) => {}
        }
        let digest = base.approval_digest().map_err(|e| e.to_string())?;
        match publish_candidate(&mut store, &base, id, &digest, settings, environment, client) {
            Ok(result) => results.push(json!({"document_id":id,"generated":true,"result":result})),
            Err(error) => results.push(json!({"document_id":id,"generated":false,"error":error,
                "next_command":format!("linguist-anki-bridge plans generate {plan_id} --item-id {id} --base-revision {latest} --digest {digest} --use-current-settings")})),
        }
    }
    Ok(results)
}
