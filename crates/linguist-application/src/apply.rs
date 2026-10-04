//! ALG-APPLY, ALG-MIGRATE and ALG-RECONCILE for one approved plan item over an
//! injected `lab-native-v1` port. Authority, identity, source, model, deck,
//! media and checkpoint checks all run before any durable intent. The intent,
//! a fresh pre-write snapshot and the journal are stored before each effect,
//! and only an actual read-back that matches the desired post-state commits.
//! There is no generic retry or rewind: an uncertain effect is reconciled
//! against native status and collection evidence, never blindly re-sent.
use crate::backup::{
    CheckpointAuthorization, DependentScope, PortFailure, Result, binding_digest,
    require_checkpoint,
};
use crate::checkpoint::CoverageRequirement;
use crate::model_install::{ObservedModel, exact_match, manifest_digest};
use linguist_core::{
    canonical,
    document::{AnkiId, LearningContent, LearningDocument, Task},
    model::ManagedModel,
    records::{
        CardState, CollectionBinding, JournalStep, NativeOperationReceipt, NativeReadback,
        NativeReceiptState, OperationJournal, OperationState, PlanRevision, ResumeBindingDecision,
        Snapshot, SourceArchive, SourceRecord, StepState, TargetModelKind,
    },
    validation::{Issue, Severity},
};
use linguist_store::{
    Store, apply::ApplyOperationRecord, journal::JournalVersion, lease::LeaseToken,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// One card as read through consistent native inspection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedCard {
    pub id: i64,
    pub ordinal: u16,
    pub deck_id: i64,
    /// Anki `odid`: non-zero while the card sits in a filtered deck.
    pub original_deck_id: i64,
    pub scheduler: BTreeMap<String, String>,
    pub history_digest: String,
    pub review_count: u64,
}

/// One note with its model identity and every card. `model_manifest_digest`
/// must be computed exactly as source capture computes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedNote {
    pub id: i64,
    pub model_id: i64,
    pub model_name: String,
    pub model_manifest_digest: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub cards: Vec<ObservedCard>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedMedia {
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedDeck {
    pub id: i64,
    pub name: String,
    pub filtered: bool,
}

/// Temporary native writer token (`labBegin`) with its monotonic fence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerToken {
    pub token: Uuid,
    pub fence: u64,
}

/// `labOperationStatus` for one native operation UUID. Only `Verified` is a
/// native success claim, and it is still checked against an actual read-back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum NativeStatus {
    /// The companion ledger has no row for this UUID.
    Absent,
    Queued,
    Running,
    FailedBeforeWrite {
        reason: String,
    },
    Unknown {
        reason: String,
    },
    Verified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinalMapping {
    pub source: u16,
    pub target: u16,
}

/// Explicit mapped note-type change. Retained cards keep their IDs and history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub source_model_id: i64,
    pub target_model_id: i64,
    pub target_model_name: String,
    pub ordinal_map: Vec<OrdinalMapping>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateNote {
    pub note_id: i64,
    /// Content precondition the native critical section compares before writing.
    pub expected_pre_digest: String,
    pub migration: Option<Migration>,
    pub fields: BTreeMap<String, String>,
    /// Added tags only; no existing tag is removed.
    pub add_tags: Vec<String>,
    /// Every card of the note ends in this deck.
    pub deck_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardDeck {
    pub card_id: i64,
    pub deck_id: i64,
}

/// Exact reverse effect for ALG-RESTORE: an optional reverse mapped
/// note-type change, the complete field set, the exact tag set and one deck
/// per kept card. Cards keep their IDs and current scheduling and history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreNote {
    pub note_id: i64,
    /// Content precondition captured by the restore preflight.
    pub expected_pre_digest: String,
    pub migration: Option<Migration>,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
    /// Every card that remains after the restore, with its deck.
    pub card_decks: Vec<CardDeck>,
    /// Unstudied cards the reverse mapping removes; reviewed explicitly.
    pub removed_card_ids: Vec<i64>,
}

/// Exact typed `labMutate` body. `CreateNote` carries the complete create-note
/// wire envelope accepted by `linguist_anki::native::validate_create_note_intent`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "variant")]
pub enum Effect {
    StoreMedia {
        filename: String,
        sha256: String,
        size_bytes: u64,
        /// Local content-addressed asset the adapter stages by reference.
        staged_asset: String,
    },
    CreateNote {
        envelope: serde_json::Value,
    },
    UpdateNote(UpdateNote),
    RestoreNote(RestoreNote),
    /// Remove one note this app created, only while its content is unchanged
    /// and none of its cards has any review.
    DeleteUnstudiedCreatedNote {
        note_id: i64,
        expected_pre_digest: String,
    },
}

impl Effect {
    pub fn variant(&self) -> &'static str {
        match self {
            Self::StoreMedia { .. } => "store_media",
            Self::CreateNote { .. } => "create_note",
            Self::UpdateNote(_) => "update_note",
            Self::RestoreNote(_) => "restore_note",
            Self::DeleteUnstudiedCreatedNote { .. } => "delete_unstudied_created_note",
        }
    }
    /// Raw SHA-256 of the exact canonical wire bytes.
    pub fn payload_digest(&self) -> Result<String> {
        let bytes = match self {
            Self::CreateNote { envelope } => canonical::bytes(envelope),
            other => canonical::bytes(other),
        }
        .map_err(|e| e.to_string())?;
        Ok(canonical::asset_digest(&bytes))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MutationRequest {
    /// Native operation UUID: the journal step ID. Re-sending the same UUID
    /// and payload is deduplicated by the companion ledger.
    pub operation_id: Uuid,
    pub parent_operation_id: Uuid,
    pub approval_digest: String,
    pub binding: CollectionBinding,
    pub owner: OwnerToken,
    pub payload_digest: String,
    pub effect: Effect,
}

/// The native boundary. Reads are side-effect free; `mutate` submits one typed
/// effect and polls its status under the adapter's deadline.
pub trait ApplyPort {
    /// Current execution binding from `labCapabilities` with a live session.
    fn execution_binding(&mut self) -> Result<CollectionBinding>;
    /// Declared and pinned-compatible `labMutate` variants.
    fn mutation_variants(&mut self) -> Result<Vec<String>>;
    fn begin(&mut self, binding: &CollectionBinding, approval_digest: &str) -> Result<OwnerToken>;
    fn end(&mut self, owner: &OwnerToken) -> Result<()>;
    fn note(&mut self, note_id: i64) -> Result<Option<ObservedNote>>;
    fn notes_tagged(&mut self, tag: &str) -> Result<Vec<ObservedNote>>;
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>>;
    fn deck(&mut self, name: &str) -> Result<Option<ObservedDeck>>;
    fn media(&mut self, filename: &str) -> Result<Option<ObservedMedia>>;
    /// Exact bytes of one collection media file, for the pre-write archive.
    /// Without this capability source media is recorded by name and hash only.
    fn media_bytes(&mut self, _filename: &str, _max_bytes: u64) -> Result<Option<Vec<u8>>> {
        Err("CAPABILITY_UNAVAILABLE: native media byte reads".into())
    }
    fn mutate(
        &mut self,
        request: &MutationRequest,
    ) -> std::result::Result<NativeStatus, PortFailure>;
    fn status(&mut self, operation_id: Uuid) -> Result<NativeStatus>;
}

/// Desired or observed card. New cards have no ID, scheduler or history and
/// must have zero reviews; retained cards keep the freshly captured values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardProjection {
    pub ordinal: u16,
    pub deck_id: i64,
    pub id: Option<i64>,
    pub scheduler: Option<BTreeMap<String, String>>,
    pub history_digest: Option<String>,
    pub review_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteProjection {
    pub model_name: String,
    pub model_manifest_digest: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub cards: Vec<CardProjection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaProjection {
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemAction {
    Create,
    Update,
    Migrate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentStep {
    pub step_id: Uuid,
    pub effect: Effect,
    pub payload_digest: String,
    pub precondition_digest: String,
    /// Canonical bytes of the expected read-back projection for this step.
    pub expected: serde_json::Value,
    pub expected_digest: String,
}

/// Frozen apply intent stored in `apply_operations` before the journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyIntent {
    pub schema_version: u16,
    pub action: ItemAction,
    pub note_id: Option<i64>,
    pub marker_tag: Option<String>,
    pub target_model_name: String,
    pub target_model_id: i64,
    pub target_manifest_digest: String,
    pub deck_id: i64,
    pub retained_card_ids: Vec<i64>,
    pub pre_state: Option<NoteProjection>,
    pub desired: NoteProjection,
    pub steps: Vec<IntentStep>,
    pub checkpoint: serde_json::Value,
    pub reused_media: Vec<MediaProjection>,
}

pub struct ApplyRequest<'a> {
    /// The current invocation's explicit `--apply`.
    pub apply: bool,
    pub plan_id: Uuid,
    pub revision: u32,
    pub digest: &'a str,
    pub item_id: Uuid,
    pub approval_id: Uuid,
    pub checkpoint_id: Uuid,
    pub group_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
    pub max_media_bytes: u64,
    /// Explicit acceptance of the schema/full-sync warning for a mapped migration.
    pub accept_schema_change: bool,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApplyItemOutcome {
    pub item_id: Uuid,
    pub operation_id: Option<Uuid>,
    pub action: Option<ItemAction>,
    pub state: OperationState,
    pub note_id: Option<i64>,
    pub snapshot_id: Option<Uuid>,
    pub checkpoint_id: Uuid,
    pub receipt_digest: Option<String>,
    pub issues: Vec<Issue>,
    pub next_command: Option<String>,
}

pub(crate) fn issue(code: &str, message: impl Into<String>) -> Issue {
    let mut issue = Issue::new(code, Severity::Error, None, message);
    issue.stage = "apply".into();
    issue
}

pub fn reconcile_command_for(operation: Uuid) -> String {
    reconcile_command(operation)
}

fn reconcile_command(operation: Uuid) -> String {
    format!("linguist-anki-bridge recover reconcile {operation} --apply")
}

pub(crate) fn digest_of<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(canonical::asset_digest(
        &canonical::bytes(value).map_err(|e| e.to_string())?,
    ))
}

/// Stable approval identity: everything except the per-load session epoch and
/// the capability declaration.
pub fn same_collection(a: &CollectionBinding, b: &CollectionBinding) -> bool {
    a.endpoint == b.endpoint
        && a.profile_fingerprint == b.profile_fingerprint
        && a.path_fingerprint == b.path_fingerprint
        && a.bridge_id == b.bridge_id
        && a.lineage_id == b.lineage_id
}

pub(crate) fn loopback(endpoint: &str) -> bool {
    url::Url::parse(endpoint).is_ok_and(|url| {
        matches!(
            url.host(),
            Some(url::Host::Domain("localhost"))
                | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
                | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        )
    })
}

/// Template ordinal of each managed task.
pub fn task_ordinal(model: &ManagedModel, task: Task) -> Option<u16> {
    let name = match task {
        Task::Comprehension => "Comprehension",
        Task::Production => "Production",
        Task::Spelling => "Spelling",
        Task::Recognition => "Recognition",
        Task::Application => "Application",
    };
    model
        .templates
        .iter()
        .find(|template| template.name == name)
        .map(|template| template.ordinal)
}

pub(crate) fn ordinal_task(model: &ManagedModel, ordinal: u16) -> Option<Task> {
    [
        Task::Comprehension,
        Task::Production,
        Task::Spelling,
        Task::Recognition,
        Task::Application,
    ]
    .into_iter()
    .find(|task| task_ordinal(model, *task) == Some(ordinal))
}

/// Reviewed purpose for a document: `{japanese|english}_{vocab|grammar}`.
pub fn purpose(document: &LearningDocument) -> Result<String> {
    let language = match document
        .target_language
        .as_str()
        .split('-')
        .next()
        .unwrap_or("")
    {
        "ja" => "japanese",
        "en" => "english",
        _ => return Err("APPLY_PURPOSE_UNRESOLVED".into()),
    };
    let kind = match document.content {
        LearningContent::Vocabulary(_) => "vocab",
        LearningContent::Grammar(_) => "grammar",
    };
    Ok(format!("{language}_{kind}"))
}

pub(crate) fn target_deck_name(
    plan: &PlanRevision,
    document: &LearningDocument,
) -> Result<Option<String>> {
    let key = format!("purposes.{}.target_deck", purpose(document)?);
    Ok(plan
        .settings
        .values
        .get(&key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

/// The one captured Anki source note for an update, if any.
pub fn source_note(document: &LearningDocument) -> Result<Option<(&SourceRecord, i64)>> {
    let notes: Vec<_> = document
        .sources
        .iter()
        .filter_map(|source| {
            source
                .location
                .strip_prefix("anki_note:")
                .map(|id| (source, id))
        })
        .collect();
    match notes.as_slice() {
        [] => Ok(None),
        [(source, id)] => {
            let id = id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or("APPLY_SOURCE_INVALID")?;
            Ok(Some((source, id)))
        }
        _ => Err("APPLY_SOURCE_AMBIGUOUS: more than one Anki source note".into()),
    }
}

pub(crate) fn sorted_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Content precondition: model, fields, tags and card placement, excluding
/// scheduling so that normal study between preparation and apply is allowed.
pub fn content_digest(note: &ObservedNote) -> Result<String> {
    let mut cards: Vec<_> = note
        .cards
        .iter()
        .map(|card| (card.id, card.ordinal, card.deck_id, card.original_deck_id))
        .collect();
    cards.sort();
    canonical::digest(
        "lab-apply-precondition-v1",
        &(
            &note.model_name,
            &note.model_manifest_digest,
            &note.fields,
            sorted_tags(&note.tags),
            cards,
        ),
    )
    .map_err(|e| e.to_string())
}

/// Read-back projection. Retained card IDs keep their scheduling evidence;
/// any other card is reported as new with its actual review count.
pub fn project(
    note: &ObservedNote,
    retained: &BTreeSet<i64>,
    exact_model: Option<(i64, &str)>,
) -> NoteProjection {
    let model_manifest_digest = match exact_model {
        Some((id, digest)) if id == note.model_id => digest.to_owned(),
        _ => format!("unverified:{}", note.model_manifest_digest),
    };
    let mut cards: Vec<_> = note
        .cards
        .iter()
        .map(|card| {
            let kept = retained.contains(&card.id);
            CardProjection {
                ordinal: card.ordinal,
                deck_id: card.deck_id,
                id: kept.then_some(card.id),
                scheduler: kept.then(|| card.scheduler.clone()),
                history_digest: kept.then(|| card.history_digest.clone()),
                review_count: card.review_count,
            }
        })
        .collect();
    cards.sort();
    NoteProjection {
        model_name: note.model_name.clone(),
        model_manifest_digest,
        fields: note.fields.clone(),
        tags: sorted_tags(&note.tags),
        cards,
    }
}

/// Plan-time checks shared by preview and apply. They read only local state.
pub struct AuthorizedItem {
    pub plan: PlanRevision,
    pub document: LearningDocument,
    pub rendered: linguist_core::render::RenderedNote,
    pub approval_digest: String,
}

pub fn authorize(
    store: &Store,
    plan_id: Uuid,
    revision: u32,
    digest: &str,
    item_id: Uuid,
    approval_id: Uuid,
) -> Result<AuthorizedItem> {
    authorize_role(
        store,
        plan_id,
        revision,
        digest,
        item_id,
        approval_id,
        Role::Standalone,
    )
}

/// How one item is written: alone, or as one unit of an ALG-SPLIT group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Standalone,
    /// A non-anchor unit: always a fresh note, whatever source it cites.
    SplitChild,
    /// The one unit that keeps the source note's identity and history.
    SplitAnchor,
}

/// Operation identity allocated before any write.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub operation: Uuid,
    pub main_step: Uuid,
    pub role: Role,
}

pub(crate) fn authorize_role(
    store: &Store,
    plan_id: Uuid,
    revision: u32,
    digest: &str,
    item_id: Uuid,
    approval_id: Uuid,
    role: Role,
) -> Result<AuthorizedItem> {
    let plan = store.revision(plan_id, revision)?;
    if store.latest_revision(plan_id)? != revision {
        return Err("APPLY_REVISION_STALE: a newer plan revision exists".into());
    }
    let approval_digest = plan.approval_digest().map_err(|e| e.to_string())?;
    if approval_digest != digest {
        return Err("APPLY_DIGEST_MISMATCH".into());
    }
    let approval = store.approval(approval_id)?.approval;
    if approval.plan_id != plan_id
        || approval.revision != revision
        || approval.digest != digest
        || !approval.item_ids.contains(&item_id)
    {
        return Err("APPLY_APPROVAL_MISMATCH: approval does not cover this exact item".into());
    }
    let evidence = linguist_core::plan_validation::inspect(&plan).map_err(|e| e.to_string())?;
    if !evidence
        .items
        .iter()
        .any(|item| item.document_id == item_id && item.content_ready)
    {
        return Err("APPLY_ITEM_NOT_READY".into());
    }
    let group = plan
        .grammar_groups
        .iter()
        .find(|group| group.units.contains(&item_id));
    let expected = match group {
        None => Role::Standalone,
        Some(group) if group.anchor_document == item_id => Role::SplitAnchor,
        Some(_) => Role::SplitChild,
    };
    if role != expected {
        return Err(if role == Role::Standalone {
            "APPLY_SPLIT_GROUP_REQUIRED: a reviewed grammar split unit is applied only with its whole group (children first, then the anchor)".into()
        } else {
            "SPLIT_UNIT_ROLE_MISMATCH".into()
        });
    }
    let document = plan
        .documents
        .iter()
        .find(|doc| doc.id == item_id)
        .cloned()
        .ok_or("APPLY_ITEM_NOT_FOUND")?;
    let rendered = plan
        .rendered
        .iter()
        .find(|note| note.document_id == item_id)
        .cloned()
        .ok_or("APPLY_ITEM_NOT_RENDERED")?;
    Ok(AuthorizedItem {
        plan,
        document,
        rendered,
        approval_digest,
    })
}

/// True when every effect of an earlier attempt is either a verified
/// content-addressed media upload or a native failure proven to have had no
/// collection effect; a new attempt may then supersede it.
pub(crate) fn superseded_safely(journal: &OperationJournal) -> bool {
    journal.state == OperationState::NeedsRecovery
        && journal.steps.iter().all(|step| {
            step.state == StepState::ObservedFailure
                || (step.state == StepState::Verified && step.action == "store_media")
        })
        && journal
            .steps
            .iter()
            .any(|step| step.state == StepState::ObservedFailure)
}

fn prior_attempts(store: &Store, plan_id: Uuid, item_id: Uuid) -> Result<()> {
    for record in store.apply_operations_for_item(plan_id, item_id)? {
        match store.journal(record.operation_id) {
            Ok(version) if version.journal.state == OperationState::Committed => {
                return Err(format!(
                    "APPLY_ALREADY_COMMITTED: operation {} already applied this item",
                    record.operation_id
                ));
            }
            Ok(version) if version.pending_recovery && !superseded_safely(&version.journal) => {
                return Err(format!(
                    "APPLY_RECOVERY_REQUIRED: reconcile operation {} first",
                    record.operation_id
                ));
            }
            Ok(_) => {}
            // The intent record precedes the journal; no journal means nothing was sent.
            Err(code) if code == "JOURNAL_NOT_FOUND" => {}
            Err(code) => return Err(code),
        }
    }
    Ok(())
}

pub(crate) struct Journal<'a> {
    pub store: &'a mut Store,
    pub version: JournalVersion,
    /// Command that resumes this journal after a local durability failure.
    pub recovery: String,
}

impl Journal<'_> {
    pub fn advance(&mut self, change: impl FnOnce(&mut OperationJournal)) -> Result<()> {
        let mut next = self.version.journal.clone();
        change(&mut next);
        self.version = self
            .store
            .append_journal(&next, Some(&self.version))
            .map_err(|code| {
                format!(
                    "APPLY_LOCAL_DURABILITY_FAILED: {code}; stop all writes and run {}",
                    self.recovery
                )
            })?;
        Ok(())
    }
    pub fn note_issue(&mut self, code: &str, message: String) -> Result<()> {
        self.advance(|j| j.issues.push(issue(code, message)))
    }
}

/// Everything resolved against the live collection before any intent.
struct Preflight {
    action: ItemAction,
    observed: Option<ObservedNote>,
    target_model_id: i64,
    target_digest: String,
    deck_id: i64,
    migration: Option<Migration>,
    media_steps: Vec<(MediaProjection, String)>,
    reused_media: Vec<MediaProjection>,
}

fn media_filename_safe(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | ':' | '\0') || ch.is_control())
}

fn source_conflicts(source: &SourceRecord, note: &ObservedNote) -> Vec<&'static str> {
    let mut out = Vec::new();
    if source.fields != note.fields {
        out.push("fields");
    }
    if sorted_tags(&source.tags) != sorted_tags(&note.tags) {
        out.push("tags");
    }
    if source.model_manifest != note.model_manifest_digest {
        out.push("model");
    }
    if !source.cards.is_empty() {
        let captured: BTreeSet<_> = source
            .cards
            .iter()
            .map(|card| {
                (
                    String::from(card.id.clone()),
                    String::from(card.deck_id.clone()),
                )
            })
            .collect();
        let current: BTreeSet<_> = note
            .cards
            .iter()
            .map(|card| (card.id.to_string(), card.deck_id.to_string()))
            .collect();
        if captured != current {
            out.push("cards");
        }
    }
    out
}

fn migration_plan(
    document: &LearningDocument,
    source: &SourceRecord,
    note: &ObservedNote,
    target: &ManagedModel,
    target_model_id: i64,
) -> Result<Migration> {
    let kind = match document.content {
        LearningContent::Vocabulary(_) => TargetModelKind::Vocabulary,
        LearningContent::Grammar(_) => TargetModelKind::Grammar,
    };
    let map = document
        .task_maps
        .iter()
        .find(|map| map.source_id == source.id)
        .ok_or("APPLY_MIGRATION_MAP_MISSING: an explicit card-template mapping is required")?;
    map.validate().map_err(|e| e.to_string())?;
    if map.target_model != kind || map.source_model_digest != note.model_manifest_digest {
        return Err(
            "APPLY_MIGRATION_MAP_STALE: mapping does not match the observed source model".into(),
        );
    }
    let mut targets = BTreeSet::new();
    let mut ordinal_map = Vec::new();
    for entry in &map.entries {
        if task_ordinal(target, entry.target_task) != Some(entry.target_ordinal)
            || !targets.insert(entry.target_ordinal)
        {
            return Err("APPLY_MIGRATION_MAP_INVALID".into());
        }
        ordinal_map.push(OrdinalMapping {
            source: entry.source_ordinal,
            target: entry.target_ordinal,
        });
    }
    // Native note-type change deletes cards whose template is not mapped.
    for card in &note.cards {
        if !ordinal_map.iter().any(|m| m.source == card.ordinal) {
            return Err(format!(
                "APPLY_MIGRATION_DROPS_CARD: card {} (ordinal {}) has no mapped target task",
                card.id, card.ordinal
            ));
        }
    }
    ordinal_map.sort_by_key(|m| m.source);
    Ok(Migration {
        source_model_id: note.model_id,
        target_model_id,
        target_model_name: target.name.clone(),
        ordinal_map,
    })
}

fn preflight(
    store: &Store,
    port: &mut dyn ApplyPort,
    item: &AuthorizedItem,
    request: &ApplyRequest,
    role: Role,
) -> Result<Preflight> {
    let target = &item.rendered.model;
    let target_digest = manifest_digest(target)?;
    let models = port.models_named(&target.name)?;
    let target_model_id = match models.as_slice() {
        [] => {
            return Err(format!(
                "APPLY_MODEL_MISSING: run `models install {}` first",
                target.name
            ));
        }
        [one] if exact_match(one, target) => one.id,
        [_] => {
            return Err("MODEL_NAME_COLLISION: same-name model differs; never overwritten".into());
        }
        _ => return Err("MODEL_NAME_AMBIGUOUS".into()),
    };
    let deck_name = target_deck_name(&item.plan, &item.document)?;
    let target_deck = match &deck_name {
        Some(name) => {
            let deck = port
                .deck(name)?
                .ok_or("APPLY_TARGET_DECK_MISSING: the reviewed target deck does not exist")?;
            if deck.filtered {
                return Err("APPLY_TARGET_DECK_FILTERED".into());
            }
            Some(deck.id)
        }
        None => None,
    };
    // A split child cites the shared source only as a reference; it is
    // always created as a fresh note with fresh scheduling.
    let source = match role {
        Role::SplitChild => None,
        _ => source_note(&item.document)?,
    };
    if role == Role::SplitAnchor && source.is_none() {
        return Err("SPLIT_ANCHOR_SOURCE_MISSING".into());
    }
    let (action, observed, deck_id, migration) = match source {
        None => (
            ItemAction::Create,
            None,
            target_deck.ok_or("APPLY_TARGET_DECK_UNCONFIGURED: map a target deck first")?,
            None,
        ),
        Some((source, note_id)) => {
            let note = port.note(note_id)?.ok_or("APPLY_SOURCE_MISSING")?;
            let conflicts = source_conflicts(source, &note);
            if !conflicts.is_empty() {
                return Err(format!(
                    "APPLY_SOURCE_CONFLICT: source changed since preparation ({}); original preserved",
                    conflicts.join(",")
                ));
            }
            if note.cards.iter().any(|card| card.original_deck_id != 0) {
                return Err(
                    "APPLY_FILTERED_DECK_BLOCKS: return cards to their home deck first".into(),
                );
            }
            let deck_id = match target_deck {
                Some(id) => id,
                None => {
                    let decks: BTreeSet<_> = note.cards.iter().map(|c| c.deck_id).collect();
                    match decks.into_iter().collect::<Vec<_>>().as_slice() {
                        [one] => *one,
                        _ => return Err("APPLY_DECK_MAPPING_AMBIGUOUS".into()),
                    }
                }
            };
            if note.model_name == target.name {
                if note.model_id != target_model_id {
                    return Err("APPLY_MODEL_MISMATCH".into());
                }
                (ItemAction::Update, Some(note), deck_id, None)
            } else {
                if !request.accept_schema_change {
                    return Err("APPLY_SCHEMA_CHANGE_NOT_ACCEPTED: mapped migration requires an accepted schema/full-sync warning".into());
                }
                let migration =
                    migration_plan(&item.document, source, &note, target, target_model_id)?;
                (ItemAction::Migrate, Some(note), deck_id, Some(migration))
            }
        }
    };
    let mut media_steps = Vec::new();
    let mut reused_media = Vec::new();
    for digest in &item.rendered.media_digests {
        let asset = item
            .document
            .media
            .iter()
            .find(|asset| &asset.digest == digest)
            .ok_or("APPLY_MEDIA_MANIFEST_MISSING")?;
        if !media_filename_safe(&asset.filename) {
            return Err("APPLY_MEDIA_FILENAME_UNSAFE".into());
        }
        if asset.size_bytes > request.max_media_bytes {
            return Err("APPLY_MEDIA_TOO_LARGE".into());
        }
        let bytes = store.asset(digest, request.max_media_bytes)?;
        let sha256 = canonical::asset_digest(&bytes);
        if &sha256 != digest || bytes.len() as u64 != asset.size_bytes {
            return Err("APPLY_MEDIA_STAGED_MISMATCH".into());
        }
        let wanted = MediaProjection {
            filename: asset.filename.clone(),
            sha256,
            size_bytes: asset.size_bytes,
        };
        match port.media(&asset.filename)? {
            None => media_steps.push((wanted, digest.clone())),
            Some(existing) if existing.sha256 == wanted.sha256 => reused_media.push(wanted),
            Some(_) => {
                return Err(format!(
                    "APPLY_MEDIA_FILENAME_COLLISION: {} exists with different bytes; revise the plan with a new filename, nothing is overwritten",
                    asset.filename
                ));
            }
        }
    }
    let mut variants: BTreeSet<&str> = BTreeSet::new();
    variants.insert(if action == ItemAction::Create {
        "create_note"
    } else {
        "update_note"
    });
    if !media_steps.is_empty() {
        variants.insert("store_media");
    }
    let available = port.mutation_variants()?;
    if let Some(missing) = variants
        .iter()
        .find(|variant| !available.iter().any(|a| a == *variant))
    {
        return Err(format!(
            "CAPABILITY_UNAVAILABLE: native {missing} variant is not declared"
        ));
    }
    Ok(Preflight {
        action,
        observed,
        target_model_id,
        target_digest,
        deck_id,
        migration,
        media_steps,
        reused_media,
    })
}

fn desired_projection(
    item: &AuthorizedItem,
    pre: &Preflight,
    marker: Option<&str>,
) -> Result<(NoteProjection, Vec<i64>, Option<NoteProjection>)> {
    let target = &item.rendered.model;
    let requested: BTreeSet<u16> = item
        .rendered
        .tasks
        .iter()
        .map(|task| task_ordinal(target, *task).ok_or_else(|| "APPLY_TASK_UNMAPPED".to_owned()))
        .collect::<Result<_>>()?;
    let mut tags = item.document.tags.clone();
    let mut cards = Vec::new();
    let mut retained = Vec::new();
    let mut pre_state = None;
    match &pre.observed {
        None => {
            tags.push(marker.ok_or("APPLY_MARKER_MISSING")?.to_owned());
            for ordinal in &requested {
                cards.push(CardProjection {
                    ordinal: *ordinal,
                    deck_id: pre.deck_id,
                    id: None,
                    scheduler: None,
                    history_digest: None,
                    review_count: 0,
                });
            }
        }
        Some(note) => {
            tags.extend(note.tags.iter().cloned());
            let all: BTreeSet<i64> = note.cards.iter().map(|c| c.id).collect();
            pre_state = Some(project(note, &all, None));
            let mut covered = BTreeSet::new();
            for card in &note.cards {
                let ordinal = match &pre.migration {
                    Some(migration) => {
                        migration
                            .ordinal_map
                            .iter()
                            .find(|m| m.source == card.ordinal)
                            .ok_or("APPLY_MIGRATION_DROPS_CARD")?
                            .target
                    }
                    None => card.ordinal,
                };
                covered.insert(ordinal);
                retained.push(card.id);
                cards.push(CardProjection {
                    ordinal,
                    deck_id: pre.deck_id,
                    id: Some(card.id),
                    scheduler: Some(card.scheduler.clone()),
                    history_digest: Some(card.history_digest.clone()),
                    review_count: card.review_count,
                });
            }
            for ordinal in requested.difference(&covered) {
                cards.push(CardProjection {
                    ordinal: *ordinal,
                    deck_id: pre.deck_id,
                    id: None,
                    scheduler: None,
                    history_digest: None,
                    review_count: 0,
                });
            }
        }
    }
    cards.sort();
    Ok((
        NoteProjection {
            model_name: target.name.clone(),
            model_manifest_digest: pre.target_digest.clone(),
            fields: item.rendered.fields.clone(),
            tags: sorted_tags(&tags),
            cards,
        },
        retained,
        pre_state,
    ))
}

fn create_envelope(
    item: &AuthorizedItem,
    step_id: Uuid,
    desired: &NoteProjection,
    deck_id: i64,
    checkpoint_digest: &str,
    binding: &CollectionBinding,
) -> Result<serde_json::Value> {
    let envelope = serde_json::json!({
        "schema_version": 1,
        "variant": "create_note",
        "body": {
            "model_name": desired.model_name,
            "model_manifest_digest": desired.model_manifest_digest,
            "deck_id": deck_id.to_string(),
            "fields": desired.fields,
            "tags": desired.tags,
            "marker_tag": format!("lab_op_{}", step_id.simple()),
            "source_plan_digest": item.approval_digest,
            "checkpoint_digest": checkpoint_digest,
            "binding": {
                "profile_fingerprint": binding.profile_fingerprint,
                "path_fingerprint": binding.path_fingerprint,
            },
            "expected_absent": true,
        }
    });
    let bytes = canonical::bytes(&envelope).map_err(|e| e.to_string())?;
    linguist_anki::native::validate_create_note_intent(&bytes, step_id, &item.approval_digest)?;
    Ok(envelope)
}

/// Archive the exact bytes of media the original fields reference, when the
/// native port can read them and they match the collection's recorded hash.
fn archive_source_media(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    note: &ObservedNote,
    source_id: Uuid,
    max_bytes: u64,
) -> Vec<linguist_core::records::MediaAsset> {
    let Ok(discovery) = crate::capture::discover_media(&note.fields, 100 * 1024 * 1024, 10000)
    else {
        return vec![];
    };
    let names: BTreeSet<String> = discovery
        .references
        .into_iter()
        .map(|reference| reference.filename)
        .collect();
    let mut out = Vec::new();
    for name in names {
        let Ok(Some(observed)) = port.media(&name) else {
            continue;
        };
        let Ok(Some(bytes)) = port.media_bytes(&name, max_bytes) else {
            continue;
        };
        if canonical::asset_digest(&bytes) != observed.sha256
            || bytes.len() as u64 != observed.size_bytes
        {
            continue;
        }
        let Ok(digest) = store.publish_asset(&bytes, max_bytes) else {
            continue;
        };
        out.push(linguist_core::records::MediaAsset {
            digest,
            filename: name,
            original_filename: None,
            size_bytes: observed.size_bytes,
            mime: "application/octet-stream".into(),
            owner: linguist_core::records::MediaOwner::Source,
            role: linguist_core::records::MediaRole::Archive,
            source_id: Some(source_id),
            attribution: "anki_collection".into(),
            license: None,
        });
    }
    out
}

pub(crate) fn fresh_snapshot(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    operation: Uuid,
    note: Option<&ObservedNote>,
    target: &ManagedModel,
    migration: Option<&Migration>,
    now_ms: u64,
) -> Result<Snapshot> {
    let mut originals = Vec::new();
    let mut archives = Vec::new();
    let mut media: Vec<linguist_core::records::MediaAsset> = vec![];
    if let Some(note) = note {
        let bytes = canonical::bytes(note).map_err(|e| e.to_string())?;
        let digest = store.publish_asset(&bytes, 100 * 1024 * 1024)?;
        let cards = note
            .cards
            .iter()
            .map(|card| {
                let ordinal = migration
                    .and_then(|m| m.ordinal_map.iter().find(|o| o.source == card.ordinal))
                    .map(|o| o.target)
                    .unwrap_or(card.ordinal);
                Ok(CardState {
                    id: AnkiId::try_from(card.id.to_string())?,
                    task: ordinal_task(target, ordinal)
                        .or_else(|| ordinal_task(target, card.ordinal))
                        .ok_or("APPLY_TASK_UNMAPPED")?,
                    deck_id: AnkiId::try_from(card.deck_id.to_string())?,
                    home_deck_id: AnkiId::try_from(card.deck_id.to_string())?,
                    scheduler: card.scheduler.clone(),
                    history_digest: card.history_digest.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let source_id = Uuid::new_v4();
        originals.push(SourceRecord {
            id: source_id,
            kind: "lab_apply_prestate_v1".into(),
            location: format!("anki_note:{}", note.id),
            digest: digest.clone(),
            text: None,
            fields: note.fields.clone(),
            model_manifest: note.model_manifest_digest.clone(),
            template_manifest: None,
            captured_at_unix_seconds: Some(now_ms / 1000),
            tags: note.tags.clone(),
            cards,
            media_refs: vec![],
        });
        archives.push(SourceArchive {
            id: Uuid::new_v4(),
            source_id,
            digest: digest.clone(),
            original_text: None,
            original_fields: note.fields.clone(),
            asset_digests: vec![digest],
        });
        media = archive_source_media(store, port, note, source_id, 100 * 1024 * 1024);
    }
    let before_digest = canonical::digest("snapshot-original", &(&originals, &archives, &media))
        .map_err(|e| e.to_string())?;
    let snapshot = Snapshot {
        id: Uuid::new_v4(),
        operation_id: operation,
        originals,
        archives,
        media,
        before_digest,
    };
    store.publish_snapshot(&snapshot)?;
    Ok(snapshot)
}

/// ALG-APPLY for one item under an already-held collection-writer lease.
/// Every refusal before the first `request_started` performs zero mutations.
pub fn apply_item(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &ApplyRequest,
) -> Result<ApplyItemOutcome> {
    apply_item_with(
        store,
        lease,
        port,
        request,
        &Slot {
            operation: Uuid::new_v4(),
            main_step: Uuid::new_v4(),
            role: Role::Standalone,
        },
    )
}

/// Everything ALG-APPLY checks before a durable intent, for one item in its
/// role. It reads the store and the collection and writes nothing.
pub(crate) struct Checked {
    item: AuthorizedItem,
    current: CollectionBinding,
    pre: Preflight,
    authorization: CheckpointAuthorization,
}

impl Checked {
    pub fn observed(&self) -> Option<&ObservedNote> {
        self.pre.observed.as_ref()
    }
    pub fn model(&self) -> &ManagedModel {
        &self.item.rendered.model
    }
    pub fn migration(&self) -> Option<&Migration> {
        self.pre.migration.as_ref()
    }
}

pub(crate) fn check_item(
    store: &Store,
    port: &mut dyn ApplyPort,
    request: &ApplyRequest,
    role: Role,
) -> Result<Checked> {
    let item = authorize_role(
        store,
        request.plan_id,
        request.revision,
        request.digest,
        request.item_id,
        request.approval_id,
        role,
    )?;
    prior_attempts(store, request.plan_id, request.item_id)?;
    let approved =
        item.plan.binding.clone().ok_or(
            "APPLY_BINDING_WEAK: the plan has no collection binding; preparation/export only",
        )?;
    let current = port.execution_binding()?;
    if !same_collection(&approved, &current) {
        return Err("APPLY_IDENTITY_MISMATCH: profile, path, bridge or lineage changed".into());
    }
    if !loopback(&current.endpoint) {
        return Err(
            "APPLY_REMOTE_MUTATION_UNAVAILABLE: managed writes require a loopback bridge".into(),
        );
    }
    let pre = preflight(store, port, &item, request, role)?;
    let schema = pre.migration.is_some();
    let (note_ids, card_ids, model_ids) = match &pre.observed {
        Some(note) => (
            vec![note.id],
            note.cards.iter().map(|c| c.id).collect::<Vec<_>>(),
            if schema { vec![note.model_id] } else { vec![] },
        ),
        None => (vec![], vec![], vec![]),
    };
    let authorization: CheckpointAuthorization = require_checkpoint(
        store,
        request.checkpoint_id,
        &DependentScope {
            binding: &current,
            requirement: CoverageRequirement {
                scheduling: true,
                media: true,
                schema,
            },
            note_ids: &note_ids,
            card_ids: &card_ids,
            model_ids: &model_ids,
            media_names: &[],
            group_id: request.group_id,
            protected_manifest_digest: request.protected_manifest_digest,
            now_ms: request.now_ms,
            reuse_max_age_seconds: request.reuse_max_age_seconds,
            max_package_bytes: request.max_package_bytes,
        },
    )?;
    Ok(Checked {
        item,
        current,
        pre,
        authorization,
    })
}

pub(crate) fn apply_item_with(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &ApplyRequest,
    slot: &Slot,
) -> Result<ApplyItemOutcome> {
    if !request.apply {
        return Err("APPLY_FLAG_REQUIRED: the current invocation must pass --apply".into());
    }
    store.validate_lease(lease)?;
    let Checked {
        item,
        current,
        pre,
        authorization,
    } = check_item(store, port, request, slot.role)?;
    let checkpoint_digest = store.checkpoint(request.checkpoint_id)?.receipt.checksum;
    let operation = slot.operation;
    let main_step = slot.main_step;
    let marker =
        (pre.action == ItemAction::Create).then(|| format!("lab_op_{}", main_step.simple()));
    let (desired, retained, pre_state) = desired_projection(&item, &pre, marker.as_deref())?;
    if let Some(note) = &pre.observed {
        let retained: BTreeSet<i64> = note.cards.iter().map(|c| c.id).collect();
        let current = project(
            note,
            &retained,
            Some((pre.target_model_id, &pre.target_digest)),
        );
        if current == desired && pre.media_steps.is_empty() {
            return Err(
                "APPLY_NOTHING_TO_CHANGE: the collection already matches the approved item".into(),
            );
        }
    }
    let mut steps = Vec::new();
    for (media, asset) in &pre.media_steps {
        let effect = Effect::StoreMedia {
            filename: media.filename.clone(),
            sha256: media.sha256.clone(),
            size_bytes: media.size_bytes,
            staged_asset: asset.clone(),
        };
        steps.push(IntentStep {
            step_id: Uuid::new_v4(),
            payload_digest: effect.payload_digest()?,
            precondition_digest: canonical::digest("lab-apply-media-absent-v1", &media.filename)
                .map_err(|e| e.to_string())?,
            expected: serde_json::to_value(media).map_err(|e| e.to_string())?,
            expected_digest: digest_of(media)?,
            effect,
        });
    }
    let (effect, precondition) = match &pre.observed {
        None => (
            Effect::CreateNote {
                envelope: create_envelope(
                    &item,
                    main_step,
                    &desired,
                    pre.deck_id,
                    &checkpoint_digest,
                    &current,
                )?,
            },
            canonical::digest(
                "lab-apply-absent-v1",
                &(marker.as_deref(), binding_digest(&current)?),
            )
            .map_err(|e| e.to_string())?,
        ),
        Some(note) => {
            let precondition = content_digest(note)?;
            let existing: BTreeSet<_> = note.tags.iter().collect();
            (
                Effect::UpdateNote(UpdateNote {
                    note_id: note.id,
                    expected_pre_digest: precondition.clone(),
                    migration: pre.migration.clone(),
                    fields: desired.fields.clone(),
                    add_tags: desired
                        .tags
                        .iter()
                        .filter(|tag| !existing.contains(tag))
                        .cloned()
                        .collect(),
                    deck_id: pre.deck_id,
                }),
                precondition,
            )
        }
    };
    steps.push(IntentStep {
        step_id: main_step,
        payload_digest: effect.payload_digest()?,
        precondition_digest: precondition,
        expected: serde_json::to_value(&desired).map_err(|e| e.to_string())?,
        expected_digest: digest_of(&desired)?,
        effect,
    });
    let intent = ApplyIntent {
        schema_version: 1,
        action: pre.action,
        note_id: pre.observed.as_ref().map(|n| n.id),
        marker_tag: marker,
        target_model_name: item.rendered.model.name.clone(),
        target_model_id: pre.target_model_id,
        target_manifest_digest: pre.target_digest.clone(),
        deck_id: pre.deck_id,
        retained_card_ids: retained,
        pre_state,
        desired,
        steps,
        checkpoint: serde_json::to_value(&authorization).map_err(|e| e.to_string())?,
        reused_media: pre.reused_media.clone(),
    };
    store.publish_apply_operation(&ApplyOperationRecord {
        operation_id: operation,
        plan_id: request.plan_id,
        revision: request.revision,
        item_id: request.item_id,
        approval_id: request.approval_id,
        created_ms: request.now_ms,
        intent: serde_json::to_value(&intent).map_err(|e| e.to_string())?,
    })?;
    let snapshot = fresh_snapshot(
        store,
        port,
        operation,
        pre.observed.as_ref(),
        &item.rendered.model,
        pre.migration.as_ref(),
        request.now_ms,
    )?;
    let journal = OperationJournal {
        id: operation,
        group_id: request.group_id,
        approval_digest: item.approval_digest.clone(),
        binding: current.clone(),
        snapshot_id: snapshot.id,
        backup_id: request.checkpoint_id,
        state: OperationState::Prepared,
        steps: intent
            .steps
            .iter()
            .map(|step| JournalStep {
                id: step.step_id,
                action: step.effect.variant().into(),
                payload_digest: step.payload_digest.clone(),
                precondition_digest: step.precondition_digest.clone(),
                expected_post_digest: step.expected_digest.clone(),
                state: StepState::IntentRecorded,
                observed_digest: None,
            })
            .collect(),
        issues: vec![],
    };
    let version = store.append_journal(&journal, None)?;
    let mut journal = Journal {
        store,
        version,
        recovery: reconcile_command(operation),
    };
    journal.advance(|j| j.state = OperationState::Preflight)?;
    let owner = match port.begin(&current, &item.approval_digest) {
        Ok(owner) => owner,
        Err(code) => {
            journal.advance(|j| {
                j.state = OperationState::FailedBeforeWrite;
                j.issues
                    .push(issue("APPLY_OWNER_UNAVAILABLE", code.clone()));
            })?;
            return Err(format!("APPLY_OWNER_UNAVAILABLE: {code}"));
        }
    };
    journal.advance(|j| j.state = OperationState::Checkpointed)?;
    let context = Context::for_apply(
        &intent,
        current,
        item.approval_digest.clone(),
        owner.clone(),
        operation,
    );
    let result = drive(&mut journal, port, &context);
    let outcome = finish(
        &mut journal,
        port,
        &context,
        &intent,
        result,
        request.item_id,
    )?;
    // Release the native owner only after durable accounting.
    let _ = port.end(&owner);
    Ok(outcome)
}

/// What the shared step driver needs: the frozen steps plus the read-back
/// facts that apply effects do not carry themselves.
pub(crate) struct Context<'a> {
    pub steps: &'a [IntentStep],
    pub marker_tag: Option<&'a str>,
    pub retained: BTreeSet<i64>,
    pub target_model: (i64, &'a str),
    pub binding: CollectionBinding,
    pub approval_digest: String,
    pub owner: OwnerToken,
    pub operation: Uuid,
}

impl<'a> Context<'a> {
    pub fn for_apply(
        intent: &'a ApplyIntent,
        binding: CollectionBinding,
        approval_digest: String,
        owner: OwnerToken,
        operation: Uuid,
    ) -> Self {
        Self {
            steps: &intent.steps,
            marker_tag: intent.marker_tag.as_deref(),
            retained: intent.retained_card_ids.iter().copied().collect(),
            target_model: (intent.target_model_id, &intent.target_manifest_digest),
            binding,
            approval_digest,
            owner,
            operation,
        }
    }
    fn request(&self, step: &IntentStep) -> MutationRequest {
        MutationRequest {
            operation_id: step.step_id,
            parent_operation_id: self.operation,
            approval_digest: self.approval_digest.clone(),
            binding: self.binding.clone(),
            owner: self.owner.clone(),
            payload_digest: step.payload_digest.clone(),
            effect: step.effect.clone(),
        }
    }
    fn retained(&self) -> BTreeSet<i64> {
        self.retained.clone()
    }
}

/// What the collection shows for one step right now.
pub(crate) enum Readback {
    /// Matches the step's expected post-state exactly.
    Expected(String),
    /// Matches the step's recorded precondition: no effect is visible.
    Unchanged,
    /// Anything else, with its digest.
    Different(String),
}

/// Projection a restore expects: every kept card with its current
/// scheduling, and the observed model manifest digest compared verbatim.
pub fn restore_projection(note: &ObservedNote, kept: &BTreeSet<i64>) -> NoteProjection {
    project(note, kept, None)
}

pub(crate) fn deleted_projection(note_id: i64) -> serde_json::Value {
    serde_json::json!({ "deleted_note_id": note_id })
}

fn compare(digest: String, step: &IntentStep) -> Readback {
    if digest == step.expected_digest {
        Readback::Expected(digest)
    } else {
        Readback::Different(digest)
    }
}

pub(crate) fn read_step(
    port: &mut dyn ApplyPort,
    context: &Context,
    step: &IntentStep,
) -> Result<Readback> {
    match &step.effect {
        Effect::StoreMedia { filename, .. } => match port.media(filename)? {
            None => Ok(Readback::Unchanged),
            Some(found) => {
                let actual = MediaProjection {
                    filename: found.filename,
                    sha256: found.sha256,
                    size_bytes: found.size_bytes,
                };
                let digest = digest_of(&actual)?;
                Ok(if digest == step.expected_digest {
                    Readback::Expected(digest)
                } else {
                    Readback::Different(digest)
                })
            }
        },
        Effect::CreateNote { .. } => {
            let marker = context.marker_tag.ok_or("APPLY_INTENT_CORRUPT")?;
            let candidates = port.notes_tagged(marker)?;
            match candidates.as_slice() {
                [] => Ok(Readback::Unchanged),
                [note] => {
                    let projection = project(note, &BTreeSet::new(), Some(context.target_model));
                    let digest = digest_of(&projection)?;
                    Ok(if digest == step.expected_digest {
                        Readback::Expected(digest)
                    } else {
                        Readback::Different(digest)
                    })
                }
                many => Ok(Readback::Different(
                    canonical::digest("lab-apply-marker-candidates-v1", &many.len())
                        .map_err(|e| e.to_string())?,
                )),
            }
        }
        Effect::UpdateNote(update) => {
            let note = port
                .note(update.note_id)?
                .ok_or("APPLY_TARGET_NOTE_MISSING")?;
            if content_digest(&note)? == update.expected_pre_digest {
                return Ok(Readback::Unchanged);
            }
            let projection = project(&note, &context.retained(), Some(context.target_model));
            let digest = digest_of(&projection)?;
            Ok(if digest == step.expected_digest {
                Readback::Expected(digest)
            } else {
                Readback::Different(digest)
            })
        }
        Effect::RestoreNote(restore) => {
            let Some(note) = port.note(restore.note_id)? else {
                return Ok(Readback::Different(digest_of(&deleted_projection(
                    restore.note_id,
                ))?));
            };
            if content_digest(&note)? == restore.expected_pre_digest {
                return Ok(Readback::Unchanged);
            }
            let kept = restore.card_decks.iter().map(|c| c.card_id).collect();
            Ok(compare(digest_of(&restore_projection(&note, &kept))?, step))
        }
        Effect::DeleteUnstudiedCreatedNote {
            note_id,
            expected_pre_digest,
        } => match port.note(*note_id)? {
            None => Ok(compare(digest_of(&deleted_projection(*note_id))?, step)),
            Some(note) if &content_digest(&note)? == expected_pre_digest => Ok(Readback::Unchanged),
            Some(note) => Ok(Readback::Different(digest_of(&project(
                &note,
                &note.cards.iter().map(|c| c.id).collect(),
                None,
            ))?)),
        },
    }
}

/// Outcome of driving the remaining steps.
pub(crate) enum Drive {
    Committed,
    Stopped,
}

/// Record a matching read-back as verified from the step's current state.
pub(crate) fn verify_step(journal: &mut Journal, index: usize, observed: String) -> Result<()> {
    if journal.version.journal.steps[index].state == StepState::RequestStarted {
        let digest = observed.clone();
        journal.advance(|j| {
            j.steps[index].state = StepState::ObservedSuccess;
            j.steps[index].observed_digest = Some(digest);
        })?;
    }
    journal.advance(|j| {
        j.steps[index].state = StepState::Verified;
        j.steps[index].observed_digest = Some(observed);
    })
}

pub(crate) fn to_unknown(
    journal: &mut Journal,
    index: usize,
    code: &str,
    detail: String,
) -> Result<()> {
    journal.advance(|j| {
        j.steps[index].state = StepState::Unknown;
        j.state = OperationState::NeedsRecovery;
        j.issues.push(issue(
            code,
            format!("{detail}; effect uncertain, dependent writes stopped"),
        ));
    })
}

/// A native refusal counts as "no effect" only with a read-back that still
/// shows the precondition.
pub(crate) fn observed_failure(journal: &mut Journal, index: usize, reason: &str) -> Result<()> {
    let digest = canonical::digest("lab-apply-failure-v1", reason).ok();
    journal.advance(|j| {
        j.steps[index].state = StepState::ObservedFailure;
        j.steps[index].observed_digest = digest;
        let only_failures = j
            .steps
            .iter()
            .all(|s| matches!(s.state, StepState::IntentRecorded | StepState::ObservedFailure));
        j.state = if only_failures {
            OperationState::FailedBeforeWrite
        } else {
            OperationState::NeedsRecovery
        };
        j.issues.push(issue(
            if only_failures {
                "APPLY_REJECTED_BEFORE_WRITE"
            } else {
                "APPLY_KNOWN_PARTIAL"
            },
            format!(
                "native refused the effect ({reason}) and read-back shows no change; earlier verified effects remain"
            ),
        ));
    })
}

pub(crate) fn dispatch(
    journal: &mut Journal,
    port: &mut dyn ApplyPort,
    context: &Context,
    index: usize,
) -> Result<Option<Drive>> {
    let step = &context.steps[index];
    let response = port.mutate(&context.request(step));
    let failure_reason = match response {
        Ok(NativeStatus::Verified) => None,
        Ok(NativeStatus::FailedBeforeWrite { reason }) | Err(PortFailure::Rejected(reason)) => {
            Some(reason)
        }
        Ok(other) => {
            let detail = match other {
                NativeStatus::Unknown { reason } => reason,
                NativeStatus::Queued | NativeStatus::Running => {
                    "deadline passed while pending".into()
                }
                _ => "native ledger has no row after submission".into(),
            };
            to_unknown(journal, index, "APPLY_OUTCOME_UNKNOWN", detail)?;
            return Ok(Some(Drive::Stopped));
        }
        Err(PortFailure::Unknown(reason)) => {
            to_unknown(journal, index, "APPLY_OUTCOME_UNKNOWN", reason)?;
            return Ok(Some(Drive::Stopped));
        }
    };
    let readback = read_step(port, context, step);
    match (failure_reason, readback) {
        (None, Ok(Readback::Expected(digest))) => {
            verify_step(journal, index, digest)?;
            Ok(None)
        }
        (None, Ok(Readback::Different(digest))) => {
            journal.advance(|j| {
                j.steps[index].state = StepState::ObservedSuccess;
                j.steps[index].observed_digest = Some(digest);
                j.state = OperationState::NeedsRecovery;
                j.issues.push(issue(
                    "APPLY_READBACK_MISMATCH",
                    "actual state differs from the desired post-state; no success is claimed",
                ));
            })?;
            Ok(Some(Drive::Stopped))
        }
        (None, Ok(Readback::Unchanged)) | (None, Err(_)) => {
            to_unknown(
                journal,
                index,
                "APPLY_READBACK_UNAVAILABLE",
                "native success without a matching read-back".into(),
            )?;
            Ok(Some(Drive::Stopped))
        }
        (Some(reason), Ok(Readback::Unchanged)) => {
            observed_failure(journal, index, &reason)?;
            Ok(Some(Drive::Stopped))
        }
        (Some(reason), _) => {
            to_unknown(
                journal,
                index,
                "APPLY_REJECTED_WITH_EFFECT",
                format!("native refused ({reason}) but read-back is not the precondition"),
            )?;
            Ok(Some(Drive::Stopped))
        }
    }
}

/// Executes remaining `intent_recorded` steps in order.
pub(crate) fn drive(
    journal: &mut Journal,
    port: &mut dyn ApplyPort,
    context: &Context,
) -> Result<Drive> {
    for index in 0..context.steps.len() {
        match journal.version.journal.steps[index].state {
            StepState::Verified => continue,
            StepState::IntentRecorded => {}
            _ => return Ok(Drive::Stopped),
        }
        journal.advance(|j| {
            j.state = OperationState::Mutating;
            j.steps[index].state = StepState::RequestStarted;
        })?;
        if let Some(stop) = dispatch(journal, port, context, index)? {
            return Ok(stop);
        }
    }
    Ok(Drive::Committed)
}

/// Save the observed post-state and receipt, then commit.
fn commit(
    journal: &mut Journal,
    context: &Context,
    intent: &ApplyIntent,
    port: &mut dyn ApplyPort,
) -> Result<(Option<i64>, String)> {
    let last = intent.steps.last().ok_or("APPLY_INTENT_CORRUPT")?;
    let note = match &last.effect {
        Effect::CreateNote { .. } => port
            .notes_tagged(intent.marker_tag.as_deref().ok_or("APPLY_INTENT_CORRUPT")?)?
            .into_iter()
            .next(),
        Effect::UpdateNote(update) => port.note(update.note_id)?,
        _ => None,
    }
    .ok_or("APPLY_READBACK_UNAVAILABLE")?;
    let retained = if matches!(last.effect, Effect::CreateNote { .. }) {
        BTreeSet::new()
    } else {
        context.retained()
    };
    let projection = project(
        &note,
        &retained,
        Some((intent.target_model_id, &intent.target_manifest_digest)),
    );
    let projection_bytes = canonical::bytes(&projection).map_err(|e| e.to_string())?;
    if canonical::asset_digest(&projection_bytes) != last.expected_digest {
        journal.advance(|j| {
            j.state = OperationState::NeedsRecovery;
            j.issues.push(issue(
                "APPLY_FINAL_READBACK_MISMATCH",
                "final read-back changed after step verification; later edits or study need review",
            ));
        })?;
        return Err("APPLY_FINAL_READBACK_MISMATCH".into());
    }
    let observed = journal
        .store
        .publish_asset(&projection_bytes, 100 * 1024 * 1024)?;
    let evidence = serde_json::json!({
        "schema_version": 1,
        "kind": "lab_apply_readback_v1",
        "operation_id": context.operation,
        "note": note,
        "reused_media": intent.reused_media,
        "binding": context.binding,
    });
    let evidence_digest = journal.store.publish_asset(
        &canonical::bytes(&evidence).map_err(|e| e.to_string())?,
        100 * 1024 * 1024,
    )?;
    let retained_history: Vec<_> = projection
        .cards
        .iter()
        .filter(|card| card.id.is_some())
        .map(|card| (card.id, &card.history_digest))
        .collect();
    let receipt = NativeOperationReceipt {
        schema_version: 1,
        lineage_id: context.binding.lineage_id,
        operation_id: last.step_id,
        session_epoch: context.binding.session_epoch,
        payload_digest: last.payload_digest.clone(),
        approved_digest: context.approval_digest.clone(),
        state: NativeReceiptState::Verified,
        readback: Some(NativeReadback {
            observed_state_digest: observed.clone(),
            note_ids: vec![AnkiId::try_from(note.id.to_string())?],
            card_ids: note
                .cards
                .iter()
                .map(|card| AnkiId::try_from(card.id.to_string()))
                .collect::<std::result::Result<_, _>>()?,
            history_digest: Some(digest_of(&retained_history)?),
            manifest_digests: vec![intent.target_manifest_digest.clone()],
        }),
        evidence_digest,
    };
    journal.advance(|j| j.state = OperationState::Verifying)?;
    let snapshot = journal.version.journal.snapshot_id;
    // A crash after the receipt was saved but before commit leaves the stored,
    // immutable receipt; it already names this exact verified post-state.
    let receipt = match journal.store.snapshot(snapshot)?.after {
        Some(stored) => stored,
        None => {
            journal.store.append_snapshot_after(snapshot, &receipt)?;
            receipt
        }
    };
    journal.advance(|j| j.state = OperationState::Committed)?;
    let receipt_digest =
        canonical::digest("lab-apply-receipt-v1", &receipt).map_err(|e| e.to_string())?;
    Ok((Some(note.id), receipt_digest))
}

fn finish(
    journal: &mut Journal,
    port: &mut dyn ApplyPort,
    context: &Context,
    intent: &ApplyIntent,
    result: Result<Drive>,
    item_id: Uuid,
) -> Result<ApplyItemOutcome> {
    let (note_id, receipt_digest) = match result? {
        Drive::Committed => commit(journal, context, intent, port)?,
        Drive::Stopped => (None, String::new()),
    };
    let j = &journal.version.journal;
    let unresolved =
        j.state != OperationState::Committed && j.state != OperationState::FailedBeforeWrite;
    Ok(ApplyItemOutcome {
        item_id,
        operation_id: Some(j.id),
        action: Some(intent.action),
        state: j.state,
        note_id: note_id.or(intent.note_id),
        snapshot_id: Some(j.snapshot_id),
        checkpoint_id: j.backup_id,
        receipt_digest: (!receipt_digest.is_empty()).then_some(receipt_digest),
        issues: j.issues.clone(),
        next_command: unresolved.then(|| reconcile_command(j.id)),
    })
}

/// ALG-APPLY over several items. An identity, shared-model or local
/// durability fault stops the remaining items; other item failures are
/// reported and later independent items continue.
pub fn apply_items(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    requests: &[ApplyRequest],
) -> Vec<std::result::Result<ApplyItemOutcome, (Uuid, String)>> {
    let mut out = Vec::new();
    for request in requests {
        let result = apply_item(store, lease, port, request).map_err(|e| (request.item_id, e));
        let stop = match &result {
            Err((_, code)) => [
                "APPLY_IDENTITY_MISMATCH",
                "APPLY_LOCAL_DURABILITY_FAILED",
                "MODEL_",
                "APPLY_MODEL_",
                "LEASE_",
            ]
            .iter()
            .any(|prefix| code.starts_with(prefix)),
            Ok(outcome) => {
                outcome.state == OperationState::NeedsRecovery
                    && outcome
                        .issues
                        .iter()
                        .any(|i| i.code == "APPLY_OUTCOME_UNKNOWN")
            }
        };
        out.push(result);
        if stop {
            break;
        }
    }
    out
}

/// One proposed or executed recovery action.
#[derive(Clone, Debug, Serialize)]
pub struct StepFinding {
    pub step_id: Uuid,
    pub variant: String,
    pub journal_state: StepState,
    pub native_status: Option<NativeStatus>,
    pub evidence: &'static str,
    pub action: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconcileOutcome {
    pub operation_id: Uuid,
    pub state: OperationState,
    pub applied: bool,
    pub rebound: bool,
    pub steps: Vec<StepFinding>,
    pub note_id: Option<i64>,
    pub receipt_digest: Option<String>,
    pub issues: Vec<Issue>,
    pub next_command: Option<String>,
}

pub struct ReconcileRequest {
    pub operation_id: Uuid,
    /// The current invocation's explicit `--apply`; without it nothing is written.
    pub apply: bool,
    /// Explicit `--rebind` decision for a changed session epoch.
    pub rebind: Option<ResumeBindingDecision>,
}

pub(crate) fn load_intent(
    store: &Store,
    operation: Uuid,
) -> Result<(ApplyOperationRecord, ApplyIntent)> {
    let record = store.apply_operation(operation)?;
    let intent: ApplyIntent =
        serde_json::from_value(record.intent.clone()).map_err(|_| "APPLY_OPERATION_CORRUPT")?;
    let journal = store.journal(operation)?.journal;
    if intent.schema_version != 1
        || intent.steps.len() != journal.steps.len()
        || intent.steps.iter().zip(&journal.steps).any(|(a, b)| {
            a.step_id != b.id
                || a.payload_digest != b.payload_digest
                || a.expected_digest != b.expected_post_digest
                || a.effect.payload_digest().ok().as_ref() != Some(&a.payload_digest)
        })
    {
        return Err("APPLY_OPERATION_CORRUPT".into());
    }
    Ok((record, intent))
}

/// Read-only classification of one non-verified step.
fn classify(
    port: &mut dyn ApplyPort,
    context: &Context,
    index: usize,
    state: StepState,
    trusted_lineage: bool,
) -> Result<(Option<NativeStatus>, Readback, &'static str, &'static str)> {
    let step = &context.steps[index];
    let status = if state == StepState::IntentRecorded {
        None
    } else {
        Some(port.status(step.step_id)?)
    };
    let readback = read_step(port, context, step)?;
    let (evidence, action) = match (&status, &readback, state) {
        (None, Readback::Expected(_), _) => ("matched", "dispatch_blocked_already_matching"),
        (None, Readback::Unchanged, _) => ("absent", "dispatch"),
        (None, Readback::Different(_), _) => ("conflict", "review"),
        (Some(NativeStatus::Queued | NativeStatus::Running), _, _) => ("pending", "wait"),
        (Some(_), Readback::Expected(_), _) => ("matched", "adopt"),
        (
            Some(NativeStatus::FailedBeforeWrite { .. }),
            Readback::Unchanged,
            StepState::RequestStarted | StepState::Unknown,
        ) => ("absent", "record_failure"),
        (
            Some(NativeStatus::Absent),
            Readback::Unchanged,
            StepState::RequestStarted | StepState::Unknown,
        ) if trusted_lineage => ("absent", "resubmit_same_uuid"),
        (Some(_), Readback::Unchanged, _) => ("absent_unproven", "review"),
        (Some(_), Readback::Different(_), _) => ("partial_or_conflict", "review"),
    };
    Ok((status, readback, evidence, action))
}

fn mark_unknown_if_started(journal: &mut Journal, index: usize, state: StepState) -> Result<()> {
    if state == StepState::RequestStarted {
        to_unknown(
            journal,
            index,
            "APPLY_OUTCOME_UNKNOWN",
            "request started without a recorded observation".into(),
        )?;
    }
    Ok(())
}

/// ALG-RECONCILE over every unverified step of one journal. Without `apply`
/// it only classifies the first unresolved step. Returns the findings and
/// whether the journal stopped before every step was verified.
pub(crate) fn reconcile_steps(
    journal: &mut Journal,
    port: &mut dyn ApplyPort,
    context: &Context,
    apply: bool,
    trusted_lineage: bool,
) -> Result<(Vec<StepFinding>, bool)> {
    let mut findings = Vec::new();
    for index in 0..context.steps.len() {
        let state = journal.version.journal.steps[index].state;
        if state == StepState::Verified {
            continue;
        }
        if state == StepState::ObservedFailure {
            return Ok((findings, true));
        }
        let (status, readback, evidence, action) =
            classify(port, context, index, state, trusted_lineage)?;
        findings.push(StepFinding {
            step_id: context.steps[index].step_id,
            variant: context.steps[index].effect.variant().into(),
            journal_state: state,
            native_status: status.clone(),
            evidence,
            action,
        });
        if !apply {
            return Ok((findings, true));
        }
        match (action, readback) {
            ("adopt", Readback::Expected(digest)) => {
                mark_unknown_if_started(journal, index, state)?;
                verify_step(journal, index, digest)?;
            }
            ("record_failure", _) => {
                mark_unknown_if_started(journal, index, state)?;
                let reason = match status {
                    Some(NativeStatus::FailedBeforeWrite { reason }) => reason,
                    _ => "failed_before_write".into(),
                };
                observed_failure(journal, index, &reason)?;
                return Ok((findings, true));
            }
            ("resubmit_same_uuid", _) => {
                mark_unknown_if_started(journal, index, state)?;
                // Durable record of the resubmission before dispatch; the
                // native ledger deduplicates the same UUID and payload.
                journal.note_issue(
                    "APPLY_RESUBMIT_SAME_UUID",
                    format!(
                        "native ledger has no row for {} and the collection shows the precondition",
                        context.steps[index].step_id
                    ),
                )?;
                let response = port.mutate(&context.request(&context.steps[index]));
                let readback = read_step(port, context, &context.steps[index]);
                match (response, readback) {
                    (Ok(NativeStatus::Verified), Ok(Readback::Expected(digest))) => {
                        verify_step(journal, index, digest)?;
                    }
                    _ => {
                        journal.advance(|j| {
                            j.state = OperationState::NeedsRecovery;
                            j.issues.push(issue(
                                "APPLY_RESUBMIT_UNVERIFIED",
                                "resubmission did not produce a matching read-back",
                            ));
                        })?;
                        return Ok((findings, true));
                    }
                }
            }
            ("dispatch", _) => {
                if matches!(
                    journal.version.journal.state,
                    OperationState::NeedsRecovery
                        | OperationState::Checkpointed
                        | OperationState::Mutating
                ) {
                    journal.advance(|j| {
                        j.state = OperationState::Mutating;
                        j.steps[index].state = StepState::RequestStarted;
                    })?;
                } else {
                    // Prepared/preflight: the operation never reached dispatch.
                    journal.advance(|j| {
                        j.state = OperationState::FailedBeforeWrite;
                        j.issues.push(issue(
                            "APPLY_NOT_DISPATCHED",
                            "the operation stopped before its checkpointed state; start a new attempt",
                        ));
                    })?;
                    return Ok((findings, true));
                }
                if dispatch(journal, port, context, index)?.is_some() {
                    return Ok((findings, true));
                }
            }
            _ => {
                mark_unknown_if_started(journal, index, state)?;
                let code = match evidence {
                    "pending" => "APPLY_NATIVE_PENDING",
                    "absent_unproven" => "APPLY_ABSENCE_UNPROVEN",
                    _ => "APPLY_RECONCILE_REVIEW_REQUIRED",
                };
                let already = journal
                    .version
                    .journal
                    .issues
                    .last()
                    .is_some_and(|last| last.code == code);
                if !already {
                    journal.note_issue(
                        code,
                        "evidence cannot prove this step's outcome; choose adopt, restore or export evidence after review".into(),
                    )?;
                }
                return Ok((findings, true));
            }
        }
    }
    let unverified = journal
        .version
        .journal
        .steps
        .iter()
        .any(|s| s.state != StepState::Verified);
    Ok((findings, unverified))
}

/// ALG-RECONCILE for one apply operation. It never creates a new operation ID,
/// never re-sends an unknown or pending row and only writes with `--apply`.
pub fn reconcile(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &ReconcileRequest,
) -> Result<ReconcileOutcome> {
    store.validate_lease(lease)?;
    let (_record, intent) = load_intent(store, request.operation_id)?;
    let version = store.journal(request.operation_id)?;
    let decisions = store.binding_decisions(request.operation_id)?;
    let execution = decisions
        .last()
        .map(|d| d.new_binding.clone())
        .unwrap_or_else(|| version.journal.binding.clone());
    let approval_digest = version.journal.approval_digest.clone();
    let terminal = matches!(
        version.journal.state,
        OperationState::Committed | OperationState::FailedBeforeWrite | OperationState::Restored
    );
    let base = |journal: &OperationJournal, steps: Vec<StepFinding>, applied, rebound| {
        let unresolved = !matches!(
            journal.state,
            OperationState::Committed
                | OperationState::FailedBeforeWrite
                | OperationState::Restored
        );
        ReconcileOutcome {
            operation_id: journal.id,
            state: journal.state,
            applied,
            rebound,
            steps,
            note_id: intent.note_id,
            receipt_digest: None,
            issues: journal.issues.clone(),
            next_command: unresolved.then(|| reconcile_command(journal.id)),
        }
    };
    if terminal {
        return Ok(base(&version.journal, vec![], false, false));
    }
    let current = port.execution_binding()?;
    if !same_collection(&execution, &current) {
        return Err("APPLY_IDENTITY_MISMATCH: collection identity changed; no automatic lineage replacement".into());
    }
    let mut rebound = false;
    if current.session_epoch != execution.session_epoch {
        let Some(decision) = &request.rebind else {
            return Err(
                "SESSION_CHANGED: rerun with --rebind --apply after reviewing current evidence"
                    .into(),
            );
        };
        if !request.apply {
            return Err(
                "APPLY_FLAG_REQUIRED: --rebind needs the current invocation's --apply".into(),
            );
        }
        if decision.new_binding != current || decision.operation_id != request.operation_id {
            return Err("BINDING_DECISION_CONFLICT".into());
        }
        // The decision must bind the evidence observed right now.
        let observed = current_state_digest(port, &intent)?;
        if decision.observed_state_digest != observed {
            return Err("BINDING_DECISION_STALE: observed state changed since the decision".into());
        }
        store.append_binding_decision(decision)?;
        rebound = true;
    }
    let owner = if request.apply {
        Some(port.begin(&current, &approval_digest)?)
    } else {
        None
    };
    let context = Context::for_apply(
        &intent,
        current.clone(),
        approval_digest,
        owner.clone().unwrap_or(OwnerToken {
            token: Uuid::nil(),
            fence: 0,
        }),
        request.operation_id,
    );
    let trusted_lineage =
        execution.lineage_id == current.lineage_id && execution.bridge_id == current.bridge_id;
    let mut journal = Journal {
        store,
        version,
        recovery: reconcile_command(request.operation_id),
    };
    let (findings, stopped) =
        reconcile_steps(&mut journal, port, &context, request.apply, trusted_lineage)?;
    let mut outcome_note = intent.note_id;
    let mut receipt_digest = None;
    if !stopped {
        let (note, receipt) = commit(&mut journal, &context, &intent, port)?;
        outcome_note = note;
        receipt_digest = Some(receipt);
    }
    if let Some(owner) = owner {
        let _ = port.end(&owner);
    }
    let mut outcome = base(&journal.version.journal, findings, request.apply, rebound);
    outcome.note_id = outcome_note;
    outcome.receipt_digest = receipt_digest;
    Ok(outcome)
}

/// Digest of the current affected state, which a rebinding decision must name.
pub fn current_state_digest(port: &mut dyn ApplyPort, intent: &ApplyIntent) -> Result<String> {
    let note = match (intent.note_id, &intent.marker_tag) {
        (Some(id), _) => port.note(id)?.into_iter().collect::<Vec<_>>(),
        (None, Some(marker)) => port.notes_tagged(marker)?,
        (None, None) => return Err("APPLY_INTENT_CORRUPT".into()),
    };
    let media: Vec<_> = intent
        .steps
        .iter()
        .filter_map(|step| match &step.effect {
            Effect::StoreMedia { filename, .. } => Some(filename.clone()),
            _ => None,
        })
        .map(|name| port.media(&name))
        .collect::<Result<_>>()?;
    digest_of(&(note, media))
}

/// Local-only preflight for `apply PLAN` without `--apply`.
#[derive(Clone, Debug, Serialize)]
pub struct PreviewItem {
    pub item_id: Uuid,
    pub approved: bool,
    pub approval_ids: Vec<Uuid>,
    pub content_ready: bool,
    pub action: &'static str,
    pub source_note_id: Option<i64>,
    pub target_model: String,
    pub target_deck: Option<String>,
    pub media_files: Vec<String>,
    pub required_variants: Vec<&'static str>,
    pub blockers: Vec<String>,
    pub unresolved_operations: Vec<Uuid>,
    /// Reviewed grammar split group, applied only as a whole (ALG-SPLIT).
    pub split_group: Option<Uuid>,
    /// `anchor` keeps the source note; `child` is always a fresh note.
    pub split_role: Option<&'static str>,
}

pub fn preview(
    store: &Store,
    plan_id: Uuid,
    revision: u32,
    item_ids: &[Uuid],
) -> Result<Vec<PreviewItem>> {
    let plan = store.revision(plan_id, revision)?;
    let latest = store.latest_revision(plan_id)? == revision;
    let evidence = linguist_core::plan_validation::inspect(&plan).map_err(|e| e.to_string())?;
    let approvals = store.approvals_for(plan_id, revision)?;
    let selected: Vec<&LearningDocument> = if item_ids.is_empty() {
        plan.documents.iter().collect()
    } else {
        item_ids
            .iter()
            .map(|id| {
                plan.documents
                    .iter()
                    .find(|doc| doc.id == *id)
                    .ok_or_else(|| "APPLY_ITEM_NOT_FOUND".to_owned())
            })
            .collect::<Result<_>>()?
    };
    let mut out = Vec::new();
    for document in selected {
        let mut blockers = Vec::new();
        if !latest {
            blockers.push("APPLY_REVISION_STALE".into());
        }
        if plan.binding.is_none() {
            blockers.push("APPLY_BINDING_WEAK".into());
        }
        let approval_ids: Vec<Uuid> = approvals
            .iter()
            .filter(|a| a.approval.item_ids.contains(&document.id))
            .map(|a| a.id)
            .collect();
        if approval_ids.is_empty() {
            blockers.push("APPLY_APPROVAL_MISSING".into());
        }
        let content_ready = evidence
            .items
            .iter()
            .any(|item| item.document_id == document.id && item.content_ready);
        if !content_ready {
            blockers.push("APPLY_ITEM_NOT_READY".into());
        }
        let group = plan
            .grammar_groups
            .iter()
            .find(|group| group.units.contains(&document.id));
        let split_role = group.map(|group| {
            if group.anchor_document == document.id {
                "anchor"
            } else {
                "child"
            }
        });
        let rendered = plan.rendered.iter().find(|r| r.document_id == document.id);
        let source = if split_role == Some("child") {
            Ok(None)
        } else {
            source_note(document)
        };
        let (action, source_note_id) = match &source {
            Ok(None) => ("create", None),
            Ok(Some((_, id))) => ("update_or_migrate", Some(*id)),
            Err(code) => {
                blockers.push(code.clone());
                ("blocked", None)
            }
        };
        let target_deck = target_deck_name(&plan, document).unwrap_or(None);
        if action == "create" && target_deck.is_none() {
            blockers.push("APPLY_TARGET_DECK_UNCONFIGURED".into());
        }
        let media_files: Vec<String> = rendered
            .map(|r| {
                r.media_digests
                    .iter()
                    .filter_map(|d| document.media.iter().find(|m| &m.digest == d))
                    .map(|m| m.filename.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut required_variants = vec![if action == "create" {
            "create_note"
        } else {
            "update_note"
        }];
        if !media_files.is_empty() {
            required_variants.push("store_media");
        }
        let mut unresolved_operations = Vec::new();
        for record in store.apply_operations_for_item(plan_id, document.id)? {
            if let Ok(version) = store.journal(record.operation_id)
                && version.pending_recovery
                && !superseded_safely(&version.journal)
            {
                unresolved_operations.push(record.operation_id);
            }
        }
        if !unresolved_operations.is_empty() {
            blockers.push("APPLY_RECOVERY_REQUIRED".into());
        }
        out.push(PreviewItem {
            item_id: document.id,
            approved: !approval_ids.is_empty(),
            approval_ids,
            content_ready,
            action,
            source_note_id,
            target_model: rendered.map(|r| r.model.name.clone()).unwrap_or_default(),
            target_deck,
            media_files,
            required_variants,
            blockers,
            unresolved_operations,
            split_group: group.map(|group| group.id),
            split_role,
        });
    }
    Ok(out)
}

/// Local-only proposal for `recover reconcile OPERATION` without live evidence.
#[derive(Clone, Debug, Serialize)]
pub struct LocalProposal {
    pub operation_id: Uuid,
    pub plan_id: Uuid,
    pub revision: u32,
    pub item_id: Uuid,
    pub action: ItemAction,
    pub state: OperationState,
    pub steps: Vec<(Uuid, String, StepState)>,
    pub rebinding_decisions: usize,
    pub next_live_check: &'static str,
}

pub fn local_proposal(store: &Store, operation: Uuid) -> Result<LocalProposal> {
    let (record, intent) = load_intent(store, operation)?;
    let journal = store.journal(operation)?.journal;
    let next_live_check = match journal.state {
        OperationState::Committed => "none_committed",
        OperationState::FailedBeforeWrite => "none_failed_before_write",
        _ if journal
            .steps
            .iter()
            .any(|s| matches!(s.state, StepState::RequestStarted | StepState::Unknown)) =>
        {
            "native_status_and_readback"
        }
        _ => "readback_before_dispatch",
    };
    Ok(LocalProposal {
        operation_id: operation,
        plan_id: record.plan_id,
        revision: record.revision,
        item_id: record.item_id,
        action: intent.action,
        state: journal.state,
        steps: journal
            .steps
            .iter()
            .map(|s| (s.id, s.action.clone(), s.state))
            .collect(),
        rebinding_decisions: store.binding_decisions(operation)?.len(),
        next_live_check,
    })
}
