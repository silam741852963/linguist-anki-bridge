//! Explicit authored segmentation of retained grammar sources; no native effects.
use linguist_core::{records::*, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitRequest {
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub document_id: uuid::Uuid,
    pub input_digest: String,
    pub actor: String,
    pub anchor_index: usize,
    pub units: Vec<Grammar>,
}

/// Publish one explicit source-history anchor and fresh sibling identities.
/// Original archives are references, never scheduling instructions for siblings.
pub fn split(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    request: &SplitRequest,
    raw: &[u8],
) -> Result<PlanRevision, String> {
    let parsed: SplitRequest = canonical::parse(raw).map_err(|e| e.to_string())?;
    if canonical::bytes(&parsed).map_err(|e| e.to_string())?
        != canonical::bytes(request).map_err(|e| e.to_string())?
        || request.schema_version != 2
        || request.base_revision != base.revision
        || request.base_digest != base.approval_digest().map_err(|e| e.to_string())?
        || store.latest_revision(base.id)? != base.revision
        || store.revision(base.id, base.revision)? != *base
    {
        return Err("GRAMMAR_SPLIT_BASE_CONFLICT".into());
    }
    let original = base
        .documents
        .iter()
        .find(|document| document.id == request.document_id)
        .ok_or("GRAMMAR_SPLIT_DOCUMENT_MISSING")?;
    if !matches!(original.content, LearningContent::Grammar(_))
        || request.input_digest != original.semantic_digest().map_err(|e| e.to_string())?
        || !(2..=100).contains(&request.units.len())
        || request.anchor_index >= request.units.len()
        || base
            .grammar_groups
            .iter()
            .any(|group| group.units.contains(&original.id))
    {
        return Err("GRAMMAR_SPLIT_INPUT_CONFLICT".into());
    }
    let limit = base
        .settings
        .values
        .get("input.max_file_mb")
        .and_then(serde_json::Value::as_u64)
        .filter(|limit| (1..=100).contains(limit))
        .ok_or("GRAMMAR_SPLIT_SETTING_MISSING")?
        * 1024
        * 1024;
    if raw.len() as u64 > limit {
        return Err("GRAMMAR_SPLIT_INPUT_LIMIT".into());
    }
    let chars = base
        .settings
        .values
        .get("input.max_record_chars")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| (1..=1_000_000).contains(value))
        .ok_or("GRAMMAR_SPLIT_SETTING_MISSING")?;
    if std::str::from_utf8(raw)
        .map_err(|_| "INPUT_ENCODING")?
        .chars()
        .count() as u64
        > chars
    {
        return Err("GRAMMAR_SPLIT_INPUT_LIMIT".into());
    }
    let retained = canonical::bytes(original).map_err(|e| e.to_string())?.len() as u64;
    let estimate = (raw.len() as u64)
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(retained))
        .and_then(|bytes| bytes.checked_mul(request.units.len() as u64))
        .ok_or("GRAMMAR_SPLIT_ARCHIVE_LIMIT")?;
    if estimate > limit {
        return Err("GRAMMAR_SPLIT_ARCHIVE_LIMIT".into());
    }
    let sources: Vec<_> = original
        .sources
        .iter()
        .filter(|source| source.kind == "anki_read_capture_v2")
        .collect();
    if sources.len() != 1 {
        return Err("GRAMMAR_SPLIT_SOURCE_CONFLICT".into());
    }
    let source_id = sources[0].id;
    let raw_digest = canonical::asset_digest(raw);
    let request_source = uuid::Uuid::new_v4();
    let fields = std::collections::BTreeMap::from([(
        "split_request".into(),
        String::from_utf8(raw.to_vec()).map_err(|_| "INPUT_ENCODING")?,
    )]);
    let mut child = base.clone();
    let position = child
        .documents
        .iter()
        .position(|document| document.id == original.id)
        .unwrap();
    let mut documents = Vec::new();
    let mut units = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for (index, grammar) in request.units.iter().enumerate() {
        if grammar.pattern.trim().is_empty()
            || grammar.use_key.trim().is_empty()
            || !keys.insert((&grammar.pattern, &grammar.use_key))
            || grammar.examples.iter().any(|example| {
                example.provenance != Provenance::User || !example.evidence_ids.is_empty()
            })
        {
            return Err("GRAMMAR_SPLIT_UNIT_INVALID".into());
        }
        let mut document = original.clone();
        document.id = if index == request.anchor_index {
            original.id
        } else {
            uuid::Uuid::new_v4()
        };
        document.content = LearningContent::Grammar(grammar.clone());
        if index != request.anchor_index {
            document.requested_tasks = vec![Task::Recognition];
        }
        document.edits.clear();
        document.reviews.clear();
        document.issues.retain(|issue| issue.stage != "validation");
        document.sources.push(SourceRecord {
            id: request_source,
            kind: "grammar_split_request_v1".into(),
            location: "local_split_request".into(),
            digest: raw_digest.clone(),
            fields: fields.clone(),
            model_manifest: "grammar-split-request-v1".into(),
            tags: vec![],
            cards: vec![],
            media_refs: vec![],
        });
        document.archives.push(SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id: request_source,
            digest: raw_digest.clone(),
            original_fields: fields.clone(),
            asset_digests: vec![raw_digest.clone()],
        });
        let mut issue = Issue::new(
            "GRAMMAR_SPLIT_NATIVE_REVIEW",
            Severity::Review,
            None,
            "One reviewed anchor retains source identity; sibling units require fresh native notes. Native task/history mapping is unverified.",
        );
        issue.stage = "capture".into();
        issue.source_refs = vec![source_id.to_string()];
        document.issues.push(issue);
        document.issues = validation::validate(&document);
        units.push(document.id);
        documents.push(document);
    }
    child.documents.splice(position..position + 1, documents);
    child.grammar_groups.push(GrammarGroup {
        id: uuid::Uuid::new_v4(),
        source_id,
        anchor_document: original.id,
        units,
        actor: request.actor.clone(),
        request_asset_digest: raw_digest,
    });
    child.review_decisions.retain(|decision| {
        !original
            .reviews
            .iter()
            .any(|review| review.id == decision.id)
    });
    child.revision = base.revision.checked_add(1).ok_or("REVISION_LIMIT")?;
    child.parent_digest = Some(request.base_digest.clone());
    child.binding = None;
    child.rendered.clear();
    let sources: Vec<_> = child
        .documents
        .iter()
        .flat_map(|document| &document.sources)
        .collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    if canonical::bytes(&child).map_err(|e| e.to_string())?.len() as u64 > limit {
        return Err("GRAMMAR_SPLIT_ARCHIVE_LIMIT".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = raw.len() as u64;
    seen.insert(canonical::asset_digest(raw));
    for digest in base
        .documents
        .iter()
        .flat_map(|document| &document.archives)
        .flat_map(|archive| &archive.asset_digests)
    {
        if seen.insert(digest.clone()) {
            total = total
                .checked_add(store.asset(digest, limit)?.len() as u64)
                .ok_or("GRAMMAR_SPLIT_ARCHIVE_LIMIT")?;
            if total > limit {
                return Err("GRAMMAR_SPLIT_ARCHIVE_LIMIT".into());
            }
        }
    }
    store.publish_asset(raw, limit)?;
    store.publish_revision(&child)?;
    Ok(child)
}
