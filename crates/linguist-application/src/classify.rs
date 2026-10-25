//! WP-27 `plans classify`: judged profiles for notes drafted before the
//! enrichment call asked for them. A batch of compact notes goes to the
//! configured agent in one call; each reply profile is checked like an
//! enrichment profile, and the plan gets one new revision that differs from
//! its base only in `profile`. That revision is reviewed and approved as
//! usual; `notes retag` then writes the approved profile's tags to the notes.
use crate::generation::{GenerationRequest, judged, judged_extras};
use linguist_config::Effective;
use linguist_core::document::JudgedProfile;
use linguist_core::records::PlanRevision;
use linguist_core::{LearningContent, LearningDocument, canonical};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const CLASSIFY_PROMPT_V1: &str = include_str!("../../../resources/prompts/classify-v1.txt");
/// Notes per call (decision 7).
pub const BATCH: usize = 25;

/// One reply profile, keyed by the note's document ID.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedProfile {
    pub id: String,
    pub domain: String,
    pub domains_secondary: Vec<String>,
    pub functions: Vec<String>,
    pub register: Vec<String>,
    pub connotation: String,
    pub transitivity_partner: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyReply {
    pub profiles: Vec<ClassifiedProfile>,
}

pub fn system_prompt() -> &'static str {
    static PROMPT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROMPT.get_or_init(|| {
        format!(
            "{CLASSIFY_PROMPT_V1}\nThe {} list of leaf paths:\n{}",
            linguist_core::profile::TAXONOMY,
            linguist_core::profile::prompt_list(true)
        )
    })
}

/// The documents of a revision that have no judged profile yet, in plan order.
pub fn pending(plan: &PlanRevision) -> Vec<&LearningDocument> {
    plan.documents
        .iter()
        .filter(|d| match &d.content {
            LearningContent::Vocabulary(v) => v.profile.is_none(),
            LearningContent::Grammar(g) => g.profile.is_none(),
        })
        .collect()
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

/// The compact form of one note sent to the agent.
pub fn compact(doc: &LearningDocument) -> Value {
    match &doc.content {
        LearningContent::Vocabulary(v) => {
            let language = doc.target_language.as_str();
            let sense = v
                .dictionary
                .iter()
                .flat_map(|e| &e.senses)
                .find(|s| s.key == v.sense_key);
            let derived = linguist_core::profile::vocabulary(language, v);
            json!({
                "id": doc.id,
                "kind": "vocabulary",
                "language": language,
                "expression": v.expression,
                "reading": v.reading,
                "sense": sense.map(|s| json!({"definitions": s.definitions, "labels": s.labels}))
                    .unwrap_or_else(|| json!({"definitions": [first_line(&v.meaning)], "labels": []})),
                "class": derived.class,
                "other_classes": derived.also,
                "domain_hints": linguist_core::profile::domain_hints(language, v),
                "example": v.examples.first().map(|e| first_line(&e.sentence)).unwrap_or_default(),
            })
        }
        LearningContent::Grammar(g) => json!({
            "id": doc.id,
            "kind": "grammar",
            "language": doc.target_language.as_str(),
            "pattern": g.pattern,
            "meaning": first_line(&g.meaning),
            "formation": first_line(&g.formation),
            "example": g.examples.first().map(|e| first_line(&e.sentence)).unwrap_or_default(),
        }),
    }
}

fn schema(notes: usize) -> Result<Value, String> {
    let mut schema =
        serde_json::to_value(schema_for!(ClassifyReply)).map_err(|_| "CLASSIFY_SCHEMA_INVALID")?;
    schema["properties"]["profiles"]["minItems"] = json!(notes);
    schema["properties"]["profiles"]["maxItems"] = json!(notes);
    let profile = &mut schema["$defs"]["ClassifiedProfile"];
    let properties = profile["properties"]
        .as_object_mut()
        .ok_or("CLASSIFY_SCHEMA_INVALID")?;
    let names: Vec<Value> = properties.keys().map(|name| json!(name)).collect();
    properties["domains_secondary"]["maxItems"] = json!(linguist_core::profile::SECONDARY_MAX);
    properties["functions"]["maxItems"] = json!(linguist_core::profile::FUNCTIONS_MAX);
    properties["register"]["maxItems"] = json!(linguist_core::profile::REGISTER_MAX);
    properties["register"]["items"]["enum"] = json!(linguist_core::profile::REGISTERS);
    let mut connotations = vec![""];
    connotations.extend(linguist_core::profile::CONNOTATIONS);
    properties["connotation"]["enum"] = json!(connotations);
    profile["required"] = Value::Array(names);
    Ok(schema)
}

/// One call's request: the prompt with the taxonomy, and the notes.
pub fn build_request(docs: &[&LearningDocument]) -> Result<GenerationRequest, String> {
    let notes: Vec<Value> = docs.iter().map(|d| compact(d)).collect();
    let user =
        json!({"schema_version": 1, "taxonomy": linguist_core::profile::TAXONOMY, "notes": notes});
    let output_schema = schema(docs.len())?;
    let prompt = system_prompt();
    let ids: Vec<String> = docs
        .iter()
        .map(|d| d.semantic_digest().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(GenerationRequest {
        system_prompt: prompt.into(),
        user_json: String::from_utf8(canonical::bytes(&user).map_err(|e| e.to_string())?)
            .map_err(|_| "CLASSIFY_ENCODING")?,
        prompt_digest: canonical::asset_digest(prompt.as_bytes()),
        schema_digest: canonical::digest("classify-schema-v1", &output_schema)
            .map_err(|e| e.to_string())?,
        input_digest: canonical::digest("classify-input-v1", &ids).map_err(|e| e.to_string())?,
        output_schema,
        allowed_fields: vec!["domain".into()],
        examples_requested: 0,
    })
}

/// The checked profile of each note the reply classified. A profile whose
/// primary path is not a leaf, or an ID outside the batch, is dropped; the
/// note stays without a profile and the next run sends it again.
pub fn merge_reply(
    docs: &[&LearningDocument],
    content: &str,
    max_bytes: usize,
) -> Result<BTreeMap<Uuid, JudgedProfile>, String> {
    if content.len() > max_bytes {
        return Err("CLASSIFY_OUTPUT_LIMIT".into());
    }
    let reply: ClassifyReply =
        serde_json::from_str(content).map_err(|e| format!("CLASSIFY_OUTPUT_INVALID: {e}"))?;
    let mut out = BTreeMap::new();
    for p in reply.profiles {
        let Some(doc) = docs.iter().find(|d| d.id.to_string() == p.id.trim()) else {
            continue;
        };
        let grammar = matches!(doc.content, LearningContent::Grammar(_));
        let Some(mut profile) = judged(p.domain, p.domains_secondary, p.functions, grammar) else {
            continue;
        };
        match &doc.content {
            LearningContent::Grammar(_) => {
                judged_extras(&mut profile, p.register, String::new(), None)
            }
            LearningContent::Vocabulary(v) => judged_extras(
                &mut profile,
                Vec::new(),
                p.connotation,
                Some((doc, v, p.transitivity_partner)),
            ),
        }
        out.entry(doc.id).or_insert(profile);
    }
    Ok(out)
}

pub struct ClassifyRequest<'a> {
    pub settings: &'a Effective,
    pub environment: &'a BTreeMap<String, String>,
    pub batch: usize,
    /// At most this many notes in this run.
    pub limit: Option<usize>,
}

/// Classify the pending documents of `base` (the plan's latest revision) and
/// publish one child revision with their profiles. A failed call ends the
/// run; the profiles of earlier calls are still published.
pub fn classify_plan(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    expected_digest: &str,
    request: &ClassifyRequest,
) -> Result<Value, String> {
    if store.latest_revision(base.id)? != base.revision
        || base.approval_digest().map_err(|e| e.to_string())? != expected_digest
    {
        return Err("CLASSIFY_BASE_CONFLICT".into());
    }
    let settings = request.settings;
    if settings.values["llm.enabled"] != true {
        return Err("GENERATION_DISABLED: enable llm.enabled in current settings".into());
    }
    let providers: Vec<_> = crate::agents::chain(settings)?
        .into_iter()
        .filter(|p| *p != crate::agents::Provider::Ollama)
        .collect();
    if providers.is_empty() {
        return Err(
            "CLASSIFY_NO_AGENT: classify needs an agent provider (llm.provider or llm.fallback)"
                .into(),
        );
    }
    let max_bytes = settings
        .values
        .get("network.max_response_mb")
        .and_then(Value::as_u64)
        .unwrap_or(16) as usize
        * 1024
        * 1024;
    let mut todo = pending(base);
    if let Some(limit) = request.limit {
        todo.truncate(limit);
    }
    let mut profiles = BTreeMap::new();
    let mut calls = Vec::new();
    let mut stopped = None;
    'batches: for chunk in todo.chunks(request.batch.clamp(1, 50)) {
        let call = build_request(chunk)?;
        let mut errors = Vec::new();
        for provider in &providers {
            match crate::agents::generate(*provider, &call, settings, request.environment) {
                Ok(Some(external)) => match merge_reply(chunk, &external.content, max_bytes) {
                    Ok(found) => {
                        calls.push(json!({"provider": provider.name(), "identity": external.identity, "notes": chunk.len(), "classified": found.len()}));
                        profiles.extend(found);
                        continue 'batches;
                    }
                    Err(error) => {
                        crate::agents::keep_rejected(
                            *provider,
                            settings,
                            request.environment,
                            &chunk[0].id.to_string(),
                            &error,
                            external.content.as_bytes(),
                        );
                        errors.push(format!("{}: {error}", provider.name()));
                    }
                },
                Ok(None) => errors.push(format!("{}: not available", provider.name())),
                Err(error) => errors.push(format!("{}: {error}", provider.name())),
            }
        }
        stopped = Some(errors.join("; "));
        break;
    }
    let mut result = json!({
        "schema_version": 1,
        "plan_id": base.id,
        "base_revision": base.revision,
        "pending": pending(base).len(),
        "sent": calls.iter().map(|c| c["notes"].as_u64().unwrap_or(0)).sum::<u64>(),
        "classified": profiles.len(),
        "calls": calls,
        "stopped": stopped,
    });
    if profiles.is_empty() {
        return match stopped {
            Some(error) => Err(format!("CLASSIFY_FAILED: {error}")),
            None => Ok(result),
        };
    }
    let mut child = base.clone();
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("GENERATION_REVISION_LIMIT")?;
    child.parent_digest = Some(expected_digest.into());
    child.binding = None;
    for document in &mut child.documents {
        let Some(profile) = profiles.remove(&document.id) else {
            continue;
        };
        match &mut document.content {
            LearningContent::Vocabulary(v) => v.profile = Some(profile),
            LearningContent::Grammar(g) => g.profile = Some(profile),
        }
    }
    // A classified document differs from its base only in the profile, which
    // no review decision judges: decisions and review records bound to its
    // base digest, or to its digest without a profile, move to its new
    // digest. Documents left alone keep theirs.
    let mut moved: BTreeMap<String, String> = BTreeMap::new();
    for (old, new) in base.documents.iter().zip(child.documents.iter_mut()) {
        if !profile_only(old, new) {
            continue;
        }
        let to = new.semantic_digest().map_err(|e| e.to_string())?;
        let mut bare = new.clone();
        match &mut bare.content {
            LearningContent::Vocabulary(v) => v.profile = None,
            LearningContent::Grammar(g) => g.profile = None,
        }
        for from in [
            old.semantic_digest().map_err(|e| e.to_string())?,
            bare.semantic_digest().map_err(|e| e.to_string())?,
        ] {
            if from == to {
                continue;
            }
            for review in new.reviews.iter_mut().filter(|r| r.input_digest == from) {
                review.input_digest = to.clone();
            }
            moved.insert(from, to.clone());
        }
    }
    for decision in &mut child.review_decisions {
        if let Some(to) = moved.get(&decision.input_digest) {
            decision.input_digest = to.clone();
        }
    }
    child.rendered = linguist_core::render::render_ready(&child.documents);
    child.approval_digest().map_err(|e| e.to_string())?;
    let digest = store.publish_revision(&child)?;
    result["revision"] = json!(child.revision);
    result["digest"] = json!(digest);
    Ok(result)
}

/// True when `newer` is `older` with a profile added or changed: the only
/// difference `notes retag` may carry from a later approved revision. The
/// digest a review is bound to moves with the profile and is not compared.
pub fn profile_only(older: &LearningDocument, newer: &LearningDocument) -> bool {
    let strip = |d: &LearningDocument| {
        let mut d = d.clone();
        match &mut d.content {
            LearningContent::Vocabulary(v) => v.profile = None,
            LearningContent::Grammar(g) => g.profile = None,
        }
        for review in &mut d.reviews {
            review.input_digest.clear();
        }
        d
    };
    strip(older) == strip(newer)
}
