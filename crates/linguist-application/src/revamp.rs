//! Conservative source-to-document staging; source ownership/history still requires native review.
use crate::{mapping::SourceKind, source_archive::RevampCapture};
use linguist_config::Effective;
use linguist_core::{
    LearningDocument, Provenance, canonical,
    records::{Evidence, EvidenceTarget, SelectionInput, SelectionReceipt},
    validation::{self, Issue, Severity},
};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SourceExample {
    sentence: String,
    translation: String,
}
fn source_examples(raw: &str, translation_required: bool) -> Result<Vec<SourceExample>, String> {
    let pairs: Vec<SourceExample> =
        canonical::parse(raw.as_bytes()).map_err(|_| "SOURCE_EXAMPLES_SCHEMA_REVIEW")?;
    if pairs.len() > 1000 {
        return Err("SOURCE_EXAMPLES_LIMIT_REVIEW".into());
    }
    if pairs.iter().any(|pair| {
        pair.sentence.trim().is_empty()
            || (translation_required && pair.translation.trim().is_empty())
            || [&pair.sentence, &pair.translation]
                .iter()
                .any(|text| text.contains('<') || text.contains("[sound:") || text.contains("{{"))
    }) {
        return Err("SOURCE_EXAMPLES_CONTENT_REVIEW".into());
    }
    Ok(pairs)
}

fn strip_sound_markers(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find("[sound:") {
        out.push_str(&rest[..start]);
        match rest[start..].find(']') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                // Malformed marker: keep it verbatim (its media issue is separate).
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
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
        || archive.original_text != source.text
        || source.template_manifest.as_ref().is_some_and(|digest| {
            !archive.asset_digests.contains(digest) || !capture.captured.assets.contains_key(digest)
        })
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
    if source
        .template_manifest
        .as_ref()
        .is_some_and(|digest| manifest["template_manifest"].as_str() != Some(digest.as_str()))
    {
        return Err("REVAMP_CAPTURE_MANIFEST_CONFLICT".into());
    }
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
    let rebuilt_manifest: serde_json::Value =
        canonical::parse(&rebuilt.assets[&rebuilt.source.digest])
            .map_err(|_| "REVAMP_CAPTURE_MANIFEST_INVALID")?;
    if rebuilt.source.fields != source.fields
        || rebuilt.source.tags != source.tags
        || rebuilt.source.model_manifest != source.model_manifest
        || source.template_manifest.is_some()
            && rebuilt.source.template_manifest != source.template_manifest
        || rebuilt.source.location != source.location
        || rebuilt.source.media_refs != source.media_refs
        || rebuilt_manifest["media_discovery"] != manifest["media_discovery"]
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
    let model_name = model["name"]
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
    if manifest.get("source_task_mapping").is_some()
        && manifest["source_task_mapping"]
            != settings.values[&format!("purposes.{purpose}.card_tasks")]
    {
        return Err("REVAMP_TASK_MAPPING_CONFLICT".into());
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
    for field in &mapping.unmapped_fields {
        if source.fields[field]
            .chars()
            .any(|character| !character.is_whitespace())
        {
            issues.push(issue(
                "SOURCE_UNMAPPED_FIELD_REVIEW",
                Some(field),
                source_id,
            ));
        }
    }
    if let Some(discovery) = manifest["media_discovery"]["issues"].as_array() {
        for entry in discovery {
            let field = entry["field"]
                .as_str()
                .ok_or("REVAMP_MEDIA_DISCOVERY_INVALID")?;
            let code = entry["code"]
                .as_str()
                .ok_or("REVAMP_MEDIA_DISCOVERY_INVALID")?;
            let mut review = issue("SOURCE_MEDIA_DISCOVERY_REVIEW", Some(field), source_id);
            review.message =
                format!("Media syntax requires review: {code}. Original field remains archived.");
            issues.push(review);
        }
    }
    let mut values = BTreeMap::new();
    let mut example_candidates = Vec::new();
    let mut task_candidates = Vec::new();
    for (role, field) in &mapping.roles {
        if field.raw_value.trim().is_empty() {
            continue;
        }
        if mapping.shared_fields.contains_key(&field.source_field) {
            issues.push(issue("SOURCE_COMBINED_FIELD_REVIEW", Some(role), source_id));
            continue;
        }
        if role == "examples" {
            match source_examples(
                &field.raw_value,
                target.split('-').next() != explanation.split('-').next(),
            ) {
                Ok(pairs) => {
                    example_candidates = pairs;
                    issues.push(issue("SOURCE_EXAMPLES_REVIEW", Some(role), source_id));
                }
                Err(code) => issues.push(issue(&code, Some(role), source_id)),
            }
            continue;
        }
        let task = match role.as_str() {
            "enable_production" => Some("production"),
            "enable_spelling" => Some("spelling"),
            "enable_application" => Some("application"),
            _ => None,
        };
        if let Some(task) = task {
            // A mapped nonempty enable field is a task candidate, not proof of
            // a native template/card relationship. Never discard it to render.
            task_candidates.push(task);
            values.insert(role.clone(), field.raw_value.clone());
            issues.push(issue("SOURCE_TASK_MAPPING_REVIEW", Some(role), source_id));
            continue;
        }
        if ["picture", "audio"].contains(&role.as_str()) {
            // A field holding only discovered media references needs no extra
            // review: each referenced file is archived and gets its own
            // SOURCE_MEDIA_CONTENT_REVIEW (role decision) or missing-media review.
            let referenced = manifest["media_discovery"]["references"]
                .as_array()
                .is_some_and(|refs| {
                    refs.iter()
                        .any(|r| r["field"].as_str() == Some(field.source_field.as_str()))
                });
            let text_only = visible_text(&strip_sound_markers(&field.raw_value));
            if !referenced || !text_only.trim().is_empty() {
                issues.push(issue(
                    "SOURCE_STRUCTURED_ROLE_REVIEW",
                    Some(role),
                    source_id,
                ));
            }
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
        if field.raw_value.contains("{{") || field.raw_value.to_ascii_lowercase().contains("<ruby")
        {
            issues.push(issue("SOURCE_RICH_FIELD_REVIEW", Some(role), source_id));
            continue;
        }
        // `[sound:...]` markers are media references (captured separately and
        // given a role by their own review); the remaining visible text is a
        // derived candidate that needs the same review as HTML-derived text.
        let without_sound = strip_sound_markers(&field.raw_value);
        let derived = visible_text(&without_sound);
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
    let mut tasks = vec![task];
    tasks.extend(task_candidates);
    let bytes=canonical::bytes(&json!({"schema_version":2,"id":uuid::Uuid::new_v4(),"target_language":target,"explanation_language":explanation,"content":content,"requested_tasks":tasks,"tags":capture.captured.source.tags,"personal_notes":text("personal_notes"),"source_summary":text("source")})).map_err(|e|e.to_string())?;
    let mut doc = LearningDocument::from_json(&bytes).map_err(|e| e.to_string())?;
    doc.sources.push(capture.captured.source.clone());
    doc.archives.push(capture.captured.archive.clone());
    if let Some(media) = manifest.get("media") {
        crate::media::validate_settings(settings)?;
        crate::audio::validate_settings(settings)?;
        let receipts: Vec<crate::source_archive::media::MediaReceipt> =
            serde_json::from_value(media.clone()).map_err(|_| "REVAMP_MEDIA_MANIFEST_INVALID")?;
        if manifest["media_content_verified"] != false
            || manifest["media_bytes_archived"]
                != json!(receipts.iter().all(|entry| entry.digest.is_some()))
        {
            return Err("REVAMP_MEDIA_MANIFEST_INVALID".into());
        }
        linguist_config::Registry::builtin().validate_value(
            "media.max_asset_mb",
            settings
                .values
                .get("media.max_asset_mb")
                .ok_or("REVAMP_SETTING_MISSING")?,
        )?;
        let max_media_bytes = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
        if receipts
            .iter()
            .map(|entry| &entry.filename)
            .collect::<Vec<_>>()
            != source.media_refs.iter().collect::<Vec<_>>()
        {
            return Err("REVAMP_MEDIA_MANIFEST_CONFLICT".into());
        }
        for entry in receipts {
            match (entry.digest, entry.size_bytes) {
                (None, None) => issues.push(issue(
                    "SOURCE_MEDIA_MISSING_REVIEW",
                    Some(&entry.filename),
                    source_id,
                )),
                (Some(digest), Some(size)) => {
                    if size > max_media_bytes {
                        return Err("REVAMP_MEDIA_LIMIT".into());
                    }
                    let bytes = capture
                        .captured
                        .assets
                        .get(&digest)
                        .filter(|_| archive.asset_digests.contains(&digest))
                        .ok_or("REVAMP_CAPTURE_ASSET_MISSING")?;
                    if bytes.len() as u64 != size {
                        return Err("REVAMP_MEDIA_MANIFEST_CONFLICT".into());
                    }
                    if size == 0 {
                        issues.push(issue(
                            "SOURCE_MEDIA_EMPTY_REVIEW",
                            Some(&entry.filename),
                            source_id,
                        ));
                    } else {
                        let inspection = crate::media::inspect_source_media(bytes, settings);
                        let mime = match inspection {
                            Ok(inspection) => {
                                if inspection.requires_audio_completeness_review() {
                                    let mut review = issue(
                                        "SOURCE_AUDIO_COMPLETENESS_REVIEW",
                                        Some(&entry.filename),
                                        source_id,
                                    );
                                    review.message = "Audio packets decoded, but complete container extent is unverified. Inspect the original recording for truncation before assigning a rendering role.".into();
                                    issues.push(review);
                                }
                                doc.evidence.push(Evidence {
                                    id: uuid::Uuid::new_v4(),
                                    field: "media_format".into(),
                                    provenance: Provenance::Source,
                                    source_id: Some(source_id),
                                    region_id: None,
                                    target: Some(EvidenceTarget::MediaAsset {
                                        digest: digest.clone(),
                                    }),
                                    source_span: None,
                                    language: doc.target_language.clone(),
                                    claim: serde_json::to_string(&json!({
                                        "asset_digest": digest,
                                        "filename": entry.filename,
                                        "inspection": inspection,
                                        "scope": "decoded media buffers; container extent is separately reported for audio; no authorship, rendering-role, hard memory or deadline certification"
                                    })).map_err(|_| "REVAMP_MEDIA_INSPECTION_ENCODING")?,
                                    source_url: None,
                                    ambiguous: false,
                                });
                                inspection.mime().to_owned()
                            }
                            Err(failure) => {
                                // Failed inspection is recoverable evidence, not a success receipt.
                                doc.evidence.push(Evidence {
                                    id: uuid::Uuid::new_v4(),
                                    field: "media_format".into(),
                                    provenance: Provenance::Source,
                                    source_id: Some(source_id),
                                    region_id: None,
                                    target: Some(EvidenceTarget::MediaAsset {
                                        digest: digest.clone(),
                                    }),
                                    source_span: None,
                                    language: doc.target_language.clone(),
                                    claim: serde_json::to_string(&json!({
                                        "asset_digest": digest,
                                        "filename": entry.filename,
                                        "failure": failure,
                                        "available_decoders": ["image/0.25.10", "symphonia/0.6.1"],
                                        "scope": "media inspection failed; original bytes retained; no verified MIME or rendering role"
                                    })).map_err(|_| "REVAMP_MEDIA_INSPECTION_ENCODING")?,
                                    source_url: None,
                                    ambiguous: true,
                                });
                                let mut review = issue(
                                    "SOURCE_MEDIA_FORMAT_REVIEW",
                                    Some(&entry.filename),
                                    source_id,
                                );
                                review.message = format!(
                                    "{failure}. {} Original bytes remain archived; no rendering role is authorized.",
                                    failure.guidance()
                                );
                                issues.push(review);
                                "application/octet-stream".into()
                            }
                        };
                        doc.media.push(linguist_core::records::MediaAsset {
                            digest,
                            filename: entry.filename.clone(),
                            original_filename: Some(entry.filename.clone()),
                            size_bytes: size,
                            mime,
                            owner: linguist_core::records::MediaOwner::Source,
                            role: linguist_core::records::MediaRole::Archive,
                            source_id: Some(source_id),
                            attribution:
                                "Original source media; authorship and rendering role unverified."
                                    .into(),
                            license: None,
                        });
                        issues.push(issue(
                            "SOURCE_MEDIA_CONTENT_REVIEW",
                            Some(&entry.filename),
                            source_id,
                        ));
                    }
                }
                _ => return Err("REVAMP_MEDIA_MANIFEST_INVALID".into()),
            }
        }
    }
    for pair in example_candidates {
        let evidence_id = uuid::Uuid::new_v4();
        let example_index = match &doc.content {
            linguist_core::LearningContent::Vocabulary(v) => v.examples.len(),
            linguist_core::LearningContent::Grammar(g) => g.examples.len(),
        };
        doc.evidence.push(Evidence {
            id: evidence_id,
            field: "examples".into(),
            provenance: Provenance::Source,
            source_id: Some(source_id),
            region_id: None,
            target: Some(EvidenceTarget::Example {
                index: example_index,
            }),
            source_span: None,
            language: doc.target_language.clone(),
            claim: serde_json::to_string(&pair).map_err(|_| "REVAMP_EXAMPLES_ENCODING")?,
            source_url: None,
            ambiguous: false,
        });
        let example = linguist_core::Example {
            sentence: pair.sentence,
            translation: pair.translation,
            provenance: Provenance::Source,
            evidence_ids: vec![evidence_id],
        };
        match &mut doc.content {
            linguist_core::LearningContent::Vocabulary(v) => v.examples.push(example),
            linguist_core::LearningContent::Grammar(g) => g.examples.push(example),
        }
    }
    for (field, claim) in values {
        let target = (field == "formation").then_some(EvidenceTarget::GrammarFormation);
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
            target,
            source_span: None,
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
    publish_capture_drafts(
        std::slice::from_ref(capture),
        settings,
        purpose,
        environment,
    )?
    .into_iter()
    .next()
    .ok_or_else(|| "REVAMP_CAPTURE_MISSING".into())
}

/// Validate every source before creating state and publish the batch as one revision.
pub fn publish_capture_drafts(
    captures: &[RevampCapture],
    settings: &Effective,
    purpose: &str,
    environment: &BTreeMap<String, String>,
) -> Result<Vec<crate::Prepared>, String> {
    publish_selected_captures(captures, settings, purpose, environment, None)
}
fn publish_selected_captures(
    captures: &[RevampCapture],
    settings: &Effective,
    purpose: &str,
    environment: &BTreeMap<String, String>,
    selection: Option<SelectionReceipt>,
) -> Result<Vec<crate::Prepared>, String> {
    if captures.is_empty() {
        return Err("REVAMP_CAPTURE_MISSING".into());
    }
    let registry = linguist_config::Registry::builtin();
    for key in ["selection.max_notes", "input.max_file_mb"] {
        registry.validate_value(
            key,
            settings.values.get(key).ok_or("REVAMP_SETTING_MISSING")?,
        )?;
    }
    let note_limit = selection
        .as_ref()
        .and_then(|receipt| receipt.command_limit)
        .unwrap_or(settings.values["selection.max_notes"].as_u64().unwrap());
    if captures.len() as u64 > note_limit {
        return Err("REVAMP_SELECTION_LIMIT".into());
    }
    let limit = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    let mut total = 0u64;
    for bytes in captures
        .iter()
        .flat_map(|capture| capture.captured.assets.values())
    {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or("REVAMP_BATCH_ARCHIVE_LIMIT")?;
        if total > limit {
            return Err("REVAMP_BATCH_ARCHIVE_LIMIT".into());
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for capture in captures {
        if !ids.insert(&capture.captured.source.location) {
            return Err("REVAMP_SELECTION_DUPLICATE".into());
        }
    }
    let frozen = crate::freeze_settings(settings, environment)?;
    let mut documents = captures
        .iter()
        .map(|capture| stage_document(capture, settings, purpose))
        .collect::<Result<Vec<_>, _>>()?;
    // Source images are inspected before the plan exists, so generation can
    // depend on OCR evidence and failures leave no partial state.
    let captured_assets: BTreeMap<String, Vec<u8>> = captures
        .iter()
        .flat_map(|capture| capture.captured.assets.clone())
        .collect();
    let ocr_assets = crate::ocr_inspection::inspect_documents(
        &mut documents,
        &captured_assets,
        settings,
        environment,
    )?;
    let sources: Vec<_> = documents
        .iter()
        .flat_map(|document| &document.sources)
        .collect();
    let source_digest = canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    let plan = linguist_core::records::PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: frozen,
        binding: None,
        source_digest,
        selection,
        documents,
        rendered: vec![],
        review_decisions: vec![],
    };
    let root = std::path::Path::new(
        plan.settings.values["storage.state_dir"]
            .as_str()
            .ok_or("REVAMP_STATE_PATH_MISSING")?,
    );
    plan.approval_digest().map_err(|e| e.to_string())?;
    let mut store = linguist_store::Store::open(root)?;
    for (expected, bytes) in captures
        .iter()
        .flat_map(|capture| &capture.captured.assets)
        .chain(&ocr_assets)
    {
        if store.publish_asset(bytes, 100 * 1024 * 1024)? != *expected {
            return Err("REVAMP_CAPTURE_ASSET_CONFLICT".into());
        }
    }
    let digest = store.publish_revision(&plan)?;
    plan.documents
        .iter()
        .zip(captures)
        .map(|(document, capture)| {
            Ok(crate::Prepared {
                document_id: document.id,
                input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
                schema_version: 2,
                plan_id: plan.id,
                revision: 1,
                digest: digest.clone(),
                ready: false,
                issues: document.issues.clone(),
                original_input_digest: capture.captured.source.digest.clone(),
                apply_eligible: false,
                duplicate_check_performed: false,
                next_commands: vec![],
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|mut receipts| {
            let llm = settings.values["llm.enabled"] == true;
            for receipt in &mut receipts {
                receipt.refresh_next_commands(llm);
            }
            receipts
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
    prepare_source_revamps(
        client,
        settings,
        purpose,
        &[note_id.to_owned()],
        environment,
    )?
    .into_iter()
    .next()
    .ok_or_else(|| "REVAMP_CAPTURE_MISSING".into())
}

pub fn prepare_source_revamps(
    client: &linguist_anki::Client,
    settings: &Effective,
    purpose: &str,
    note_ids: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Vec<crate::Prepared>, String> {
    validate_source_revamp(settings, purpose, environment)?;
    prepare_ids(
        client,
        settings,
        purpose,
        note_ids,
        environment,
        SelectionInput::NoteIds(note_ids.to_vec()),
        None,
    )
}

pub enum SourceSelector {
    NoteIds(Vec<String>),
    Query(String),
    Deck(String),
}

/// Resolve an existing-note selection once; empty search creates no local state.
pub fn prepare_source_selection(
    client: &linguist_anki::Client,
    settings: &Effective,
    purpose: &str,
    selector: SourceSelector,
    environment: &BTreeMap<String, String>,
) -> Result<Vec<crate::Prepared>, String> {
    prepare_source_selection_limited(client, settings, purpose, selector, environment, None)
}
pub fn prepare_source_selection_limited(
    client: &linguist_anki::Client,
    settings: &Effective,
    purpose: &str,
    selector: SourceSelector,
    environment: &BTreeMap<String, String>,
    limit: Option<u64>,
) -> Result<Vec<crate::Prepared>, String> {
    validate_source_revamp(settings, purpose, environment)?;
    if limit.is_some_and(|value| !(1..=100000).contains(&value)) {
        return Err("REVAMP_SELECTION_LIMIT_INVALID".into());
    }
    let (query, receipt_input) = match selector {
        SourceSelector::NoteIds(ids) => {
            if limit.is_some() {
                return Err("REVAMP_EXPLICIT_IDS_LIMIT_CONFLICT".into());
            }
            let input = SelectionInput::NoteIds(ids.clone());
            return prepare_ids(client, settings, purpose, &ids, environment, input, None);
        }
        SourceSelector::Query(query) => (query.clone(), SelectionInput::Query(query)),
        SourceSelector::Deck(deck) => {
            let query = linguist_anki::deck_query(&deck)?;
            (query.clone(), SelectionInput::Deck { name: deck, query })
        }
    };
    if query.trim().is_empty() {
        return Err("REVAMP_QUERY_EMPTY".into());
    }
    if query.chars().count() as u64 > settings.values["input.max_record_chars"].as_u64().unwrap() {
        return Err("REVAMP_QUERY_LIMIT".into());
    }
    let ids = client.find_notes(&query)?;
    client.check_profile()?;
    if ids.is_empty() {
        return Ok(vec![]);
    }
    prepare_ids(
        client,
        settings,
        purpose,
        &ids,
        environment,
        receipt_input,
        limit,
    )
}

pub(crate) fn validate_source_revamp(
    settings: &Effective,
    purpose: &str,
    environment: &BTreeMap<String, String>,
) -> Result<(), String> {
    if !matches!(
        purpose,
        "japanese_vocab" | "english_vocab" | "japanese_grammar" | "english_grammar"
    ) {
        return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into());
    }
    crate::authored_capabilities(settings)?;
    let grammar = purpose.ends_with("_grammar");
    // Grammar has no dictionary stage; `auto` does not apply to it.
    if settings.values["dictionary.provider"] != "authored"
        && !(grammar && settings.values["dictionary.provider"] == "auto")
    {
        let supported = match purpose {
            "japanese_vocab" => matches!(
                settings.values["dictionary.provider"].as_str(),
                Some("auto" | "jisho")
            ),
            "english_vocab" => matches!(
                settings.values["dictionary.provider"].as_str(),
                Some("auto" | "wiktionary" | "cambridge")
            ),
            _ => false,
        };
        if !supported
            || settings.values["learning.explanation_language"]
                .as_str()
                .and_then(|language| language.split('-').next())
                != Some("en")
        {
            return Err("CAPABILITY_UNAVAILABLE: this revamp dictionary/language adapter is not implemented".into());
        }
    }
    crate::ocr_inspection::preflight(settings, environment)?;
    crate::freeze_settings(settings, environment)?;
    let registry = linguist_config::Registry::builtin();
    for key in [
        "selection.order",
        "selection.max_notes",
        "input.max_file_mb",
        "input.max_record_chars",
    ] {
        registry.validate_value(
            key,
            settings.values.get(key).ok_or("REVAMP_SETTING_MISSING")?,
        )?;
    }
    Ok(())
}

fn prepare_ids(
    client: &linguist_anki::Client,
    settings: &Effective,
    purpose: &str,
    note_ids: &[String],
    environment: &BTreeMap<String, String>,
    selector: SelectionInput,
    command_limit: Option<u64>,
) -> Result<Vec<crate::Prepared>, String> {
    if note_ids.is_empty()
        || note_ids.len() > 100000
        || (command_limit.is_none()
            && note_ids.len() as u64 > settings.values["selection.max_notes"].as_u64().unwrap())
    {
        return Err("REVAMP_SELECTION_LIMIT".into());
    }
    let mut selection = note_ids.to_vec();
    let mut unique = std::collections::BTreeSet::new();
    for id in &selection {
        linguist_anki::wire_id(&json!(id))?;
        if !unique.insert(id) {
            return Err("REVAMP_SELECTION_DUPLICATE".into());
        }
    }
    if settings.values["selection.order"] == "note_id" {
        selection.sort_by_key(|id| id.parse::<u64>().unwrap());
    }
    if let Some(limit) = command_limit {
        selection.truncate(limit as usize);
    }
    let mut captures = Vec::new();
    let mut total = 0u64;
    let limit = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    for id in &selection {
        let capture = crate::source_archive::capture_for_revamp(client, settings, purpose, id)?;
        for bytes in capture.captured.assets.values() {
            total = total
                .checked_add(bytes.len() as u64)
                .ok_or("REVAMP_BATCH_ARCHIVE_LIMIT")?;
        }
        if total > limit {
            return Err("REVAMP_BATCH_ARCHIVE_LIMIT".into());
        }
        captures.push(capture);
    }
    let receipt = SelectionReceipt {
        schema_version: 1,
        purpose: purpose.into(),
        selector,
        matched_note_ids: note_ids.to_vec(),
        selected_note_ids: selection,
        order: settings.values["selection.order"].as_str().unwrap().into(),
        max_notes: settings.values["selection.max_notes"].as_u64().unwrap(),
        command_limit,
    };
    let mut prepared =
        publish_selected_captures(&captures, settings, purpose, environment, Some(receipt))?;
    if settings.values["dictionary.provider"] != "authored" && !purpose.ends_with("_grammar") {
        let first = &prepared[0];
        let frozen = crate::freeze_settings(settings, environment)?;
        let root = std::path::Path::new(frozen.values["storage.state_dir"].as_str().unwrap());
        let mut store = linguist_store::Store::open_existing(root)?;
        let base = store.revision(first.plan_id, 1)?;
        let child =
            crate::dictionary::enrich_revision(&mut store, &base, None).map_err(|error| {
                format!(
                    "{error}; retained_source_plan={} revision=1 digest={}",
                    first.plan_id, first.digest
                )
            })?;
        let digest = child.approval_digest().map_err(|e| e.to_string())?;
        for (receipt, document) in prepared.iter_mut().zip(&child.documents) {
            receipt.revision = child.revision;
            receipt.digest = digest.clone();
            receipt.input_digest = document.semantic_digest().map_err(|e| e.to_string())?;
            receipt.issues = document.issues.clone();
        }
    }
    // Optional kanji, picture and audio enrichment follows OCR and dictionary
    // lookup as its own child, so a failure leaves earlier revisions intact.
    let first = &prepared[0];
    let frozen = crate::freeze_settings(settings, environment)?;
    let root = std::path::Path::new(frozen.values["storage.state_dir"].as_str().unwrap());
    let mut store = linguist_store::Store::open_existing(root)?;
    let latest = store.latest_revision(first.plan_id)?;
    let base = store.revision(first.plan_id, latest)?;
    let mut effective = settings.clone();
    effective.values = base.settings.values.clone();
    if crate::vocab::requested(&effective, &base) {
        let child = crate::vocab::enrich_revision(
            &mut store,
            &base,
            environment,
            crate::vocab::Providers::default(),
        )
        .map_err(|error| {
            format!(
                "{error}; retained_plan={} revision={} digest={}",
                first.plan_id, latest, first.digest
            )
        })?;
        let digest = child.approval_digest().map_err(|e| e.to_string())?;
        for (receipt, document) in prepared.iter_mut().zip(&child.documents) {
            receipt.revision = child.revision;
            receipt.digest = digest.clone();
            receipt.input_digest = document.semantic_digest().map_err(|e| e.to_string())?;
            receipt.issues = document.issues.clone();
        }
    }
    let llm = settings.values["llm.enabled"] == true;
    for receipt in &mut prepared {
        receipt.refresh_next_commands(llm);
    }
    Ok(prepared)
}

/// RI-04: a `SOURCE_NATIVE_HISTORY_REVIEW` resolution from companion note
/// evidence (`labInspect` kind `note_evidence`, two matching bounded reads).
/// The evidence must describe exactly the archived source (note, fields,
/// tags and the canonical model manifest); every observed card is recorded
/// with its study, and `maps` must map every card's template ordinal to a
/// requested target task. Nothing here contacts Anki or writes state.
pub fn native_history_request(
    plan: &linguist_core::records::PlanRevision,
    document_id: uuid::Uuid,
    evidence: &serde_json::Value,
    maps: &[(u16, linguist_core::document::Task)],
    actor: &str,
) -> Result<linguist_core::review::ResolutionRequest, String> {
    use linguist_core::records::{
        NativeCardEvidence, ReviewChoice, SourceTaskMap, SourceTaskMapEntry, TargetModelKind,
        native_history_digest,
    };
    use linguist_core::{document::Task, model::ManifestProjection, model::Template};
    let invalid = |code: &str| code.to_owned();
    let document = plan
        .documents
        .iter()
        .find(|doc| doc.id == document_id)
        .ok_or("PLAN_DOCUMENT_NOT_FOUND")?;
    let issue = linguist_core::validation::validate(document)
        .into_iter()
        .find(|issue| issue.code == "SOURCE_NATIVE_HISTORY_REVIEW")
        .ok_or("REVIEW_ISSUE_NOT_UNRESOLVED: no open SOURCE_NATIVE_HISTORY_REVIEW")?;
    let source_id: uuid::Uuid = issue
        .source_refs
        .first()
        .and_then(|id| id.parse().ok())
        .ok_or("REVIEW_ISSUE_SOURCE_INVALID")?;
    let source = document
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or("REVIEW_ISSUE_SOURCE_INVALID")?;
    let note_id = source
        .location
        .strip_prefix("anki_note:")
        .ok_or("REVIEW_ISSUE_SOURCE_INVALID")?;
    if evidence["schema_version"] != 1
        || evidence["note_id"].as_str() != Some(note_id)
        || evidence["repeated_reads_matched"] != true
        || evidence["review_rows_untruncated"] != true
    {
        return Err(invalid("NATIVE_HISTORY_EVIDENCE_INVALID"));
    }
    let fields: BTreeMap<String, String> =
        serde_json::from_value(evidence["note"]["fields"].clone())
            .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?;
    let mut tags: Vec<String> = serde_json::from_value(evidence["note"]["tags"].clone())
        .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?;
    let mut archived_tags = source.tags.clone();
    tags.sort();
    archived_tags.sort();
    let model = &evidence["model"];
    let model_fields: Vec<String> = serde_json::from_value(model["fields"].clone())
        .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?;
    let templates: Vec<Template> = serde_json::from_value(model["templates"].clone())
        .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?;
    let manifest = ManifestProjection::new(
        model["name"]
            .as_str()
            .ok_or("NATIVE_HISTORY_EVIDENCE_INVALID")?,
        &model_fields,
        &templates,
        model["css"]
            .as_str()
            .ok_or("NATIVE_HISTORY_EVIDENCE_INVALID")?,
    )
    .digest()
    .map_err(|e| e.to_string())?;
    if fields != source.fields || tags != archived_tags || manifest != source.model_manifest {
        return Err(invalid(
            "NATIVE_HISTORY_SOURCE_CONFLICT: the live note no longer matches the captured source; prepare again",
        ));
    }
    let mut cards = Vec::new();
    for card in evidence["cards"]
        .as_array()
        .ok_or("NATIVE_HISTORY_EVIDENCE_INVALID")?
    {
        let reviews = card["reviews"]
            .as_array()
            .ok_or("NATIVE_HISTORY_EVIDENCE_INVALID")?;
        let number = |key: &str| card[key].as_u64().ok_or("NATIVE_HISTORY_EVIDENCE_INVALID");
        let id = |key: &str| -> Result<linguist_core::AnkiId, String> {
            card[key]
                .as_str()
                .map(str::to_owned)
                .and_then(|text| linguist_core::AnkiId::try_from(text).ok())
                .ok_or_else(|| "NATIVE_HISTORY_EVIDENCE_INVALID".into())
        };
        cards.push(NativeCardEvidence {
            card_id: id("id")?,
            ordinal: u16::try_from(number("ordinal")?)
                .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?,
            deck_id: id("deck_id")?,
            repetitions: u32::try_from(number("repetitions")?)
                .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?,
            review_count: u32::try_from(reviews.len())
                .map_err(|_| "NATIVE_HISTORY_EVIDENCE_INVALID")?,
            history_digest: canonical::asset_digest(
                &canonical::bytes(reviews).map_err(|e| e.to_string())?,
            ),
        });
    }
    let kind = match &document.content {
        linguist_core::LearningContent::Vocabulary(_) => TargetModelKind::Vocabulary,
        linguist_core::LearningContent::Grammar(_) => TargetModelKind::Grammar,
    };
    let entries = maps
        .iter()
        .map(|(source_ordinal, task)| {
            let target_ordinal = match (kind, task) {
                (TargetModelKind::Vocabulary, Task::Comprehension) => 0,
                (TargetModelKind::Vocabulary, Task::Production) => 1,
                (TargetModelKind::Vocabulary, Task::Spelling) => 2,
                (TargetModelKind::Grammar, Task::Recognition) => 0,
                (TargetModelKind::Grammar, Task::Application) => 1,
                _ => {
                    return Err(
                        "SOURCE_TASK_MAP_INVALID: task does not belong to the target model"
                            .to_owned(),
                    );
                }
            };
            Ok(SourceTaskMapEntry {
                source_ordinal: *source_ordinal,
                target_task: *task,
                target_ordinal,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let task_map = SourceTaskMap {
        schema_version: 1,
        source_id,
        source_model_digest: source.model_manifest.clone(),
        target_model: kind,
        entries,
    };
    task_map.validate()?;
    let unmapped: Vec<u16> = cards
        .iter()
        .map(|card| card.ordinal)
        .filter(|ordinal| {
            !task_map
                .entries
                .iter()
                .any(|e| e.source_ordinal == *ordinal)
        })
        .collect();
    if !unmapped.is_empty() {
        return Err(format!(
            "NATIVE_HISTORY_CARD_UNMAPPED: map every source card's template ordinal {unmapped:?}; no card or review history is dropped"
        ));
    }
    let evidence_digest = native_history_digest(source, &cards).map_err(|e| e.to_string())?;
    Ok(linguist_core::review::ResolutionRequest {
        schema_version: 2,
        base_revision: plan.revision,
        base_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        document_id,
        issue_id: issue.id,
        input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
        actor: actor.into(),
        choice: ReviewChoice::NativeHistory {
            source_id,
            cards,
            evidence_digest,
            task_map,
        },
    })
}
