//! WP-25 harvest tables: schema 15 upgrades to 16, a note's contexts are
//! replaced as a whole, statuses survive a rescan, and pruning drops only new
//! candidates no context names.
use linguist_store::Store;
use linguist_store::harvest::{HarvestCandidate, HarvestContext};

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("lab-harvest-{}", uuid::Uuid::new_v4()))
}

fn context(note: &str, base: &str, sentence: &str) -> HarvestContext {
    HarvestContext {
        note: note.into(),
        base: base.into(),
        sentence: sentence.into(),
        translation: String::new(),
        provenance: "generated".into(),
        others: vec![],
    }
}

fn candidate(base: &str) -> HarvestCandidate {
    HarvestCandidate {
        base: base.into(),
        reading: String::new(),
        pos: "名詞".into(),
        status: "new".into(),
        frequency: None,
    }
}

#[test]
fn schema_fifteen_upgrades_and_statuses_survive_a_rescan() {
    let root = root();
    drop(Store::open(&root).unwrap());
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TABLE harvest_candidates; DROP TABLE harvest_contexts; DROP TABLE harvest_marks; PRAGMA user_version=15;",
    )
    .unwrap();
    drop(db);
    let mut store = Store::open(&root).unwrap();
    store
        .harvest_replace_note(
            "ja",
            "n1",
            "d1",
            &[context("n1", "交通費", "交通費を払う")],
            &[candidate("交通費")],
            1,
        )
        .unwrap();
    store
        .harvest_set_status("ja", &["交通費".into()], "queued", 2)
        .unwrap();
    // The note changed: its contexts are replaced, the status stays.
    store
        .harvest_replace_note(
            "ja",
            "n1",
            "d2",
            &[
                context("n1", "交通費", "交通費が高い"),
                context("n1", "高い", "交通費が高い"),
            ],
            &[candidate("交通費"), candidate("高い")],
            3,
        )
        .unwrap();
    let contexts = store.harvest_contexts("ja").unwrap();
    assert_eq!(contexts.len(), 2);
    assert!(contexts.iter().all(|c| c.sentence == "交通費が高い"));
    let candidates = store.harvest_candidates("ja").unwrap();
    assert_eq!(
        candidates
            .iter()
            .find(|c| c.base == "交通費")
            .unwrap()
            .status,
        "queued"
    );
    assert_eq!(store.harvest_marks("ja").unwrap()["n1"], "d2");
    assert!(
        store
            .harvest_set_status("ja", &["x".into()], "bogus", 4)
            .is_err()
    );
    // A rescan without the word: only a `new` candidate is pruned.
    store
        .harvest_replace_note("ja", "n1", "d3", &[], &[], 5)
        .unwrap();
    assert_eq!(store.harvest_prune("ja").unwrap(), 1);
    let left: Vec<String> = store
        .harvest_candidates("ja")
        .unwrap()
        .into_iter()
        .map(|c| c.base)
        .collect();
    assert_eq!(left, ["交通費"]);
    store.harvest_reset("ja").unwrap();
    assert!(store.harvest_marks("ja").unwrap().is_empty());
    let _ = std::fs::remove_dir_all(root);
}
