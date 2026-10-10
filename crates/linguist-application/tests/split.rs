//! ALG-SPLIT over the fake native port, with a group built by
//! `grammar::split`. These tests prove ordering, identity and recovery rules;
//! they do not prove native history preservation.
mod common;
use common::*;
use linguist_application::{
    grammar::{SplitRequest, split},
    restore::{RestoreDecision, RestoreRequest, plan_group_rollback, rollback_group},
    split::{SplitApplyRequest, SplitOutcome, apply_group, status},
};
use linguist_core::document::{Example, Grammar, LearningContent, Provenance};

const GRAMMAR_DECK: i64 = 600;
const LATER: u64 = 1_200_000;

fn anchor_fields() -> BTreeMap<String, String> {
    let mut fields: BTreeMap<String, String> = model::grammar()
        .fields
        .iter()
        .map(|f| (f.clone(), String::new()))
        .collect();
    fields.insert("Pattern".into(), "〜ても / 〜てもいい".into());
    fields.insert("Meaning".into(), "even if / may".into());
    fields.insert("Language".into(), "ja".into());
    fields
}

fn unit(pattern: &str, key: &str) -> Grammar {
    Grammar {
        pattern: pattern.into(),
        use_key: key.into(),
        meaning: format!("Nghĩa của {pattern}"),
        formation: "Động từ thể て + も".into(),
        recognition_prompt: "Mẫu này thể hiện quan hệ gì?".into(),
        examples: vec![Example {
            sentence: format!("雨が降{pattern}行きます。"),
            translation: "Dù trời mưa tôi vẫn đi.".into(),
            provenance: Provenance::User,
            evidence_ids: vec![],
        }],
        usage: String::new(),
        exercise_prompt: String::new(),
        exercise_answer: String::new(),
        ..Default::default()
    }
}

struct Group {
    s: Setup,
    anki: Anki,
    group: Uuid,
    anchor: Uuid,
    children: Vec<Uuid>,
}

fn group() -> Group {
    let mut s = setup(Kind::Create);
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/grammar.json"
    ))
    .unwrap();
    let fields = anchor_fields();
    let bytes = canonical::bytes(&fields).unwrap();
    let digest = s.store.publish_asset(&bytes, 1024 * 1024).unwrap();
    let source_id = Uuid::new_v4();
    doc.sources.push(SourceRecord {
        id: source_id,
        kind: "anki_read_capture_v2".into(),
        location: "anki_note:10".into(),
        digest: digest.clone(),
        text: None,
        fields: fields.clone(),
        model_manifest: GRAMMAR_DIGEST.into(),
        template_manifest: None,
        captured_at_unix_seconds: Some(1),
        tags: vec!["old".into()],
        cards: vec![],
        media_refs: vec![],
    });
    doc.archives.push(SourceArchive {
        id: Uuid::new_v4(),
        source_id,
        digest: digest.clone(),
        original_text: None,
        original_fields: fields,
        asset_digests: vec![digest],
    });
    let mut values = BTreeMap::new();
    values.insert(
        "purposes.japanese_grammar.target_deck".to_owned(),
        serde_json::json!("Japanese::Grammar"),
    );
    values.insert("input.max_file_mb".to_owned(), serde_json::json!(10));
    values.insert(
        "input.max_record_chars".to_owned(),
        serde_json::json!(100000),
    );
    let mut base = s.plan.clone();
    base.id = Uuid::new_v4();
    base.settings.values = values;
    base.documents = vec![doc.clone()];
    base.rendered = vec![];
    s.store.publish_revision(&base).unwrap();
    let request = SplitRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: base.approval_digest().unwrap(),
        document_id: doc.id,
        input_digest: doc.semantic_digest().unwrap(),
        actor: "reviewer".into(),
        anchor_index: 0,
        units: vec![
            unit("〜ても", "concession"),
            unit("〜てもいい", "permission"),
            unit("〜てもかまわない", "permission-formal"),
        ],
    };
    let raw = serde_json::to_vec(&request).unwrap();
    let child = split(&mut s.store, &base, &request, &raw).unwrap();
    // The reviewer accepts the native split plan for every unit, naming the
    // anchor, then binds and renders the revision.
    let anchor = child.grammar_groups[0].anchor_document;
    let mut reviewed = child.clone();
    for index in 0..reviewed.documents.len() {
        let document = reviewed.documents[index].clone();
        let issue = linguist_core::validation::validate(&document)
            .into_iter()
            .find(|i| i.code == "GRAMMAR_SPLIT_NATIVE_REVIEW")
            .unwrap();
        reviewed = linguist_core::review::resolve(
            &reviewed,
            &linguist_core::review::ResolutionRequest {
                schema_version: 2,
                base_revision: reviewed.revision,
                base_digest: reviewed.approval_digest().unwrap(),
                document_id: document.id,
                issue_id: issue.id,
                input_digest: document.semantic_digest().unwrap(),
                actor: "reviewer".into(),
                choice: ReviewChoice::Anchor(anchor),
            },
            "unix-seconds:1".into(),
        )
        .unwrap()
        .revision;
    }
    let mut ready = reviewed;
    ready.revision = 3;
    ready.parent_digest = Some(child.approval_digest().unwrap());
    ready.binding = Some(binding());
    ready.rendered = ready
        .documents
        .iter()
        .map(|d| render::render(d, &anchor_fields()).unwrap())
        .collect();
    let digest = s.store.publish_revision(&ready).unwrap();
    let evidence = linguist_core::plan_validation::inspect(&ready).unwrap();
    for item in &evidence.items {
        assert!(item.content_ready, "{:?}", item.issues);
    }
    let warnings: Vec<String> = evidence
        .items
        .iter()
        .flat_map(|item| item.issues.iter())
        .filter(|i| i.severity == linguist_core::Severity::Warning)
        .map(|i| i.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let approval = s
        .store
        .approve_revision(&ApprovalRequest {
            plan_id: ready.id,
            revision: 3,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: warnings,
        })
        .unwrap()
        .unwrap()
        .id;
    let g = ready.grammar_groups[0].clone();
    s.plan = ready;
    s.digest = Box::leak(digest.into_boxed_str());
    s.approval = approval;
    let mut anki = Anki::new(&s, Kind::Create);
    anki.models
        .push(observed_model(&model::grammar(), GRAMMAR_MODEL_ID));
    anki.decks.push(ObservedDeck {
        id: GRAMMAR_DECK,
        name: "Japanese::Grammar".into(),
        filtered: false,
    });
    anki.notes.insert(
        10,
        ObservedNote {
            id: 10,
            model_id: GRAMMAR_MODEL_ID,
            model_name: model::grammar().name,
            model_manifest_digest: GRAMMAR_DIGEST.into(),
            fields: anchor_fields(),
            tags: vec!["old".into()],
            cards: vec![studied_card()],
        },
    );
    Group {
        anchor: g.anchor_document,
        children: g
            .units
            .iter()
            .copied()
            .filter(|u| *u != g.anchor_document)
            .collect(),
        group: g.id,
        s,
        anki,
    }
}

impl Group {
    fn request(&self) -> SplitApplyRequest<'static> {
        SplitApplyRequest {
            apply: true,
            plan_id: self.s.plan.id,
            revision: 3,
            digest: self.s.digest,
            grammar_group: self.group,
            approval_id: self.s.approval,
            checkpoint_id: self.s.checkpoint,
            protected_manifest_digest: "protected-v1",
            reuse_max_age_seconds: 600,
            max_package_bytes: 4 * 1024 * 1024,
            max_media_bytes: 1024 * 1024,
            accept_schema_change: false,
            now_ms: 1_010_000,
            new_execution_id: None,
        }
    }
    fn run(&mut self) -> SplitOutcome {
        let request = self.request();
        apply_group(&mut self.s.store, &self.s.token, &mut self.anki, &request).unwrap()
    }
    fn created(&self) -> Vec<&ObservedNote> {
        self.anki.notes.values().filter(|n| n.id != 10).collect()
    }
}

#[test]
fn children_are_created_first_then_the_anchor_keeps_its_history() {
    let mut g = group();
    let before = g.anki.notes[&10].cards[0].clone();
    let outcome = g.run();
    assert_eq!(outcome.state, "complete", "{outcome:?}");
    assert_eq!(
        g.anki.dispatched,
        ["create_note", "create_note", "update_note"]
    );
    assert_eq!(outcome.units.len(), 3);
    assert_eq!(outcome.units[2].role, "anchor");
    assert_eq!(outcome.units[2].document_id, g.anchor);
    assert_eq!(outcome.units[2].note_id, Some(10));
    // Children are fresh notes with distinct markers and zero reviews.
    let created = g.created();
    assert_eq!(created.len(), 2);
    let markers: BTreeSet<_> = created
        .iter()
        .flat_map(|n| n.tags.iter().filter(|t| t.starts_with("lab_op_")))
        .collect();
    assert_eq!(markers.len(), 2);
    assert!(created.iter().all(|n| {
        n.cards
            .iter()
            .all(|c| c.review_count == 0 && c.deck_id == GRAMMAR_DECK && c.id != 20)
    }));
    // WP-23: each child card copies the source card's schedule but none of
    // its reviews, lapses or flag.
    let mut inherited = before.scheduler.clone();
    for key in ["reps", "lapses", "flags", "odue"] {
        inherited.insert(key.into(), "0".into());
    }
    assert!(
        created
            .iter()
            .all(|n| n.cards.iter().all(|c| c.scheduler == inherited)),
        "{created:?}"
    );
    // The anchor keeps its card ID, scheduling and history.
    let card = &g.anki.notes[&10].cards[0];
    assert_eq!(
        (card.id, &card.scheduler, &card.history_digest),
        (20, &before.scheduler, &before.history_digest)
    );
    // Every unit has its own journal and receipt, linked to the group.
    for unit in &outcome.units {
        let journal = g.s.journal(unit.operation_id);
        assert_eq!(journal.group_id, Some(outcome.execution_id));
        assert!(
            g.s.store
                .snapshot(journal.snapshot_id)
                .unwrap()
                .after
                .is_some()
        );
    }
    // One source snapshot for the whole group.
    let source = g.s.store.snapshot(outcome.source_snapshot).unwrap();
    assert_eq!(source.snapshot.operation_id, outcome.execution_id);
    assert_eq!(source.snapshot.originals[0].fields, anchor_fields());
    // Resuming a complete group writes nothing.
    let again = g.run();
    assert_eq!(again.state, "complete");
    assert_eq!(g.anki.dispatched.len(), 3);
    assert_eq!(
        status(&g.s.store, outcome.execution_id).unwrap().state,
        "complete"
    );
}

#[test]
fn child_accepted_and_anchor_failed_resumes_without_duplicates() {
    let mut g = group();
    g.anki.skip = 2;
    g.anki.fault = Fault::Reject;
    let outcome = g.run();
    assert_eq!(outcome.state, "partial");
    assert_eq!(
        outcome.units.iter().map(|u| u.status).collect::<Vec<_>>(),
        ["verified", "verified", "failed_before_write"]
    );
    assert!(
        outcome
            .next_command
            .as_deref()
            .unwrap()
            .contains("--split-group")
    );
    // The anchor is untouched; child receipts are exact.
    assert_eq!(g.anki.notes[&10].fields, anchor_fields());
    let child_notes: Vec<i64> = outcome.units[..2]
        .iter()
        .map(|u| u.note_id.unwrap())
        .collect();
    assert_eq!(g.created().len(), 2);
    for _ in 0..2 {
        let resumed = g.run();
        assert_eq!(resumed.state, "complete", "{resumed:?}");
        assert_eq!(
            resumed.units[..2]
                .iter()
                .map(|u| u.note_id.unwrap())
                .collect::<Vec<_>>(),
            child_notes
        );
        assert_eq!(resumed.units[2].attempts.len(), 2);
    }
    assert_eq!(g.created().len(), 2);
    assert_eq!(
        g.anki.dispatched,
        ["create_note", "create_note", "update_note", "update_note"]
    );
}

#[test]
fn unknown_child_outcomes_are_reconciled_on_resume_never_recreated() {
    for fault in [Fault::LoseAfterEffect, Fault::LoseBeforeEffect] {
        let mut g = group();
        g.anki.fault = fault;
        let outcome = g.run();
        assert_eq!(outcome.state, "partial");
        assert_eq!(
            outcome.units.iter().map(|u| u.status).collect::<Vec<_>>(),
            ["needs_recovery", "not_started", "not_started"]
        );
        assert!(
            outcome.units[0]
                .next_command
                .as_deref()
                .unwrap()
                .contains("recover reconcile")
        );
        assert_eq!(g.anki.notes[&10].fields, anchor_fields());
        let resumed = g.run();
        assert_eq!(resumed.state, "complete", "{fault:?} {resumed:?}");
        assert_eq!(g.created().len(), 2, "{fault:?}");
        assert_eq!(resumed.units[0].attempts, outcome.units[0].attempts);
        // Only a request that never reached the ledger is re-sent, with its UUID.
        let creates = g
            .anki
            .dispatched
            .iter()
            .filter(|v| *v == "create_note")
            .count();
        assert_eq!(creates, 2 + usize::from(fault == Fault::LoseBeforeEffect));
    }
}

#[test]
fn unprovable_child_outcome_stops_the_group_before_the_anchor() {
    let mut g = group();
    g.anki.fault = Fault::UnknownNoEffect;
    let outcome = g.run();
    assert_eq!(outcome.state, "partial");
    let resumed = g.run();
    assert_eq!(resumed.state, "partial");
    assert_eq!(resumed.units[0].status, "needs_recovery");
    assert_eq!(resumed.units[2].status, "not_started");
    assert!(resumed.issues.iter().any(|i| i == "APPLY_ABSENCE_UNPROVEN"));
    assert_eq!(g.anki.dispatched, ["create_note"]);
}

#[test]
fn group_preflight_failure_writes_nothing() {
    let mut g = group();
    g.anki
        .notes
        .get_mut(&10)
        .unwrap()
        .fields
        .insert("Meaning".into(), "edited after preparation".into());
    let request = g.request();
    let error = apply_group(&mut g.s.store, &g.s.token, &mut g.anki, &request).unwrap_err();
    assert!(error.starts_with("SPLIT_PREFLIGHT_FAILED"), "{error}");
    assert!(error.contains("APPLY_SOURCE_CONFLICT"), "{error}");
    assert!(g.anki.dispatched.is_empty());
    assert!(
        g.s.store
            .split_execution_for(g.s.plan.id, 3, g.group)
            .unwrap()
            .is_none()
    );
    let mut request = g.request();
    request.apply = false;
    let error = apply_group(&mut g.s.store, &g.s.token, &mut g.anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_FLAG_REQUIRED"));
}

#[test]
fn a_split_unit_is_never_applied_alone() {
    let mut g = group();
    let mut request = g.s.request();
    request.revision = 3;
    request.item_id = g.children[0];
    let error = apply_item(&mut g.s.store, &g.s.token, &mut g.anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_SPLIT_GROUP_REQUIRED"), "{error}");
    let items = preview(&g.s.store, g.s.plan.id, 3, &[]).unwrap();
    let child = items.iter().find(|i| i.item_id == g.children[0]).unwrap();
    assert_eq!((child.action, child.split_role), ("create", Some("child")));
    let anchor = items.iter().find(|i| i.item_id == g.anchor).unwrap();
    assert_eq!(
        (anchor.action, anchor.split_role),
        ("update_or_migrate", Some("anchor"))
    );
    assert!(items.iter().all(|i| i.split_group == Some(g.group)));
    assert!(g.anki.dispatched.is_empty());
}

#[test]
fn rollback_restores_the_anchor_and_protects_studied_children() {
    let mut g = group();
    let outcome = g.run();
    assert_eq!(outcome.state, "complete");
    let execution = outcome.execution_id;
    let source_before = g.s.store.snapshot(outcome.source_snapshot).unwrap();
    let anchor_journal = g.s.journal(outcome.units[2].operation_id);
    let anchor_snapshot_before = g.s.store.snapshot(anchor_journal.snapshot_id).unwrap();
    let plan_before = g.s.store.revision(g.s.plan.id, 3).unwrap();
    // The user studies the anchor and one child after apply.
    let studied_child = outcome.units[0].note_id.unwrap();
    let fresh_child = outcome.units[1].note_id.unwrap();
    g.anki.study();
    g.anki.notes.get_mut(&studied_child).unwrap().cards[0].review_count = 1;
    let items = plan_group_rollback(&g.s.store, &mut g.anki, execution).unwrap();
    assert_eq!(items.len(), 3);
    // The anchor restore comes first; created children are kept by default.
    let anchor_plan = items[0].plan.clone().unwrap();
    assert_eq!(anchor_plan.note_id, Some(10));
    for item in &items[1..] {
        assert_eq!(item.plan.as_ref().unwrap().created_notes[0].action, "keep");
    }
    let snapshot_of = |note: i64| {
        items
            .iter()
            .find(|i| {
                i.plan
                    .as_ref()
                    .unwrap()
                    .created_notes
                    .iter()
                    .any(|n| n.note_id == note)
            })
            .unwrap()
            .snapshot_id
    };
    let notes: Vec<ObservedNote> = [10, studied_child, fresh_child]
        .iter()
        .map(|id| g.anki.notes[id].clone())
        .collect();
    let checkpoint = checkpoint_for(&mut g.s, &notes.iter().collect::<Vec<_>>(), &[], LATER);
    let decide = |snapshot: Uuid, digest: &str, delete: Vec<i64>| RestoreRequest {
        apply: true,
        snapshot_id: snapshot,
        decision: Some(RestoreDecision {
            schema_version: 1,
            snapshot_id: snapshot,
            observed_state_digest: digest.into(),
            actor: "reviewer".into(),
            fields: BTreeMap::new(),
            decks: BTreeMap::new(),
            remove_unstudied_cards: vec![],
            delete_created_notes: delete,
            accept_missing_media: vec![],
            accept_schema_change: false,
        }),
        checkpoint_id: checkpoint,
        group_id: Some(execution),
        protected_manifest_digest: "protected-v1",
        reuse_max_age_seconds: 600,
        max_package_bytes: 4 * 1024 * 1024,
        max_media_bytes: 1024 * 1024,
        now_ms: LATER + 1000,
    };
    let digest_of = |snapshot: Uuid| {
        items
            .iter()
            .find(|i| i.snapshot_id == snapshot)
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .observed_state_digest
            .clone()
    };
    let requests = vec![
        decide(
            anchor_plan.snapshot_id,
            &anchor_plan.observed_state_digest,
            vec![],
        ),
        decide(
            snapshot_of(studied_child),
            &digest_of(snapshot_of(studied_child)),
            vec![studied_child],
        ),
        decide(
            snapshot_of(fresh_child),
            &digest_of(snapshot_of(fresh_child)),
            vec![fresh_child],
        ),
    ];
    let results = rollback_group(
        &mut g.s.store,
        &g.s.token,
        &mut g.anki,
        execution,
        &requests,
    )
    .unwrap();
    assert_eq!(results.len(), 3);
    // Anchor restored with its later study kept.
    let anchor = results[0].as_ref().unwrap();
    assert_eq!(anchor.target_state, OperationState::Restored);
    assert_eq!(g.anki.notes[&10].fields, anchor_fields());
    assert_eq!(g.anki.notes[&10].cards[0].review_count, 2);
    // The studied child is protected; the unstudied one is deleted.
    let refused = results.iter().find_map(|r| r.as_ref().err()).unwrap();
    assert_eq!(refused.0, snapshot_of(studied_child));
    assert!(
        refused.1.contains("RESTORE_CREATED_NOTE_STUDIED"),
        "{}",
        refused.1
    );
    assert!(g.anki.notes.contains_key(&studied_child));
    // Resuming the rollback lists only what is still unrestored: the
    // protected child, never the restored anchor (WP-23).
    let again = plan_group_rollback(&g.s.store, &mut g.anki, execution).unwrap();
    assert_eq!(
        again.iter().map(|i| i.snapshot_id).collect::<Vec<_>>(),
        [snapshot_of(studied_child)]
    );
    assert!(again[0].error.is_none(), "{:?}", again[0].error);
    let deleted = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .find(|o| o.snapshot_id == snapshot_of(fresh_child))
        .unwrap();
    assert_eq!(deleted.target_state, OperationState::Restored);
    assert!(!g.anki.notes.contains_key(&fresh_child));
    // Original source snapshot, anchor apply receipt and source archive unchanged.
    assert_eq!(
        g.s.store
            .snapshot(outcome.source_snapshot)
            .unwrap()
            .snapshot,
        source_before.snapshot
    );
    let after = g.s.store.snapshot(anchor_journal.snapshot_id).unwrap();
    assert_eq!(after.snapshot, anchor_snapshot_before.snapshot);
    assert_eq!(after.after, anchor_snapshot_before.after);
    assert_eq!(g.s.store.revision(g.s.plan.id, 3).unwrap(), plan_before);
    // The group is rolled back; resuming never re-applies it.
    let dispatched = g.anki.dispatched.len();
    let resumed = g.run();
    assert_eq!(resumed.state, "rolled_back");
    assert!(
        resumed
            .issues
            .iter()
            .any(|i| i.starts_with("SPLIT_GROUP_ROLLED_BACK"))
    );
    assert_eq!(g.anki.dispatched.len(), dispatched);
}

#[test]
fn split_records_and_attempts_are_immutable() {
    let mut g = group();
    let outcome = g.run();
    let db = rusqlite::Connection::open(g.s.root.join("state").join("state.sqlite3")).unwrap();
    for statement in [
        "UPDATE split_executions SET created_ms=0",
        "DELETE FROM split_executions",
        "UPDATE split_attempts SET sequence=9",
        "DELETE FROM split_attempts",
    ] {
        assert!(db.execute(statement, []).is_err(), "{statement}");
    }
    let record = g.s.store.split_execution(outcome.execution_id).unwrap();
    assert_eq!(record.children, g.children);
    assert_eq!(record.anchor_document, g.anchor);
}

#[test]
fn native_split_review_accepts_only_the_recorded_anchor() {
    let g = group();
    let anchor =
        g.s.plan
            .documents
            .iter()
            .find(|d| d.id == g.anchor)
            .unwrap();
    assert!(matches!(anchor.content, LearningContent::Grammar(_)));
    // On the unreviewed split revision, name a child as the anchor.
    let plan = g.s.store.revision(g.s.plan.id, 2).unwrap();
    let document = plan
        .documents
        .iter()
        .find(|d| d.id == g.children[0])
        .unwrap()
        .clone();
    let issue = linguist_core::validation::validate(&document)
        .into_iter()
        .find(|i| i.code == "GRAMMAR_SPLIT_NATIVE_REVIEW")
        .unwrap();
    let error = linguist_core::review::resolve(
        &plan,
        &linguist_core::review::ResolutionRequest {
            schema_version: 2,
            base_revision: plan.revision,
            base_digest: plan.approval_digest().unwrap(),
            document_id: document.id,
            issue_id: issue.id,
            input_digest: document.semantic_digest().unwrap(),
            actor: "reviewer".into(),
            choice: ReviewChoice::Anchor(g.children[0]),
        },
        "unix-seconds:1".into(),
    )
    .unwrap_err();
    assert_eq!(error.0, "REVIEW_SPLIT_ANCHOR_MISMATCH");
}

#[test]
fn a_lease_that_lapses_during_a_long_group_is_renewed_between_units() {
    // WP-23: a checkpoint plus many units outlasted one lease period and the
    // group stopped with LEASE_STALE_OR_EXPIRED; each unit now renews it.
    let mut g = group();
    // Each write outlasts the whole lease period.
    g.s.store.renew_lease(&g.s.token, 1).unwrap();
    g.anki.mutate_delay = Some(std::time::Duration::from_millis(1100));
    let outcome = g.run();
    assert_eq!(outcome.state, "complete", "{outcome:?}");
}
