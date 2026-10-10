//! ALG-GRAMMAR: explicit authored segmentation of retained grammar sources; no native effects.
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
    let mut keys = std::collections::BTreeSet::new();
    let mut units = Vec::new();
    for grammar in &request.units {
        if grammar.pattern.trim().is_empty()
            || grammar.use_key.trim().is_empty()
            || !keys.insert((&grammar.pattern, &grammar.use_key))
            || grammar.examples.iter().any(|example| {
                example.provenance != Provenance::User || !example.evidence_ids.is_empty()
            })
        {
            return Err("GRAMMAR_SPLIT_UNIT_INVALID".into());
        }
        // Siblings are new notes with the one grammar task (WP-23).
        units.push(SplitUnit {
            content: LearningContent::Grammar(grammar.clone()),
            sibling_tasks: vec![Task::Recognition],
            keep_media: None,
        });
    }
    publish_split(
        store,
        base,
        original,
        raw,
        &request.actor,
        request.anchor_index,
        SplitKind::Grammar,
        units,
    )
}

/// One unit of a reviewed split: its content, the tasks a fresh sibling
/// note gets, and (when given) the only source files it keeps for review.
pub(crate) struct SplitUnit {
    pub content: LearningContent,
    pub sibling_tasks: Vec<Task>,
    pub keep_media: Option<std::collections::BTreeSet<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitKind {
    Grammar,
    Vocabulary,
}
impl SplitKind {
    /// Error-code prefix and the request source kind recorded on every unit.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Grammar => ("GRAMMAR_SPLIT", GRAMMAR_SPLIT_SOURCE),
            Self::Vocabulary => ("VOCAB_SPLIT", VOCABULARY_SPLIT_SOURCE),
        }
    }
}
pub use linguist_core::records::{GRAMMAR_SPLIT_SOURCE, VOCABULARY_SPLIT_SOURCE};

/// Publish one explicit source-history anchor and fresh sibling identities
/// for an already validated request. The original item is replaced by the
/// units; one group records the anchor and the exact request bytes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_split(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    original: &LearningDocument,
    raw: &[u8],
    actor: &str,
    anchor_index: usize,
    kind: SplitKind,
    split_units: Vec<SplitUnit>,
) -> Result<PlanRevision, String> {
    let (prefix, request_kind) = kind.names();
    let code = |suffix: &str| format!("{prefix}_{suffix}");
    let limit = base
        .settings
        .values
        .get("input.max_file_mb")
        .and_then(serde_json::Value::as_u64)
        .filter(|limit| (1..=100).contains(limit))
        .ok_or(code("SETTING_MISSING"))?
        * 1024
        * 1024;
    if raw.len() as u64 > limit {
        return Err(code("INPUT_LIMIT"));
    }
    let chars = base
        .settings
        .values
        .get("input.max_record_chars")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| (1..=1_000_000).contains(value))
        .ok_or(code("SETTING_MISSING"))?;
    if std::str::from_utf8(raw)
        .map_err(|_| "INPUT_ENCODING")?
        .chars()
        .count() as u64
        > chars
    {
        return Err(code("INPUT_LIMIT"));
    }
    let retained = canonical::bytes(original).map_err(|e| e.to_string())?.len() as u64;
    let estimate = (raw.len() as u64)
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(retained))
        .and_then(|bytes| bytes.checked_mul(split_units.len() as u64))
        .ok_or(code("ARCHIVE_LIMIT"))?;
    if estimate > limit {
        return Err(code("ARCHIVE_LIMIT"));
    }
    let sources: Vec<_> = original
        .sources
        .iter()
        .filter(|source| source.kind == "anki_read_capture_v2")
        .collect();
    if sources.len() != 1 {
        return Err(code("SOURCE_CONFLICT"));
    }
    let source_id = sources[0].id;
    let raw_digest = canonical::asset_digest(raw);
    let request_text = String::from_utf8(raw.to_vec()).map_err(|_| "INPUT_ENCODING")?;
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
    // One unit left (the others merged into existing notes): the item is
    // narrowed in place and applied as a plain revamp, without a group.
    let group = split_units.len() > 1;
    for (index, unit) in split_units.into_iter().enumerate() {
        let mut document = original.clone();
        document.id = if index == anchor_index {
            original.id
        } else {
            uuid::Uuid::new_v4()
        };
        document.content = unit.content;
        if index != anchor_index {
            // Siblings are new notes: fresh scheduling, no inherited card state or
            // task maps. The source archive stays linked as a reference only.
            document.requested_tasks = unit.sibling_tasks;
            document.task_maps.clear();
            for source in &mut document.sources {
                source.cards.clear();
            }
            // A new note has no native cards to map: the source's history and
            // task-mapping reviews belong to the anchor only.
            document.issues.retain(|issue| {
                !matches!(
                    issue.code.as_str(),
                    "SOURCE_NATIVE_HISTORY_REVIEW" | "SOURCE_TASK_MAPPING_REVIEW"
                )
            });
        }
        // A unit keeps review of only its own source files; the rest stay archived.
        if let Some(keep) = &unit.keep_media {
            document.issues.retain(|issue| {
                !(matches!(
                    issue.code.as_str(),
                    "SOURCE_MEDIA_CONTENT_REVIEW" | "SOURCE_MEDIA_FORMAT_REVIEW"
                ) && issue.field.as_ref().is_some_and(|f| !keep.contains(f)))
            });
        }
        document.edits.clear();
        document.reviews.clear();
        // The split itself carries out the reviewed segmentation.
        document
            .issues
            .retain(|issue| issue.stage != "validation" && issue.stage != "segmentation");
        document.sources.push(SourceRecord {
            id: request_source,
            kind: request_kind.into(),
            location: "local_split_request".into(),
            digest: raw_digest.clone(),
            text: Some(request_text.clone()),
            fields: fields.clone(),
            model_manifest: request_kind.replace('_', "-"),
            template_manifest: None,
            captured_at_unix_seconds: None,
            tags: vec![],
            cards: vec![],
            media_refs: vec![],
        });
        document.archives.push(SourceArchive {
            id: uuid::Uuid::new_v4(),
            source_id: request_source,
            digest: raw_digest.clone(),
            original_text: Some(request_text.clone()),
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
        if group {
            document.issues.push(issue);
        }
        document.issues = validation::validate(&document);
        units.push(document.id);
        documents.push(document);
    }
    child.documents.splice(position..position + 1, documents);
    if group {
        child.grammar_groups.push(GrammarGroup {
            id: uuid::Uuid::new_v4(),
            source_id,
            anchor_document: original.id,
            units,
            actor: actor.to_owned(),
            request_asset_digest: raw_digest,
        });
    }
    child.review_decisions.retain(|decision| {
        !original
            .reviews
            .iter()
            .any(|review| review.id == decision.id)
    });
    child.revision = base.revision.checked_add(1).ok_or("REVISION_LIMIT")?;
    child.parent_digest = Some(base.approval_digest().map_err(|e| e.to_string())?);
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
        return Err(code("ARCHIVE_LIMIT"));
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
                .ok_or(code("ARCHIVE_LIMIT"))?;
            if total > limit {
                return Err(code("ARCHIVE_LIMIT"));
            }
        }
    }
    store.publish_asset(raw, limit)?;
    store.publish_revision(&child)?;
    Ok(child)
}

/// Build an editable split request from the item's reviewed multi-unit OCR
/// segmentation. Patterns are copied verbatim from the chosen regions; every
/// use key, meaning and formation is left empty for the reviewer to author.
pub fn split_template(
    plan: &PlanRevision,
    document_id: uuid::Uuid,
) -> Result<SplitRequest, String> {
    let document = plan
        .documents
        .iter()
        .find(|document| document.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    if !matches!(document.content, LearningContent::Grammar(_)) {
        return Err("GRAMMAR_SPLIT_INPUT_CONFLICT".into());
    }
    let ids = document
        .reviews
        .iter()
        .rev()
        .find_map(|review| match &review.choice {
            ReviewChoice::Segmentation(ids) if ids.len() > 1 => Some(ids.clone()),
            _ => None,
        })
        .ok_or("GRAMMAR_SEGMENTATION_DECISION_REQUIRED: resolve GRAMMAR_SEGMENTATION_REVIEW with several regions first")?;
    let units = ids
        .iter()
        .map(|id| {
            document
                .regions
                .iter()
                .find(|region| region.id == *id)
                .map(|region| Grammar {
                    pattern: region.text.trim().to_owned(),
                    ..Default::default()
                })
                .ok_or("GRAMMAR_SEGMENTATION_REGION_MISSING")
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SplitRequest {
        schema_version: 2,
        base_revision: plan.revision,
        base_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        document_id,
        input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
        actor: String::new(),
        anchor_index: 0,
        units,
    })
}
