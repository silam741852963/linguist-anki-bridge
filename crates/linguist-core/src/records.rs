//! Typed evidence and immutable history contracts; persistence is implemented separately.
use crate::{document::*, validation::Issue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub id: Uuid,
    pub kind: String,
    pub location: String,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub model_manifest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at_unix_seconds: Option<u64>,
    pub tags: Vec<String>,
    pub cards: Vec<CardState>,
    pub media_refs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CardState {
    pub id: AnkiId,
    pub task: Task,
    pub deck_id: AnkiId,
    pub home_deck_id: AnkiId,
    pub scheduler: BTreeMap<String, String>,
    pub history_digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetModelKind {
    Vocabulary,
    Grammar,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceTaskMapEntry {
    pub source_ordinal: u16,
    pub target_task: Task,
    pub target_ordinal: u16,
}
/// A declared source-template mapping. Native ordinal/history verification belongs to WP-03.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceTaskMap {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub source_id: Uuid,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub source_model_digest: String,
    pub target_model: TargetModelKind,
    pub entries: Vec<SourceTaskMapEntry>,
}
impl SourceTaskMap {
    pub fn validate(&self) -> Result<(), String> {
        let invalid = || "SOURCE_TASK_MAP_INVALID".to_owned();
        if self.schema_version != 1
            || self.source_id.is_nil()
            || self.source_model_digest.len() != 64
            || !self
                .source_model_digest
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || self.entries.is_empty()
            || self.entries.len() > 3
        {
            return Err(invalid());
        }
        let mut source_ordinals = std::collections::BTreeSet::new();
        let mut target_tasks = std::collections::BTreeSet::new();
        for entry in &self.entries {
            let expected = match (self.target_model, entry.target_task) {
                (TargetModelKind::Vocabulary, Task::Comprehension) => 0,
                (TargetModelKind::Vocabulary, Task::Production) => 1,
                (TargetModelKind::Vocabulary, Task::Spelling) => 2,
                (TargetModelKind::Grammar, Task::Recognition) => 0,
                (TargetModelKind::Grammar, Task::Application) => 1,
                _ => return Err(invalid()),
            };
            if entry.target_ordinal != expected
                || !source_ordinals.insert(entry.source_ordinal)
                || !target_tasks.insert(entry.target_task)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArchive {
    pub id: Uuid,
    pub source_id: Uuid,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    pub original_fields: BTreeMap<String, String>,
    pub asset_digests: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRegion {
    pub id: Uuid,
    pub source_id: Uuid,
    pub image_digest: String,
    pub bounds: [u32; 4],
    pub reading_order: u32,
    pub engine: String,
    pub settings_digest: String,
    pub text: String,
    pub confidence: Option<f64>,
    pub language: Language,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: Uuid,
    pub field: String,
    pub provenance: Provenance,
    pub source_id: Option<Uuid>,
    pub region_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EvidenceTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceTextSpan>,
    pub language: Language,
    pub claim: String,
    pub source_url: Option<String>,
    pub ambiguous: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceTarget {
    DictionarySense {
        entry_index: usize,
        sense_index: usize,
    },
    Example {
        index: usize,
    },
    GrammarFormation,
    MediaAsset {
        digest: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceTextSpan {
    pub start_byte: u32,
    pub end_byte: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaOwner {
    Source,
    App,
    External,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaRole {
    Picture,
    Audio,
    Archive,
    /// Animated stroke-order image for one kanji (v3 Kanji field).
    KanjiStroke,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaAsset {
    pub digest: String,
    pub filename: String,
    pub original_filename: Option<String>,
    pub size_bytes: u64,
    pub mime: String,
    pub owner: MediaOwner,
    pub role: MediaRole,
    pub source_id: Option<Uuid>,
    pub attribution: String,
    pub license: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "decision",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReviewChoice {
    Sense(String),
    SenseWithReading {
        key: String,
        reading: String,
    },
    Anchor(Uuid),
    Segmentation(Vec<Uuid>),
    Duplicate {
        note_id: AnkiId,
        action: String,
    },
    Media(String),
    // Take a missing vocabulary expression from one recorded OCR region.
    Expression {
        region_id: Uuid,
    },
    Cue {
        task: Task,
        text: String,
    },
    Exercise {
        prompt: String,
        answer: String,
    },
    ContentVerified {
        evidence_ids: Vec<Uuid>,
    },
    SourceContentVerified {
        source_id: Uuid,
        evidence_ids: Vec<Uuid>,
    },
    /// An unmapped source field is intentionally not carried into the target
    /// model; its original value stays in the source archive.
    SourceFieldDropped {
        source_id: Uuid,
        field: String,
    },
    SourceMediaRole {
        source_id: Uuid,
        asset_digest: String,
        original_filename: String,
        evidence_id: Uuid,
        role: MediaRole,
        attribution: String,
        license: Option<String>,
    },
    /// Resolves `SOURCE_NATIVE_HISTORY_REVIEW` from companion note evidence:
    /// every source card with its observed study, and the reviewed mapping of
    /// each source template ordinal to a target task. No card is dropped.
    /// Acknowledges that a media file the source note references is absent
    /// from the collection at capture time. The original field (with its
    /// reference) stays archived; no bytes exist to archive or render.
    MissingMedia {
        source_id: Uuid,
        filename: String,
    },
    NativeHistory {
        source_id: Uuid,
        cards: Vec<NativeCardEvidence>,
        evidence_digest: String,
        task_map: SourceTaskMap,
    },
}
/// One source card as observed through `labInspect` note evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeCardEvidence {
    pub card_id: AnkiId,
    pub ordinal: u16,
    pub deck_id: AnkiId,
    pub repetitions: u32,
    pub review_count: u32,
    /// SHA-256 of the card's canonical review rows.
    pub history_digest: String,
}
/// Digest a `NativeHistory` decision must carry for its source and cards.
pub fn native_history_digest(
    source: &SourceRecord,
    cards: &[NativeCardEvidence],
) -> Result<String, crate::canonical::ContractError> {
    crate::canonical::digest(
        "native-history-v1",
        &(&source.id, &source.location, &source.model_manifest, cards),
    )
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    pub id: Uuid,
    pub issue_id: String,
    pub input_digest: String,
    pub actor: String,
    pub created_at: String,
    pub choice: ReviewChoice,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSettings {
    pub version: u16,
    pub values: BTreeMap<String, serde_json::Value>,
    pub provenance: BTreeMap<String, String>,
    pub resource_hashes: BTreeMap<String, String>,
    pub secret_refs: BTreeMap<String, String>,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub semantic_fingerprint: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub execution_fingerprint: String,
}
/// Settings allowed to vary in a new execution envelope without changing
/// content approval. New keys are semantic until explicitly reviewed here.
pub fn execution_setting(key: &str) -> bool {
    key.starts_with("output.") || key.starts_with("logging.") || key.starts_with("retry.")
}
pub fn setting_fingerprints(
    values: &BTreeMap<String, serde_json::Value>,
) -> Result<(String, String), crate::canonical::ContractError> {
    let mut semantic = BTreeMap::new();
    let mut execution = BTreeMap::new();
    for (key, value) in values {
        if execution_setting(key) {
            execution.insert(key, value);
        } else {
            semantic.insert(key, value);
        }
    }
    Ok((
        crate::canonical::digest("semantic-settings", &semantic)?,
        crate::canonical::digest("execution-settings", &execution)?,
    ))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectionBinding {
    pub endpoint: String,
    pub profile_fingerprint: String,
    pub path_fingerprint: String,
    pub bridge_id: Uuid,
    pub lineage_id: Uuid,
    pub session_epoch: Uuid,
    pub capability_digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Preparing,
    Draft,
    NeedsReview,
    Ready,
    Approved,
    Applying,
    Applied,
    Partial,
    Abandoned,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRevision {
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u16,
    pub id: Uuid,
    pub revision: u32,
    pub parent_digest: Option<String>,
    pub settings: ResolvedSettings,
    pub binding: Option<CollectionBinding>,
    pub source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grammar_groups: Vec<GrammarGroup>,
    pub documents: Vec<LearningDocument>,
    pub rendered: Vec<crate::render::RenderedNote>,
    pub review_decisions: Vec<ReviewDecision>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "input",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SelectionInput {
    NoteIds(#[schemars(with = "Vec<AnkiId>")] Vec<String>),
    Query(String),
    Deck { name: String, query: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionReceipt {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub purpose: String,
    pub selector: SelectionInput,
    #[schemars(with = "Vec<AnkiId>")]
    pub matched_note_ids: Vec<String>,
    #[schemars(with = "Vec<AnkiId>")]
    pub selected_note_ids: Vec<String>,
    pub order: String,
    pub max_notes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_limit: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrammarGroup {
    pub id: Uuid,
    pub source_id: Uuid,
    pub anchor_document: Uuid,
    pub units: Vec<Uuid>,
    pub actor: String,
    pub request_asset_digest: String,
}
impl GrammarGroup {
    pub fn validate(&self, plan: &PlanRevision) -> Result<(), crate::canonical::ContractError> {
        let fail = || crate::canonical::ContractError("GRAMMAR_GROUP_INVALID".into());
        if self.units.len() < 2
            || self.units.len() > 100
            || !self.units.contains(&self.anchor_document)
            || self
                .units
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.units.len()
            || self.actor.trim().is_empty()
            || self.actor.chars().count() > 200
            || self.actor.chars().any(char::is_control)
        {
            return Err(fail());
        }
        for id in &self.units {
            let document = plan
                .documents
                .iter()
                .find(|document| document.id == *id)
                .ok_or_else(fail)?;
            if !matches!(document.content, crate::LearningContent::Grammar(_))
                || !document.sources.iter().any(|source| {
                    source.id == self.source_id && source.kind == "anki_read_capture_v2"
                })
                || !document
                    .archives
                    .iter()
                    .any(|archive| archive.source_id == self.source_id)
                || !document.sources.iter().any(|source| {
                    source.kind == "grammar_split_request_v1"
                        && source.digest == self.request_asset_digest
                        && document.archives.iter().any(|archive| {
                            archive.source_id == source.id
                                && archive.asset_digests.contains(&self.request_asset_digest)
                        })
                })
            {
                return Err(fail());
            }
            let source = document
                .sources
                .iter()
                .find(|source| {
                    source.kind == "grammar_split_request_v1"
                        && source.digest == self.request_asset_digest
                })
                .ok_or_else(fail)?;
            let raw = source.fields.get("split_request").ok_or_else(fail)?;
            if crate::canonical::asset_digest(raw.as_bytes()) != self.request_asset_digest {
                return Err(fail());
            }
            let request: serde_json::Value = crate::canonical::parse(raw.as_bytes())?;
            let index = request["anchor_index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or_else(fail)?;
            if request["actor"].as_str() != Some(self.actor.as_str())
                || request["document_id"].as_str()
                    != Some(self.anchor_document.to_string().as_str())
                || request["units"].as_array().map(Vec::len) != Some(self.units.len())
                || self.units.get(index) != Some(&self.anchor_document)
            {
                return Err(fail());
            }
        }
        Ok(())
    }
}
impl PlanRevision {
    pub fn approval_digest(&self) -> Result<String, crate::canonical::ContractError> {
        if !self.settings.semantic_fingerprint.is_empty()
            || !self.settings.execution_fingerprint.is_empty()
        {
            let (semantic, execution) = setting_fingerprints(&self.settings.values)?;
            if self.settings.semantic_fingerprint != semantic
                || self.settings.execution_fingerprint != execution
                || self.settings.fingerprint
                    != crate::canonical::digest("resolved-settings", &self.settings.values)?
            {
                return Err(crate::canonical::ContractError(
                    "SETTINGS_FINGERPRINT_INVALID".into(),
                ));
            }
        }
        let mut grouped = std::collections::BTreeSet::new();
        let mut group_sources = std::collections::BTreeSet::new();
        for group in &self.grammar_groups {
            if !group_sources.insert(group.source_id) {
                return Err(crate::canonical::ContractError(
                    "GRAMMAR_GROUP_SOURCE_CONFLICT".into(),
                ));
            }
            group.validate(self)?;
            for unit in &group.units {
                if !grouped.insert(*unit) {
                    return Err(crate::canonical::ContractError(
                        "GRAMMAR_GROUP_OVERLAP".into(),
                    ));
                }
            }
        }
        if let Some(selection) = &self.selection {
            selection.validate(self)?;
        }
        let mut projection = serde_json::to_value(self)?;
        // Epoch is execution identity; stable lineage remains approval-bound.
        if let Some(binding) = projection
            .get_mut("binding")
            .and_then(|v| v.as_object_mut())
        {
            binding.remove("session_epoch");
        }
        if !self.settings.semantic_fingerprint.is_empty()
            && let Some(settings) = projection
                .get_mut("settings")
                .and_then(|value| value.as_object_mut())
        {
            settings.remove("fingerprint");
            settings.remove("execution_fingerprint");
            if let Some(values) = settings.get_mut("values").and_then(|v| v.as_object_mut()) {
                values.retain(|key, _| !execution_setting(key));
            }
            if let Some(provenance) = settings
                .get_mut("provenance")
                .and_then(|v| v.as_object_mut())
            {
                provenance.retain(|key, _| !execution_setting(key));
            }
        }
        if let Some(documents) = projection
            .get_mut("documents")
            .and_then(|v| v.as_array_mut())
        {
            for (value, document) in documents.iter_mut().zip(&self.documents) {
                *value = serde_json::json!({"semantic_digest":document.semantic_digest()?});
            }
        }
        if let Some(reviews) = projection
            .get_mut("review_decisions")
            .and_then(|v| v.as_array_mut())
        {
            for review in reviews {
                if let Some(review) = review.as_object_mut() {
                    review.remove("created_at");
                }
            }
        }
        crate::canonical::digest("plan", &projection)
    }
}
impl SelectionReceipt {
    pub fn validate_inputs(
        &self,
        settings: &ResolvedSettings,
    ) -> Result<(), crate::canonical::ContractError> {
        let fail = || crate::canonical::ContractError("PLAN_SELECTION_INVALID".into());
        if self.schema_version != 1
            || !matches!(
                self.purpose.as_str(),
                "japanese_vocab" | "english_vocab" | "japanese_grammar" | "english_grammar"
            )
            || !(1..=100000).contains(&self.max_notes)
            || self.matched_note_ids.is_empty()
            || self.matched_note_ids.len() > 100000
            || self
                .command_limit
                .is_some_and(|limit| !(1..=100000).contains(&limit))
            || (self.command_limit.is_none() && self.matched_note_ids.len() as u64 > self.max_notes)
            || settings
                .values
                .get("selection.max_notes")
                .and_then(serde_json::Value::as_u64)
                != Some(self.max_notes)
            || settings
                .values
                .get("selection.order")
                .and_then(serde_json::Value::as_str)
                != Some(self.order.as_str())
        {
            return Err(fail());
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.matched_note_ids {
            let value = id.parse::<u64>().map_err(|_| fail())?;
            if value == 0
                || value > 9_007_199_254_740_991
                || value.to_string() != *id
                || !seen.insert(id)
            {
                return Err(fail());
            }
        }
        let mut expected = self.matched_note_ids.clone();
        match self.order.as_str() {
            "input" => (),
            "note_id" => expected.sort_by_key(|id| id.parse::<u64>().unwrap()),
            _ => return Err(fail()),
        }
        if let Some(limit) = self.command_limit {
            if matches!(self.selector, SelectionInput::NoteIds(_)) {
                return Err(fail());
            }
            expected.truncate(limit as usize);
        }
        if expected != self.selected_note_ids {
            return Err(fail());
        }
        match &self.selector {
            SelectionInput::NoteIds(ids) if ids != &self.matched_note_ids => return Err(fail()),
            SelectionInput::Query(query) if query.trim().is_empty() => return Err(fail()),
            SelectionInput::Deck { name, query }
                if name.trim().is_empty() || query.trim().is_empty() =>
            {
                return Err(fail());
            }
            _ => (),
        }
        Ok(())
    }
    fn validate(&self, plan: &PlanRevision) -> Result<(), crate::canonical::ContractError> {
        self.validate_inputs(&plan.settings)?;
        let fail = || crate::canonical::ContractError("PLAN_SELECTION_INVALID".into());
        let mut captured = Vec::new();
        let mut captured_seen = std::collections::BTreeSet::new();
        for document in &plan.documents {
            for source in &document.sources {
                if source.kind == "anki_read_capture_v2" {
                    let id = source
                        .location
                        .strip_prefix("anki_note:")
                        .ok_or_else(fail)?
                        .to_owned();
                    if captured_seen.insert(id.clone()) {
                        captured.push(id);
                    }
                }
            }
        }
        if captured != self.selected_note_ids {
            return Err(fail());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub plan_id: Uuid,
    pub revision: u32,
    pub digest: String,
    pub item_ids: Vec<Uuid>,
    pub actor: String,
    pub approved_at: String,
    pub accepted_warnings: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Prepared,
    Preflight,
    Checkpointed,
    Mutating,
    Verifying,
    Committed,
    FailedBeforeWrite,
    NeedsRecovery,
    Compensated,
    Restored,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    IntentRecorded,
    RequestStarted,
    ObservedSuccess,
    ObservedFailure,
    Unknown,
    Verified,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalStep {
    pub id: Uuid,
    pub action: String,
    pub payload_digest: String,
    pub precondition_digest: String,
    pub expected_post_digest: String,
    pub state: StepState,
    pub observed_digest: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationJournal {
    pub id: Uuid,
    pub group_id: Option<Uuid>,
    pub approval_digest: String,
    pub binding: CollectionBinding,
    pub snapshot_id: Uuid,
    pub backup_id: Uuid,
    pub state: OperationState,
    pub steps: Vec<JournalStep>,
    pub issues: Vec<Issue>,
}
/// Application-side evidence contract. A queued/running/unknown companion status
/// is an observation, not proof that an Anki effect occurred or completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NativeReceiptState {
    Queued,
    Running,
    Unknown,
    FailedBeforeWrite,
    Verified,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeReadback {
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub observed_state_digest: String,
    pub note_ids: Vec<AnkiId>,
    pub card_ids: Vec<AnkiId>,
    pub history_digest: Option<String>,
    pub manifest_digests: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeOperationReceipt {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub lineage_id: Uuid,
    pub operation_id: Uuid,
    pub session_epoch: Uuid,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub payload_digest: String,
    #[schemars(pattern(r"^lab-jcs-v1:plan:[0-9a-f]{64}$"))]
    pub approved_digest: String,
    pub state: NativeReceiptState,
    pub readback: Option<NativeReadback>,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub evidence_digest: String,
}
impl NativeOperationReceipt {
    pub fn validate(&self) -> Result<(), crate::canonical::ContractError> {
        let invalid = || crate::canonical::ContractError("NATIVE_RECEIPT_INVALID".into());
        let raw_digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if self.schema_version != 1
            || self.lineage_id.is_nil()
            || self.operation_id.is_nil()
            || self.session_epoch.is_nil()
            || !raw_digest(&self.payload_digest)
            || !raw_digest(&self.evidence_digest)
            || !self
                .approved_digest
                .strip_prefix("lab-jcs-v1:plan:")
                .is_some_and(raw_digest)
            || (self.state == NativeReceiptState::Verified) != self.readback.is_some()
        {
            return Err(invalid());
        }
        if let Some(readback) = &self.readback
            && (!raw_digest(&readback.observed_state_digest)
                || readback
                    .history_digest
                    .as_deref()
                    .is_some_and(|digest| !raw_digest(digest))
                || readback
                    .manifest_digests
                    .iter()
                    .any(|digest| !raw_digest(digest))
                || readback
                    .note_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != readback.note_ids.len()
                || readback
                    .card_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != readback.card_ids.len())
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResumeBindingScope {
    ContinueOperation,
}
/// Records an explicit decision about one existing operation after a collection
/// session change. Validation is structural; live identity and safe continuation
/// still require the recovery algorithm and current `--apply` authorization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeBindingDecision {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub operation_id: Uuid,
    #[schemars(pattern(r"^lab-jcs-v1:plan:[0-9a-f]{64}$"))]
    pub approval_digest: String,
    pub old_binding: CollectionBinding,
    pub new_binding: CollectionBinding,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub observed_state_digest: String,
    pub actor: String,
    pub decided_at: String,
    pub scope: ResumeBindingScope,
}
impl ResumeBindingDecision {
    pub fn validate(&self) -> Result<(), crate::canonical::ContractError> {
        let invalid = || crate::canonical::ContractError("RESUME_BINDING_DECISION_INVALID".into());
        let raw_digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        let binding_valid = |binding: &CollectionBinding| {
            !binding.endpoint.trim().is_empty()
                && binding.endpoint.len() <= 2048
                && raw_digest(&binding.profile_fingerprint)
                && raw_digest(&binding.path_fingerprint)
                && binding
                    .capability_digest
                    .strip_prefix("lab-jcs-v1:lab-native-capabilities-v1:")
                    .is_some_and(raw_digest)
        };
        if self.schema_version != 1
            || self.operation_id.is_nil()
            || self.old_binding.bridge_id.is_nil()
            || self.old_binding.lineage_id.is_nil()
            || self.old_binding.session_epoch.is_nil()
            || self.new_binding.session_epoch.is_nil()
            || !binding_valid(&self.old_binding)
            || !binding_valid(&self.new_binding)
            || self.old_binding.bridge_id != self.new_binding.bridge_id
            || self.old_binding.lineage_id != self.new_binding.lineage_id
            || self.old_binding.session_epoch == self.new_binding.session_epoch
            || !self
                .approval_digest
                .strip_prefix("lab-jcs-v1:plan:")
                .is_some_and(raw_digest)
            || !raw_digest(&self.observed_state_digest)
            || self.actor.trim().is_empty()
            || self.actor.chars().count() > 200
            || self.actor.chars().any(char::is_control)
            || self.decided_at.trim().is_empty()
            || self.decided_at.len() > 64
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    NotRun,
    Pass,
    Fail,
    Blocked,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GateCommandResult {
    /// Human-readable command with credentials and private inputs removed.
    pub redacted_argv: Vec<String>,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub invocation_digest: String,
    pub exit_code: i32,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub output_digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GateAssertion {
    pub name: String,
    pub passed: bool,
    /// Redacted observation summary; raw private evidence belongs in controlled artifacts.
    pub observed: String,
}
/// A scoped release-gate result. A parsed record is not proof that its commands
/// ran; the referenced artifacts and fixture hashes must be independently checked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GateEvidence {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub gate_id: String,
    pub status: GateStatus,
    pub version_matrix: BTreeMap<String, String>,
    pub fixture_hashes: BTreeMap<String, String>,
    pub commands: Vec<GateCommandResult>,
    pub assertions: Vec<GateAssertion>,
    pub artifact_refs: Vec<String>,
    pub failure_code: Option<String>,
}
impl GateEvidence {
    pub fn validate(&self) -> Result<(), crate::canonical::ContractError> {
        let invalid = || crate::canonical::ContractError("GATE_EVIDENCE_INVALID".into());
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if self.schema_version != 1
            || !matches!(
                self.gate_id.as_str(),
                "EV-01"
                    | "EV-02"
                    | "EV-03"
                    | "EV-04"
                    | "EV-05"
                    | "EV-06"
                    | "EV-07"
                    | "EV-08"
                    | "EV-09"
                    | "EV-10"
                    | "EV-11"
                    | "EV-12"
                    | "EV-13"
                    | "EV-14"
            )
            || self
                .version_matrix
                .iter()
                .any(|(name, version)| name.trim().is_empty() || version.trim().is_empty())
            || self
                .fixture_hashes
                .iter()
                .any(|(name, hash)| name.trim().is_empty() || !digest(hash))
            || self.commands.iter().any(|command| {
                command.redacted_argv.is_empty()
                    || command.redacted_argv.iter().any(|part| part.is_empty())
                    || !digest(&command.invocation_digest)
                    || !digest(&command.output_digest)
            })
            || self.assertions.iter().any(|assertion| {
                assertion.name.trim().is_empty() || assertion.observed.trim().is_empty()
            })
            || self
                .artifact_refs
                .iter()
                .any(|artifact| artifact.trim().is_empty() || artifact.len() > 1024)
        {
            return Err(invalid());
        }
        match self.status {
            GateStatus::NotRun => {
                if !self.version_matrix.is_empty()
                    || !self.fixture_hashes.is_empty()
                    || !self.commands.is_empty()
                    || !self.assertions.is_empty()
                    || !self.artifact_refs.is_empty()
                    || self.failure_code.is_some()
                {
                    return Err(invalid());
                }
            }
            GateStatus::Pass => {
                if self.version_matrix.is_empty()
                    || self.fixture_hashes.is_empty()
                    || self.commands.is_empty()
                    || self.commands.iter().any(|command| command.exit_code != 0)
                    || self.assertions.is_empty()
                    || self.assertions.iter().any(|assertion| !assertion.passed)
                    || self.artifact_refs.is_empty()
                    || self.failure_code.is_some()
                {
                    return Err(invalid());
                }
            }
            GateStatus::Fail => {
                if self.commands.is_empty()
                    || self.assertions.is_empty()
                    || !self
                        .failure_code
                        .as_ref()
                        .is_some_and(|code| !code.trim().is_empty())
                    || self.commands.iter().all(|command| command.exit_code == 0)
                        && self.assertions.iter().all(|assertion| assertion.passed)
                {
                    return Err(invalid());
                }
            }
            GateStatus::Blocked => {
                if !self
                    .failure_code
                    .as_ref()
                    .is_some_and(|code| !code.trim().is_empty())
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionCapabilityState {
    Unavailable,
    Declared,
    GateTested,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionCapability {
    pub action: String,
    pub state: ActionCapabilityState,
    pub gate_id: Option<String>,
    pub gate_evidence_digest: Option<String>,
    pub failure_code: Option<String>,
}
/// An inspection summary, never a substitute for checking the referenced gate
/// evidence, current collection binding, or native preconditions before a write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u16,
    pub protocol: String,
    pub anki_version: String,
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub anki_connect_source_digest: String,
    pub companion_version: String,
    pub resource_versions: BTreeMap<String, String>,
    pub actions: Vec<ActionCapability>,
    pub auth_constraints: Vec<String>,
    pub identity_constraints: Vec<String>,
    pub serialization_constraints: Vec<String>,
}
impl CapabilityReport {
    pub fn validate(&self) -> Result<(), crate::canonical::ContractError> {
        let invalid = || crate::canonical::ContractError("CAPABILITY_REPORT_INVALID".into());
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        let mut seen = std::collections::HashSet::new();
        let known_actions = [
            "labCapabilities",
            "labBegin",
            "labInspect",
            "labMutate",
            "labOperationStatus",
            "labRebind",
            "labEnd",
        ];
        if self.schema_version != 1
            || self.protocol != "lab-native-v1"
            || self.anki_version.trim().is_empty()
            || self.companion_version.trim().is_empty()
            || !digest(&self.anki_connect_source_digest)
            || self.actions.is_empty()
            || self.actions.iter().any(|action| {
                !seen.insert(&action.action)
                    || !known_actions.contains(&action.action.as_str())
                    || match action.state {
                        ActionCapabilityState::Unavailable => {
                            action.gate_id.is_some()
                                || action.gate_evidence_digest.is_some()
                                || !action
                                    .failure_code
                                    .as_ref()
                                    .is_some_and(|code| !code.trim().is_empty())
                        }
                        ActionCapabilityState::Declared => {
                            action.gate_id.is_some()
                                || action.gate_evidence_digest.is_some()
                                || action.failure_code.is_some()
                        }
                        ActionCapabilityState::GateTested => {
                            !action.gate_id.as_ref().is_some_and(|id| {
                                matches!(
                                    id.as_str(),
                                    "EV-01"
                                        | "EV-02"
                                        | "EV-03"
                                        | "EV-04"
                                        | "EV-05"
                                        | "EV-06"
                                        | "EV-07"
                                        | "EV-08"
                                        | "EV-09"
                                        | "EV-10"
                                        | "EV-11"
                                        | "EV-12"
                                        | "EV-13"
                                        | "EV-14"
                                )
                            }) || !action.gate_evidence_digest.as_deref().is_some_and(digest)
                                || action.failure_code.is_some()
                        }
                    }
            })
            || self
                .resource_versions
                .iter()
                .any(|(name, version)| name.trim().is_empty() || version.trim().is_empty())
            || [
                &self.auth_constraints,
                &self.identity_constraints,
                &self.serialization_constraints,
            ]
            .iter()
            .any(|items| items.is_empty() || items.iter().any(|item| item.trim().is_empty()))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub id: Uuid,
    pub operation_id: Uuid,
    pub originals: Vec<SourceRecord>,
    pub archives: Vec<SourceArchive>,
    pub media: Vec<MediaAsset>,
    pub before_digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupReceipt {
    pub id: Uuid,
    pub binding: CollectionBinding,
    pub path: String,
    pub checksum: String,
    pub scope_digest: String,
    pub includes_scheduling: bool,
    pub includes_media: bool,
    pub includes_schema: bool,
    pub verification_digest: String,
    pub restoration_evidence: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobMode {
    Prepare,
    Simulate,
    Apply,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: Uuid,
    pub mode: JobMode,
    pub settings: ResolvedSettings,
    pub plan_refs: Vec<String>,
    pub item_ids: Vec<Uuid>,
    pub pause_requested: bool,
    pub cancel_requested: bool,
}
