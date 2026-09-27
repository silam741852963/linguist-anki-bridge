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
    pub fields: BTreeMap<String, String>,
    pub model_manifest: String,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArchive {
    pub id: Uuid,
    pub source_id: Uuid,
    pub digest: String,
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
    pub language: Language,
    pub claim: String,
    pub source_url: Option<String>,
    pub ambiguous: bool,
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
    Anchor(Uuid),
    Segmentation(Vec<Uuid>),
    Duplicate {
        note_id: AnkiId,
        action: String,
    },
    Media(String),
    Cue {
        task: Task,
        text: String,
    },
    ContentVerified {
        evidence_ids: Vec<Uuid>,
    },
    SourceContentVerified {
        source_id: Uuid,
        evidence_ids: Vec<Uuid>,
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
    pub schema_version: u16,
    pub id: Uuid,
    pub revision: u32,
    pub parent_digest: Option<String>,
    pub settings: ResolvedSettings,
    pub binding: Option<CollectionBinding>,
    pub source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionReceipt>,
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
    NoteIds(Vec<String>),
    Query(String),
    Deck { name: String, query: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionReceipt {
    pub schema_version: u16,
    pub purpose: String,
    pub selector: SelectionInput,
    pub matched_note_ids: Vec<String>,
    pub selected_note_ids: Vec<String>,
    pub order: String,
    pub max_notes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_limit: Option<u64>,
}
impl PlanRevision {
    pub fn approval_digest(&self) -> Result<String, crate::canonical::ContractError> {
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
    fn validate(&self, plan: &PlanRevision) -> Result<(), crate::canonical::ContractError> {
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
            || plan
                .settings
                .values
                .get("selection.max_notes")
                .and_then(serde_json::Value::as_u64)
                != Some(self.max_notes)
            || plan
                .settings
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub id: Uuid,
    pub operation_id: Uuid,
    pub originals: Vec<SourceRecord>,
    pub archives: Vec<SourceArchive>,
    pub media: Vec<MediaAsset>,
    pub before_digest: String,
    pub verified_after_digest: Option<String>,
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
