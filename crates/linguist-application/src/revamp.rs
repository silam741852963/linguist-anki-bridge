//! Conservative source-to-document staging; source ownership/history still requires native review.
use crate::{mapping::SourceKind, source_archive::RevampCapture};
use linguist_config::Effective;
use linguist_core::{
    LearningDocument, Provenance, canonical,
    records::Evidence,
    validation::{self, Issue, Severity},
};
use serde_json::json;
use std::collections::BTreeMap;

fn visible_text(raw: &str) -> String {
    let cleaned = ammonia::Builder::default()
        .tags(std::collections::HashSet::from([
            "br", "p", "div", "li", "tr", "td", "th",
        ]))
        .generic_attributes(std::collections::HashSet::new())
        .tag_attributes(std::collections::HashMap::new())
        .clean(raw)
        .to_string();
    let mut text = cleaned.replace("<br>", "\n");
    for tag in ["p", "div", "li", "tr"] {
        text = text
            .replace(&format!("<{tag}>"), "\n")
            .replace(&format!("</{tag}>"), "\n");
    }
    for tag in ["td", "th"] {
        text = text
            .replace(&format!("<{tag}>"), " ")
            .replace(&format!("</{tag}>"), " ");
    }
    html_escape::decode_html_entities(&text).trim().to_owned()
}
fn issue(code: &str, field: Option<&str>, source: uuid::Uuid) -> Issue {
    let mut result = Issue::new(
        code,
        Severity::Review,
        field,
        "Source interpretation requires review before revamp readiness.",
    );
    result.stage = "capture".into();
    result.source_refs = vec![source.to_string()];
    result
}
/// Plain scalar roles become source-backed candidates. Rich/combined fields remain archived only.
/// No examples are split, task history inferred, optional task disabled or source field discarded.
pub fn stage_document(
    capture: &RevampCapture,
    settings: &Effective,
    purpose: &str,
) -> Result<LearningDocument, String> {
    for (digest, bytes) in &capture.captured.assets {
        if canonical::asset_digest(bytes) != *digest {
            return Err("REVAMP_CAPTURE_ASSET_CONFLICT".into());
        }
    }
    let source = &capture.captured.source;
    let archive = &capture.captured.archive;
    if !archive.asset_digests.contains(&source.digest)
        || archive
            .asset_digests
            .iter()
            .any(|digest| !capture.captured.assets.contains_key(digest))
    {
        return Err("REVAMP_CAPTURE_ASSET_MISSING".into());
    }
    if archive.source_id != source.id
        || archive.digest != source.digest
        || archive.original_fields != source.fields
    {
        return Err("REVAMP_CAPTURE_ARCHIVE_CONFLICT".into());
    }
    let manifest: serde_json::Value = canonical::parse(
        capture
            .captured
            .assets
            .get(&source.digest)
            .ok_or("REVAMP_CAPTURE_MANIFEST_MISSING")?,
    )
    .map_err(|_| "REVAMP_CAPTURE_MANIFEST_INVALID")?;
    let payload = |name: &str| -> Result<&[u8], String> {
        let digest = manifest["payloads"][name]
            .as_str()
            .ok_or("REVAMP_CAPTURE_MANIFEST_INVALID")?;
        if !archive.asset_digests.iter().any(|entry| entry == digest) {
            return Err("REVAMP_CAPTURE_ARCHIVE_CONFLICT".into());
        }
        Ok(capture
            .captured
            .assets
            .get(digest)
            .ok_or("REVAMP_CAPTURE_ASSET_MISSING")?
            .as_slice())
    };
    let rebuilt = crate::source_archive::archive_read_capture(
        payload("note")?,
        payload("model")?,
        payload("cards")?,
        100 * 1024 * 1024,
    )?;
    if rebuilt.source.fields != source.fields
        || rebuilt.source.tags != source.tags
        || rebuilt.source.model_manifest != source.model_manifest
        || rebuilt.source.location != source.location
        || rebuilt.source.media_refs != source.media_refs
    {
        return Err("REVAMP_CAPTURE_SOURCE_CONFLICT".into());
    }
    let model: serde_json::Value = canonical::parse(
        capture
            .captured
            .assets
            .get(&capture.captured.source.model_manifest)
            .ok_or("REVAMP_CAPTURE_MODEL_MISSING")?,
    )
    .map_err(|_| "REVAMP_CAPTURE_MODEL_INVALID")?;
    let model_name = model["model"]["name"]
        .as_str()
        .ok_or("REVAMP_CAPTURE_MODEL_INVALID")?;
    let mapping = crate::mapping::map_purpose_fields(
        settings,
        purpose,
        model_name,
        &capture.captured.source.fields,
    )?;
    if mapping.mapping_digest != capture.mapping.mapping_digest
        || mapping.source_digest != capture.mapping.source_digest
    {
        return Err("REVAMP_MAPPING_CONFLICT".into());
    }
    let target = settings
        .values
        .get(&format!("purposes.{purpose}.target_language"))
        .and_then(serde_json::Value::as_str)
        .ok_or("REVAMP_TARGET_LANGUAGE_REQUIRED")?;
    let explanation = settings.values["learning.explanation_language"]
        .as_str()
        .ok_or("REVAMP_EXPLANATION_LANGUAGE_REQUIRED")?;
    let source_id = capture.captured.source.id;
    let mut issues = vec![issue("SOURCE_NATIVE_HISTORY_REVIEW", None, source_id)];
    let mut values = BTreeMap::new();
    for (role, field) in &mapping.roles {
        if field.raw_value.trim().is_empty() {
            continue;
        }
        if mapping.shared_fields.contains_key(&field.source_field) {
            issues.push(issue("SOURCE_COMBINED_FIELD_REVIEW", Some(role), source_id));
            continue;
        }
        if [
            "picture",
            "audio",
            "examples",
            "enable_production",
            "enable_spelling",
            "enable_application",
        ]
        .contains(&role.as_str())
        {
            issues.push(issue(
                "SOURCE_STRUCTURED_ROLE_REVIEW",
                Some(role),
                source_id,
            ));
            continue;
        }
        if role == "language" || role == "explanation_language" {
            let expected = if role == "language" {
                target
            } else {
                explanation
            };
            if !field.raw_value.trim().eq_ignore_ascii_case(expected) {
                issues.push(issue("SOURCE_LANGUAGE_CONFLICT", Some(role), source_id));
            }
            continue;
        }
        if field.raw_value.contains("[sound:")
            || field.raw_value.contains("{{")
            || field.raw_value.to_ascii_lowercase().contains("<ruby")
        {
            issues.push(issue("SOURCE_RICH_FIELD_REVIEW", Some(role), source_id));
            continue;
        }
        let derived = visible_text(&field.raw_value);
        if derived != field.raw_value.trim() {
            issues.push(issue("SOURCE_HTML_TEXT_REVIEW", Some(role), source_id));
        }
        if !derived.is_empty() {
            values.insert(role.clone(), derived);
        }
    }
    let text = |role: &str| values.get(role).cloned().unwrap_or_default();
    let content = match mapping.kind {
        SourceKind::Vocabulary => {
            json!({"kind":"vocabulary","body":{"expression":text("expression"),"reading":text("reading"),"pronunciation":text("pronunciation"),"meaning":text("meaning"),"sense_key":text("sense_key"),"usage":text("usage"),"kanji":text("kanji"),"production_prompt":text("production_prompt"),"spelling_prompt":text("spelling_prompt"),"examples":[],"dictionary":[]}})
        }
        SourceKind::Grammar => {
            json!({"kind":"grammar","body":{"pattern":text("pattern"),"meaning":text("meaning"),"formation":text("formation"),"use_key":text("use_key"),"usage":text("usage"),"recognition_prompt":text("recognition_prompt"),"exercise_prompt":text("exercise_prompt"),"exercise_answer":text("exercise_answer"),"examples":[]}})
        }
    };
    let task = match mapping.kind {
        SourceKind::Vocabulary => "comprehension",
        SourceKind::Grammar => "recognition",
    };
    let bytes=canonical::bytes(&json!({"schema_version":2,"id":uuid::Uuid::new_v4(),"target_language":target,"explanation_language":explanation,"content":content,"requested_tasks":[task],"tags":capture.captured.source.tags,"personal_notes":text("personal_notes"),"source_summary":text("source")})).map_err(|e|e.to_string())?;
    let mut doc = LearningDocument::from_json(&bytes).map_err(|e| e.to_string())?;
    doc.sources.push(capture.captured.source.clone());
    doc.archives.push(capture.captured.archive.clone());
    for (field, claim) in values {
        let language = if [
            "expression",
            "reading",
            "pronunciation",
            "pattern",
            "kanji",
            "production_prompt",
            "spelling_prompt",
            "recognition_prompt",
            "exercise_prompt",
            "exercise_answer",
        ]
        .contains(&field.as_str())
        {
            doc.target_language.clone()
        } else {
            doc.explanation_language.clone()
        };
        doc.evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field,
            provenance: Provenance::Source,
            source_id: Some(source_id),
            region_id: None,
            language,
            claim,
            source_url: None,
            ambiguous: false,
        });
    }
    doc.issues = issues;
    doc.issues = validation::validate(&doc);
    Ok(doc)
}

/// Persist a source-derived draft only; enrichment and native apply eligibility are separate.
/// Assets precede the immutable revision, so interrupted publication cannot expose dangling links.
pub fn publish_capture_draft(
    capture: &RevampCapture,
    settings: &Effective,
    purpose: &str,
    environment: &BTreeMap<String, String>,
) -> Result<crate::Prepared, String> {
    let frozen = crate::freeze_settings(settings, environment)?;
    let document = stage_document(capture, settings, purpose)?;
    let input_digest = document.semantic_digest().map_err(|e| e.to_string())?;
    let source_digest =
        canonical::digest("source-capture", &document.sources).map_err(|e| e.to_string())?;
    let plan = linguist_core::records::PlanRevision {
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: frozen,
        binding: None,
        source_digest,
        documents: vec![document],
        rendered: vec![],
        review_decisions: vec![],
    };
    let root = std::path::Path::new(
        plan.settings.values["storage.state_dir"]
            .as_str()
            .ok_or("REVAMP_STATE_PATH_MISSING")?,
    );
    let mut store = linguist_store::Store::open(root)?;
    for (expected, bytes) in &capture.captured.assets {
        if store.publish_asset(bytes, 100 * 1024 * 1024)? != *expected {
            return Err("REVAMP_CAPTURE_ASSET_CONFLICT".into());
        }
    }
    let digest = store.publish_revision(&plan)?;
    Ok(crate::Prepared {
        document_id: plan.documents[0].id,
        input_digest,
        schema_version: 2,
        plan_id: plan.id,
        revision: 1,
        digest,
        ready: false,
        issues: plan.documents[0].issues.clone(),
        original_input_digest: capture.captured.source.digest.clone(),
        apply_eligible: false,
        duplicate_check_performed: false,
    })
}

/// Initial CLI preparation path. Explicitly selected unavailable enrichment never gets skipped.
pub fn prepare_source_revamp(
    client: &linguist_anki::Client,
    settings: &Effective,
    purpose: &str,
    note_id: &str,
    environment: &BTreeMap<String, String>,
) -> Result<crate::Prepared, String> {
    crate::authored_capabilities(settings)?;
    if settings.values["dictionary.provider"] != "authored" {
        return Err("CAPABILITY_UNAVAILABLE: revamp dictionary integration is pending; select dictionary.provider=authored for a source draft".into());
    }
    if purpose == "japanese_vocab" && settings.values["kanji.enabled"] == true {
        return Err("CAPABILITY_UNAVAILABLE: revamp kanji enrichment is pending; select kanji.enabled=false for a source draft".into());
    }
    crate::freeze_settings(settings, environment)?;
    let capture = crate::source_archive::capture_for_revamp(client, settings, purpose, note_id)?;
    publish_capture_draft(&capture, settings, purpose, environment)
}
