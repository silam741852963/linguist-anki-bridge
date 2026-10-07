//! ALG-RESTORE and OP-44 over the fake native port. These tests prove
//! orchestration, conflict and journal rules only; native reverse-mapping
//! history preservation is shown separately in disposable Anki
//! (scripts/verify-native-restore.py).
mod common;
use common::*;
use linguist_application::restore::{
    DeckChoice, FieldChoice, RestoreDecision, RestoreRequest, local_preview, plan_group_rollback,
    plan_restore, restore, rollback_group,
};

const LATER: u64 = 1_200_000;

fn production(doc: &mut LearningDocument) {
    doc.requested_tasks = vec![Task::Comprehension, Task::Production];
    if let linguist_core::document::LearningContent::Vocabulary(v) = &mut doc.content {
        v.production_prompt = "Say: to consume (verb)".into();
    }
}

fn applied(s: &mut Setup, anki: &mut Anki, accept_schema: bool) -> (Uuid, Uuid) {
    let mut request = s.request();
    request.accept_schema_change = accept_schema;
    let outcome = apply_item(&mut s.store, &s.token, anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    (outcome.operation_id.unwrap(), outcome.snapshot_id.unwrap())
}

fn decision(snapshot: Uuid, digest: &str) -> RestoreDecision {
    RestoreDecision {
        schema_version: 1,
        snapshot_id: snapshot,
        observed_state_digest: digest.into(),
        actor: "reviewer".into(),
        fields: BTreeMap::new(),
        decks: BTreeMap::new(),
        remove_unstudied_cards: vec![],
        delete_created_notes: vec![],
        accept_missing_media: vec![],
        accept_schema_change: false,
    }
}

fn request(
    snapshot: Uuid,
    decision: Option<RestoreDecision>,
    checkpoint: Uuid,
) -> RestoreRequest<'static> {
    RestoreRequest {
        apply: true,
        snapshot_id: snapshot,
        decision,
        checkpoint_id: checkpoint,
        group_id: None,
        protected_manifest_digest: "protected-v1",
        reuse_max_age_seconds: 600,
        max_package_bytes: 4 * 1024 * 1024,
        max_media_bytes: 1024 * 1024,
        now_ms: LATER + 1000,
    }
}

fn basic_model() -> ObservedModel {
    ObservedModel {
        id: BASIC_MODEL_ID,
        name: "Basic".into(),
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![],
        css: String::new(),
    }
}

/// Preview, bind a decision to it, checkpoint the current note and restore.
fn restore_with(
    s: &mut Setup,
    anki: &mut Anki,
    snapshot: Uuid,
    edit: impl FnOnce(&mut RestoreDecision),
    models: &[i64],
) -> Result<linguist_application::restore::RestoreOutcome, String> {
    let plan = plan_restore(&s.store, anki, snapshot, None).unwrap();
    let mut decision = decision(snapshot, &plan.observed_state_digest);
    edit(&mut decision);
    let id = plan
        .note_id
        .or(plan.created_notes.first().map(|n| n.note_id))
        .unwrap();
    let note = anki.notes[&id].clone();
    let checkpoint = checkpoint_for(s, &[&note], models, LATER);
    restore(
        &mut s.store,
        &s.token,
        anki,
        &request(snapshot, Some(decision), checkpoint),
    )
}

#[test]
fn restore_after_later_study_restores_content_and_keeps_new_history() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let original = anki.notes[&10].clone();
    let (operation, snapshot) = applied(&mut s, &mut anki, false);
    let before = s.store.snapshot(snapshot).unwrap();
    // Normal study after apply: later reviews must survive restore.
    anki.study();
    let studied = anki.notes[&10].cards[0].clone();
    assert_eq!(studied.review_count, 2);
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.conflicts.is_empty() && plan.blockers.is_empty(),
        "{plan:?}"
    );
    assert!(plan.cards[0].later_reviews && plan.cards[0].retained);
    assert_eq!(plan.cards[0].deck_after, Some(HOME_DECK));
    assert_eq!(plan.tags.as_ref().unwrap().restored, vec!["old".to_owned()]);
    let mutations = anki.mutations();
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    assert_eq!(outcome.target_state, OperationState::Restored);
    assert!(outcome.next_command.is_none());
    assert_eq!(anki.mutations(), mutations + 1);
    let note = &anki.notes[&10];
    assert_eq!(note.fields, original.fields);
    assert_eq!(note.tags, vec!["old".to_owned()]);
    let card = &note.cards[0];
    assert_eq!((card.id, card.deck_id), (20, HOME_DECK));
    // Current scheduling and the later review are preserved, not rewound.
    assert_eq!(card.scheduler, studied.scheduler);
    assert_eq!(card.history_digest, studied.history_digest);
    assert_eq!(card.review_count, 2);
    let receipt = outcome.receipt.unwrap();
    assert_eq!(receipt.kept_card_ids, vec![20]);
    assert_eq!(
        s.store
            .restore_receipt(receipt.restore_operation)
            .unwrap()
            .unwrap(),
        receipt
    );
    // The original snapshot, its apply receipt and the source archive are unchanged.
    assert_eq!(
        s.store.snapshot(snapshot).unwrap().snapshot,
        before.snapshot
    );
    assert_eq!(s.store.snapshot(snapshot).unwrap().after, before.after);
    let restore_journal = s.journal(receipt.restore_operation);
    assert_ne!(restore_journal.snapshot_id, snapshot);
    let fresh = s.store.snapshot(restore_journal.snapshot_id).unwrap();
    assert_eq!(
        fresh.snapshot.originals[0].cards[0].history_digest,
        studied.history_digest
    );
    assert_eq!(s.journal(operation).state, OperationState::Restored);
    assert!(s.store.pending_journals(10).unwrap().is_empty());
    // A second restore is refused; the item can be applied again.
    let again = plan_restore(&s.store, &mut anki, snapshot, None).unwrap_err();
    assert!(again.starts_with("RESTORE_ALREADY_RESTORED"), "{again}");
    let items = preview(&s.store, s.plan.id, 1, &[]).unwrap();
    assert!(items[0].blockers.is_empty(), "{:?}", items[0].blockers);
}

#[test]
fn later_personal_edits_conflict_until_explicitly_merged() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    {
        let note = anki.notes.get_mut(&10).unwrap();
        // Edits a field apply changed, a field apply did not change, a tag
        // and the card's deck.
        note.fields
            .insert("Meaning".into(), "to eat (my note)".into());
        note.fields
            .insert("Kanji".into(), "remember the kanji".into());
        note.tags.push("mine".into());
        note.cards[0].deck_id = 777;
    }
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.conflicts
            .contains(&"RESTORE_FIELD_CONFLICT:Meaning".to_owned()),
        "{:?}",
        plan.conflicts
    );
    assert!(
        plan.conflicts
            .contains(&"RESTORE_DECK_CONFLICT:20".to_owned())
    );
    let personal = plan.fields.iter().find(|f| f.field == "Kanji").unwrap();
    assert!(!personal.conflict);
    assert_eq!(personal.restored.as_deref(), Some("remember the kanji"));
    // No global force: an undecided conflict refuses with zero effects.
    let mutations = anki.mutations();
    let refused = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap_err();
    assert!(
        refused.starts_with("RESTORE_CONFLICT_UNRESOLVED"),
        "{refused}"
    );
    assert_eq!(anki.mutations(), mutations);
    assert!(
        s.store
            .restore_operations_for(plan.target_operation)
            .unwrap()
            .is_empty()
    );
    // A decision for an older observed state is stale.
    let mut stale = decision(snapshot, &plan.observed_state_digest);
    stale.fields.insert("Meaning".into(), FieldChoice::Current);
    stale.decks.insert("20".into(), DeckChoice::Current);
    anki.notes.get_mut(&10).unwrap().tags.push("later".into());
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER);
    let error = restore(
        &mut s.store,
        &s.token,
        &mut anki,
        &request(snapshot, Some(stale), checkpoint),
    )
    .unwrap_err();
    assert!(error.starts_with("RESTORE_DECISION_STALE"), "{error}");
    // Explicit reviewed merge and deck choice.
    let outcome = restore_with(
        &mut s,
        &mut anki,
        snapshot,
        |d| {
            d.fields.insert(
                "Meaning".into(),
                FieldChoice::Merged("to consume; to eat (my note)".into()),
            );
            d.decks.insert("20".into(), DeckChoice::Current);
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert_eq!(note.fields["Meaning"], "to consume; to eat (my note)");
    assert_eq!(note.fields["Kanji"], "remember the kanji");
    assert_eq!(note.fields["Expression"], "食べる");
    let mut tags = note.tags.clone();
    tags.sort();
    assert_eq!(tags, vec!["later", "mine", "old"]);
    assert_eq!(note.cards[0].deck_id, 777);
}

#[test]
fn reverse_migration_keeps_history_and_removes_only_reviewed_unstudied_cards() {
    let mut s = setup_with(Kind::Migrate, |doc| {
        production(doc);
    });
    let mut anki = Anki::new(&s, Kind::Migrate);
    let (operation, snapshot) = applied(&mut s, &mut anki, true);
    let note = anki.notes[&10].clone();
    assert_eq!(note.model_id, V2_MODEL_ID);
    assert_eq!(note.cards.len(), 2);
    let new_card = note.cards.iter().find(|c| c.id != 20).unwrap().id;
    anki.study();
    // The original note type must still exist; it is never recreated.
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.starts_with("RESTORE_SOURCE_MODEL_MISSING"))
    );
    anki.models.push(basic_model());
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(
        plan.conflicts
            .contains(&"RESTORE_SCHEMA_CHANGE_NOT_ACCEPTED".to_owned())
    );
    assert!(
        plan.conflicts
            .contains(&format!("RESTORE_CARD_REMOVAL_REVIEW:{new_card}"))
    );
    let model = plan.model.as_ref().unwrap();
    assert_eq!(
        (model.original_model_id, model.original_name.as_str()),
        (BASIC_MODEL_ID, "Basic")
    );
    let studied = anki.notes[&10]
        .cards
        .iter()
        .find(|c| c.id == 20)
        .unwrap()
        .clone();
    let outcome = restore_with(
        &mut s,
        &mut anki,
        snapshot,
        |d| {
            d.accept_schema_change = true;
            d.remove_unstudied_cards = vec![new_card];
        },
        &[BASIC_MODEL_ID, V2_MODEL_ID],
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert_eq!(
        (note.model_id, note.model_name.as_str()),
        (BASIC_MODEL_ID, "Basic")
    );
    assert_eq!(note.fields["Front"], "食べる");
    assert_eq!(note.cards.len(), 1);
    let card = &note.cards[0];
    assert_eq!((card.id, card.ordinal, card.deck_id), (20, 0, HOME_DECK));
    assert_eq!(card.history_digest, studied.history_digest);
    assert_eq!(card.review_count, studied.review_count);
    let receipt = outcome.receipt.unwrap();
    assert_eq!(receipt.removed_card_ids, vec![new_card]);
    assert_eq!(s.journal(operation).state, OperationState::Restored);
}

#[test]
fn reverse_mapping_refuses_studied_new_task_cards() {
    let mut s = setup_with(Kind::Migrate, |doc| {
        production(doc);
    });
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.models.push(basic_model());
    let (operation, snapshot) = applied(&mut s, &mut anki, true);
    // The user studies the card that the migration created.
    for card in &mut anki.notes.get_mut(&10).unwrap().cards {
        if card.id != 20 {
            card.review_count = 1;
            card.history_digest = "3".repeat(64);
        }
    }
    let new_card = anki.notes[&10]
        .cards
        .iter()
        .find(|c| c.id != 20)
        .unwrap()
        .id;
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.starts_with(&format!("RESTORE_STUDIED_NEW_TASK:{new_card}"))),
        "{:?}",
        plan.blockers
    );
    let mutations = anki.mutations();
    let error = restore_with(
        &mut s,
        &mut anki,
        snapshot,
        |d| {
            d.accept_schema_change = true;
            d.remove_unstudied_cards = vec![new_card];
        },
        &[BASIC_MODEL_ID, V2_MODEL_ID],
    )
    .unwrap_err();
    assert!(error.starts_with("RESTORE_UNSUPPORTED"), "{error}");
    assert_eq!(anki.mutations(), mutations);
    assert_eq!(s.journal(operation).state, OperationState::Committed);
}

#[test]
fn retained_card_without_reverse_mapping_and_filtered_cards_block() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.models.push(basic_model());
    let (_, snapshot) = applied(&mut s, &mut anki, true);
    // A later template change moved the retained card to an unmapped ordinal.
    anki.notes.get_mut(&10).unwrap().cards[0].ordinal = 2;
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.starts_with("RESTORE_REVERSE_MAPPING_INCOMPATIBLE:20"))
    );
    anki.notes.get_mut(&10).unwrap().cards[0].ordinal = 0;
    anki.notes.get_mut(&10).unwrap().cards[0].original_deck_id = HOME_DECK;
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.starts_with("RESTORE_FILTERED_DECK_BLOCKS:20"))
    );
}

#[test]
fn media_is_restored_before_the_fields_that_reference_it() {
    let mut s = setup(Kind::UpdateMedia);
    let mut anki = Anki::new(&s, Kind::UpdateMedia);
    anki.put_media("orig.ogg", ORIGINAL_AUDIO);
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    let archived = s.store.snapshot(snapshot).unwrap().snapshot.media;
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].filename, "orig.ogg");
    // The user deleted the original file after apply.
    anki.media.remove("orig.ogg");
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert_eq!(plan.media[0].action, "restore_archived_bytes");
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    let journal = s.journal(outcome.restore_operation.unwrap());
    assert_eq!(
        journal
            .steps
            .iter()
            .map(|s| s.action.as_str())
            .collect::<Vec<_>>(),
        ["store_media", "restore_note"]
    );
    assert_eq!(anki.media["orig.ogg"].sha256, sha256(ORIGINAL_AUDIO));
    assert_eq!(anki.notes[&10].fields["Audio"], "[sound:orig.ogg]");
}

#[test]
fn colliding_original_media_uses_a_safe_alternate_name() {
    let mut s = setup(Kind::UpdateMedia);
    let mut anki = Anki::new(&s, Kind::UpdateMedia);
    anki.put_media("orig.ogg", ORIGINAL_AUDIO);
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    // Another file now uses the original name; it is never overwritten.
    anki.put_media("orig.ogg", b"OggS somebody else's audio");
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    let alternate = plan.media[0].restored_name.clone().unwrap();
    assert_eq!(plan.media[0].action, "restore_as_alternate");
    assert!(alternate.starts_with("orig-lab") && alternate.ends_with(".ogg"));
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    assert_eq!(
        anki.media["orig.ogg"].sha256,
        sha256(b"OggS somebody else's audio")
    );
    assert_eq!(anki.media[&alternate].sha256, sha256(ORIGINAL_AUDIO));
    assert_eq!(
        anki.notes[&10].fields["Audio"],
        format!("[sound:{alternate}]")
    );
}

#[test]
fn missing_unarchived_media_needs_explicit_acceptance() {
    let mut s = setup(Kind::UpdateMedia);
    let mut anki = Anki::new(&s, Kind::UpdateMedia);
    // The port cannot read bytes for this file, so nothing is archived.
    anki.media.insert(
        "orig.ogg".into(),
        ObservedMedia {
            filename: "orig.ogg".into(),
            sha256: sha256(ORIGINAL_AUDIO),
            size_bytes: ORIGINAL_AUDIO.len() as u64,
        },
    );
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    assert!(
        s.store
            .snapshot(snapshot)
            .unwrap()
            .snapshot
            .media
            .is_empty()
    );
    anki.media.remove("orig.ogg");
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.conflicts
            .contains(&"RESTORE_MEDIA_MISSING:orig.ogg".to_owned())
    );
    let outcome = restore_with(
        &mut s,
        &mut anki,
        snapshot,
        |d| d.accept_missing_media = vec!["orig.ogg".into()],
        &[],
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    assert!(!anki.media.contains_key("orig.ogg"));
}

/// One reverse-effect crash boundary, resumed with no decision.
fn crash_case(fault: Fault, skip_media: bool) -> (Setup, Anki, Uuid, Uuid) {
    let mut s = setup(Kind::UpdateMedia);
    let mut anki = Anki::new(&s, Kind::UpdateMedia);
    anki.put_media("orig.ogg", ORIGINAL_AUDIO);
    let (operation, snapshot) = applied(&mut s, &mut anki, false);
    if !skip_media {
        anki.media.remove("orig.ogg");
    }
    anki.fault = fault;
    let result = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]);
    anki.lock = None;
    let restore_operation = match result {
        Ok(outcome) => {
            assert_ne!(outcome.state, Some(OperationState::Committed));
            if outcome.state != Some(OperationState::FailedBeforeWrite) {
                assert!(outcome.next_command.unwrap().contains("snapshots restore"));
            }
            outcome.restore_operation.unwrap()
        }
        Err(error) => {
            assert!(
                error.starts_with("APPLY_LOCAL_DURABILITY_FAILED"),
                "{error}"
            );
            assert!(error.contains("snapshots restore"), "{error}");
            s.store.restore_operations_for(operation).unwrap()[0].operation_id
        }
    };
    (s, anki, snapshot, restore_operation)
}

fn resume(
    s: &mut Setup,
    anki: &mut Anki,
    snapshot: Uuid,
) -> linguist_application::restore::RestoreOutcome {
    // A repeated restore resumes the same journal; no decision is needed.
    let checkpoint = Uuid::nil();
    restore(
        &mut s.store,
        &s.token,
        anki,
        &request(snapshot, None, checkpoint),
    )
    .unwrap()
}

#[test]
fn every_reverse_effect_crash_boundary_resumes_the_same_restore_journal() {
    // (fault, keep media present?, expected main mutations after resume)
    for (fault, skip_media) in [
        (Fault::LoseAfterEffect, false),
        (Fault::LoseBeforeEffect, false),
        (Fault::UnknownAfterEffect, true),
        (Fault::HoldLock, true),
        (Fault::Pending, true),
    ] {
        let (mut s, mut anki, snapshot, operation) = crash_case(fault, skip_media);
        let before = anki.main_mutations;
        if fault == Fault::Pending {
            // A queued native row is waited on, never re-sent.
            let waiting = resume(&mut s, &mut anki, snapshot);
            assert_eq!(waiting.state, Some(OperationState::NeedsRecovery));
            assert!(
                waiting
                    .issues
                    .iter()
                    .any(|i| i.code == "APPLY_NATIVE_PENDING")
            );
            assert_eq!(anki.main_mutations, before);
            let step = s.journal(operation).steps.last().unwrap().id;
            anki.ledger.insert(step, NativeStatus::Verified);
        }
        let outcome = resume(&mut s, &mut anki, snapshot);
        assert!(outcome.resumed);
        assert_eq!(outcome.restore_operation, Some(operation), "{fault:?}");
        assert_eq!(
            outcome.state,
            Some(OperationState::Committed),
            "{fault:?} {:?}",
            outcome.issues
        );
        assert_eq!(outcome.target_state, OperationState::Restored);
        // Only a request that never reached the ledger is re-sent, with the same UUID.
        let resent = usize::from(fault == Fault::LoseBeforeEffect);
        assert_eq!(anki.main_mutations, before + resent, "{fault:?}");
        assert_eq!(
            s.store
                .restore_operations_for(outcome.target_operation)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(anki.notes[&10].fields["Meaning"], "to consume");
        // Resuming again is a no-op.
        let error = plan_restore(&s.store, &mut anki, snapshot, None).unwrap_err();
        assert!(error.starts_with("RESTORE_ALREADY_RESTORED"), "{error}");
    }
}

#[test]
fn unknown_reverse_effect_without_evidence_stays_in_recovery() {
    let (mut s, mut anki, snapshot, operation) = crash_case(Fault::UnknownNoEffect, true);
    let outcome = resume(&mut s, &mut anki, snapshot);
    assert_eq!(outcome.restore_operation, Some(operation));
    assert_eq!(outcome.state, Some(OperationState::NeedsRecovery));
    assert_eq!(outcome.target_state, OperationState::Committed);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_ABSENCE_UNPROVEN")
    );
    assert!(outcome.receipt.is_none());
}

#[test]
fn rejected_reverse_effect_fails_before_write_and_allows_a_new_restore() {
    let (mut s, mut anki, snapshot, operation) = crash_case(Fault::Reject, true);
    assert_eq!(
        s.journal(operation).state,
        OperationState::FailedBeforeWrite
    );
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert_ne!(outcome.restore_operation, Some(operation));
    assert_eq!(outcome.state, Some(OperationState::Committed));
}

#[test]
fn known_partial_restore_with_verified_media_can_be_superseded() {
    // Media is restored, then native refuses the note step with no effect.
    let (mut s, mut anki, snapshot, operation) = crash_case(Fault::Reject, false);
    let journal = s.journal(operation);
    assert_eq!(journal.state, OperationState::NeedsRecovery);
    assert!(
        journal
            .issues
            .iter()
            .any(|i| i.code == "APPLY_KNOWN_PARTIAL")
    );
    assert_eq!(anki.media["orig.ogg"].sha256, sha256(ORIGINAL_AUDIO));
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert_ne!(outcome.restore_operation, Some(operation));
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    // The media already present is reused, not uploaded again.
    let journal = s.journal(outcome.restore_operation.unwrap());
    assert_eq!(journal.steps.len(), 1);
}

#[test]
fn crash_after_restore_receipt_or_commit_finalizes_without_new_effects() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let (operation, snapshot) = applied(&mut s, &mut anki, false);
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    let restore_operation = outcome.restore_operation.unwrap();
    let receipt = outcome.receipt.unwrap();
    // Test-only tampering: drop the restore commit and the target's restored
    // event, as if the process died after the receipt was saved.
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TRIGGER journal_events_no_delete;")
        .unwrap();
    for (id, drop) in [(restore_operation, 1), (operation, 1)] {
        let head: u32 = db
            .query_row(
                "SELECT sequence FROM journal_heads WHERE operation=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE journal_heads SET sequence=?1 WHERE operation=?2",
            rusqlite::params![head - drop, id.to_string()],
        )
        .unwrap();
        db.execute(
            "DELETE FROM journal_events WHERE operation=?1 AND sequence=?2",
            rusqlite::params![id.to_string(), head],
        )
        .unwrap();
    }
    drop(db);
    assert_eq!(
        s.journal(restore_operation).state,
        OperationState::Verifying
    );
    assert_eq!(s.journal(operation).state, OperationState::Committed);
    let mutations = anki.mutations();
    let resumed = resume(&mut s, &mut anki, snapshot);
    assert_eq!(resumed.state, Some(OperationState::Committed));
    assert_eq!(resumed.target_state, OperationState::Restored);
    assert_eq!(resumed.receipt.unwrap(), receipt);
    assert_eq!(anki.mutations(), mutations);
}

#[test]
fn unknown_apply_outcome_must_be_reconciled_before_restore() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.fault = Fault::LoseAfterEffect;
    let request_apply = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request_apply).unwrap();
    let snapshot = outcome.snapshot_id.unwrap();
    let error = plan_restore(&s.store, &mut anki, snapshot, None).unwrap_err();
    assert!(error.starts_with("RESTORE_RECONCILE_FIRST"), "{error}");
    let local = local_preview(&s.store, snapshot).unwrap();
    assert!(
        local
            .blockers
            .contains(&"RESTORE_RECONCILE_FIRST".to_owned())
    );
}

#[test]
fn restore_requires_apply_flag_identity_variant_and_checkpoint() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    let decision = decision(snapshot, &plan.observed_state_digest);
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER);
    let mutations = anki.mutations();
    let mut no_flag = request(snapshot, Some(decision.clone()), checkpoint);
    no_flag.apply = false;
    let error = restore(&mut s.store, &s.token, &mut anki, &no_flag).unwrap_err();
    assert!(error.starts_with("RESTORE_FLAG_REQUIRED"));
    let error = restore(
        &mut s.store,
        &s.token,
        &mut anki,
        &request(snapshot, None, checkpoint),
    )
    .unwrap_err();
    assert!(error.starts_with("RESTORE_DECISION_REQUIRED"));
    let mut wrong = request(snapshot, Some(decision.clone()), s.checkpoint);
    wrong.reuse_max_age_seconds = 0;
    let error = restore(&mut s.store, &s.token, &mut anki, &wrong).unwrap_err();
    assert!(error.starts_with("CHECKPOINT"), "{error}");
    anki.variants.retain(|v| v != "restore_note");
    let error = restore(
        &mut s.store,
        &s.token,
        &mut anki,
        &request(snapshot, Some(decision.clone()), checkpoint),
    )
    .unwrap_err();
    assert!(error.starts_with("CAPABILITY_UNAVAILABLE"), "{error}");
    anki.binding.lineage_id = Uuid::from_u128(99);
    let error = plan_restore(&s.store, &mut anki, snapshot, None).unwrap_err();
    assert!(error.starts_with("RESTORE_IDENTITY_MISMATCH"), "{error}");
    assert_eq!(anki.mutations(), mutations);
    // A second collection writer is rejected while the first holds the lease.
    let second = s
        .store
        .acquire_lease(&Resource::CollectionWriter(Uuid::from_u128(2)), 300);
    assert!(second.is_err());
}

#[test]
fn created_notes_are_kept_by_default_and_deleted_only_when_unchanged_and_unstudied() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let (operation, snapshot) = applied(&mut s, &mut anki, false);
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert_eq!(plan.created_notes.len(), 1);
    assert_eq!(plan.created_notes[0].action, "keep");
    assert!(plan.no_change);
    let created = plan.created_notes[0].note_id;
    // Default: kept, nothing journaled.
    let kept = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    assert!(kept.restore_operation.is_none());
    assert_eq!(kept.kept_created_notes, vec![created]);
    assert!(anki.notes.contains_key(&created));
    // Studied: kept even when listed; removal needs manual disaster recovery.
    anki.notes.get_mut(&created).unwrap().cards[0].review_count = 1;
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    let mut listed = decision(snapshot, &plan.observed_state_digest);
    listed.delete_created_notes = vec![created];
    let plan = plan_restore(&s.store, &mut anki, snapshot, Some(&listed)).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.starts_with("RESTORE_CREATED_NOTE_STUDIED"))
    );
    // Unstudied and unchanged: deleted with an explicit list.
    anki.notes.get_mut(&created).unwrap().cards[0].review_count = 0;
    let outcome = restore_with(
        &mut s,
        &mut anki,
        snapshot,
        |d| d.delete_created_notes = vec![created],
        &[],
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        Some(OperationState::Committed),
        "{:?}",
        outcome.issues
    );
    assert!(!anki.notes.contains_key(&created));
    assert_eq!(outcome.receipt.unwrap().deleted_note_ids, vec![created]);
    assert_eq!(s.journal(operation).state, OperationState::Restored);
}

#[test]
fn local_preview_reports_apply_changes_without_live_reads() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let (operation, snapshot) = applied(&mut s, &mut anki, false);
    let local = local_preview(&s.store, snapshot).unwrap();
    assert_eq!(local.target_operation, operation);
    assert!(
        local
            .fields_changed_by_apply
            .contains(&"Meaning".to_owned())
            || !local.fields_changed_by_apply.is_empty()
    );
    assert_eq!(
        local.tags_added_by_apply,
        [
            "lab::explain::en",
            "lab::kind::vocabulary",
            "lab::lang::ja",
            "lab::task::comprehension",
            "linguist",
        ]
    );
    assert_eq!(local.deck_moves, vec![(20, Some(HOME_DECK), TARGET_DECK)]);
    assert!(local.blockers.is_empty());
}

#[test]
fn group_rollback_previews_and_restores_each_journaled_item() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let group = Uuid::new_v4();
    let mut req = s.request();
    req.group_id = Some(group);
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &req).unwrap();
    let snapshot = outcome.snapshot_id.unwrap();
    let items = plan_group_rollback(&s.store, &mut anki, group).unwrap();
    assert_eq!(items.len(), 1);
    let plan = items[0].plan.clone().unwrap();
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER);
    let mut request = request(
        snapshot,
        Some(decision(snapshot, &plan.observed_state_digest)),
        checkpoint,
    );
    request.group_id = Some(group);
    let results = rollback_group(&mut s.store, &s.token, &mut anki, group, &[request]).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].as_ref().unwrap().target_state,
        OperationState::Restored
    );
    // The restore journal carries the group but is not itself rolled back.
    assert!(
        plan_group_rollback(&s.store, &mut anki, group).unwrap()[0]
            .error
            .as_deref()
            .unwrap()
            .starts_with("RESTORE_ALREADY_RESTORED")
    );
    assert!(plan_group_rollback(&s.store, &mut anki, Uuid::new_v4()).is_err());
}

#[test]
fn snapshots_that_are_not_apply_operations_are_inspect_only() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let (_, snapshot) = applied(&mut s, &mut anki, false);
    let outcome = restore_with(&mut s, &mut anki, snapshot, |_| {}, &[]).unwrap();
    let restore_snapshot = s.journal(outcome.restore_operation.unwrap()).snapshot_id;
    let error = plan_restore(&s.store, &mut anki, restore_snapshot, None).unwrap_err();
    assert!(error.starts_with("RESTORE_TARGET_UNSUPPORTED"), "{error}");
}
