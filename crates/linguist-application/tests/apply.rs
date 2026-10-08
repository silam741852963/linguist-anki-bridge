//! ALG-APPLY/ALG-MIGRATE/ALG-RECONCILE over a fake native port. These tests
//! prove orchestration and journal rules only; native scheduling preservation
//! is shown separately in disposable Anki (scripts/verify-native-apply.py).
mod common;
use common::*;

// ---------- tests ----------

#[test]
fn create_commits_with_marker_snapshot_receipt_and_owner_release() {
    let mut s = setup(Kind::CreateWithMedia);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    assert!(outcome.next_command.is_none() && outcome.receipt_digest.is_some());
    assert_eq!((anki.main_mutations, anki.media_mutations), (1, 1));
    assert_eq!(anki.owners, 1);
    assert_eq!(anki.ended, 1);
    let note = &anki.notes[&outcome.note_id.unwrap()];
    assert_eq!(note.cards.len(), 1);
    assert_eq!(note.cards[0].deck_id, TARGET_DECK);
    let operation = outcome.operation_id.unwrap();
    let journal = s.journal(operation);
    assert_eq!(journal.steps.len(), 2);
    assert_eq!(journal.steps[0].action, "store_media");
    assert!(
        journal
            .steps
            .iter()
            .all(|step| step.state == StepState::Verified)
    );
    let marker = format!("lab_op_{}", journal.steps[1].id.simple());
    assert!(note.tags.contains(&marker) && note.tags.contains(&"linguist".to_owned()));
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    assert!(snapshot.snapshot.originals.is_empty());
    let receipt = snapshot.after.unwrap();
    assert_eq!(receipt.operation_id, journal.steps[1].id);
    assert_eq!(
        receipt.readback.unwrap().note_ids[0],
        linguist_core::document::AnkiId::try_from(note.id.to_string()).unwrap()
    );
    // A committed item cannot be applied again.
    let request = s.request();
    let again = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(again.starts_with("APPLY_ALREADY_COMMITTED"), "{again}");
    assert_eq!(anki.mutations(), 2);
}

#[test]
fn missing_apply_flag_and_preview_have_zero_effects() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let mut request = s.request();
    request.apply = false;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_FLAG_REQUIRED"));
    let items = preview(&s.store, s.plan.id, 1, &[]).unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        items[0].approved && items[0].content_ready,
        "{:?}",
        items[0]
    );
    assert_eq!(items[0].action, "create");
    assert!(items[0].blockers.is_empty(), "{:?}", items[0].blockers);
    assert_eq!(anki.mutations(), 0);
    assert_eq!(anki.owners, 0);
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
}

type Adjust = dyn Fn(&mut ApplyRequest, &mut Anki);

#[test]
fn authority_identity_and_checkpoint_refusals_mutate_nothing() {
    let mut s = setup(Kind::Create);
    let digest = s.digest;
    let cases: Vec<(Box<Adjust>, &str)> = vec![
        (
            Box::new(|r, _| r.digest = "lab-jcs-v1:plan:0"),
            "APPLY_DIGEST_MISMATCH",
        ),
        (
            Box::new(|r, _| r.approval_id = Uuid::new_v4()),
            "APPROVAL_NOT_FOUND",
        ),
        (
            Box::new(|r, _| r.item_id = Uuid::new_v4()),
            "APPLY_APPROVAL_MISMATCH",
        ),
        (
            Box::new(|_, a| a.binding.profile_fingerprint = "d".repeat(64)),
            "APPLY_IDENTITY_MISMATCH",
        ),
        (
            Box::new(|_, a| a.binding.lineage_id = Uuid::from_u128(99)),
            "APPLY_IDENTITY_MISMATCH",
        ),
        (
            Box::new(|r, _| r.checkpoint_id = Uuid::new_v4()),
            "CHECKPOINT_NOT_FOUND",
        ),
        (
            // A changed session epoch no longer matches the checkpoint binding.
            Box::new(|_, a| a.binding.session_epoch = Uuid::from_u128(4)),
            "CHECKPOINT_BINDING_MISMATCH",
        ),
        (
            Box::new(|r, _| r.reuse_max_age_seconds = 1),
            "CHECKPOINT_REUSE_REJECTED",
        ),
        (
            Box::new(|_, a| a.variants.retain(|v| v != "create_note")),
            "CAPABILITY_UNAVAILABLE",
        ),
        (Box::new(|_, a| a.models.clear()), "APPLY_MODEL_MISSING"),
        (
            Box::new(|_, a| a.models[0].css.push_str("/* changed */")),
            "MODEL_NAME_COLLISION",
        ),
        (
            Box::new(|_, a| a.decks.clear()),
            "APPLY_TARGET_DECK_MISSING",
        ),
    ];
    for (mutate, expected) in cases {
        let mut anki = Anki::new(&s, Kind::Create);
        let mut request = s.request();
        request.digest = digest;
        mutate(&mut request, &mut anki);
        let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
        assert_eq!(anki.mutations(), 0, "{expected}");
    }
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
}

#[test]
fn weak_binding_stale_revision_and_remote_bridge_are_refused() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.binding.endpoint = "http://192.0.2.1:8765".into();
    let mut plan = s.plan.clone();
    plan.binding = Some(anki.binding.clone());
    // Remote endpoint: plan and bridge agree, but writes need loopback.
    let mut remote = s.plan.clone();
    remote.id = Uuid::new_v4();
    remote.binding = Some(anki.binding.clone());
    let digest = s.store.publish_revision(&remote).unwrap();
    let approval = s
        .store
        .approve_revision(&ApprovalRequest {
            plan_id: remote.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: vec![],
        })
        .unwrap()
        .unwrap()
        .id;
    let mut request = s.request();
    request.plan_id = remote.id;
    request.digest = &digest;
    request.approval_id = approval;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_REMOTE_MUTATION_UNAVAILABLE"),
        "{error}"
    );
    // Weak binding.
    let mut weak = s.plan.clone();
    weak.id = Uuid::new_v4();
    weak.binding = None;
    let digest = s.store.publish_revision(&weak).unwrap();
    let approval = s
        .store
        .approve_revision(&ApprovalRequest {
            plan_id: weak.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: vec![],
        })
        .unwrap()
        .unwrap()
        .id;
    let mut anki = Anki::new(&s, Kind::Create);
    let mut request = s.request();
    request.plan_id = weak.id;
    request.digest = &digest;
    request.approval_id = approval;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_BINDING_WEAK"), "{error}");
    // A newer revision makes the approved one stale.
    let mut next = s.plan.clone();
    next.revision = 2;
    next.parent_digest = Some(s.digest.to_owned());
    s.store.publish_revision(&next).unwrap();
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_REVISION_STALE"), "{error}");
    assert_eq!(anki.mutations(), 0);
}

#[test]
fn second_writer_is_rejected_by_the_collection_lease() {
    let s = setup(Kind::Create);
    let mut other = Store::open(&s.root.join("state")).unwrap();
    let error = other
        .acquire_lease(&Resource::CollectionWriter(Uuid::from_u128(2)), 300)
        .unwrap_err();
    assert!(error.contains("LEASE"), "{error}");
}

#[test]
fn source_edit_between_preparation_and_apply_conflicts_and_preserves_original() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.notes
        .get_mut(&10)
        .unwrap()
        .fields
        .insert("Meaning".into(), "user edit".into());
    let before = anki.notes[&10].clone();
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_SOURCE_CONFLICT"), "{error}");
    assert!(error.contains("fields"));
    assert_eq!(anki.notes[&10], before);
    assert_eq!(anki.mutations(), 0);
    // A model change since preparation is also a conflict.
    let mut anki = Anki::new(&s, Kind::Update);
    anki.notes.get_mut(&10).unwrap().model_manifest_digest = "9".repeat(64);
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.contains("model"), "{error}");
}

#[test]
fn study_between_preparation_and_apply_is_captured_fresh_and_preserved() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.study();
    let studied = anki.notes[&10].cards[0].clone();
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert!(note.fields["Meaning"].contains("to eat"));
    assert!(note.tags.contains(&"old".to_owned()) && note.tags.contains(&"linguist".to_owned()));
    assert!(!note.tags.iter().any(|t| t.starts_with("lab_op_")));
    let card = &note.cards[0];
    assert_eq!(card.id, 20);
    assert_eq!(card.deck_id, TARGET_DECK);
    assert_eq!(card.scheduler, studied.scheduler);
    assert_eq!(card.history_digest, studied.history_digest);
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    let original = &snapshot.snapshot.originals[0];
    assert_eq!(original.fields["Meaning"], "to consume");
    assert_eq!(original.cards[0].scheduler, studied.scheduler);
    assert_eq!(original.cards[0].history_digest, studied.history_digest);
    assert_eq!(snapshot.after.unwrap().readback.unwrap().card_ids.len(), 1);
}

#[test]
fn study_during_mutation_needs_recovery_without_success_claim() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.fault = Fault::StudyDuringMutation;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_READBACK_MISMATCH")
    );
    assert!(outcome.next_command.unwrap().contains("recover reconcile"));
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    assert!(snapshot.after.is_none());
    // Reconciliation cannot adopt a state that differs from the desired post-state.
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(outcome.operation_id.unwrap(), true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(rec.steps[0].action, "review");
    assert_eq!(anki.main_mutations, 1);
}

#[test]
fn timeout_after_accepted_create_reconciles_to_one_note() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    assert_eq!(s.journal(operation).steps[0].state, StepState::Unknown);
    // A new attempt for the same item is blocked until reconciliation.
    let request = s.request();
    let blocked = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(blocked.starts_with("APPLY_RECOVERY_REQUIRED"), "{blocked}");
    // Read-only proposal first.
    let proposal = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, false),
    )
    .unwrap();
    assert_eq!(proposal.state, OperationState::NeedsRecovery);
    assert_eq!(proposal.steps[0].action, "adopt");
    assert!(!proposal.applied);
    assert_eq!(
        local_proposal(&s.store, operation).unwrap().next_live_check,
        "native_status_and_readback"
    );
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert!(rec.receipt_digest.is_some());
    assert_eq!(marker_notes(&anki), 1);
    assert_eq!(anki.main_mutations, 1);
    // Reconciling again is a no-op on a committed operation.
    let again = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(again.state, OperationState::Committed);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn lost_request_without_ledger_row_resubmits_the_same_uuid_once() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseBeforeEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    let step = s.journal(operation).steps[0].id;
    let proposal = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, false),
    )
    .unwrap();
    assert_eq!(proposal.steps[0].action, "resubmit_same_uuid");
    assert_eq!(anki.main_mutations, 1);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert!(
        rec.issues
            .iter()
            .any(|i| i.code == "APPLY_RESUBMIT_SAME_UUID")
    );
    assert_eq!(anki.ledger.len(), 1);
    assert!(anki.ledger.contains_key(&step));
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn unknown_add_with_exact_marker_candidate_is_adopted() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::UnknownAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(rec.steps[0].action, "adopt");
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn unknown_status_without_candidates_never_resubmits() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::UnknownNoEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    for _ in 0..2 {
        let rec = reconcile(
            &mut s.store,
            &s.token,
            &mut anki,
            &reconcile_request(operation, true),
        )
        .unwrap();
        assert_eq!(rec.state, OperationState::NeedsRecovery);
        assert_eq!(rec.steps[0].evidence, "absent_unproven");
    }
    let issues = s.journal(operation).issues;
    assert_eq!(
        issues
            .iter()
            .filter(|i| i.code == "APPLY_ABSENCE_UNPROVEN")
            .count(),
        1
    );
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 0);
}

#[test]
fn copied_marker_candidates_block_for_review() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let copy = anki.notes.values().next().unwrap().clone();
    let mut copy = copy;
    copy.id = 1;
    anki.notes.insert(1, copy);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(rec.steps[0].evidence, "partial_or_conflict");
    assert_eq!(anki.main_mutations, 1);
}

#[test]
fn disk_full_after_native_acceptance_stops_and_recovery_discovers_result() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.fault = Fault::HoldLock;
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_LOCAL_DURABILITY_FAILED"),
        "{error}"
    );
    anki.lock = None;
    assert_eq!(anki.main_mutations, 1);
    let pending = s.store.pending_journals(10).unwrap();
    assert_eq!(pending.len(), 1);
    let journal = &pending[0].journal;
    assert_eq!(journal.steps[0].state, StepState::RequestStarted);
    let snapshot = s.store.snapshot(journal.snapshot_id).unwrap();
    assert_eq!(
        snapshot.snapshot.originals[0].fields["Meaning"],
        "to consume"
    );
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(journal.id, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(anki.main_mutations, 1);
    // The original snapshot is untouched; the receipt is stored beside it.
    let after = s.store.snapshot(journal.snapshot_id).unwrap();
    assert_eq!(after.snapshot, snapshot.snapshot);
    assert!(after.after.is_some());
}

#[test]
fn media_collision_never_overwrites_and_identical_media_is_reused() {
    let mut s = setup(Kind::CreateWithMedia);
    let filename = s.plan.documents[0].media[0].filename.clone();
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.media.insert(
        filename.clone(),
        ObservedMedia {
            filename: filename.clone(),
            sha256: "9".repeat(64),
            size_bytes: 3,
        },
    );
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_MEDIA_FILENAME_COLLISION"),
        "{error}"
    );
    assert_eq!(anki.media[&filename].sha256, "9".repeat(64));
    assert_eq!(anki.mutations(), 0);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.media.insert(
        filename.clone(),
        ObservedMedia {
            filename: filename.clone(),
            sha256: sha256(AUDIO),
            size_bytes: AUDIO.len() as u64,
        },
    );
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::Committed);
    assert_eq!((anki.main_mutations, anki.media_mutations), (1, 0));
    assert_eq!(s.journal(outcome.operation_id.unwrap()).steps.len(), 1);
}

#[test]
fn rejected_create_fails_before_write_and_allows_a_new_attempt() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::Reject;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::FailedBeforeWrite);
    assert!(outcome.next_command.is_none());
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::Committed);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn rejection_after_verified_media_is_a_known_partial_that_can_be_superseded() {
    let mut s = setup(Kind::CreateWithMedia);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.fault = Fault::Reject;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_KNOWN_PARTIAL")
    );
    // The stored media is reused by the superseding attempt; nothing is deleted.
    let next = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(next.state, OperationState::Committed);
    assert_eq!(anki.media_mutations, 1);
    assert_eq!(anki.media.len(), 1);
}

#[test]
fn pending_native_operation_is_waited_on_not_resent() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::Pending;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    // The fake applied the effect while still reporting queued: the read-back
    // matches, so the status alone does not block adoption.
    assert_eq!(rec.steps[0].native_status, Some(NativeStatus::Queued));
    assert_eq!(rec.steps[0].action, "wait");
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(anki.main_mutations, 1);
    anki.ledger
        .insert(s.journal(operation).steps[0].id, NativeStatus::Verified);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
}

#[test]
fn migration_requires_accepted_schema_change_and_complete_mapping() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_SCHEMA_CHANGE_NOT_ACCEPTED"),
        "{error}"
    );
    let mut request = s.request();
    request.accept_schema_change = true;
    // A second, unmapped source card would be deleted by the native change.
    let mut extra = studied_card();
    extra.id = 21;
    extra.ordinal = 1;
    anki.notes.get_mut(&10).unwrap().cards.push(extra);
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_MIGRATION_DROPS_CARD"), "{error}");
    // Filtered-deck membership blocks migration.
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.notes.get_mut(&10).unwrap().cards[0].original_deck_id = HOME_DECK;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_FILTERED_DECK_BLOCKS"), "{error}");
    assert_eq!(anki.mutations(), 0);
}

#[test]
fn mapped_migration_retains_card_id_history_and_scheduling() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    let before = anki.notes[&10].cards[0].clone();
    let mut request = s.request();
    request.accept_schema_change = true;
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert_eq!(note.model_id, V2_MODEL_ID);
    let card = &note.cards[0];
    assert_eq!((card.id, card.ordinal), (20, 0));
    assert_eq!(card.scheduler, before.scheduler);
    assert_eq!(card.history_digest, before.history_digest);
    let journal = s.journal(outcome.operation_id.unwrap());
    let record = s.store.apply_operation(journal.id).unwrap();
    assert_eq!(record.intent["action"], "migrate");
    assert_eq!(
        record.intent["steps"][0]["effect"]["migration"]["ordinal_map"][0]["target"],
        0
    );
}

#[test]
fn partial_native_migration_with_replaced_card_needs_recovery() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.fault = Fault::ReplaceCard;
    let mut request = s.request();
    request.accept_schema_change = true;
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_READBACK_MISMATCH")
    );
    assert!(
        s.store
            .snapshot(outcome.snapshot_id.unwrap())
            .unwrap()
            .after
            .is_none()
    );
}

#[test]
fn profile_switch_or_session_change_stops_reconciliation_until_rebind() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    // Profile switch: different collection identity.
    anki.binding.profile_fingerprint = "d".repeat(64);
    let error = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap_err();
    assert!(error.starts_with("APPLY_IDENTITY_MISMATCH"), "{error}");
    // Same collection, new session epoch.
    anki.binding = binding();
    anki.binding.session_epoch = Uuid::from_u128(44);
    let error = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap_err();
    assert!(error.starts_with("SESSION_CHANGED"), "{error}");
    let record = s.store.apply_operation(operation).unwrap();
    let intent: linguist_application::apply::ApplyIntent =
        serde_json::from_value(record.intent).unwrap();
    let observed = current_state_digest(&mut anki, &intent).unwrap();
    let mut decision = ResumeBindingDecision {
        schema_version: 1,
        operation_id: operation,
        approval_digest: s.digest.to_owned(),
        old_binding: binding(),
        new_binding: anki.binding.clone(),
        observed_state_digest: "0".repeat(64),
        actor: "operator".into(),
        decided_at: "unix-seconds:1".into(),
        scope: ResumeBindingScope::ContinueOperation,
    };
    let stale = ReconcileRequest {
        operation_id: operation,
        apply: true,
        rebind: Some(decision.clone()),
    };
    let error = reconcile(&mut s.store, &s.token, &mut anki, &stale).unwrap_err();
    assert!(error.starts_with("BINDING_DECISION_STALE"), "{error}");
    decision.observed_state_digest = observed;
    // --rebind without the current invocation's --apply is refused.
    let unauthorized = ReconcileRequest {
        operation_id: operation,
        apply: false,
        rebind: Some(decision.clone()),
    };
    let error = reconcile(&mut s.store, &s.token, &mut anki, &unauthorized).unwrap_err();
    assert!(error.starts_with("APPLY_FLAG_REQUIRED"), "{error}");
    let request = ReconcileRequest {
        operation_id: operation,
        apply: true,
        rebind: Some(decision),
    };
    let rec = reconcile(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert!(rec.rebound);
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(s.store.binding_decisions(operation).unwrap().len(), 1);
    // The receipt carries the rebound epoch and is accepted by the store.
    let snapshot = s.store.snapshot(s.journal(operation).snapshot_id).unwrap();
    assert_eq!(snapshot.after.unwrap().session_epoch, Uuid::from_u128(44));
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn batch_stops_on_identity_fault_and_continues_after_item_failures() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.binding.lineage_id = Uuid::from_u128(77);
    let first = s.request();
    let second = s.request();
    let results = apply_items(&mut s.store, &s.token, &mut anki, &[first, second]);
    assert_eq!(results.len(), 1);
    assert!(
        results[0]
            .as_ref()
            .unwrap_err()
            .1
            .starts_with("APPLY_IDENTITY_MISMATCH")
    );
    let mut anki = Anki::new(&s, Kind::Create);
    let mut first = s.request();
    first.approval_id = Uuid::new_v4();
    let second = s.request();
    let results = apply_items(&mut s.store, &s.token, &mut anki, &[first, second]);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert_eq!(
        results[1].as_ref().unwrap().state,
        OperationState::Committed
    );
}

#[test]
fn intent_records_and_binding_decisions_are_immutable() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE apply_operations SET created_ms=0", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM apply_operations", []).is_err());
    drop(db);
    let records = s
        .store
        .apply_operations_for_item(s.plan.id, s.item)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_id, operation);
    // An intent cannot be recorded for an unapproved item.
    let mut forged = records[0].clone();
    forged.operation_id = Uuid::new_v4();
    forged.item_id = Uuid::new_v4();
    assert_eq!(
        s.store.publish_apply_operation(&forged).unwrap_err(),
        "APPLY_OPERATION_APPROVAL_CONFLICT"
    );
}

/// Shared vector with addons/linguist_bridge/tests/test_effects.py: the native
/// critical section must compute the identical precondition digest.
#[test]
fn precondition_digest_matches_the_companion_vector() {
    let note = ObservedNote {
        id: 1700000000001,
        model_id: 1700000000002,
        model_name: "Linguist Vocabulary v2".into(),
        model_manifest_digest: "ab".repeat(32),
        fields: [("Expression", "食べる"), ("Meaning", "to \"eat\"\n")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        tags: vec!["zeta".into(), "alpha".into(), "zeta".into()],
        cards: vec![
            ObservedCard {
                id: 1700000000004,
                ordinal: 1,
                deck_id: 1,
                original_deck_id: 0,
                scheduler: BTreeMap::new(),
                history_digest: String::new(),
                review_count: 0,
            },
            ObservedCard {
                id: 1700000000003,
                ordinal: 0,
                deck_id: 1,
                original_deck_id: 0,
                scheduler: BTreeMap::new(),
                history_digest: String::new(),
                review_count: 3,
            },
        ],
    };
    assert_eq!(
        content_digest(&note).unwrap(),
        "lab-jcs-v1:lab-apply-precondition-v1:719d9ff2eac8a52ebe85cc0ed0c38c91a46ceafb852850fc65536615488c314c"
    );
}

#[test]
fn crash_after_receipt_before_commit_finalizes_with_the_stored_receipt() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let snapshot = outcome.snapshot_id.unwrap();
    let stored = s.store.snapshot(snapshot).unwrap().after.unwrap();
    // Simulate a crash between the receipt write and the commit event. Test-only
    // tampering removes the final committed event, which the store otherwise forbids.
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    let head: u32 = db
        .query_row(
            "SELECT sequence FROM journal_heads WHERE operation=?1",
            [operation.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("DROP TRIGGER journal_events_no_delete;")
        .unwrap();
    db.execute(
        "UPDATE journal_heads SET sequence=?1 WHERE operation=?2",
        rusqlite::params![head - 1, operation.to_string()],
    )
    .unwrap();
    db.execute(
        "DELETE FROM journal_events WHERE operation=?1 AND sequence=?2",
        rusqlite::params![operation.to_string(), head],
    )
    .unwrap();
    drop(db);
    assert_eq!(s.journal(operation).state, OperationState::Verifying);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(s.store.snapshot(snapshot).unwrap().after.unwrap(), stored);
    assert_eq!(anki.main_mutations, 1);
}

#[test]
fn an_archive_copy_of_the_same_bytes_never_shadows_the_rendered_media() {
    // A revamp can capture an older copy of bytes the plan renders under a
    // new name; apply must store the rendered name, not reuse the copy.
    let mut s = setup_with(Kind::CreateWithMedia, |doc| {
        let mut copy = doc.media[0].clone();
        copy.filename = "older_copy.mp3".into();
        copy.role = linguist_core::records::MediaRole::Archive;
        doc.media.insert(0, copy);
    });
    let rendered = s.plan.documents[0]
        .media
        .iter()
        .find(|m| m.role != linguist_core::records::MediaRole::Archive)
        .unwrap()
        .filename
        .clone();
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.media.insert(
        "older_copy.mp3".into(),
        ObservedMedia {
            filename: "older_copy.mp3".into(),
            sha256: sha256(AUDIO),
            size_bytes: AUDIO.len() as u64,
        },
    );
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::Committed);
    assert_eq!(anki.media_mutations, 1);
    assert!(anki.media.contains_key(&rendered));
}
