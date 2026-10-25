//! WP-27 `notes retag` over the fake native port: an applied note still equal
//! to what apply wrote is rewritten in place with the current render under the
//! apply journal; an edited note is never touched.
mod common;
use common::*;
use linguist_application::restore::{RestoreDecision, RestoreRequest, plan_restore, restore};
use linguist_application::retag::{
    Applied, Proposal, RetagRequest, Revisions, Verdict, latest_by_note, propose, retag,
};

const LATER: u64 = 1_200_000;

fn applied(s: &mut Setup, anki: &mut Anki) -> Uuid {
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    outcome.operation_id.unwrap()
}

fn latest(s: &Setup, note: i64) -> Applied {
    latest_by_note(&s.store).unwrap()[&note].clone().unwrap()
}

/// What an older renderer wrote: apply's projection and the note both carry
/// it, so the note is unedited and the current render differs.
fn older_render(s: &Setup, anki: &mut Anki, note: i64) -> Applied {
    let mut applied = latest(s, note);
    applied
        .desired
        .fields
        .insert("Meaning".into(), "<p>old meaning html</p>".into());
    applied
        .desired
        .tags
        .push("lab::pos::wikipedia-definition".into());
    applied.desired.tags.sort();
    let observed = anki.notes.get_mut(&note).unwrap();
    observed.fields = applied.desired.fields.clone();
    observed.tags = applied.desired.tags.clone();
    applied
}

fn proposal(s: &Setup, anki: &mut Anki, note: i64, applied: Applied) -> Proposal {
    propose(
        &s.store,
        anki,
        &mut Revisions::default(),
        note,
        Some(&Ok(applied)),
    )
    .unwrap()
}

fn request(checkpoint: Uuid) -> RetagRequest<'static> {
    RetagRequest {
        checkpoint_id: checkpoint,
        group_id: None,
        protected_manifest_digest: "protected-v1",
        reuse_max_age_seconds: 600,
        max_package_bytes: 4 * 1024 * 1024,
        now_ms: LATER + 1000,
    }
}

#[test]
fn an_unedited_note_is_rewritten_with_the_current_render_and_can_be_restored() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let first = applied(&mut s, &mut anki);
    // Freshly applied with today's renderer: nothing to do.
    let current = proposal(&s, &mut anki, 10, latest(&s, 10));
    assert!(matches!(current.verdict, Verdict::Current), "{current:?}");

    let old = older_render(&s, &mut anki, 10);
    let change = proposal(&s, &mut anki, 10, old);
    match &change.verdict {
        Verdict::Change {
            fields,
            tags_removed,
            ..
        } => {
            assert_eq!(fields, &vec!["Meaning".to_owned()]);
            assert_eq!(
                tags_removed,
                &vec!["lab::pos::wikipedia-definition".to_owned()]
            );
        }
        other => panic!("{other:?}"),
    }
    let before = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&before], &[], LATER);
    let outcome = retag(
        &mut s.store,
        &s.token,
        &mut anki,
        &change,
        &request(checkpoint),
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    assert!(outcome.receipt_digest.is_some());
    let after = &anki.notes[&10];
    assert_ne!(after.fields["Meaning"], "<p>old meaning html</p>");
    assert!(
        !after
            .tags
            .iter()
            .any(|t| t == "lab::pos::wikipedia-definition")
    );
    // Cards keep their IDs, deck and history.
    assert_eq!(after.cards, before.cards);
    let journal = s.journal(outcome.operation);
    assert_eq!(journal.steps.len(), 1);
    assert_eq!(journal.steps[0].action, "restore_note");
    // The companion takes an exact restore_note only under a restore decision.
    assert!(
        journal
            .approval_digest
            .starts_with("lab-jcs-v1:lab-restore-decision-v1:"),
        "{}",
        journal.approval_digest
    );
    assert!(
        journal
            .steps
            .iter()
            .all(|step| step.state == StepState::Verified)
    );

    // The retag is now the note's latest operation, and it is current.
    let now = latest(&s, 10);
    assert_eq!(now.operation, outcome.operation);
    assert_ne!(now.operation, first);
    let again = proposal(&s, &mut anki, 10, now);
    assert!(matches!(again.verdict, Verdict::Current), "{again:?}");

    // `snapshots restore` of the retag puts the older render back.
    let snapshot = s.journal(outcome.operation).snapshot_id;
    let plan = plan_restore(&s.store, &mut anki, snapshot, None).unwrap();
    assert!(
        plan.conflicts.is_empty() && plan.blockers.is_empty(),
        "{plan:?}"
    );
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER + 2000);
    let restored = restore(
        &mut s.store,
        &s.token,
        &mut anki,
        &RestoreRequest {
            apply: true,
            snapshot_id: snapshot,
            decision: Some(RestoreDecision {
                schema_version: 1,
                snapshot_id: snapshot,
                observed_state_digest: plan.observed_state_digest.clone(),
                actor: "reviewer".into(),
                fields: BTreeMap::new(),
                decks: BTreeMap::new(),
                remove_unstudied_cards: vec![],
                delete_created_notes: vec![],
                accept_missing_media: vec![],
                accept_schema_change: false,
            }),
            checkpoint_id: checkpoint,
            group_id: None,
            protected_manifest_digest: "protected-v1",
            reuse_max_age_seconds: 600,
            max_package_bytes: 4 * 1024 * 1024,
            max_media_bytes: 1024 * 1024,
            now_ms: LATER + 3000,
        },
    )
    .unwrap();
    assert!(restored.receipt.is_some(), "{:?}", restored.issues);
    assert_eq!(anki.notes[&10].fields["Meaning"], "<p>old meaning html</p>");
    // A restored retag is no longer a note to retag.
    assert!(latest_by_note(&s.store).unwrap()[&10].is_err());
}

#[test]
fn a_note_edited_since_apply_is_skipped_and_never_written() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    applied(&mut s, &mut anki);
    let old = older_render(&s, &mut anki, 10);
    anki.notes
        .get_mut(&10)
        .unwrap()
        .fields
        .insert("Meaning".into(), "my own wording".into());
    let skipped = proposal(&s, &mut anki, 10, old);
    match &skipped.verdict {
        Verdict::Skip { code } => assert!(code.starts_with("RETAG_NOTE_EDITED"), "{code}"),
        other => panic!("{other:?}"),
    }
    let mutations = anki.mutations();
    let error = retag(
        &mut s.store,
        &s.token,
        &mut anki,
        &skipped,
        &request(s.checkpoint),
    )
    .unwrap_err();
    assert!(error.starts_with("RETAG_NOTHING_TO_WRITE"), "{error}");
    assert_eq!(anki.mutations(), mutations);
}

#[test]
fn a_change_after_the_proposal_refuses_before_the_journal() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    applied(&mut s, &mut anki);
    let old = older_render(&s, &mut anki, 10);
    let change = proposal(&s, &mut anki, 10, old);
    assert!(matches!(change.verdict, Verdict::Change { .. }));
    anki.notes.get_mut(&10).unwrap().tags.push("leech".into());
    let pending = s.store.pending_journal_count().unwrap();
    let mutations = anki.mutations();
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER);
    let error = retag(
        &mut s.store,
        &s.token,
        &mut anki,
        &change,
        &request(checkpoint),
    )
    .unwrap_err();
    assert!(error.starts_with("RETAG_NOTE_CHANGED"), "{error}");
    assert_eq!(anki.mutations(), mutations);
    assert_eq!(s.store.pending_journal_count().unwrap(), pending);
}

#[test]
fn a_note_no_apply_wrote_or_one_gone_is_skipped() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let none = propose(&s.store, &mut anki, &mut Revisions::default(), 10, None).unwrap();
    assert!(
        matches!(&none.verdict, Verdict::Skip { code } if code.starts_with("RETAG_NOT_APPLIED"))
    );
    applied(&mut s, &mut anki);
    let applied = latest(&s, 10);
    anki.notes.remove(&10);
    let gone = proposal(&s, &mut anki, 10, applied);
    assert!(
        matches!(&gone.verdict, Verdict::Skip { code } if code.starts_with("RETAG_NOTE_MISSING"))
    );
}
