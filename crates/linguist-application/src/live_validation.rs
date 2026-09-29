//! Time-bound read-only comparison with archived revamp sources. No native CAS claim.
use linguist_core::{
    canonical,
    records::{PlanRevision, SourceRecord},
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct LiveSource {
    pub note_id: String,
    pub document_ids: Vec<uuid::Uuid>,
    pub source_digest: String,
    pub fields_match: bool,
    pub tags_match: bool,
    pub model_match: bool,
    pub card_ids_match: bool,
    pub card_structure_match: bool,
    pub deck_membership_match: bool,
    pub card_payload_changed: bool,
    pub source_conflict: bool,
    pub repeated_reads_matched: bool,
    pub atomic_snapshot_verified: bool,
    pub native_history_verified: bool,
    pub media_bytes_rechecked: bool,
    pub media_conflict: bool,
    pub media: Vec<LiveMedia>,
}

#[derive(Debug, Serialize)]
pub struct LiveMedia {
    pub filename: String,
    pub saved_digest: Option<String>,
    pub live_digest: Option<String>,
    pub saved_size_bytes: Option<u64>,
    pub live_size_bytes: Option<u64>,
    pub matched: bool,
}

#[derive(Debug, Serialize)]
pub struct LivePage {
    pub schema_version: u16,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub plan_digest: String,
    pub total_sources: usize,
    pub after_index: u32,
    pub next_index: Option<u32>,
    pub sources: Vec<LiveSource>,
    pub all_sources_checked: bool,
    pub source_conflicts: bool,
    pub native_history_verified: bool,
    pub apply_eligible: bool,
}

fn tags(value: &Value) -> Result<BTreeSet<String>, String> {
    let items = value.as_array().ok_or("LIVE_TAGS_INVALID")?;
    if items.len() > 1000 {
        return Err("LIVE_TAGS_INVALID".into());
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or("LIVE_TAGS_INVALID".into())
        })
        .collect()
}

fn cards(value: &Value) -> Result<BTreeMap<String, Value>, String> {
    let items = value.as_array().ok_or("LIVE_CARDS_INVALID")?;
    if items.len() > 10000 {
        return Err("LIVE_CARDS_INVALID".into());
    }
    let mut result = BTreeMap::new();
    for card in items {
        let id = card["cardId"].as_str().ok_or("LIVE_CARDS_INVALID")?;
        if result.insert(id.to_owned(), card.clone()).is_some() {
            return Err("LIVE_CARDS_INVALID".into());
        }
    }
    Ok(result)
}

fn compare(
    source: &SourceRecord,
    document_ids: Vec<uuid::Uuid>,
    saved_note: &Value,
    saved_model: &Value,
    saved_cards: &Value,
    live: &linguist_anki::ReadCapture,
) -> Result<LiveSource, String> {
    let note_id = source
        .location
        .strip_prefix("anki_note:")
        .ok_or("LIVE_SOURCE_LOCATION_INVALID")?;
    if saved_note["noteId"].as_str() != Some(note_id)
        || live.note["noteId"].as_str() != Some(note_id)
    {
        return Err("LIVE_SOURCE_ID_CONFLICT".into());
    }
    let live_model = serde_json::to_value(&live.model).map_err(|_| "LIVE_MODEL_INVALID")?;
    let old_cards = cards(saved_cards)?;
    let new_cards = cards(&Value::Array(live.cards.clone()))?;
    let card_ids_match = old_cards.keys().eq(new_cards.keys());
    let card_structure_match = card_ids_match
        && old_cards.iter().all(|(id, old)| {
            let new = &new_cards[id];
            ["cardId", "note", "ord"]
                .iter()
                .all(|key| !old[*key].is_null() && !new[*key].is_null() && old[*key] == new[*key])
        });
    let deck_membership_match = card_ids_match
        && old_cards.iter().all(|(id, old)| {
            let new = &new_cards[id];
            ["deckId", "originalDeckId"]
                .iter()
                .all(|key| !old[*key].is_null() && !new[*key].is_null() && old[*key] == new[*key])
        });
    let fields_match = saved_note["fields"] == live.note["fields"];
    let tags_match = tags(&saved_note["tags"])? == tags(&live.note["tags"])?;
    let model_match = ["model", "fields", "templates", "css"]
        .iter()
        .all(|key| saved_model[*key] == live_model[*key])
        && saved_note["modelName"] == live.note["modelName"];
    let source_conflict = !(fields_match
        && tags_match
        && model_match
        && card_structure_match
        && deck_membership_match);
    Ok(LiveSource {
        note_id: note_id.into(),
        document_ids,
        source_digest: source.digest.clone(),
        fields_match,
        tags_match,
        model_match,
        card_ids_match,
        card_structure_match,
        deck_membership_match,
        card_payload_changed: old_cards != new_cards,
        source_conflict,
        repeated_reads_matched: live.repeated_reads_matched,
        atomic_snapshot_verified: false,
        native_history_verified: false,
        media_bytes_rechecked: false,
        media_conflict: false,
        media: vec![],
    })
}

fn recheck_media<M>(
    store: &linguist_store::Store,
    source: &SourceRecord,
    archived_assets: &BTreeSet<String>,
    manifest: &Value,
    max_asset_bytes: u64,
    max_total_bytes: u64,
    retrieve: &mut M,
) -> Result<(bool, Vec<LiveMedia>), String>
where
    M: FnMut(&str) -> Result<Option<linguist_anki::MediaFile>, String>,
{
    if source.media_refs.is_empty() {
        if manifest
            .get("media")
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        {
            return Err("LIVE_MEDIA_ARCHIVE_CONFLICT".into());
        }
        return Ok((true, vec![]));
    }
    let Some(raw_receipts) = manifest.get("media") else {
        return Ok((false, vec![]));
    };
    let receipts: Vec<crate::source_archive::media::MediaReceipt> =
        serde_json::from_value(raw_receipts.clone()).map_err(|_| "LIVE_MEDIA_MANIFEST_INVALID")?;
    if receipts.len() != source.media_refs.len()
        || receipts
            .iter()
            .zip(&source.media_refs)
            .any(|(receipt, expected)| receipt.filename != *expected)
    {
        return Err("LIVE_MEDIA_ARCHIVE_CONFLICT".into());
    }
    let mut total = 0u64;
    let mut observations = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        if receipt.digest.is_some() != receipt.size_bytes.is_some() {
            return Err("LIVE_MEDIA_MANIFEST_INVALID".into());
        }
        if let (Some(digest), Some(size)) = (&receipt.digest, receipt.size_bytes)
            && (!archived_assets.contains(digest)
                || store.asset(digest, max_asset_bytes)?.len() as u64 != size)
        {
            return Err("LIVE_MEDIA_ARCHIVE_CONFLICT".into());
        }
        let live = retrieve(&receipt.filename)?;
        if let Some(file) = &live {
            if file.filename != receipt.filename
                || canonical::asset_digest(&file.bytes) != file.digest
                || file.bytes.len() as u64 > max_asset_bytes
            {
                return Err("LIVE_MEDIA_READ_INVALID".into());
            }
            total = total
                .checked_add(file.bytes.len() as u64)
                .ok_or("LIVE_MEDIA_LIMIT")?;
            if total > max_total_bytes {
                return Err("LIVE_MEDIA_LIMIT".into());
            }
        }
        let live_digest = live.as_ref().map(|file| file.digest.clone());
        let live_size_bytes = live.as_ref().map(|file| file.bytes.len() as u64);
        let matched = receipt.digest == live_digest && receipt.size_bytes == live_size_bytes;
        observations.push(LiveMedia {
            filename: receipt.filename,
            saved_digest: receipt.digest,
            live_digest,
            saved_size_bytes: receipt.size_bytes,
            live_size_bytes,
            matched,
        });
    }
    Ok((true, observations))
}

/// Inspect one bounded page. The caller must recheck at apply time; this is not a lock.
pub fn inspect(
    store: &linguist_store::Store,
    plan: &PlanRevision,
    client: &linguist_anki::Client,
    after_index: u32,
    limit: u32,
) -> Result<LivePage, String> {
    let page = inspect_with(
        store,
        plan,
        after_index,
        limit,
        |note_id| client.capture_note(note_id),
        |filename| client.retrieve_media_file(filename),
    )?;
    client.check_profile()?;
    Ok(page)
}

fn inspect_with<F, M>(
    store: &linguist_store::Store,
    plan: &PlanRevision,
    after_index: u32,
    limit: u32,
    mut capture: F,
    mut retrieve_media: M,
) -> Result<LivePage, String>
where
    F: FnMut(&str) -> Result<linguist_anki::ReadCapture, String>,
    M: FnMut(&str) -> Result<Option<linguist_anki::MediaFile>, String>,
{
    if !(1..=1000).contains(&limit) {
        return Err("LIVE_VALIDATION_LIMIT_INVALID".into());
    }
    let cap = plan
        .settings
        .values
        .get("input.max_file_mb")
        .ok_or("LIVE_VALIDATION_SETTING_MISSING")?;
    linguist_config::Registry::builtin().validate_value("input.max_file_mb", cap)?;
    let cap = cap.as_u64().unwrap() * 1024 * 1024;
    let mut source_map =
        BTreeMap::<String, (SourceRecord, Vec<uuid::Uuid>, BTreeSet<String>)>::new();
    for document in &plan.documents {
        for source in document
            .sources
            .iter()
            .filter(|source| source.kind == "anki_read_capture_v2")
        {
            let note_id = source
                .location
                .strip_prefix("anki_note:")
                .ok_or("LIVE_SOURCE_LOCATION_INVALID")?;
            linguist_anki::wire_id(&serde_json::json!(note_id))?;
            let archive = document
                .archives
                .iter()
                .find(|archive| {
                    archive.source_id == source.id
                        && archive.digest == source.digest
                        && archive.original_fields == source.fields
                        && archive.asset_digests.contains(&source.digest)
                        && archive.asset_digests.contains(&source.model_manifest)
                })
                .ok_or("LIVE_SOURCE_ARCHIVE_CONFLICT")?;
            let assets: BTreeSet<_> = archive.asset_digests.iter().cloned().collect();
            if assets.len() != archive.asset_digests.len() {
                return Err("LIVE_SOURCE_ARCHIVE_CONFLICT".into());
            }
            let entry = source_map
                .entry(note_id.to_owned())
                .or_insert_with(|| (source.clone(), Vec::new(), assets.clone()));
            if entry.0 != *source || entry.2 != assets {
                return Err("LIVE_SOURCE_DUPLICATE_CONFLICT".into());
            }
            if !entry.1.contains(&document.id) {
                entry.1.push(document.id);
            }
        }
    }
    let total_sources = source_map.len();
    if after_index as usize >= total_sources && (after_index != 0 || total_sources != 0) {
        return Err("LIVE_VALIDATION_CURSOR_OUT_OF_RANGE".into());
    }
    let mut ordered: Vec<_> = source_map.into_iter().collect();
    ordered.sort_by_key(|(note_id, _)| note_id.parse::<u64>().unwrap());
    let selected: Vec<_> = ordered
        .into_iter()
        .skip(after_index as usize)
        .take(limit as usize)
        .collect();
    if selected
        .iter()
        .map(|(_, (source, _, _))| source.media_refs.len())
        .sum::<usize>()
        > 1000
    {
        return Err("LIVE_MEDIA_REFERENCE_LIMIT".into());
    }
    let max_media_asset = if selected
        .iter()
        .any(|(_, (source, _, _))| !source.media_refs.is_empty())
    {
        let setting = plan
            .settings
            .values
            .get("media.max_asset_mb")
            .ok_or("LIVE_MEDIA_SETTING_MISSING")?;
        linguist_config::Registry::builtin().validate_value("media.max_asset_mb", setting)?;
        setting.as_u64().unwrap() * 1024 * 1024
    } else {
        0
    };
    let mut sources = Vec::new();
    for (_, (source, document_ids, archived_assets)) in selected {
        let manifest: Value = canonical::parse(&store.asset(&source.digest, cap)?)
            .map_err(|_| "LIVE_SOURCE_MANIFEST_INVALID")?;
        if manifest["kind"] != "anki_read_capture_v2"
            || manifest["note_id"] != source.location.trim_start_matches("anki_note:")
        {
            return Err("LIVE_SOURCE_MANIFEST_CONFLICT".into());
        }
        let payload = |kind: &str| -> Result<Value, String> {
            let digest = manifest["payloads"][kind]
                .as_str()
                .ok_or("LIVE_SOURCE_MANIFEST_INVALID")?;
            if !archived_assets.contains(digest) {
                return Err("LIVE_SOURCE_ARCHIVE_CONFLICT".into());
            }
            let asset = store.asset(digest, cap)?;
            canonical::parse(&asset).map_err(|_| "LIVE_SOURCE_PAYLOAD_INVALID".into())
        };
        let saved_note = payload("note")?;
        let saved_model = payload("model")?;
        let saved_cards = payload("cards")?;
        if source.model_manifest != manifest["payloads"]["model"] {
            return Err("LIVE_SOURCE_MODEL_CONFLICT".into());
        }
        let saved_fields = saved_note["fields"]
            .as_object()
            .ok_or("LIVE_SOURCE_PAYLOAD_INVALID")?
            .iter()
            .map(|(key, field)| {
                field["value"]
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or("LIVE_SOURCE_PAYLOAD_INVALID".into())
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        if saved_fields != source.fields
            || tags(&saved_note["tags"])? != source.tags.iter().cloned().collect()
        {
            return Err("LIVE_SOURCE_ARCHIVE_CONFLICT".into());
        }
        let discovered = crate::capture::discover_media(&saved_fields, cap, 10000)?;
        let discovered_refs: Vec<_> = discovered
            .references
            .into_iter()
            .map(|reference| reference.filename)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if discovered_refs != source.media_refs {
            return Err("LIVE_MEDIA_ARCHIVE_CONFLICT".into());
        }
        let note_id = source.location.strip_prefix("anki_note:").unwrap();
        let live = capture(note_id)?;
        let mut compared = compare(
            &source,
            document_ids,
            &saved_note,
            &saved_model,
            &saved_cards,
            &live,
        )?;
        let (rechecked, media) = recheck_media(
            store,
            &source,
            &archived_assets,
            &manifest,
            max_media_asset,
            cap,
            &mut retrieve_media,
        )?;
        compared.media_bytes_rechecked = rechecked;
        compared.media_conflict = !rechecked || media.iter().any(|entry| !entry.matched);
        compared.source_conflict |= compared.media_conflict;
        compared.media = media;
        sources.push(compared);
    }
    let end = (after_index as usize).saturating_add(sources.len());
    let next_index = if end < total_sources {
        Some(end as u32)
    } else {
        None
    };
    Ok(LivePage {
        schema_version: 2,
        plan_id: plan.id,
        revision: plan.revision,
        plan_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        total_sources,
        after_index,
        next_index,
        source_conflicts: sources.iter().any(|source| source.source_conflict),
        sources,
        all_sources_checked: after_index == 0 && next_index.is_none(),
        native_history_verified: false,
        apply_eligible: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use linguist_anki::{ModelInspection, NamedId};
    use linguist_core::{LearningDocument, records::ResolvedSettings};
    use serde_json::json;

    fn fixture() -> (
        SourceRecord,
        Value,
        Value,
        Value,
        linguist_anki::ReadCapture,
    ) {
        let model = ModelInspection {
            model: NamedId {
                id: "12".into(),
                name: "Legacy".into(),
            },
            fields: vec!["Expression".into()],
            templates: BTreeMap::from([("Card".into(), json!({"Front":"front","Back":"back"}))]),
            css: "style".into(),
            template_order_verified: false,
            managed_verified: false,
            content_matches_managed: false,
            compatibility: vec![],
        };
        let note = json!({"noteId":"123","modelName":"Legacy","fields":{"Expression":{"value":"猫","order":0}},"cards":["456"],"tags":["source"]});
        let cards = json!([{"cardId":"456","note":"123","ord":0,"deckId":"1","originalDeckId":"0","due":10,"reps":2}]);
        let source = SourceRecord {
            id: uuid::Uuid::new_v4(),
            kind: "anki_read_capture_v2".into(),
            location: "anki_note:123".into(),
            digest: "archive".into(),
            fields: BTreeMap::from([("Expression".into(), "猫".into())]),
            model_manifest: "model".into(),
            tags: vec!["source".into()],
            cards: vec![],
            media_refs: vec![],
        };
        let saved_model = serde_json::to_value(&model).unwrap();
        let live = linguist_anki::ReadCapture {
            note: note.clone(),
            model,
            cards: cards.as_array().unwrap().clone(),
            repeated_reads_matched: true,
            atomic_snapshot_verified: false,
            native_history_verified: false,
        };
        (source, note, saved_model, cards, live)
    }

    #[test]
    fn scheduler_only_study_is_reported_without_content_drift() {
        let (source, note, model, cards, mut live) = fixture();
        live.cards[0]["due"] = json!(20);
        live.cards[0]["reps"] = json!(3);
        let report = compare(&source, vec![], &note, &model, &cards, &live).unwrap();
        assert!(!report.source_conflict);
        assert!(report.card_payload_changed);
        assert!(!report.native_history_verified);
        assert!(!report.atomic_snapshot_verified);
    }

    #[test]
    fn field_model_card_and_deck_drift_each_block_live_match() {
        let (source, note, model, cards, mut live) = fixture();
        live.note["fields"]["Expression"]["value"] = json!("犬");
        assert!(
            !compare(&source, vec![], &note, &model, &cards, &live)
                .unwrap()
                .fields_match
        );
        live.note = note.clone();
        live.model.css = "changed".into();
        assert!(
            !compare(&source, vec![], &note, &model, &cards, &live)
                .unwrap()
                .model_match
        );
        live.model.css = "style".into();
        live.cards[0]["deckId"] = json!("2");
        let report = compare(&source, vec![], &note, &model, &cards, &live).unwrap();
        assert!(!report.deck_membership_match && report.source_conflict);
        live.cards[0]["deckId"] = json!("1");
        live.cards[0]["ord"] = json!(1);
        assert!(
            !compare(&source, vec![], &note, &model, &cards, &live)
                .unwrap()
                .card_structure_match
        );
        live.cards[0]["ord"] = json!(0);
        live.cards[0]["cardId"] = json!("457");
        assert!(
            !compare(&source, vec![], &note, &model, &cards, &live)
                .unwrap()
                .card_ids_match
        );
    }

    #[test]
    fn missing_native_card_identity_details_never_match() {
        let (source, note, model, mut cards, mut live) = fixture();
        cards[0].as_object_mut().unwrap().remove("ord");
        live.cards[0].as_object_mut().unwrap().remove("ord");
        live.cards[0].as_object_mut().unwrap().remove("deckId");
        cards[0].as_object_mut().unwrap().remove("deckId");
        let report = compare(&source, vec![], &note, &model, &cards, &live).unwrap();
        assert!(!report.card_structure_match);
        assert!(!report.deck_membership_match);
        assert!(report.source_conflict);
    }

    #[test]
    fn archived_source_links_are_checked_before_live_comparison() {
        let (_, note, model, cards, live) = fixture();
        let captured = crate::source_archive::archive_read_capture(
            &canonical::bytes(&note).unwrap(),
            &canonical::bytes(&model).unwrap(),
            &canonical::bytes(&cards).unwrap(),
            1024 * 1024,
        )
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("lab-live-validation-{}", uuid::Uuid::new_v4()));
        let mut store = linguist_store::Store::open(&root).unwrap();
        for (digest, bytes) in &captured.assets {
            assert_eq!(&store.publish_asset(bytes, 1024 * 1024).unwrap(), digest);
        }
        let mut document = LearningDocument::from_json(include_bytes!(
            "../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        document.sources.push(captured.source);
        document.archives.push(captured.archive);
        let plan = PlanRevision {
            schema_version: 2,
            id: uuid::Uuid::new_v4(),
            revision: 1,
            parent_digest: None,
            settings: ResolvedSettings {
                version: 2,
                values: BTreeMap::from([("input.max_file_mb".into(), json!(1))]),
                provenance: BTreeMap::new(),
                resource_hashes: BTreeMap::new(),
                secret_refs: BTreeMap::new(),
                fingerprint: "fixture".into(),
            },
            binding: None,
            source_digest: "fixture".into(),
            selection: None,
            grammar_groups: vec![],
            documents: vec![document],
            rendered: vec![],
            review_decisions: vec![],
        };
        let mut one = Some(live);
        let page = inspect_with(
            &store,
            &plan,
            0,
            1,
            |id| {
                assert_eq!(id, "123");
                Ok(one.take().unwrap())
            },
            |_| panic!("unexpected media read"),
        )
        .unwrap();
        assert!(page.all_sources_checked);
        assert!(!page.source_conflicts);
        assert_eq!(page.sources.len(), 1);
        assert!(page.sources[0].card_structure_match);
        assert!(page.sources[0].deck_membership_match);
        assert_eq!(
            inspect_with(
                &store,
                &plan,
                1,
                1,
                |_| panic!("unexpected read"),
                |_| panic!("unexpected media read")
            )
            .unwrap_err(),
            "LIVE_VALIDATION_CURSOR_OUT_OF_RANGE"
        );
        let mut forged = plan.clone();
        forged.documents[0].sources[0].model_manifest = "forged".into();
        assert_eq!(
            inspect_with(
                &store,
                &forged,
                0,
                1,
                |_| panic!("unexpected read"),
                |_| panic!("unexpected media read")
            )
            .unwrap_err(),
            "LIVE_SOURCE_ARCHIVE_CONFLICT"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn live_media_capture() -> linguist_anki::ReadCapture {
        let (_, mut note, _, _, mut live) = fixture();
        note["fields"]["Expression"]["value"] = json!("猫 [sound:cat.mp3]");
        live.note = note;
        live
    }

    fn media_fixture(
        attach: bool,
    ) -> (
        linguist_store::Store,
        std::path::PathBuf,
        PlanRevision,
        linguist_anki::ReadCapture,
    ) {
        let (_, _, model, cards, _) = fixture();
        let live = live_media_capture();
        let note = live.note.clone();
        let mut captured = crate::source_archive::archive_read_capture(
            &canonical::bytes(&note).unwrap(),
            &canonical::bytes(&model).unwrap(),
            &canonical::bytes(&cards).unwrap(),
            1024 * 1024,
        )
        .unwrap();
        assert_eq!(captured.source.media_refs, ["cat.mp3"]);
        if attach {
            crate::source_archive::media::attach_original_media(
                &mut captured,
                BTreeMap::from([("cat.mp3".into(), Some(b"original audio".to_vec()))]),
                1024 * 1024,
                1024 * 1024,
            )
            .unwrap();
        }
        let root = std::env::temp_dir().join(format!("lab-live-media-{}", uuid::Uuid::new_v4()));
        let mut store = linguist_store::Store::open(&root).unwrap();
        for (digest, bytes) in &captured.assets {
            assert_eq!(&store.publish_asset(bytes, 1024 * 1024).unwrap(), digest);
        }
        let mut document = LearningDocument::from_json(include_bytes!(
            "../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        document.sources.push(captured.source);
        document.archives.push(captured.archive);
        let plan = PlanRevision {
            schema_version: 2,
            id: uuid::Uuid::new_v4(),
            revision: 1,
            parent_digest: None,
            settings: ResolvedSettings {
                version: 2,
                values: BTreeMap::from([
                    ("input.max_file_mb".into(), json!(1)),
                    ("media.max_asset_mb".into(), json!(1)),
                ]),
                provenance: BTreeMap::new(),
                resource_hashes: BTreeMap::new(),
                secret_refs: BTreeMap::new(),
                fingerprint: "fixture".into(),
            },
            binding: None,
            source_digest: "fixture".into(),
            selection: None,
            grammar_groups: vec![],
            documents: vec![document],
            rendered: vec![],
            review_decisions: vec![],
        };
        (store, root, plan, live)
    }

    #[test]
    fn archived_media_bytes_are_rechecked_and_drift_blocks_live_match() {
        let (store, root, plan, live) = media_fixture(true);
        let original = b"original audio".to_vec();
        let mut read = Some(live);
        let exact = inspect_with(
            &store,
            &plan,
            0,
            1,
            |_| Ok(read.take().unwrap()),
            |name| {
                assert_eq!(name, "cat.mp3");
                Ok(Some(linguist_anki::MediaFile {
                    filename: name.into(),
                    digest: canonical::asset_digest(&original),
                    bytes: original.clone(),
                }))
            },
        )
        .unwrap();
        assert!(exact.sources[0].media_bytes_rechecked);
        assert!(exact.sources[0].media[0].matched);
        assert!(!exact.source_conflicts);
        let changed = b"changed audio".to_vec();
        let drift = inspect_with(
            &store,
            &plan,
            0,
            1,
            |_| Ok(live_media_capture()),
            |name| {
                Ok(Some(linguist_anki::MediaFile {
                    filename: name.into(),
                    digest: canonical::asset_digest(&changed),
                    bytes: changed.clone(),
                }))
            },
        )
        .unwrap();
        assert!(drift.sources[0].media_bytes_rechecked);
        assert!(drift.sources[0].media_conflict);
        assert!(!drift.sources[0].media[0].matched);
        assert!(drift.source_conflicts);
        let missing = inspect_with(
            &store,
            &plan,
            0,
            1,
            |_| Ok(live_media_capture()),
            |_| Ok(None),
        )
        .unwrap();
        assert!(missing.sources[0].media_conflict && missing.source_conflicts);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_saved_media_receipt_never_claims_recheck() {
        let (store, root, plan, live) = media_fixture(false);
        let mut live = Some(live);
        let page = inspect_with(
            &store,
            &plan,
            0,
            1,
            |_| Ok(live.take().unwrap()),
            |_| panic!("unbound media must not be read"),
        )
        .unwrap();
        assert!(!page.sources[0].media_bytes_rechecked);
        assert!(page.sources[0].media_conflict);
        assert!(page.source_conflicts);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archived_field_media_references_must_match_the_source_index() {
        let (store, root, mut plan, _) = media_fixture(true);
        plan.documents[0].sources[0].media_refs.clear();
        assert_eq!(
            inspect_with(
                &store,
                &plan,
                0,
                1,
                |_| panic!("inconsistent archive must fail before Anki reads"),
                |_| panic!("inconsistent archive must not fetch media"),
            )
            .unwrap_err(),
            "LIVE_MEDIA_ARCHIVE_CONFLICT"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
