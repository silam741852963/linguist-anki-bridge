//! WP-27 `notes retag`: re-render applied notes from their stored documents
//! with the current renderer (part-of-speech classes in the Meaning HTML, the
//! label-table tags, the judged profile) and write the fields and tags in
//! place. Nothing is generated, so there is no review: a note is rewritten
//! only while it still equals what its latest operation wrote, under a
//! verified checkpoint, a fresh snapshot and the apply journal. The write is
//! an exact `restore_note` recorded as an apply operation of the same plan
//! item, so `recover reconcile`, `snapshots restore` and the next retag treat
//! it like any other apply.
use crate::apply::{
    self, ApplyIntent, ApplyPort, CardDeck, Context, Drive, Effect, IntentStep, ItemAction,
    Journal, NoteProjection, ObservedNote, RestoreNote, content_digest, digest_of, project,
    sorted_tags,
};
use crate::backup::{DependentScope, Result, require_checkpoint};
use crate::checkpoint::CoverageRequirement;
use linguist_core::{
    LearningDocument,
    records::{JournalStep, OperationJournal, OperationState, PlanRevision, StepState},
    render,
    validation::{Issue, Severity},
};
use linguist_store::{Store, apply::ApplyOperationRecord, lease::LeaseToken};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// The latest committed operation of one note: what it wrote and the plan
/// item whose document it rendered.
#[derive(Clone, Debug)]
pub struct Applied {
    pub note_id: i64,
    pub operation: Uuid,
    pub plan_id: Uuid,
    pub revision: u32,
    pub item_id: Uuid,
    pub approval_id: Uuid,
    pub desired: NoteProjection,
}

/// The latest operation of each note, oldest operations first. A note whose
/// latest operation was restored or is unresolved maps to its state instead.
pub fn latest_by_note(
    store: &Store,
) -> Result<BTreeMap<i64, std::result::Result<Applied, String>>> {
    let mut latest = BTreeMap::new();
    for operation in store.apply_operation_ids()? {
        let record = store.apply_operation(operation)?;
        // Merge deletions and other effects are not notes this command renders.
        let Ok(intent) = serde_json::from_value::<ApplyIntent>(record.intent.clone()) else {
            continue;
        };
        let journal = match store.journal(operation) {
            Ok(version) => version.journal,
            Err(code) if code == "JOURNAL_NOT_FOUND" => continue,
            Err(code) => return Err(code),
        };
        if journal.state == OperationState::FailedBeforeWrite {
            continue;
        }
        let note_id = match intent.note_id {
            Some(id) => Some(id),
            None => store
                .snapshot(journal.snapshot_id)
                .ok()
                .and_then(|s| s.after)
                .and_then(|r| r.readback)
                .and_then(|r| r.note_ids.first().map(|id| String::from(id.clone())))
                .and_then(|id| id.parse().ok()),
        };
        let Some(note_id) = note_id else {
            continue;
        };
        let entry = match journal.state {
            OperationState::Committed => Ok(Applied {
                note_id,
                operation,
                plan_id: record.plan_id,
                revision: record.revision,
                item_id: record.item_id,
                approval_id: record.approval_id,
                desired: intent.desired,
            }),
            state => Err(format!(
                "RETAG_NOT_APPLIED: the latest operation {operation} is {}",
                serde_json::to_value(state)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            )),
        };
        latest.insert(note_id, entry);
    }
    Ok(latest)
}

/// What a retag would do to one note.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    Change {
        fields: Vec<String>,
        tags_added: Vec<String>,
        tags_removed: Vec<String>,
    },
    Current,
    Skip {
        code: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Proposal {
    pub note_id: i64,
    pub operation: Option<Uuid>,
    #[serde(flatten)]
    pub verdict: Verdict,
    #[serde(skip)]
    write: Option<Write>,
}

#[derive(Clone, Debug)]
struct Write {
    applied: Applied,
    note: ObservedNote,
    model: linguist_core::model::ManagedModel,
    fields: BTreeMap<String, String>,
    tags: Vec<String>,
}

impl Proposal {
    /// Each changed field: its name, the note's value and the rendered value.
    pub fn field_changes(&self) -> Vec<(&str, &str, &str)> {
        let Some(write) = &self.write else {
            return vec![];
        };
        write
            .fields
            .iter()
            .filter_map(|(name, after)| {
                let before = write
                    .note
                    .fields
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("");
                (before != after).then_some((name.as_str(), before, after.as_str()))
            })
            .collect()
    }

    fn skip(note_id: i64, operation: Option<Uuid>, code: impl Into<String>) -> Self {
        Self {
            note_id,
            operation,
            verdict: Verdict::Skip { code: code.into() },
            write: None,
        }
    }
}

/// Plan revisions read once per run: many notes share one.
#[derive(Default)]
pub struct Revisions(HashMap<(Uuid, u32), PlanRevision>);

impl Revisions {
    fn document(
        &mut self,
        store: &Store,
        plan: Uuid,
        revision: u32,
        item: Uuid,
    ) -> Result<LearningDocument> {
        let revision = match self.0.entry((plan, revision)) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(store.revision(plan, revision)?)
            }
        };
        revision
            .documents
            .iter()
            .find(|d| d.id == item)
            .cloned()
            .ok_or_else(|| "RETAG_DOCUMENT_MISSING".to_owned())
    }

    /// The newest later revision of the applied item that `plans classify`
    /// made: approved for this item, and different from the applied document
    /// only in its profile. A retag renders it under that approval.
    fn approved_profile(
        &mut self,
        store: &Store,
        applied: &Applied,
        base: &LearningDocument,
    ) -> Result<Option<(Applied, LearningDocument)>> {
        let latest = store.latest_revision(applied.plan_id)?;
        for revision in (applied.revision + 1..=latest).rev() {
            let Some((approval_id, _)) = store
                .approval_records_for(applied.plan_id, revision)?
                .into_iter()
                .rfind(|(_, a)| a.item_ids.contains(&applied.item_id))
            else {
                continue;
            };
            let document = self.document(store, applied.plan_id, revision, applied.item_id)?;
            if crate::classify::profile_only(base, &document) && document != *base {
                let mut newer = applied.clone();
                newer.revision = revision;
                newer.approval_id = approval_id;
                return Ok(Some((newer, document)));
            }
        }
        Ok(None)
    }
}

/// The tags apply would write today: the user's own tags (outside `lab::`)
/// from the document and the note, and every tag the renderer derives.
pub fn rendered_tags(document: &LearningDocument, note: &ObservedNote) -> Vec<String> {
    let user = |tag: &&String| !tag.starts_with("lab::");
    let mut tags: Vec<String> = document.tags.iter().filter(user).cloned().collect();
    tags.extend(render::tags(document));
    tags.extend(note.tags.iter().filter(user).cloned());
    sorted_tags(&tags)
}

/// Compare one note with its latest operation and with the current render.
pub fn propose(
    store: &Store,
    port: &mut dyn ApplyPort,
    revisions: &mut Revisions,
    note_id: i64,
    latest: Option<&std::result::Result<Applied, String>>,
) -> Result<Proposal> {
    let applied = match latest {
        None => {
            return Ok(Proposal::skip(
                note_id,
                None,
                "RETAG_NOT_APPLIED: no apply operation wrote this note",
            ));
        }
        // A restored apply that created the note deleted it again; say so
        // rather than reporting an operation state.
        Some(Err(code)) => {
            if port.note(note_id)?.is_none() {
                return Ok(Proposal::skip(
                    note_id,
                    None,
                    "RETAG_NOTE_MISSING: the note is no longer in the collection",
                ));
            }
            return Ok(Proposal::skip(note_id, None, code.clone()));
        }
        Some(Ok(applied)) => applied,
    };
    let operation = Some(applied.operation);
    let Some(note) = port.note(note_id)? else {
        return Ok(Proposal::skip(
            note_id,
            operation,
            "RETAG_NOTE_MISSING: the note is no longer in the collection",
        ));
    };
    if note.fields != applied.desired.fields || sorted_tags(&note.tags) != applied.desired.tags {
        return Ok(Proposal::skip(
            note_id,
            operation,
            "RETAG_NOTE_EDITED: the note changed since its latest apply; it is left as it is",
        ));
    }
    let document = revisions.document(store, applied.plan_id, applied.revision, applied.item_id)?;
    let (applied, document) = match revisions.approved_profile(store, applied, &document)? {
        Some((newer, document)) => (newer, document),
        None => (applied.clone(), document),
    };
    let empty = BTreeMap::new();
    let source = document
        .sources
        .first()
        .map(|s| &s.fields)
        .unwrap_or(&empty);
    let rendered = match render::render(&document, source) {
        Ok(rendered) => rendered,
        Err(e) => {
            return Ok(Proposal::skip(
                note_id,
                operation,
                format!("RETAG_RENDER_BLOCKED: {e}"),
            ));
        }
    };
    if rendered.model.name != note.model_name || rendered.fields.keys().ne(note.fields.keys()) {
        return Ok(Proposal::skip(
            note_id,
            operation,
            "RETAG_MODEL_CHANGED: the current render targets another note type or field set",
        ));
    }
    let tags = rendered_tags(&document, &note);
    let fields: Vec<String> = rendered
        .fields
        .iter()
        .filter(|(name, value)| note.fields.get(*name) != Some(*value))
        .map(|(name, _)| name.clone())
        .collect();
    let before: BTreeSet<&String> = note.tags.iter().collect();
    let after: BTreeSet<&String> = tags.iter().collect();
    let tags_added: Vec<String> = after.difference(&before).map(|t| (*t).clone()).collect();
    let tags_removed: Vec<String> = before.difference(&after).map(|t| (*t).clone()).collect();
    if fields.is_empty() && tags_added.is_empty() && tags_removed.is_empty() {
        return Ok(Proposal {
            note_id,
            operation,
            verdict: Verdict::Current,
            write: None,
        });
    }
    Ok(Proposal {
        note_id,
        operation,
        verdict: Verdict::Change {
            fields,
            tags_added,
            tags_removed,
        },
        write: Some(Write {
            applied,
            note,
            model: rendered.model,
            fields: rendered.fields,
            tags,
        }),
    })
}

pub struct RetagRequest<'a> {
    pub checkpoint_id: Uuid,
    pub group_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RetagOutcome {
    pub note_id: i64,
    pub operation: Uuid,
    pub state: OperationState,
    pub receipt_digest: Option<String>,
    pub issues: Vec<Issue>,
}

fn retag_issue(code: &str, message: impl Into<String>) -> Issue {
    let mut issue = Issue::new(code, Severity::Error, None, message);
    issue.stage = "retag".into();
    issue
}

/// Write one proposed change. The note is read again first: any change since
/// the proposal refuses before the journal.
pub fn retag(
    store: &mut Store,
    lease: &LeaseToken,
    port: &mut dyn ApplyPort,
    proposal: &Proposal,
    request: &RetagRequest,
) -> Result<RetagOutcome> {
    store.validate_lease(lease)?;
    let write = proposal
        .write
        .as_ref()
        .ok_or("RETAG_NOTHING_TO_WRITE: the proposal is not a change")?;
    let note = port
        .note(proposal.note_id)?
        .ok_or("RETAG_NOTE_MISSING: the note is no longer in the collection")?;
    let precondition = content_digest(&note)?;
    if precondition != content_digest(&write.note)? {
        return Err("RETAG_NOTE_CHANGED: the note changed since the proposal".into());
    }
    if note.cards.iter().any(|card| card.original_deck_id != 0) {
        return Err("APPLY_FILTERED_DECK_BLOCKS: a card of the note is in a filtered deck".into());
    }
    if !port
        .mutation_variants()?
        .iter()
        .any(|v| v == "restore_note")
    {
        return Err("CAPABILITY_UNAVAILABLE: native restore_note variant is not declared".into());
    }
    let binding = port.execution_binding()?;
    let card_ids: Vec<i64> = note.cards.iter().map(|c| c.id).collect();
    let kept: BTreeSet<i64> = card_ids.iter().copied().collect();
    let authorization = require_checkpoint(
        store,
        request.checkpoint_id,
        &DependentScope {
            binding: &binding,
            requirement: CoverageRequirement {
                scheduling: true,
                media: true,
                schema: false,
            },
            note_ids: &[note.id],
            card_ids: &card_ids,
            model_ids: &[],
            media_names: &[],
            group_id: request.group_id,
            protected_manifest_digest: request.protected_manifest_digest,
            now_ms: request.now_ms,
            reuse_max_age_seconds: request.reuse_max_age_seconds,
            max_package_bytes: request.max_package_bytes,
        },
    )?;
    let effect = Effect::RestoreNote(RestoreNote {
        note_id: note.id,
        expected_pre_digest: precondition.clone(),
        migration: None,
        fields: write.fields.clone(),
        tags: write.tags.clone(),
        card_decks: note
            .cards
            .iter()
            .map(|card| CardDeck {
                card_id: card.id,
                deck_id: card.deck_id,
            })
            .collect(),
        removed_card_ids: vec![],
    });
    // The read-back a restore step compares: every card kept with its current
    // scheduling, the observed manifest verbatim.
    let pre_state = project(&note, &kept, None);
    let mut desired = pre_state.clone();
    desired.fields = write.fields.clone();
    desired.tags = write.tags.clone();
    let step = IntentStep {
        step_id: Uuid::new_v4(),
        payload_digest: effect.payload_digest()?,
        precondition_digest: precondition,
        expected: serde_json::to_value(&desired).map_err(|e| e.to_string())?,
        expected_digest: digest_of(&desired)?,
        effect,
    };
    let intent = ApplyIntent {
        schema_version: 1,
        action: ItemAction::Update,
        note_id: Some(note.id),
        marker_tag: None,
        target_model_name: note.model_name.clone(),
        target_model_id: note.model_id,
        target_manifest_digest: note.model_manifest_digest.clone(),
        deck_id: note.cards.first().map(|c| c.deck_id).unwrap_or_default(),
        retained_card_ids: card_ids.clone(),
        pre_state: Some(pre_state),
        desired,
        steps: vec![step],
        checkpoint: serde_json::to_value(&authorization).map_err(|e| e.to_string())?,
        reused_media: vec![],
    };
    // The rewrite carries the approval of the document it renders, as it was
    // recorded: apply already acted on it, and today's validation may add
    // warnings it never accepted.
    let plan_digest = store.approval_record(write.applied.approval_id)?.digest;
    let operation = Uuid::new_v4();
    store.publish_apply_operation(&ApplyOperationRecord {
        operation_id: operation,
        plan_id: write.applied.plan_id,
        revision: write.applied.revision,
        item_id: write.applied.item_id,
        approval_id: write.applied.approval_id,
        created_ms: request.now_ms,
        intent: serde_json::to_value(&intent).map_err(|e| e.to_string())?,
    })?;
    let snapshot = apply::fresh_snapshot(
        store,
        port,
        operation,
        Some(&note),
        &write.model,
        None,
        request.now_ms,
    )?;
    // The companion takes an exact `restore_note` only under a restore
    // decision. A retag's decision binds the plan approval of the document it
    // renders to this note, its observed state and this exact payload; the
    // receipt names the plan approval.
    let approval_digest = linguist_core::canonical::digest(
        "lab-restore-decision-v1",
        &serde_json::json!({
            "schema_version": 1,
            "kind": "retag",
            "plan_approval": plan_digest,
            "plan_id": write.applied.plan_id,
            "revision": write.applied.revision,
            "item_id": write.applied.item_id,
            "note_id": note.id,
            "steps": intent
                .steps
                .iter()
                .map(|s| (&s.precondition_digest, &s.payload_digest))
                .collect::<Vec<_>>(),
        }),
    )
    .map_err(|e| e.to_string())?;
    let journal = OperationJournal {
        id: operation,
        group_id: request.group_id,
        approval_digest: approval_digest.clone(),
        binding: binding.clone(),
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
        recovery: apply::reconcile_command_for(operation),
    };
    journal.advance(|j| j.state = OperationState::Preflight)?;
    let owner = match port.begin(&binding, &approval_digest) {
        Ok(owner) => owner,
        Err(code) => {
            journal.advance(|j| {
                j.state = OperationState::FailedBeforeWrite;
                j.issues
                    .push(retag_issue("APPLY_OWNER_UNAVAILABLE", code.clone()));
            })?;
            return Err(format!("APPLY_OWNER_UNAVAILABLE: {code}"));
        }
    };
    journal.advance(|j| j.state = OperationState::Checkpointed)?;
    let context = Context::for_apply(&intent, binding, approval_digest, owner.clone(), operation);
    let result = apply::drive(&mut journal, port, &context);
    let receipt_digest = match result {
        Ok(Drive::Committed) => match apply::commit(&mut journal, &context, &intent, port) {
            Ok((_, digest)) => Some(digest),
            Err(code) => {
                let _ = port.end(&owner);
                return Err(code);
            }
        },
        Ok(Drive::Stopped) => None,
        Err(code) => {
            let _ = port.end(&owner);
            return Err(code);
        }
    };
    let _ = port.end(&owner);
    let j = &journal.version.journal;
    Ok(RetagOutcome {
        note_id: note.id,
        operation,
        state: j.state,
        receipt_digest,
        issues: j.issues.clone(),
    })
}
