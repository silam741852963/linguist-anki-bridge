//! WP-24 duplicate merge over the fake native port: the merged note is
//! archived on the survivor's item, deleted only after it was recorded with
//! its evidence, settled from the ledger after a lost response, and
//! recreated with its IDs.
mod common;
use common::*;
use linguist_application::merge::{
    MergedNote, add_merged_notes, delete_merged, merged_in, restore_merged,
};
use serde_json::json;

fn evidence(anki: &Anki, id: i64) -> serde_json::Value {
    let note = &anki.notes[&id];
    let model = anki.models.iter().find(|m| m.id == note.model_id).unwrap();
    json!({
        "note_id": id.to_string(),
        "note": {"guid": format!("guid{id}"), "model_id": note.model_id.to_string(), "fields": note.fields, "tags": note.tags},
        "model": {"name": note.model_name, "fields": model.fields},
        "cards": note.cards.iter().map(|c| json!({"id": c.id.to_string(), "ordinal": c.ordinal,
            "deck_id": c.deck_id.to_string(), "original_deck_id": "0", "type": 2, "queue": 2, "due": 5,
            "interval": 3, "ease_factor": 2500, "repetitions": 1, "lapses": 0, "remaining_steps": 0,
            "original_due": 0, "flags": 0, "custom_data": "",
            "reviews": [{"id": 1700000000000i64, "card_id": c.id, "usn": -1, "ease": 3, "interval": 3,
                "previous_interval": 1, "factor": 2500, "time": 4000, "type": 1}]})).collect::<Vec<_>>(),
        "media": [],
    })
}

#[test]
fn a_merged_duplicate_is_archived_deleted_once_and_restorable() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    // A second note with the same word: the duplicate merged into note 10.
    let mut duplicate = anki.notes[&10].clone();
    duplicate.id = 11;
    duplicate.cards[0].id = 21;
    anki.notes.insert(11, duplicate);
    let merged = MergedNote {
        evidence: evidence(&anki, 11),
        media: vec![("dup.png".into(), b"PNG".to_vec())],
    };
    let plan = add_merged_notes(
        &mut s.store,
        &s.plan,
        s.digest,
        s.item,
        &[merged],
        1 << 20,
        100,
    )
    .unwrap();
    assert_eq!(plan.revision, 2);
    assert_eq!(merged_in(&plan), vec![(s.item, "11".to_owned())]);
    let item = plan.documents.iter().find(|d| d.id == s.item).unwrap();
    assert!(
        item.archives
            .iter()
            .any(|a| a.original_fields == anki.notes[&11].fields && a.asset_digests.len() == 2)
    );
    assert!(
        !item
            .issues
            .iter()
            .any(|i| i.severity == linguist_core::Severity::Error),
        "{:?}",
        item.issues
    );
    // Not before the survivor is on its revamped type.
    let current = evidence(&anki, 11);
    let target = plan.rendered[0].model.name.clone();
    anki.notes.get_mut(&10).unwrap().model_name = "2. Picture Words".into();
    let refused = delete_merged(
        &mut s.store,
        &mut anki,
        &plan,
        "11",
        "10",
        Some(&current),
        1,
    )
    .unwrap_err();
    assert!(
        refused.starts_with("MERGE_SURVIVOR_NOT_APPLIED"),
        "{refused}"
    );
    assert!(anki.notes.contains_key(&11));
    assert_eq!(anki.main_mutations, 0);
    anki.notes.get_mut(&10).unwrap().model_name = target;
    // The response is lost after the delete: the ledger settles it.
    anki.fault = Fault::LoseAfterEffect;
    let lost = delete_merged(
        &mut s.store,
        &mut anki,
        &plan,
        "11",
        "10",
        Some(&current),
        2,
    )
    .unwrap_err();
    assert!(lost.starts_with("MERGE_OUTCOME_UNKNOWN"), "{lost}");
    assert!(!anki.notes.contains_key(&11));
    let settled = delete_merged(&mut s.store, &mut anki, &plan, "11", "10", None, 3).unwrap();
    assert_eq!(settled["state"], "deleted");
    let mutations = anki.main_mutations;
    let again = delete_merged(&mut s.store, &mut anki, &plan, "11", "10", None, 4).unwrap();
    assert_eq!(again["noop"], true);
    assert_eq!(anki.main_mutations, mutations, "no second delete");
    // Rollback recreates it with the same note and card IDs and its review.
    let restored = restore_merged(&mut s.store, &mut anki, "11", 1 << 20, 5).unwrap();
    assert_eq!(restored["state"], "restored");
    let back = &anki.notes[&11];
    assert_eq!(back.cards[0].id, 21);
    assert_eq!(back.cards[0].review_count, 1);
    // Every field of the note type comes back (the fixture note also holds
    // a field its model lacks, which a real note cannot).
    for (name, value) in &back.fields {
        assert_eq!(Some(value), anki.notes[&10].fields.get(name), "{name}");
    }
    assert_eq!(
        restore_merged(&mut s.store, &mut anki, "11", 1 << 20, 6).unwrap()["noop"],
        true
    );
}

#[test]
fn a_note_the_plan_does_not_merge_is_never_deleted() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    let error = delete_merged(
        &mut s.store,
        &mut anki,
        &s.plan.clone(),
        "10",
        "11",
        Some(&json!({})),
        1,
    )
    .unwrap_err();
    assert!(error.starts_with("MERGE_NOT_RECORDED"), "{error}");
    assert_eq!(anki.main_mutations, 0);
}
