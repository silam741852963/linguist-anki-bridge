//! Review preflight verifies source bytes before the caller publishes a child revision.
pub mod batch;
pub mod inspection;
use linguist_core::{
    records::{MediaRole, PlanRevision, ReviewChoice},
    review::{ResolutionRequest, ResolutionResult},
};

pub fn resolve(
    store: &linguist_store::Store,
    base: &PlanRevision,
    request: &ResolutionRequest,
    created_at: String,
) -> Result<ResolutionResult, String> {
    let texts: Vec<&str> = match &request.choice {
        ReviewChoice::Cue { text, .. } => vec![text],
        ReviewChoice::Exercise { prompt, answer } => vec![prompt, answer],
        _ => vec![],
    };
    if !texts.is_empty() {
        let key = "input.max_record_chars";
        let value = base
            .settings
            .values
            .get(key)
            .ok_or("REVIEW_SETTING_MISSING")?;
        linguist_config::Registry::builtin().validate_value(key, value)?;
        if texts
            .iter()
            .any(|text| text.chars().count() as u64 > value.as_u64().unwrap())
        {
            return Err("REVIEW_INPUT_LIMIT".into());
        }
    }
    // Validate identities, exact evidence and decision applicability without effects.
    let result =
        linguist_core::review::resolve(base, request, created_at).map_err(|e| e.to_string())?;
    if let ReviewChoice::SourceMediaRole {
        source_id,
        asset_digest,
        original_filename,
        evidence_id,
        role,
        ..
    } = &request.choice
    {
        let settings = linguist_config::Effective {
            version: base.settings.version,
            values: base.settings.values.clone(),
            provenance: base.settings.provenance.clone(),
            fingerprint: base.settings.fingerprint.clone(),
            semantic_fingerprint: base.settings.semantic_fingerprint.clone(),
            execution_fingerprint: base.settings.execution_fingerprint.clone(),
        };
        crate::media::validate_settings(&settings)?;
        crate::audio::validate_settings(&settings)?;
        for key in ["images.existing_policy", "audio.provider"] {
            linguist_config::Registry::builtin().validate_value(
                key,
                settings.values.get(key).ok_or("REVIEW_SETTING_MISSING")?,
            )?;
        }
        if *role == MediaRole::Picture
            && settings
                .values
                .get("images.existing_policy")
                .and_then(serde_json::Value::as_str)
                == Some("omit_reference")
            || *role == MediaRole::Audio
                && settings
                    .values
                    .get("audio.provider")
                    .and_then(serde_json::Value::as_str)
                    == Some("disabled")
        {
            return Err("REVIEW_MEDIA_FROZEN_POLICY_CONFLICT".into());
        }
        let document = base
            .documents
            .iter()
            .find(|d| d.id == request.document_id)
            .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?;
        let asset = document
            .media
            .iter()
            .find(|a| {
                a.source_id == Some(*source_id)
                    && a.digest == *asset_digest
                    && a.original_filename.as_ref() == Some(original_filename)
            })
            .ok_or("REVIEW_MEDIA_ASSET_MISSING")?;
        let cap = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
        let bytes = store.asset(asset_digest, cap)?;
        if bytes.len() as u64 != asset.size_bytes {
            return Err("REVIEW_MEDIA_SIZE_CONFLICT".into());
        }
        if *role != MediaRole::Archive {
            let inspection =
                crate::media::inspect_source_media(&bytes, &settings).map_err(|e| e.to_string())?;
            let evidence = document
                .evidence
                .iter()
                .find(|e| e.id == *evidence_id)
                .ok_or("REVIEW_MEDIA_EVIDENCE_MISSING")?;
            let receipt: serde_json::Value =
                linguist_core::canonical::parse(evidence.claim.as_bytes())
                    .map_err(|e| e.to_string())?;
            if receipt["inspection"]
                != serde_json::to_value(inspection)
                    .map_err(|_| "REVIEW_MEDIA_INSPECTION_ENCODING")?
            {
                return Err("REVIEW_MEDIA_INSPECTION_CONFLICT".into());
            }
        }
    }
    Ok(result)
}

/// Payload-free summary of a plan revision for filtered listing.
pub fn summarize(plan: &linguist_core::records::PlanRevision) -> Result<serde_json::Value, String> {
    use linguist_core::{LearningContent, validation::Severity};
    let mut errors = 0usize;
    let mut reviews = 0usize;
    let mut warnings = 0usize;
    let mut workflows = std::collections::BTreeSet::new();
    for document in &plan.documents {
        for issue in linguist_core::validation::validate(document) {
            match issue.severity {
                Severity::Error => errors += 1,
                Severity::Review => reviews += 1,
                Severity::Warning => warnings += 1,
            }
        }
        let revamp = document
            .sources
            .iter()
            .any(|s| s.kind == "anki_read_capture_v2");
        workflows.insert(match (&document.content, revamp) {
            (LearningContent::Vocabulary(_), false) => "vocab_add",
            (LearningContent::Vocabulary(_), true) => "vocab_revamp",
            (LearningContent::Grammar(_), false) => "grammar_add",
            (LearningContent::Grammar(_), true) => "grammar_revamp",
        });
    }
    let status = if errors > 0 {
        "invalid"
    } else if reviews > 0 || plan.documents.is_empty() {
        "needs_review"
    } else {
        "ready"
    };
    let workflow = match workflows.len() {
        1 => *workflows.iter().next().unwrap(),
        _ => "mixed",
    };
    Ok(serde_json::json!({
        "plan_id": plan.id,
        "revision": plan.revision,
        "digest": plan.approval_digest().map_err(|e| e.to_string())?,
        "status": status,
        "workflow": workflow,
        "items": plan.documents.len(),
        "errors": errors,
        "reviews": reviews,
        "warnings": warnings,
    }))
}
