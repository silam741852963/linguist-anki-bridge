//! WP-25 harvester over an applied note and a never-revamped deck note:
//! candidates, i+1 ranking, statuses, an incremental rescan and the export
//! that `vocab add --format jsonl` reads.
mod common;
use common::*;
use linguist_application::harvest::{DeckNote, ScanRequest, export, rank, scan};
use linguist_core::{Example, LearningContent, Provenance};

fn example(sentence: &str) -> Example {
    Example {
        sentence: sentence.into(),
        translation: format!("({sentence})"),
        provenance: Provenance::User,
        evidence_ids: vec![],
    }
}

fn deck(extra: &[(i64, &str)]) -> Vec<DeckNote> {
    let mut notes = vec![
        DeckNote {
            note_id: 10,
            expression: "食べる".into(),
            fields: vec![],
        },
        DeckNote {
            note_id: 11,
            expression: "パン".into(),
            fields: vec![
                ("Word".into(), "パン".into()),
                (
                    "Example Sentence".into(),
                    "<div>りんごを買いました。</div>".into(),
                ),
            ],
        },
    ];
    for (id, word) in extra {
        notes.push(DeckNote {
            note_id: *id,
            expression: (*word).into(),
            fields: vec![],
        });
    }
    notes
}

fn request<'a>(
    deck: &'a [DeckNote],
    jmdict: Option<&'a linguist_text::jmdict::Jmdict>,
) -> ScanRequest<'a> {
    ScanRequest {
        purpose: "japanese_vocab",
        deck,
        jmdict,
        full: false,
        limit: None,
        now_ms: 1,
    }
}

#[test]
fn words_met_in_examples_are_ranked_marked_rescanned_and_exported() {
    let mut s = setup_with(Kind::Update, |doc| {
        if let LearningContent::Vocabulary(v) = &mut doc.content {
            v.examples = vec![
                example("毎朝パンを食べます。"),
                example("交通費を払ってパンを食べます。"),
            ];
        }
    });
    let mut anki = Anki::new(&s, Kind::Update);
    let request_apply = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request_apply).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );

    let notes = deck(&[]);
    let summary = scan(&mut s.store, &request(&notes, None)).unwrap();
    assert_eq!(
        (summary.from_documents, summary.from_fields),
        (1, 1),
        "{summary:?}"
    );
    let ranked = rank(&s.store, "japanese_vocab").unwrap();
    let bases: Vec<&str> = ranked.iter().map(|r| r.base.as_str()).collect();
    // Deck words (食べる, パン) are known; without JMdict 交通費 stays split.
    assert!(
        !bases.contains(&"食べる") && !bases.contains(&"パン"),
        "{bases:?}"
    );
    for word in ["毎朝", "交通", "払う", "りんご", "買う"] {
        assert!(bases.contains(&word), "{word} in {bases:?}");
    }
    let top = &ranked[0];
    assert_eq!(top.i_plus_one, 1, "{ranked:?}");
    let maiasa = ranked.iter().find(|r| r.base == "毎朝").unwrap();
    assert_eq!(maiasa.i_plus_one, 1);
    assert_eq!(
        maiasa.best.as_ref().unwrap().sentence,
        "毎朝パンを食べます。"
    );
    assert_eq!(
        ranked.iter().find(|r| r.base == "交通").unwrap().i_plus_one,
        0
    );

    // Ignoring 払う leaves 交通 alone in its sentence.
    s.store
        .harvest_set_status("japanese_vocab", &["払う".into()], "ignored", 2)
        .unwrap();
    let ranked = rank(&s.store, "japanese_vocab").unwrap();
    assert_eq!(
        ranked.iter().find(|r| r.base == "交通").unwrap().i_plus_one,
        1
    );

    // Nothing changed: the rescan reads no note.
    let again = scan(&mut s.store, &request(&notes, None)).unwrap();
    assert_eq!(
        (again.notes_read, again.notes_unchanged),
        (0, 2),
        "{again:?}"
    );

    // A JMdict that knows 交通費 keeps it whole on a full rescan.
    let jmdict = linguist_text::jmdict::Jmdict::parse(
        "<entry><k_ele><keb>交通費</keb></k_ele><r_ele><reb>こうつうひ</reb><re_pri>nf10</re_pri></r_ele></entry>",
    );
    let mut full = request(&notes, Some(&jmdict));
    full.full = true;
    scan(&mut s.store, &full).unwrap();
    let ranked = rank(&s.store, "japanese_vocab").unwrap();
    let koutsuuhi = ranked.iter().find(|r| r.base == "交通費").unwrap();
    assert_eq!(koutsuuhi.frequency, Some(10));
    // The ignored status survived the full rescan.
    assert_eq!(
        ranked.iter().find(|r| r.base == "払う").unwrap().status,
        "ignored"
    );

    // Queue and export 毎朝: a v2 vocabulary record `vocab add` accepts.
    s.store
        .harvest_set_status("japanese_vocab", &["毎朝".into()], "queued", 3)
        .unwrap();
    let records = export(&s.store, "japanese_vocab", "queued").unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record["body"]["expression"], "毎朝");
    assert_eq!(record["body"]["reading"], "まいあさ");
    assert_eq!(
        record["body"]["examples"][0]["sentence"],
        "毎朝パンを食べます。"
    );
    assert_eq!(record["body"]["examples"][0]["provenance"], "source");
    let input: linguist_application::AddInput = serde_json::from_value(record.clone()).unwrap();
    assert!(matches!(
        input,
        linguist_application::AddInput::Vocabulary { .. }
    ));

    // Once the deck has a 毎朝 note, the queued word is added.
    let notes = deck(&[(12, "毎朝")]);
    let after = scan(&mut s.store, &request(&notes, None)).unwrap();
    assert_eq!(after.became_added, 1, "{after:?}");
}
