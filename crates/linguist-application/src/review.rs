//! Review preflight verifies source bytes before the caller publishes a child revision.
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
