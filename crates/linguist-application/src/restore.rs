//! ALG-RESTORE for one applied snapshot, and OP-44 grouped rollback, over the
//! same injected native port as apply. A restore is a new journaled operation:
//! it compares the current collection with the recorded post-state, needs an
//! observed-state-bound decision for every conflict, captures a fresh snapshot,
//! restores referenced media before the fields that use it, and preserves
//! retained card IDs with their current scheduling and history. It never
//! deletes shared models or media, never removes studied cards or notes, and
//! never claims a rollback without a matching read-back and receipt.
use crate::apply::{
    self, ApplyIntent, ApplyPort, CardDeck, Context, Drive, Effect, IntentStep, ItemAction,
    Journal, MediaProjection, Migration, ObservedNote, OrdinalMapping, OwnerToken, RestoreNote,
    content_digest, deleted_projection, digest_of, issue, project, restore_projection,
    same_collection, sorted_tags,
};
use crate::backup::{CheckpointAuthorization, DependentScope, Result, require_checkpoint};
use crate::checkpoint::CoverageRequirement;
use linguist_core::{
    canonical,
    model::ManagedModel,
    records::{
        CollectionBinding, JournalStep, OperationJournal, OperationState, SourceRecord, StepState,
    },
    validation::Issue,
};
use linguist_store::{
    Store,
    lease::LeaseToken,
    restore::{RestoreOperationRecord, RestoreReceipt},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Reviewed choice for one field whose restore would otherwise overwrite a
/// later edit, or an explicit merge for any field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "choice", content = "value")]
pub enum FieldChoice {
    /// Restore the snapshot value (or drop a field the original model lacks).
    Original,
    /// Keep the current value; the field must exist in the restored model.
    Current,
    /// Explicitly reviewed merged text.
    Merged(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeckChoice {
    Original,
    Current,
}

/// Restore authorization bound to the observed state digest of a preview.
/// There is no global force: every conflict needs its own entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreDecision {
    pub schema_version: u16,
    pub snapshot_id: Uuid,
    pub observed_state_digest: String,
    pub actor: String,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldChoice>,
    /// Card ID (decimal string) to deck choice, for cards moved after apply.
    #[serde(default)]
    pub decks: BTreeMap<String, DeckChoice>,
    /// Unstudied cards a reverse mapping removes; each must be listed.
    #[serde(default)]
    pub remove_unstudied_cards: Vec<i64>,
    /// Created notes to delete; only unchanged notes with no reviews.
    #[serde(default)]
    pub delete_created_notes: Vec<i64>,
    /// Missing original media restored without bytes; references stay.
    #[serde(default)]
    pub accept_missing_media: Vec<String>,
    /// Accepts the schema/full-sync warning of a reverse note-type change.
    #[serde(default)]
    pub accept_schema_change: bool,
}

impl RestoreDecision {
    pub fn digest(&self) -> Result<String> {
        canonical::digest("lab-restore-decision-v1", self).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldDiff {
    pub field: String,
    pub original: Option<String>,
    pub applied: Option<String>,
    pub current: Option<String>,
    pub changed_by_apply: bool,
    pub changed_later: bool,
    pub conflict: bool,
    /// Value after restore; `None` while a conflict is undecided or when the
    /// restored model has no such field.
    pub restored: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TagDiff {
    pub original: Vec<String>,
    pub applied: Vec<String>,
    pub current: Vec<String>,
    /// Original tags minus those removed after apply, plus those added after
    /// apply; tags added by apply are removed.
    pub restored: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardConsequence {
    pub card_id: i64,
    pub retained: bool,
    pub ordinal_now: u16,
    pub ordinal_after: Option<u16>,
    pub deck_now: i64,
    pub deck_original: Option<i64>,
    pub deck_applied: i64,
    pub deck_after: Option<i64>,
    pub reviews_at_apply: Option<u64>,
    pub reviews_now: u64,
    pub later_reviews: bool,
    pub deck_conflict: bool,
    pub action: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MediaConsequence {
    pub filename: String,
    pub present: bool,
    pub archived: bool,
    pub action: &'static str,
    pub restored_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CreatedNote {
    pub note_id: i64,
    pub unchanged: bool,
    pub studied: bool,
    pub action: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelConsequence {
    pub original_name: String,
    pub original_model_id: i64,
    pub current_name: String,
    pub current_model_id: i64,
    pub reverse_mapping: Vec<OrdinalMapping>,
}

/// ALG-RESTORE step 1: the live preview. It writes nothing.
#[derive(Clone, Debug, Serialize)]
pub struct RestorePlan {
    pub snapshot_id: Uuid,
    pub target_operation: Uuid,
    pub target_state: OperationState,
    pub action: ItemAction,
    pub note_id: Option<i64>,
    pub observed_state_digest: String,
    pub model: Option<ModelConsequence>,
    pub fields: Vec<FieldDiff>,
    pub tags: Option<TagDiff>,
    pub cards: Vec<CardConsequence>,
    pub media: Vec<MediaConsequence>,
    /// Files apply uploaded; restore never deletes collection media.
    pub uploaded_media_kept: Vec<String>,
    pub created_notes: Vec<CreatedNote>,
    /// Decisions still required before `--apply`.
    pub conflicts: Vec<String>,
    /// Unsupported recoveries; restore stops and reports these exactly.
    pub blockers: Vec<String>,
    pub no_change: bool,
    /// Unresolved restore journal that `--apply` resumes instead.
    pub resume_operation: Option<Uuid>,
    pub manual_next_steps: Vec<String>,
}

/// Frozen restore intent stored before the restore journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreIntent {
    pub schema_version: u16,
    pub note_id: i64,
    pub deleting: bool,
    pub steps: Vec<IntentStep>,
    pub kept_card_ids: Vec<i64>,
    pub removed_card_ids: Vec<i64>,
    pub restored_media: Vec<String>,
    pub checkpoint: serde_json::Value,
    pub plan_digest: String,
}

pub struct RestoreRequest<'a> {
    /// The current invocation's explicit `--apply`.
    pub apply: bool,
    pub snapshot_id: Uuid,
    /// Required for a new restore; ignored when an unresolved restore resumes.
    pub decision: Option<RestoreDecision>,
    pub checkpoint_id: Uuid,
    pub group_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
    pub max_media_bytes: u64,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RestoreOutcome {
    pub snapshot_id: Uuid,
    pub target_operation: Uuid,
    pub restore_operation: Option<Uuid>,
    pub resumed: bool,
    pub state: Option<OperationState>,
    pub target_state: OperationState,
    pub receipt: Option<RestoreReceipt>,
    pub kept_created_notes: Vec<i64>,
    pub issues: Vec<Issue>,
    pub next_command: Option<String>,
}

pub fn restore_command(snapshot: Uuid) -> String {
    format!("linguist-anki-bridge snapshots restore {snapshot} --apply")
}

fn restore_issue(code: &str, message: impl Into<String>) -> Issue {
    let mut issue = issue(code, message);
    issue.stage = "restore".into();
    issue
}

/// The original note record that apply captured for an update or migration.
fn original_record(snapshot: &linguist_core::records::Snapshot) -> Result<&SourceRecord> {
    snapshot
        .originals
        .iter()
        .find(|source| source.kind == "lab_apply_prestate_v1")
        .ok_or_else(|| "RESTORE_SNAPSHOT_INCOMPLETE: no original note record".into())
}

fn forward_migration(intent: &ApplyIntent) -> Option<&Migration> {
    intent
        .steps
        .iter()
        .rev()
        .find_map(|step| match &step.effect {
            Effect::UpdateNote(update) => update.migration.as_ref(),
            _ => None,
        })
}

fn uploaded_media(intent: &ApplyIntent) -> Vec<String> {
    intent
        .steps
        .iter()
        .filter_map(|step| match &step.effect {
            Effect::StoreMedia { filename, .. } => Some(filename.clone()),
            _ => None,
        })
        .chain(intent.reused_media.iter().map(|m| m.filename.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Safe alternate name for restored bytes that collide with a current file.
fn alternate_name(name: &str, digest: &str) -> String {
    let short = &digest[..digest.len().min(12)];
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-lab{short}.{ext}"),
        _ => format!("{name}-lab{short}"),
    }
}

fn rewrite_reference(value: &str, from: &str, to: &str) -> String {
    value
        .replace(&format!("[sound:{from}]"), &format!("[sound:{to}]"))
        .replace(&format!("src=\"{from}\""), &format!("src=\"{to}\""))
        .replace(&format!("src='{from}'"), &format!("src='{to}'"))
}

/// One planned media restoration step.
struct MediaStep {
    projection: MediaProjection,
    asset: String,
}

struct Planned {
    plan: RestorePlan,
    target_journal: OperationJournal,
    note: Option<ObservedNote>,
    model: Option<ManagedModel>,
    restore_effect: Option<Effect>,
    media_steps: Vec<MediaStep>,
    kept: Vec<i64>,
    removed: Vec<i64>,
    current: CollectionBinding,
}

fn execution_binding(store: &Store, target: &OperationJournal) -> Result<CollectionBinding> {
    Ok(store
        .binding_decisions(target.id)?
        .last()
        .map(|d| d.new_binding.clone())
        .unwrap_or_else(|| target.binding.clone()))
}

fn managed_model(store: &Store, target: Uuid) -> Result<ManagedModel> {
    let record = store.apply_operation(target)?;
    let plan = store.revision(record.plan_id, record.revision)?;
    plan.rendered
        .iter()
        .find(|r| r.document_id == record.item_id)
        .map(|r| r.model.clone())
        .ok_or_else(|| "RESTORE_TARGET_CORRUPT".into())
}

/// An earlier restore journal for this target that must be resumed rather
/// than replaced.
fn unresolved_restore(store: &Store, target: &OperationJournal) -> Result<Option<Uuid>> {
    for record in store.restore_operations_for(target.id)? {
        match store.journal(record.operation_id) {
            Ok(version) => {
                let finished = match version.journal.state {
                    OperationState::FailedBeforeWrite => true,
                    OperationState::Committed => target.state == OperationState::Restored,
                    // Only verified media and a proven native refusal: a new
                    // restore may supersede it; the media stays.
                    _ => apply::superseded_safely(&version.journal),
                };
                if !finished {
                    return Ok(Some(record.operation_id));
                }
            }
            // The intent precedes the journal; no journal means nothing was sent.
            Err(code) if code == "JOURNAL_NOT_FOUND" => {}
            Err(code) => return Err(code),
        }
    }
    Ok(None)
}

fn observed_digest(note: &[ObservedNote], media: &[(String, Option<String>)]) -> Result<String> {
    canonical::digest("lab-restore-observed-v1", &(note, media)).map_err(|e| e.to_string())
}

/// ALG-RESTORE steps 1-2: inspect the snapshot, the frozen apply intent, its
/// journal and the live collection, and derive every consequence.
fn plan_inner(
    store: &Store,
    port: &mut dyn ApplyPort,
    snapshot_id: Uuid,
    decision: Option<&RestoreDecision>,
) -> Result<Planned> {
    let record = store.snapshot(snapshot_id)?;
    let target = record.snapshot.operation_id;
    let (_apply_record, intent) = match apply::load_intent(store, target) {
        Ok(found) => found,
        Err(code) if code == "APPLY_OPERATION_NOT_FOUND" || code == "JOURNAL_NOT_FOUND" => {
            return Err(
                "RESTORE_TARGET_UNSUPPORTED: only snapshots of apply operations can be restored; imported, split-source and restore snapshots are inspect/export only".into(),
            );
        }
        Err(code) => return Err(code),
    };
    let target_journal = store.journal(target)?.journal;
    if target_journal.snapshot_id != snapshot_id {
        return Err("RESTORE_TARGET_CORRUPT".into());
    }
    match target_journal.state {
        OperationState::Restored => {
            return Err("RESTORE_ALREADY_RESTORED: a verified restore receipt exists".into());
        }
        OperationState::FailedBeforeWrite => {
            return Err("RESTORE_NOTHING_TO_RESTORE: the operation failed before any write".into());
        }
        OperationState::Committed | OperationState::NeedsRecovery => {}
        _ => {
            return Err(format!(
                "RESTORE_RECONCILE_FIRST: operation {target} is unresolved; run {}",
                apply::reconcile_command_for(target)
            ));
        }
    }
    if target_journal
        .steps
        .iter()
        .any(|s| matches!(s.state, StepState::RequestStarted | StepState::Unknown))
    {
        return Err(format!(
            "RESTORE_RECONCILE_FIRST: an unknown outcome must be reconciled before restore; run {}",
            apply::reconcile_command_for(target)
        ));
    }
    let execution = execution_binding(store, &target_journal)?;
    let current = port.execution_binding()?;
    if !same_collection(&execution, &current) {
        return Err("RESTORE_IDENTITY_MISMATCH: profile, path, bridge or lineage changed".into());
    }
    if !apply::loopback(&current.endpoint) {
        return Err(
            "APPLY_REMOTE_MUTATION_UNAVAILABLE: managed writes require a loopback bridge".into(),
        );
    }
    let resume_operation = unresolved_restore(store, &target_journal)?;
    let mut plan = RestorePlan {
        snapshot_id,
        target_operation: target,
        target_state: target_journal.state,
        action: intent.action,
        note_id: intent.note_id,
        observed_state_digest: String::new(),
        model: None,
        fields: vec![],
        tags: None,
        cards: vec![],
        media: vec![],
        uploaded_media_kept: uploaded_media(&intent),
        created_notes: vec![],
        conflicts: vec![],
        blockers: vec![],
        no_change: false,
        resume_operation,
        manual_next_steps: vec![],
    };
    if !plan.uploaded_media_kept.is_empty() {
        plan.manual_next_steps.push(
            "media uploaded by apply stays in the collection; use Anki's Check Media to review unused files".into(),
        );
    }
    let empty = |plan: RestorePlan| Planned {
        plan,
        target_journal: target_journal.clone(),
        note: None,
        model: None,
        restore_effect: None,
        media_steps: vec![],
        kept: vec![],
        removed: vec![],
        current: current.clone(),
    };
    if intent.action == ItemAction::Create {
        let marker = intent.marker_tag.as_deref().ok_or("APPLY_INTENT_CORRUPT")?;
        let candidates = port.notes_tagged(marker)?;
        plan.observed_state_digest = observed_digest(&candidates, &[])?;
        let mut out = empty(plan.clone());
        match candidates.as_slice() {
            [] => {
                plan.no_change = true;
                plan.manual_next_steps.push(
                    "no note carries the operation marker; it may have been deleted or untagged by the user".into(),
                );
            }
            [note] => {
                let projection = project(
                    note,
                    &BTreeSet::new(),
                    Some((intent.target_model_id, &intent.target_manifest_digest)),
                );
                let desired = &intent.desired;
                let unchanged = projection.model_name == desired.model_name
                    && projection.model_manifest_digest == desired.model_manifest_digest
                    && projection.fields == desired.fields
                    && projection.tags == desired.tags
                    && projection
                        .cards
                        .iter()
                        .map(|c| (c.ordinal, c.deck_id))
                        .eq(desired.cards.iter().map(|c| (c.ordinal, c.deck_id)));
                let studied = note.cards.iter().any(|c| c.review_count > 0);
                let requested = decision.is_some_and(|d| d.delete_created_notes.contains(&note.id));
                let action = match (requested, unchanged, studied) {
                    (false, _, _) => "keep",
                    (true, true, false) => "delete_unstudied",
                    (true, _, true) => {
                        plan.blockers.push(format!(
                            "RESTORE_CREATED_NOTE_STUDIED:{}: studied notes are kept; removal needs a separately authorized manual disaster-recovery procedure",
                            note.id
                        ));
                        "keep_studied"
                    }
                    (true, false, false) => {
                        plan.blockers.push(format!(
                            "RESTORE_CREATED_NOTE_CHANGED:{}: content changed after apply; it is kept",
                            note.id
                        ));
                        "keep_changed"
                    }
                };
                plan.created_notes.push(CreatedNote {
                    note_id: note.id,
                    unchanged,
                    studied,
                    action,
                });
                if action == "delete_unstudied" {
                    out.restore_effect = Some(Effect::DeleteUnstudiedCreatedNote {
                        note_id: note.id,
                        expected_pre_digest: content_digest(note)?,
                    });
                    out.note = Some(note.clone());
                    out.model = Some(managed_model(store, target)?);
                } else {
                    plan.no_change = true;
                    if action == "keep" {
                        plan.manual_next_steps.push(format!(
                            "created note {} is kept by default; list it in delete_created_notes to remove it while unchanged and unstudied",
                            note.id
                        ));
                    }
                }
            }
            _ => plan
                .blockers
                .push("RESTORE_MARKER_AMBIGUOUS: several notes carry the operation marker".into()),
        }
        if let Some(decision) = decision
            && decision
                .delete_created_notes
                .iter()
                .any(|id| !plan.created_notes.iter().any(|n| n.note_id == *id))
        {
            plan.blockers
                .push("RESTORE_DECISION_INVALID: unknown created note".into());
        }
        out.plan = plan;
        return Ok(out);
    }

    // Update or migration of an existing note.
    let note_id = intent.note_id.ok_or("APPLY_INTENT_CORRUPT")?;
    let original = original_record(&record.snapshot)?;
    let applied = &intent.desired;
    let Some(note) = port.note(note_id)? else {
        plan.blockers.push(format!(
            "RESTORE_NOTE_MISSING:{note_id}: the note was deleted after apply; restore the checkpoint package manually"
        ));
        plan.observed_state_digest = observed_digest(&[], &[])?;
        let mut out = empty(plan.clone());
        out.plan = plan;
        return Ok(out);
    };
    let migration = forward_migration(&intent);
    let model_change = migration.is_some();
    if note.model_name != applied.model_name || note.model_id != intent.target_model_id {
        plan.blockers.push(
            "RESTORE_MODEL_CHANGED_LATER: the note type changed after apply; manual recovery only"
                .into(),
        );
    }
    // Fields.
    let names: BTreeSet<&String> = original
        .fields
        .keys()
        .chain(applied.fields.keys())
        .chain(note.fields.keys())
        .collect();
    if let Some(decision) = decision
        && let Some(unknown) = decision.fields.keys().find(|k| !names.contains(k))
    {
        plan.blockers
            .push(format!("RESTORE_DECISION_INVALID: unknown field {unknown}"));
    }
    let mut restored_fields = BTreeMap::new();
    for name in names {
        let orig = original.fields.get(name).cloned();
        let app = applied.fields.get(name).cloned();
        let cur = note.fields.get(name).cloned();
        let changed_by_apply = orig != app;
        let changed_later = cur != app;
        let conflict = changed_later && (changed_by_apply || model_change);
        let in_restored_model = orig.is_some();
        let choice = decision.and_then(|d| d.fields.get(name));
        let restored = match (choice, in_restored_model) {
            (Some(FieldChoice::Original), true) => orig.clone(),
            (Some(FieldChoice::Original), false) => None,
            (Some(FieldChoice::Current), true) => match &cur {
                Some(value) => Some(value.clone()),
                None => {
                    plan.blockers.push(format!(
                        "RESTORE_DECISION_INVALID: field {name} has no current value"
                    ));
                    None
                }
            },
            (Some(FieldChoice::Merged(value)), true) => Some(value.clone()),
            (Some(_), false) => {
                plan.blockers.push(format!(
                    "RESTORE_DECISION_INVALID: field {name} does not exist in the restored model"
                ));
                None
            }
            (None, _) if conflict => {
                plan.conflicts
                    .push(format!("RESTORE_FIELD_CONFLICT:{name}"));
                None
            }
            (None, true) if changed_by_apply || model_change => orig.clone(),
            (None, true) => cur.clone().or(orig.clone()),
            (None, false) => None,
        };
        if in_restored_model && let Some(value) = &restored {
            restored_fields.insert(name.clone(), value.clone());
        }
        plan.fields.push(FieldDiff {
            field: name.clone(),
            original: orig,
            applied: app,
            current: cur,
            changed_by_apply,
            changed_later,
            conflict,
            restored,
        });
    }
    // Tags: conflict-free three-way merge.
    let original_tags: BTreeSet<String> = original.tags.iter().cloned().collect();
    let applied_tags: BTreeSet<String> = applied.tags.iter().cloned().collect();
    let current_tags: BTreeSet<String> = note.tags.iter().cloned().collect();
    let removed_later: BTreeSet<_> = applied_tags.difference(&current_tags).cloned().collect();
    let added_later: BTreeSet<_> = current_tags.difference(&applied_tags).cloned().collect();
    let restored_tags: Vec<String> = original_tags
        .difference(&removed_later)
        .cloned()
        .chain(added_later)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    plan.tags = Some(TagDiff {
        original: sorted_tags(&original.tags),
        applied: sorted_tags(&applied.tags),
        current: sorted_tags(&note.tags),
        restored: restored_tags.clone(),
    });
    // Model and reverse mapping.
    let mut reverse = None;
    if let Some(forward) = migration {
        let models = port.models_named(&original_name(&intent))?;
        if !models.iter().any(|m| m.id == forward.source_model_id) {
            plan.blockers.push(
                "RESTORE_SOURCE_MODEL_MISSING: the original note type no longer exists; it is never recreated or overwritten".into(),
            );
        }
        if !decision.is_some_and(|d| d.accept_schema_change) {
            plan.conflicts
                .push("RESTORE_SCHEMA_CHANGE_NOT_ACCEPTED".into());
        }
        let ordinal_map: Vec<OrdinalMapping> = forward
            .ordinal_map
            .iter()
            .map(|m| OrdinalMapping {
                source: m.target,
                target: m.source,
            })
            .collect();
        plan.model = Some(ModelConsequence {
            original_name: original_name(&intent),
            original_model_id: forward.source_model_id,
            current_name: note.model_name.clone(),
            current_model_id: note.model_id,
            reverse_mapping: ordinal_map.clone(),
        });
        reverse = Some(Migration {
            source_model_id: note.model_id,
            target_model_id: forward.source_model_id,
            target_model_name: original_name(&intent),
            ordinal_map,
        });
    }
    // Cards.
    let retained: BTreeSet<i64> = intent.retained_card_ids.iter().copied().collect();
    let original_decks: BTreeMap<i64, i64> = original
        .cards
        .iter()
        .filter_map(|card| {
            Some((
                String::from(card.id.clone()).parse().ok()?,
                String::from(card.deck_id.clone()).parse().ok()?,
            ))
        })
        .collect();
    let reviews_at_apply: BTreeMap<i64, u64> = applied
        .cards
        .iter()
        .filter_map(|card| card.id.map(|id| (id, card.review_count)))
        .collect();
    for id in &retained {
        if !note.cards.iter().any(|c| c.id == *id) {
            plan.blockers.push(format!(
                "RESTORE_RETAINED_CARD_MISSING:{id}: a retained card was deleted after apply"
            ));
        }
    }
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    let mut card_decks = Vec::new();
    for card in &note.cards {
        let is_retained = retained.contains(&card.id);
        let mut consequence = CardConsequence {
            card_id: card.id,
            retained: is_retained,
            ordinal_now: card.ordinal,
            ordinal_after: Some(card.ordinal),
            deck_now: card.deck_id,
            deck_original: original_decks.get(&card.id).copied(),
            deck_applied: intent.deck_id,
            deck_after: Some(card.deck_id),
            reviews_at_apply: reviews_at_apply.get(&card.id).copied(),
            reviews_now: card.review_count,
            later_reviews: reviews_at_apply
                .get(&card.id)
                .is_some_and(|before| card.review_count > *before),
            deck_conflict: false,
            action: "keep_history",
        };
        if card.original_deck_id != 0 {
            plan.blockers.push(format!(
                "RESTORE_FILTERED_DECK_BLOCKS:{}: return the card to its home deck first",
                card.id
            ));
        }
        let mapped = match &reverse {
            Some(reverse) => reverse
                .ordinal_map
                .iter()
                .find(|m| m.source == card.ordinal)
                .map(|m| m.target),
            None => Some(card.ordinal),
        };
        consequence.ordinal_after = mapped;
        if mapped.is_none() {
            if is_retained {
                plan.blockers.push(format!(
                    "RESTORE_REVERSE_MAPPING_INCOMPATIBLE:{}: a retained card has no reverse task mapping",
                    card.id
                ));
                consequence.action = "blocked_reverse_mapping";
            } else if card.review_count > 0 {
                plan.blockers.push(format!(
                    "RESTORE_STUDIED_NEW_TASK:{}: a card created by apply has later study and cannot be removed by first-release reverse mapping; keep the current note or use manual disaster recovery",
                    card.id
                ));
                consequence.action = "blocked_studied_new_task";
            } else if decision.is_some_and(|d| d.remove_unstudied_cards.contains(&card.id)) {
                consequence.action = "remove_unstudied";
                consequence.deck_after = None;
                removed.push(card.id);
            } else {
                plan.conflicts
                    .push(format!("RESTORE_CARD_REMOVAL_REVIEW:{}", card.id));
                consequence.action = "remove_unstudied_needs_review";
                consequence.deck_after = None;
            }
            plan.cards.push(consequence);
            continue;
        }
        if is_retained {
            let original_deck = consequence.deck_original.unwrap_or(card.deck_id);
            let moved_later = card.deck_id != intent.deck_id;
            consequence.deck_conflict = moved_later && original_deck != card.deck_id;
            let choice = decision.and_then(|d| d.decks.get(&card.id.to_string()));
            consequence.deck_after = match (consequence.deck_conflict, choice) {
                (_, Some(DeckChoice::Original)) => Some(original_deck),
                (_, Some(DeckChoice::Current)) => Some(card.deck_id),
                (true, None) => {
                    plan.conflicts
                        .push(format!("RESTORE_DECK_CONFLICT:{}", card.id));
                    None
                }
                (false, None) => Some(original_deck),
            };
        } else {
            consequence.action = "keep_new_card";
            plan.manual_next_steps.push(format!(
                "card {} was added by apply and is kept with its history; Anki's Empty Cards tool can remove it if its template is empty after restore",
                card.id
            ));
        }
        if let Some(deck) = consequence.deck_after {
            card_decks.push(CardDeck {
                card_id: card.id,
                deck_id: deck,
            });
        }
        kept.push(card.id);
        plan.cards.push(consequence);
    }
    if let Some(decision) = decision {
        if decision
            .remove_unstudied_cards
            .iter()
            .any(|id| !removed.contains(id))
        {
            plan.blockers
                .push("RESTORE_DECISION_INVALID: a listed card is not removable".into());
        }
        if decision
            .decks
            .keys()
            .any(|id| !plan.cards.iter().any(|c| c.card_id.to_string() == *id))
        {
            plan.blockers
                .push("RESTORE_DECISION_INVALID: unknown card in deck choices".into());
        }
        if !decision.delete_created_notes.is_empty() {
            plan.blockers
                .push("RESTORE_DECISION_INVALID: this operation created no note".into());
        }
    }
    // Media referenced by the restored fields, restored before the fields.
    let mut media_steps = Vec::new();
    let mut media_observations = Vec::new();
    let references: BTreeSet<String> =
        crate::capture::discover_media(&restored_fields, 100 * 1024 * 1024, 10000)?
            .references
            .into_iter()
            .map(|reference| reference.filename)
            .collect();
    for name in references {
        let archived = record.snapshot.media.iter().find(|m| m.filename == name);
        let present = port.media(&name)?;
        media_observations.push((name.clone(), present.as_ref().map(|m| m.sha256.clone())));
        let mut consequence = MediaConsequence {
            filename: name.clone(),
            present: present.is_some(),
            archived: archived.is_some(),
            action: "present",
            restored_name: Some(name.clone()),
        };
        match (present, archived) {
            (Some(found), Some(asset)) if found.sha256 != asset.digest => {
                let alternate = alternate_name(&name, &asset.digest);
                match port.media(&alternate)? {
                    Some(existing) if existing.sha256 == asset.digest => {
                        consequence.action = "reuse_alternate";
                    }
                    Some(_) => {
                        plan.blockers.push(format!(
                            "RESTORE_MEDIA_COLLISION:{name}: original bytes differ and the alternate name is taken"
                        ));
                        consequence.action = "blocked_collision";
                    }
                    None => {
                        consequence.action = "restore_as_alternate";
                        media_steps.push(MediaStep {
                            projection: MediaProjection {
                                filename: alternate.clone(),
                                sha256: asset.digest.clone(),
                                size_bytes: asset.size_bytes,
                            },
                            asset: asset.digest.clone(),
                        });
                    }
                }
                for value in restored_fields.values_mut() {
                    *value = rewrite_reference(value, &name, &alternate);
                }
                for diff in &mut plan.fields {
                    if let Some(value) = &mut diff.restored {
                        *value = rewrite_reference(value, &name, &alternate);
                    }
                }
                consequence.restored_name = Some(alternate);
            }
            (Some(_), _) => {}
            (None, Some(asset)) => {
                consequence.action = "restore_archived_bytes";
                media_steps.push(MediaStep {
                    projection: MediaProjection {
                        filename: name.clone(),
                        sha256: asset.digest.clone(),
                        size_bytes: asset.size_bytes,
                    },
                    asset: asset.digest.clone(),
                });
            }
            (None, None) => {
                if decision.is_some_and(|d| d.accept_missing_media.contains(&name)) {
                    consequence.action = "missing_accepted";
                    consequence.restored_name = None;
                } else {
                    plan.conflicts.push(format!("RESTORE_MEDIA_MISSING:{name}"));
                    consequence.action = "missing_needs_review";
                    consequence.restored_name = None;
                }
            }
        }
        plan.media.push(consequence);
    }
    plan.observed_state_digest = observed_digest(std::slice::from_ref(&note), &media_observations)?;
    let field_names_match = original.fields.keys().eq(restored_fields.keys());
    let unchanged = restored_fields == note.fields
        && restored_tags == sorted_tags(&note.tags)
        && reverse.is_none()
        && removed.is_empty()
        && media_steps.is_empty()
        && card_decks.iter().all(|c| {
            note.cards
                .iter()
                .any(|n| n.id == c.card_id && n.deck_id == c.deck_id)
        });
    plan.no_change = unchanged;
    let restore_effect =
        (field_names_match && plan.conflicts.is_empty() && plan.blockers.is_empty() && !unchanged)
            .then(|| -> Result<Effect> {
                Ok(Effect::RestoreNote(RestoreNote {
                    note_id,
                    expected_pre_digest: content_digest(&note)?,
                    migration: reverse.clone(),
                    fields: restored_fields.clone(),
                    tags: restored_tags.clone(),
                    card_decks,
                    removed_card_ids: removed.clone(),
                }))
            })
            .transpose()?;
    Ok(Planned {
        plan,
        target_journal,
        note: Some(note),
        model: Some(managed_model(store, target)?),
        restore_effect,
        media_steps,
        kept,
        removed,
        current,
    })
}

fn original_name(intent: &ApplyIntent) -> String {
    intent
        .pre_state
        .as_ref()
        .map(|pre| pre.model_name.clone())
        .unwrap_or_else(|| intent.target_model_name.clone())
}

/// Live ALG-RESTORE preview. It reads the store and the collection only.
pub fn plan_restore(
    store: &Store,
    port: &mut dyn ApplyPort,
    snapshot_id: Uuid,
    decision: Option<&RestoreDecision>,
) -> Result<RestorePlan> {
    Ok(plan_inner(store, port, snapshot_id, decision)?.plan)
}

/// Expected post-state of the main reverse effect, built from the current
/// note so that later study is part of what must survive.
fn expected_after(
    note: &ObservedNote,
    effect: &Effect,
    model_digest: &str,
) -> Result<serde_json::Value> {
    match effect {
        Effect::RestoreNote(restore) => {
            let mut after = note.clone();
            after
                .cards
                .retain(|c| !restore.removed_card_ids.contains(&c.id));
            for card in &mut after.cards {
                if let Some(migration) = &restore.migration {
                    card.ordinal = migration
                        .ordinal_map
                        .iter()
                        .find(|m| m.source == card.ordinal)
                        .map(|m| m.target)
                        .ok_or("RESTORE_REVERSE_MAPPING_INCOMPATIBLE")?;
                }
                card.deck_id = restore
                    .card_decks
                    .iter()
                    .find(|c| c.card_id == card.id)
                    .map(|c| c.deck_id)
                    .ok_or("RESTORE_INTENT_CORRUPT")?;
            }
            if let Some(migration) = &restore.migration {
                after.model_id = migration.target_model_id;
                after.model_name = migration.target_model_name.clone();
                after.model_manifest_digest = model_digest.to_owned();
            }
            after.fields = restore.fields.clone();
            after.tags = restore.tags.clone();
            let kept = restore.card_decks.iter().map(|c| c.card_id).collect();
            serde_json::to_value(restore_projection(&after, &kept)).map_err(|e| e.to_string())
        }
        Effect::DeleteUnstudiedCreatedNote { note_id, .. } => Ok(deleted_projection(*note_id)),
        _ => Err("RESTORE_INTENT_CORRUPT".into()),
    }
}

fn reverse_variants(steps: &[IntentStep]) -> BTreeSet<&'static str> {
    steps.iter().map(|s| s.effect.variant()).collect()
}

/// ALG-RESTORE with `--apply`: a new journaled restore, or the resumption of
/// an unresolved one. Repeated calls resume the same restore journal.
pub fn restore(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    request: &RestoreRequest,
) -> Result<RestoreOutcome> {
    if !request.apply {
        return Err(
            "RESTORE_FLAG_REQUIRED: the current invocation must pass --apply; without it use the preview".into(),
        );
    }
    store.validate_lease(lease)?;
    let snapshot = store.snapshot(request.snapshot_id)?;
    let target = snapshot.snapshot.operation_id;
    if let Ok(version) = store.journal(target)
        && let Some(existing) = unresolved_restore(store, &version.journal)?
    {
        return resume(store, port, request.snapshot_id, existing);
    }
    let decision = request.decision.as_ref().ok_or(
        "RESTORE_DECISION_REQUIRED: preview first and bind a decision to its observed_state_digest",
    )?;
    if decision.schema_version != 1
        || decision.snapshot_id != request.snapshot_id
        || decision.actor.trim().is_empty()
        || decision.actor.chars().any(char::is_control)
    {
        return Err("RESTORE_DECISION_INVALID".into());
    }
    let planned = plan_inner(store, port, request.snapshot_id, Some(decision))?;
    if decision.observed_state_digest != planned.plan.observed_state_digest {
        return Err(
            "RESTORE_DECISION_STALE: the collection changed since the reviewed preview".into(),
        );
    }
    if !planned.plan.blockers.is_empty() {
        return Err(format!(
            "RESTORE_UNSUPPORTED: {}",
            planned.plan.blockers.join("; ")
        ));
    }
    if !planned.plan.conflicts.is_empty() {
        return Err(format!(
            "RESTORE_CONFLICT_UNRESOLVED: {}",
            planned.plan.conflicts.join("; ")
        ));
    }
    let Some(main_effect) = planned.restore_effect.clone() else {
        return Ok(RestoreOutcome {
            snapshot_id: request.snapshot_id,
            target_operation: target,
            restore_operation: None,
            resumed: false,
            state: None,
            target_state: planned.target_journal.state,
            receipt: None,
            kept_created_notes: planned
                .plan
                .created_notes
                .iter()
                .map(|n| n.note_id)
                .collect(),
            issues: vec![restore_issue(
                "RESTORE_NOTHING_TO_RESTORE",
                "the collection already matches the restore target or only kept items remain",
            )],
            next_command: None,
        });
    };
    let note = planned.note.clone().ok_or("RESTORE_INTENT_CORRUPT")?;
    let model = planned.model.clone().ok_or("RESTORE_INTENT_CORRUPT")?;
    // Checkpoint before any restore effect, covering the note, its cards and,
    // for a reverse note-type change, both models.
    let schema = matches!(&main_effect, Effect::RestoreNote(r) if r.migration.is_some());
    let card_ids: Vec<i64> = note.cards.iter().map(|c| c.id).collect();
    let mut model_ids = vec![];
    if let Effect::RestoreNote(RestoreNote {
        migration: Some(migration),
        ..
    }) = &main_effect
    {
        model_ids = vec![migration.source_model_id, migration.target_model_id];
        model_ids.sort();
    }
    let authorization: CheckpointAuthorization = require_checkpoint(
        store,
        request.checkpoint_id,
        &DependentScope {
            binding: &planned.current,
            requirement: CoverageRequirement {
                scheduling: true,
                media: true,
                schema,
            },
            note_ids: &[note.id],
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
    let mut steps = Vec::new();
    for media in &planned.media_steps {
        let bytes = store.asset(&media.asset, request.max_media_bytes)?;
        if canonical::asset_digest(&bytes) != media.projection.sha256
            || bytes.len() as u64 != media.projection.size_bytes
        {
            return Err("RESTORE_MEDIA_ARCHIVE_MISMATCH".into());
        }
        let effect = Effect::StoreMedia {
            filename: media.projection.filename.clone(),
            sha256: media.projection.sha256.clone(),
            size_bytes: media.projection.size_bytes,
            staged_asset: media.asset.clone(),
        };
        steps.push(IntentStep {
            step_id: Uuid::new_v4(),
            payload_digest: effect.payload_digest()?,
            precondition_digest: canonical::digest(
                "lab-apply-media-absent-v1",
                &media.projection.filename,
            )
            .map_err(|e| e.to_string())?,
            expected: serde_json::to_value(&media.projection).map_err(|e| e.to_string())?,
            expected_digest: digest_of(&media.projection)?,
            effect,
        });
    }
    let original_digest = original_record(&snapshot.snapshot)
        .map(|o| o.model_manifest.clone())
        .unwrap_or_default();
    let expected = expected_after(&note, &main_effect, &original_digest)?;
    let precondition = match &main_effect {
        Effect::RestoreNote(r) => r.expected_pre_digest.clone(),
        Effect::DeleteUnstudiedCreatedNote {
            expected_pre_digest,
            ..
        } => expected_pre_digest.clone(),
        _ => return Err("RESTORE_INTENT_CORRUPT".into()),
    };
    steps.push(IntentStep {
        step_id: Uuid::new_v4(),
        payload_digest: main_effect.payload_digest()?,
        precondition_digest: precondition,
        expected_digest: canonical::asset_digest(
            &canonical::bytes(&expected).map_err(|e| e.to_string())?,
        ),
        expected,
        effect: main_effect.clone(),
    });
    let available = port.mutation_variants()?;
    if let Some(missing) = reverse_variants(&steps)
        .into_iter()
        .find(|variant| !available.iter().any(|a| a == variant))
    {
        return Err(format!(
            "CAPABILITY_UNAVAILABLE: native {missing} variant is not declared"
        ));
    }
    let deleting = matches!(main_effect, Effect::DeleteUnstudiedCreatedNote { .. });
    let intent = RestoreIntent {
        schema_version: 1,
        note_id: note.id,
        deleting,
        kept_card_ids: if deleting {
            vec![]
        } else {
            planned.kept.clone()
        },
        removed_card_ids: planned.removed.clone(),
        restored_media: planned
            .media_steps
            .iter()
            .map(|m| m.projection.filename.clone())
            .collect(),
        steps,
        checkpoint: serde_json::to_value(&authorization).map_err(|e| e.to_string())?,
        plan_digest: canonical::digest("lab-restore-plan-v1", &planned.plan)
            .map_err(|e| e.to_string())?,
    };
    let operation = Uuid::new_v4();
    let decision_digest = decision.digest()?;
    store.publish_restore_operation(&RestoreOperationRecord {
        operation_id: operation,
        target_operation: target,
        target_snapshot: request.snapshot_id,
        created_ms: request.now_ms,
        decision: serde_json::to_value(decision).map_err(|e| e.to_string())?,
        intent: serde_json::to_value(&intent).map_err(|e| e.to_string())?,
    })?;
    // Fresh capture of the current state before any reverse effect.
    let restore_snapshot = apply::fresh_snapshot(
        store,
        port,
        operation,
        Some(&note),
        &model,
        None,
        request.now_ms,
    )?;
    let journal = OperationJournal {
        id: operation,
        group_id: request.group_id,
        approval_digest: decision_digest.clone(),
        binding: planned.current.clone(),
        snapshot_id: restore_snapshot.id,
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
        recovery: restore_command(request.snapshot_id),
    };
    journal.advance(|j| j.state = OperationState::Preflight)?;
    let owner = match port.begin(&planned.current, &decision_digest) {
        Ok(owner) => owner,
        Err(code) => {
            journal.advance(|j| {
                j.state = OperationState::FailedBeforeWrite;
                j.issues
                    .push(restore_issue("APPLY_OWNER_UNAVAILABLE", code.clone()));
            })?;
            return Err(format!("APPLY_OWNER_UNAVAILABLE: {code}"));
        }
    };
    journal.advance(|j| j.state = OperationState::Checkpointed)?;
    let context = restore_context(
        &intent,
        planned.current.clone(),
        decision_digest,
        owner.clone(),
        operation,
    );
    let result = apply::drive(&mut journal, port, &context);
    let finished = match result {
        Ok(Drive::Committed) => Some(finalize(&mut journal, port, &context, &intent, target)),
        Ok(Drive::Stopped) => None,
        Err(code) => {
            let _ = port.end(&owner);
            return Err(code);
        }
    };
    let _ = port.end(&owner);
    outcome(journal, request.snapshot_id, target, false, finished)
}

fn restore_context<'a>(
    intent: &'a RestoreIntent,
    binding: CollectionBinding,
    approval_digest: String,
    owner: OwnerToken,
    operation: Uuid,
) -> Context<'a> {
    Context {
        steps: &intent.steps,
        marker_tag: None,
        retained: BTreeSet::new(),
        target_model: (0, ""),
        binding,
        approval_digest,
        owner,
        operation,
    }
}

/// Save the verified read-back as a restore receipt, commit the restore
/// journal, then finalize the restored apply operation.
fn finalize(
    journal: &mut Journal,
    port: &mut dyn ApplyPort,
    context: &Context,
    intent: &RestoreIntent,
    target: Uuid,
) -> Result<RestoreReceipt> {
    let operation = journal.version.journal.id;
    let last = intent.steps.last().ok_or("RESTORE_INTENT_CORRUPT")?;
    let receipt = match journal.store.restore_receipt(operation)? {
        Some(stored) => stored,
        None => {
            let note = port.note(intent.note_id)?;
            let (projection, kept_history) = match (&note, intent.deleting) {
                (None, true) => (deleted_projection(intent.note_id), vec![]),
                (Some(note), false) => {
                    let kept: BTreeSet<i64> = intent.kept_card_ids.iter().copied().collect();
                    let projection = restore_projection(note, &kept);
                    let history: Vec<(i64, String)> = note
                        .cards
                        .iter()
                        .filter(|c| kept.contains(&c.id))
                        .map(|c| (c.id, c.history_digest.clone()))
                        .collect();
                    (
                        serde_json::to_value(projection).map_err(|e| e.to_string())?,
                        history,
                    )
                }
                _ => {
                    journal.advance(|j| {
                        j.state = OperationState::NeedsRecovery;
                        j.issues.push(restore_issue(
                            "RESTORE_FINAL_READBACK_MISMATCH",
                            "final read-back no longer matches the restored state",
                        ));
                    })?;
                    return Err("RESTORE_FINAL_READBACK_MISMATCH".into());
                }
            };
            let bytes = canonical::bytes(&projection).map_err(|e| e.to_string())?;
            if canonical::asset_digest(&bytes) != last.expected_digest {
                journal.advance(|j| {
                    j.state = OperationState::NeedsRecovery;
                    j.issues.push(restore_issue(
                        "RESTORE_FINAL_READBACK_MISMATCH",
                        "final read-back changed after step verification; later edits or study need review",
                    ));
                })?;
                return Err("RESTORE_FINAL_READBACK_MISMATCH".into());
            }
            let observed = journal.store.publish_asset(&bytes, 100 * 1024 * 1024)?;
            let evidence = serde_json::json!({
                "schema_version": 1,
                "kind": "lab_restore_readback_v1",
                "operation_id": operation,
                "target_operation": target,
                "note": note,
                "binding": context.binding,
            });
            let evidence_digest = journal.store.publish_asset(
                &canonical::bytes(&evidence).map_err(|e| e.to_string())?,
                100 * 1024 * 1024,
            )?;
            let record = journal.store.restore_operation(operation)?;
            let receipt = RestoreReceipt {
                schema_version: 1,
                restore_operation: operation,
                target_operation: target,
                target_snapshot: record.target_snapshot,
                lineage_id: journal.version.journal.binding.lineage_id,
                session_epoch: journal.version.journal.binding.session_epoch,
                observed_state_digest: observed,
                evidence_digest,
                note_id: (!intent.deleting).then_some(intent.note_id),
                deleted_note_ids: if intent.deleting {
                    vec![intent.note_id]
                } else {
                    vec![]
                },
                kept_card_ids: intent.kept_card_ids.clone(),
                removed_card_ids: intent.removed_card_ids.clone(),
                history_digest: digest_of(&kept_history)?,
                restored_media: intent.restored_media.clone(),
            };
            if journal.version.journal.state != OperationState::Verifying {
                journal.advance(|j| j.state = OperationState::Verifying)?;
            }
            journal.store.append_restore_receipt(&receipt)?;
            receipt
        }
    };
    if journal.version.journal.state != OperationState::Committed {
        if journal.version.journal.state != OperationState::Verifying {
            journal.advance(|j| j.state = OperationState::Verifying)?;
        }
        journal.advance(|j| j.state = OperationState::Committed)?;
    }
    // Finalize the restored apply operation; its evidence is unchanged.
    let version = journal.store.journal(target)?;
    if version.journal.state != OperationState::Restored {
        let mut next = version.journal.clone();
        next.state = OperationState::Restored;
        journal
            .store
            .append_journal(&next, Some(&version))
            .map_err(|code| {
                format!(
                    "APPLY_LOCAL_DURABILITY_FAILED: {code}; stop all writes and run {}",
                    journal.recovery
                )
            })?;
    }
    Ok(receipt)
}

fn outcome(
    journal: Journal,
    snapshot: Uuid,
    target: Uuid,
    resumed: bool,
    finished: Option<Result<RestoreReceipt>>,
) -> Result<RestoreOutcome> {
    let receipt = match finished {
        Some(Ok(receipt)) => Some(receipt),
        Some(Err(code)) if code.starts_with("RESTORE_FINAL_READBACK_MISMATCH") => None,
        Some(Err(code)) => return Err(code),
        None => None,
    };
    let state = journal.version.journal.state;
    let target_state = journal.store.journal(target)?.journal.state;
    let unresolved = !(state == OperationState::Committed
        && target_state == OperationState::Restored)
        && state != OperationState::FailedBeforeWrite;
    Ok(RestoreOutcome {
        snapshot_id: snapshot,
        target_operation: target,
        restore_operation: Some(journal.version.journal.id),
        resumed,
        state: Some(state),
        target_state,
        receipt,
        kept_created_notes: vec![],
        issues: journal.version.journal.issues.clone(),
        next_command: unresolved.then(|| restore_command(snapshot)),
    })
}

fn load_restore(store: &Store, operation: Uuid) -> Result<(RestoreOperationRecord, RestoreIntent)> {
    let record = store.restore_operation(operation)?;
    let intent: RestoreIntent =
        serde_json::from_value(record.intent.clone()).map_err(|_| "RESTORE_OPERATION_CORRUPT")?;
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
        return Err("RESTORE_OPERATION_CORRUPT".into());
    }
    Ok((record, intent))
}

/// Resume one restore journal: reconcile every unverified reverse effect from
/// native status and read-back, then finish. It never assumes a transactional
/// rollback and never creates a second restore operation.
fn resume(
    store: &mut Store,
    port: &mut dyn ApplyPort,
    snapshot: Uuid,
    operation: Uuid,
) -> Result<RestoreOutcome> {
    let (record, intent) = load_restore(store, operation)?;
    let version = store.journal(operation)?;
    let target = record.target_operation;
    let current = port.execution_binding()?;
    let binding = version.journal.binding.clone();
    if !same_collection(&binding, &current) {
        return Err("RESTORE_IDENTITY_MISMATCH: profile, path, bridge or lineage changed".into());
    }
    if binding.session_epoch != current.session_epoch {
        return Err(format!(
            "RESTORE_SESSION_CHANGED: restore journal {operation} was started in another collection session; rebinding a restore journal is unsupported, inspect it with `recover inspect` and resolve it manually"
        ));
    }
    let approval = version.journal.approval_digest.clone();
    let owner = port.begin(&current, &approval)?;
    let context = restore_context(&intent, current, approval, owner.clone(), operation);
    let mut journal = Journal {
        store,
        version,
        recovery: restore_command(snapshot),
    };
    let committed = journal.version.journal.state == OperationState::Committed;
    let stopped = if committed {
        false
    } else {
        match apply::reconcile_steps(&mut journal, port, &context, true, true) {
            Ok((_, stopped)) => stopped,
            Err(code) => {
                let _ = port.end(&owner);
                return Err(code);
            }
        }
    };
    let finished = (!stopped).then(|| finalize(&mut journal, port, &context, &intent, target));
    let _ = port.end(&owner);
    outcome(journal, snapshot, target, true, finished)
}

/// OP-44 preview for one group: every apply operation that carries the group
/// ID, anchors and updates first, then created notes.
#[derive(Clone, Debug, Serialize)]
pub struct GroupRollbackItem {
    pub operation_id: Uuid,
    pub snapshot_id: Uuid,
    pub state: OperationState,
    pub plan: Option<RestorePlan>,
    pub error: Option<String>,
}

pub fn plan_group_rollback(
    store: &Store,
    port: &mut dyn ApplyPort,
    group: Uuid,
) -> Result<Vec<GroupRollbackItem>> {
    let mut items = Vec::new();
    for version in store.group_journals(group, 10000)? {
        let journal = version.journal;
        // Restore journals carry the group too; only apply operations roll back.
        if store.apply_operation(journal.id).is_err() {
            continue;
        }
        let (plan, error) = match plan_restore(store, port, journal.snapshot_id, None) {
            Ok(plan) => (Some(plan), None),
            Err(code) => (None, Some(code)),
        };
        items.push(GroupRollbackItem {
            operation_id: journal.id,
            snapshot_id: journal.snapshot_id,
            state: journal.state,
            plan,
            error,
        });
    }
    // Existing-note restores (the split anchor) come before created notes.
    items.sort_by_key(|item| {
        (
            item.plan
                .as_ref()
                .is_none_or(|plan| plan.action == ItemAction::Create),
            item.operation_id,
        )
    });
    if items.is_empty() {
        return Err("ROLLBACK_GROUP_EMPTY: no apply operation carries this group".into());
    }
    Ok(items)
}

/// OP-44 execution: one restore per reviewed decision, in the preview order.
/// Identity and local durability faults stop the group; other item failures
/// are reported and later items continue. Partial rollback is explicit.
pub fn rollback_group(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    group: Uuid,
    requests: &[RestoreRequest],
) -> Result<Vec<std::result::Result<RestoreOutcome, (Uuid, String)>>> {
    let order = plan_group_rollback(store, port, group)?;
    let mut out = Vec::new();
    for item in order {
        let Some(request) = requests.iter().find(|r| r.snapshot_id == item.snapshot_id) else {
            continue;
        };
        crate::backup::keep_writer_lease(store, lease)?;
        let result = restore(store, lease, port, request).map_err(|e| (item.snapshot_id, e));
        let stop = matches!(&result, Err((_, code)) if [
            "RESTORE_IDENTITY_MISMATCH",
            "APPLY_LOCAL_DURABILITY_FAILED",
            "LEASE_",
            "RESTORE_SESSION_CHANGED",
        ]
        .iter()
        .any(|prefix| code.starts_with(prefix)));
        out.push(result);
        if stop {
            break;
        }
    }
    if out.is_empty() {
        return Err(
            "ROLLBACK_DECISIONS_MISSING: no decision names a snapshot of this group".into(),
        );
    }
    Ok(out)
}

/// Local-only OP-50 preview from stored records: what apply changed and what
/// a restore would have to reverse. Live conflicts need the native port.
#[derive(Clone, Debug, Serialize)]
pub struct LocalRestorePreview {
    pub snapshot_id: Uuid,
    pub target_operation: Uuid,
    pub target_state: OperationState,
    pub action: ItemAction,
    pub note_id: Option<i64>,
    pub model_change: Option<(String, String)>,
    pub fields_changed_by_apply: Vec<String>,
    pub tags_added_by_apply: Vec<String>,
    pub deck_moves: Vec<(i64, Option<i64>, i64)>,
    pub cards_added_by_apply: usize,
    pub uploaded_media_kept: Vec<String>,
    pub archived_source_media: Vec<String>,
    pub restore_operations: Vec<(Uuid, Option<OperationState>, bool)>,
    pub blockers: Vec<String>,
}

pub fn local_preview(store: &Store, snapshot_id: Uuid) -> Result<LocalRestorePreview> {
    let record = store.snapshot(snapshot_id)?;
    let target = record.snapshot.operation_id;
    let (_apply, intent) = match apply::load_intent(store, target) {
        Ok(found) => found,
        Err(code) if code == "APPLY_OPERATION_NOT_FOUND" || code == "JOURNAL_NOT_FOUND" => {
            return Err(
                "RESTORE_TARGET_UNSUPPORTED: only snapshots of apply operations can be restored; imported, split-source and restore snapshots are inspect/export only".into(),
            );
        }
        Err(code) => return Err(code),
    };
    let journal = store.journal(target)?.journal;
    let mut blockers = Vec::new();
    match journal.state {
        OperationState::Restored => blockers.push("RESTORE_ALREADY_RESTORED".into()),
        OperationState::FailedBeforeWrite => blockers.push("RESTORE_NOTHING_TO_RESTORE".into()),
        OperationState::Committed | OperationState::NeedsRecovery => {}
        _ => blockers.push("RESTORE_RECONCILE_FIRST".into()),
    }
    if journal
        .steps
        .iter()
        .any(|s| matches!(s.state, StepState::RequestStarted | StepState::Unknown))
        && !blockers.iter().any(|b| b == "RESTORE_RECONCILE_FIRST")
    {
        blockers.push("RESTORE_RECONCILE_FIRST".into());
    }
    let pre = intent.pre_state.as_ref();
    let mut fields_changed = Vec::new();
    let mut tags_added = Vec::new();
    let mut deck_moves = Vec::new();
    if let Some(pre) = pre {
        let names: BTreeSet<&String> = pre
            .fields
            .keys()
            .chain(intent.desired.fields.keys())
            .collect();
        for name in names {
            if pre.fields.get(name) != intent.desired.fields.get(name) {
                fields_changed.push(name.clone());
            }
        }
        let before: BTreeSet<&String> = pre.tags.iter().collect();
        tags_added = intent
            .desired
            .tags
            .iter()
            .filter(|t| !before.contains(t))
            .cloned()
            .collect();
        for card in &pre.cards {
            if card.deck_id != intent.deck_id {
                deck_moves.push((
                    card.id.unwrap_or_default(),
                    Some(card.deck_id),
                    intent.deck_id,
                ));
            }
        }
    }
    let restore_operations = store
        .restore_operations_for(target)?
        .into_iter()
        .map(|r| {
            let state = store.journal(r.operation_id).ok().map(|v| v.journal.state);
            let receipt = store
                .restore_receipt(r.operation_id)
                .ok()
                .flatten()
                .is_some();
            (r.operation_id, state, receipt)
        })
        .collect();
    Ok(LocalRestorePreview {
        snapshot_id,
        target_operation: target,
        target_state: journal.state,
        action: intent.action,
        note_id: intent.note_id,
        model_change: forward_migration(&intent)
            .map(|_| (original_name(&intent), intent.target_model_name.clone())),
        fields_changed_by_apply: fields_changed,
        tags_added_by_apply: tags_added,
        deck_moves,
        cards_added_by_apply: intent
            .desired
            .cards
            .iter()
            .filter(|c| c.id.is_none())
            .count(),
        uploaded_media_kept: uploaded_media(&intent),
        archived_source_media: record
            .snapshot
            .media
            .iter()
            .map(|m| m.filename.clone())
            .collect(),
        restore_operations,
        blockers,
    })
}
