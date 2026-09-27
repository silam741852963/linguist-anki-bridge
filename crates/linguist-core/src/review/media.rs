use crate::{
    Issue, LearningDocument, Provenance,
    records::{MediaOwner, MediaRole, ReviewChoice},
};

pub(super) fn role_filename(
    digest: &str,
    mime: &str,
    original: &str,
    role: MediaRole,
) -> Option<String> {
    if role == MediaRole::Archive {
        return Some(original.into());
    }
    let extension = match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        _ => return None,
    };
    Some(format!("lab_{digest}.{extension}"))
}

/// Evidence shape checks do not decode bytes. The application must verify the
/// archived bytes before publishing a rendering-role decision.
pub(crate) fn source_media_matches(
    doc: &LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
    selected: bool,
) -> bool {
    let ReviewChoice::SourceMediaRole {
        source_id,
        asset_digest,
        original_filename,
        evidence_id,
        role,
        attribution,
        license,
    } = choice
    else {
        return false;
    };
    if issue.stage != "capture"
        || issue.field.as_ref() != Some(original_filename)
        || issue.source_refs != vec![source_id.to_string()]
        || !matches!(
            issue.code.as_str(),
            "SOURCE_MEDIA_CONTENT_REVIEW"
                | "SOURCE_MEDIA_FORMAT_REVIEW"
                | "SOURCE_AUDIO_COMPLETENESS_REVIEW"
        )
        || (issue.code != "SOURCE_MEDIA_CONTENT_REVIEW" && *role != MediaRole::Archive)
        || attribution.trim().is_empty()
        || attribution.chars().count() > 1000
        || attribution.chars().any(char::is_control)
        || license.as_ref().is_some_and(|v| {
            v.trim().is_empty() || v.chars().count() > 1000 || v.chars().any(char::is_control)
        })
    {
        return false;
    }
    if !doc
        .sources
        .iter()
        .any(|s| s.id == *source_id && s.media_refs.contains(original_filename))
        || !doc
            .archives
            .iter()
            .any(|a| a.source_id == *source_id && a.asset_digests.contains(asset_digest))
    {
        return false;
    }
    let assets: Vec<_> = doc
        .media
        .iter()
        .filter(|a| {
            a.source_id == Some(*source_id)
                && a.digest == *asset_digest
                && a.original_filename.as_ref() == Some(original_filename)
        })
        .collect();
    if assets.len() != 1 {
        return false;
    }
    let asset = assets[0];
    if asset.owner != MediaOwner::Source
        || (selected
            && (asset.role != *role
                || asset.attribution != *attribution
                || asset.license != *license
                || role_filename(asset_digest, &asset.mime, original_filename, *role).as_ref()
                    != Some(&asset.filename)))
    {
        return false;
    }
    let evidence: Vec<_> = doc
        .evidence
        .iter()
        .filter(|e| e.id == *evidence_id)
        .collect();
    if evidence.len() != 1 {
        return false;
    }
    let e = evidence[0];
    if e.source_id != Some(*source_id)
        || e.provenance != Provenance::Source
        || e.field != "media_format"
    {
        return false;
    }
    let Ok(receipt) = crate::canonical::parse::<serde_json::Value>(e.claim.as_bytes()) else {
        return false;
    };
    if receipt["asset_digest"] != *asset_digest || receipt["filename"] != *original_filename {
        return false;
    }
    if *role == MediaRole::Archive {
        return receipt.get("inspection").is_some() || receipt.get("failure").is_some();
    }
    if e.ambiguous
        || receipt.get("failure").is_some()
        || receipt["inspection"]["mime"] != asset.mime
    {
        return false;
    }
    let inspection = &receipt["inspection"];
    match role {
        MediaRole::Picture => {
            matches!(
                asset.mime.as_str(),
                "image/jpeg" | "image/png" | "image/webp" | "image/gif"
            ) && ["width", "height", "decoded_units", "decoded_bytes"]
                .iter()
                .all(|key| inspection[key].as_u64().is_some_and(|n| n > 0))
        }
        MediaRole::Audio => {
            matches!(
                asset.mime.as_str(),
                "audio/mpeg" | "audio/wav" | "audio/ogg"
            ) && inspection["container_extent_verified"] == true
                && inspection["stream_end_observed"] == true
                && ["sample_rate", "channels", "decoded_frames"]
                    .iter()
                    .all(|key| inspection[key].as_u64().is_some_and(|n| n > 0))
        }
        MediaRole::Archive => unreachable!(),
    }
}
