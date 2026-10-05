//! Saved revision diff enriched from exact archived capture bytes.
use linguist_core::{AnkiId, canonical, inspection::RevisionDiff, records::PlanRevision};
use serde_json::Value;
use std::collections::BTreeSet;

fn captured_cards(
    store: &linguist_store::Store,
    source: &linguist_core::records::SourceRecord,
    archives: &[linguist_core::records::SourceArchive],
    cap: u64,
) -> Result<Vec<AnkiId>, String> {
    let archive = archives
        .iter()
        .find(|archive| {
            archive.source_id == source.id
                && archive.digest == source.digest
                && archive.original_fields == source.fields
                && archive.original_text == source.text
                && archive.asset_digests.contains(&source.digest)
                && archive.asset_digests.contains(&source.model_manifest)
                && source
                    .template_manifest
                    .as_ref()
                    .is_none_or(|digest| archive.asset_digests.contains(digest))
        })
        .ok_or("PLAN_DIFF_SOURCE_ARCHIVE_CONFLICT")?;
    let manifest: Value = canonical::parse(&store.asset(&source.digest, cap)?)
        .map_err(|_| "PLAN_DIFF_SOURCE_MANIFEST_INVALID")?;
    if manifest["kind"] != "anki_read_capture_v2"
        || manifest["note_id"]
            != source
                .location
                .strip_prefix("anki_note:")
                .ok_or("PLAN_DIFF_SOURCE_LOCATION_INVALID")?
        || manifest["model_manifest"] != source.model_manifest
        || source
            .template_manifest
            .as_ref()
            .is_some_and(|digest| manifest["template_manifest"].as_str() != Some(digest.as_str()))
    {
        return Err("PLAN_DIFF_SOURCE_MANIFEST_CONFLICT".into());
    }
    let payload = |name: &str| -> Result<Vec<u8>, String> {
        let digest = manifest["payloads"][name]
            .as_str()
            .ok_or("PLAN_DIFF_SOURCE_MANIFEST_INVALID")?;
        if !archive.asset_digests.iter().any(|item| item == digest) {
            return Err("PLAN_DIFF_SOURCE_ARCHIVE_CONFLICT".into());
        }
        store.asset(digest, cap)
    };
    let note = payload("note")?;
    let model = payload("model")?;
    let cards = payload("cards")?;
    let rebuilt = crate::source_archive::archive_read_capture(&note, &model, &cards, cap)?;
    if rebuilt.source.location != source.location
        || rebuilt.source.fields != source.fields
        || rebuilt.source.tags != source.tags
        || rebuilt.source.media_refs != source.media_refs
        || rebuilt.source.model_manifest != source.model_manifest
        || source.template_manifest.is_some()
            && rebuilt.source.template_manifest != source.template_manifest
    {
        return Err("PLAN_DIFF_SOURCE_ARCHIVE_CONFLICT".into());
    }
    let rows: Value = canonical::parse(&cards).map_err(|_| "PLAN_DIFF_CARDS_INVALID")?;
    rows.as_array()
        .ok_or("PLAN_DIFF_CARDS_INVALID")?
        .iter()
        .map(|row| {
            let id = row["cardId"].as_str().ok_or("PLAN_DIFF_CARDS_INVALID")?;
            AnkiId::try_from(id.to_owned()).map_err(|_| "PLAN_DIFF_CARDS_INVALID".into())
        })
        .collect()
}

/// Task changes remain intent. Captured IDs come from original archived bytes.
pub fn revision_diff(
    store: &linguist_store::Store,
    before: &PlanRevision,
    after: &PlanRevision,
) -> Result<RevisionDiff, String> {
    let mut diff =
        linguist_core::inspection::revision_diff(before, after).map_err(|e| e.to_string())?;
    let cap = if before.documents.iter().any(|document| {
        document
            .sources
            .iter()
            .any(|source| source.kind == "anki_read_capture_v2")
    }) {
        let value = before
            .settings
            .values
            .get("input.max_file_mb")
            .ok_or("PLAN_DIFF_SETTING_MISSING")?;
        linguist_config::Registry::builtin().validate_value("input.max_file_mb", value)?;
        value.as_u64().unwrap() * 1024 * 1024
    } else {
        0
    };
    for consequence in &mut diff.card_consequences {
        let Some(document) = before
            .documents
            .iter()
            .find(|document| document.id == consequence.document_id)
        else {
            continue;
        };
        let mut ids: BTreeSet<String> = consequence
            .captured_card_ids
            .iter()
            .cloned()
            .map(String::from)
            .collect();
        for source in document
            .sources
            .iter()
            .filter(|source| source.kind == "anki_read_capture_v2")
        {
            let archived: BTreeSet<String> =
                captured_cards(store, source, &document.archives, cap)?
                    .into_iter()
                    .map(String::from)
                    .collect();
            let declared: BTreeSet<String> = source
                .cards
                .iter()
                .map(|card| card.id.clone().into())
                .collect();
            if !declared.is_empty() && declared != archived {
                return Err("PLAN_DIFF_CARD_EVIDENCE_CONFLICT".into());
            }
            ids.extend(archived);
        }
        consequence.captured_card_ids = ids
            .into_iter()
            .map(|id| AnkiId::try_from(id).unwrap())
            .collect();
        consequence.captured_card_ids.sort_by_key(|id| {
            let raw: String = id.clone().into();
            raw.parse::<u64>().unwrap()
        });
    }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use linguist_core::{LearningDocument, records::*, render};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn diff_reads_original_card_ids_from_bound_archive() {
        let note = json!({"noteId":"123","modelName":"Legacy","fields":{"Expression":{"value":"cat","order":0}},"cards":["456"],"tags":[]});
        let model = crate::source_archive::fixture_model_manifest(
            json!({"model":{"id":"12","name":"Legacy"},"fields":["Expression"],"templates":{"Card":{"Front":"front","Back":"back"}},"css":"style"}),
        );
        let cards =
            json!([{"cardId":"456","note":"123","ord":0,"deckId":"1","originalDeckId":"0"}]);
        let captured = crate::source_archive::archive_read_capture(
            &canonical::bytes(&note).unwrap(),
            &canonical::bytes(&model).unwrap(),
            &canonical::bytes(&cards).unwrap(),
            1024 * 1024,
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!("lab-plan-diff-{}", uuid::Uuid::new_v4()));
        let mut store = linguist_store::Store::open(&root).unwrap();
        for (digest, bytes) in &captured.assets {
            assert_eq!(&store.publish_asset(bytes, 1024 * 1024).unwrap(), digest);
        }
        let mut doc = LearningDocument::from_json(include_bytes!(
            "../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        doc.sources.push(captured.source);
        doc.archives.push(captured.archive);
        let before = PlanRevision {
            schema_version: 2,
            id: uuid::Uuid::new_v4(),
            revision: 1,
            parent_digest: None,
            settings: ResolvedSettings {
                semantic_fingerprint: String::new(),
                execution_fingerprint: String::new(),
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
            rendered: vec![render::render(&doc, &BTreeMap::new()).unwrap()],
            documents: vec![doc],
            review_decisions: vec![],
        };
        let mut after = before.clone();
        after.revision = 2;
        after.parent_digest = Some(before.approval_digest().unwrap());
        let diff = revision_diff(&store, &before, &after).unwrap();
        assert_eq!(diff.card_consequences.len(), 1);
        let ids: Vec<String> = diff.card_consequences[0]
            .captured_card_ids
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        assert_eq!(ids, vec!["456"]);
        assert!(!diff.card_consequences[0].native_history_verified);
        let mut forged = before.clone();
        forged.documents[0].sources[0].model_manifest = "forged".into();
        assert!(revision_diff(&store, &forged, &after).is_err());
        let mut forged = before.clone();
        forged.documents[0].sources[0].cards.push(CardState {
            id: AnkiId::try_from("999".to_owned()).unwrap(),
            task: linguist_core::Task::Comprehension,
            deck_id: AnkiId::try_from("1".to_owned()).unwrap(),
            home_deck_id: AnkiId::try_from("1".to_owned()).unwrap(),
            scheduler: BTreeMap::new(),
            history_digest: "forged".into(),
        });
        assert_eq!(
            revision_diff(&store, &forged, &after).unwrap_err(),
            "PLAN_DIFF_CARD_EVIDENCE_CONFLICT"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
