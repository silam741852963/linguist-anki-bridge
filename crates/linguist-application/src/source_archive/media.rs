//! Original media bytes only; MIME/decoder validation and render selection are separate.
use super::CapturedSource;
use linguist_core::canonical;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaReceipt {
    pub filename: String,
    pub digest: Option<String>,
    pub size_bytes: Option<u64>,
}

/// Require one explicit present/missing observation for every discovered local filename.
/// Failure leaves the original capture unchanged; no partial receipt is installed.
pub fn attach_original_media(
    capture: &mut CapturedSource,
    observations: BTreeMap<String, Option<Vec<u8>>>,
    max_asset_bytes: u64,
    max_total_bytes: u64,
) -> Result<(), String> {
    if max_asset_bytes == 0
        || max_total_bytes == 0
        || max_asset_bytes > 100 * 1024 * 1024
        || max_total_bytes > 100 * 1024 * 1024
    {
        return Err("SOURCE_MEDIA_LIMIT_INVALID".into());
    }
    if observations.keys().collect::<Vec<_>>()
        != capture.source.media_refs.iter().collect::<Vec<_>>()
    {
        return Err("SOURCE_MEDIA_SELECTION_CONFLICT".into());
    }
    let mut total = capture
        .assets
        .values()
        .map(|bytes| bytes.len() as u64)
        .sum::<u64>();
    let mut assets = BTreeMap::new();
    let mut receipts = Vec::new();
    for (filename, observation) in observations {
        let (digest, size_bytes) = match observation {
            None => (None, None),
            Some(bytes) => {
                total = total
                    .checked_add(bytes.len() as u64)
                    .ok_or("SOURCE_MEDIA_LIMIT")?;
                if bytes.len() as u64 > max_asset_bytes || total > max_total_bytes {
                    return Err("SOURCE_MEDIA_LIMIT".into());
                }
                let digest = canonical::asset_digest(&bytes);
                let size = bytes.len() as u64;
                assets.insert(digest.clone(), bytes);
                (Some(digest), Some(size))
            }
        };
        receipts.push(MediaReceipt {
            filename,
            digest,
            size_bytes,
        });
    }
    let old_digest = capture.source.digest.clone();
    let mut manifest: serde_json::Value = canonical::parse(
        capture
            .assets
            .get(&old_digest)
            .ok_or("SOURCE_MEDIA_MANIFEST_MISSING")?,
    )
    .map_err(|_| "SOURCE_MEDIA_MANIFEST_INVALID")?;
    if manifest.get("media").is_some() {
        return Err("SOURCE_MEDIA_ALREADY_CAPTURED".into());
    }
    manifest["media_bytes_archived"] =
        serde_json::json!(receipts.iter().all(|entry| entry.digest.is_some()));
    manifest["media_content_verified"] = serde_json::json!(false);
    manifest["media"] =
        serde_json::to_value(receipts).map_err(|_| "SOURCE_MEDIA_MANIFEST_INVALID")?;
    let bytes = canonical::bytes(&manifest).map_err(|_| "SOURCE_MEDIA_MANIFEST_INVALID")?;
    let final_total = total
        .checked_sub(capture.assets[&old_digest].len() as u64)
        .and_then(|total| total.checked_add(bytes.len() as u64))
        .ok_or("SOURCE_MEDIA_LIMIT")?;
    if final_total > max_total_bytes {
        return Err("SOURCE_MEDIA_LIMIT".into());
    }
    let digest = canonical::asset_digest(&bytes);
    assets.insert(digest.clone(), bytes);
    capture.assets.remove(&old_digest);
    capture.assets.extend(assets);
    capture.source.digest = digest.clone();
    capture.archive.digest = digest;
    capture.archive.asset_digests = capture.assets.keys().cloned().collect();
    Ok(())
}
