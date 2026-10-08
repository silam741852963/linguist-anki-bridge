use linguist_dictionary::cambridge::parse_page;

fn en() -> linguist_core::Language {
    "en".to_owned().try_into().unwrap()
}
fn page(word: &str, html: &str) -> linguist_dictionary::DictionaryPage {
    parse_page(word, &en(), html.as_bytes(), 10 << 20, 100).unwrap()
}

#[test]
fn american_entries_with_word_level_extras() {
    let page = page(
        "tenacious",
        include_str!("fixtures/cambridge-tenacious.html"),
    );
    assert_eq!(page.entries.len(), 1);
    let entry = &page.entries[0];
    assert_eq!(entry.metadata["dictionary"], ["cacd"]);
    assert_eq!(entry.forms, ["tenacious"]);
    assert_eq!(
        entry.senses[0].definitions,
        ["unwilling to accept defeat or stop doing or having something"]
    );
    assert_eq!(entry.senses[0].labels[0], "adjective");
    assert!(
        entry.senses[0].examples[0]
            .sentence
            .starts_with("Seles is a tenacious opponent")
    );
    assert_eq!(entry.metadata["ipa_us"], ["təˈneɪ·ʃəs"]);
    assert!(entry.metadata["audio_us"][0].ends_with("/us_pron/t/ten/tenac/tenacious.mp3"));
    assert!(entry.metadata["synonyms"].contains(&"dogged".to_string()));
    assert_eq!(entry.metadata["smart_vocabulary_topic"], ["Strong-willed"]);
    assert!(entry.metadata["smart_vocabulary"].len() > 5);
    assert!(entry.related_entries.contains(&"tenaciously".to_string()));
    assert!(
        entry
            .related_entries
            .contains(&"tenacity (noun)".to_string())
    );
    assert_eq!(page.exact_matches, [0]);
}

#[test]
fn several_parts_of_speech_and_dictionary_illustrations() {
    let record = page("record", include_str!("fixtures/cambridge-record.html"));
    let parts: Vec<_> = record
        .entries
        .iter()
        .map(|e| e.metadata["part_of_speech"][0].as_str())
        .collect();
    assert_eq!(parts, ["verb", "noun", "adjective"]);
    assert!(
        record.entries[0].senses[0]
            .labels
            .contains(&"store information".to_string())
    );
    let supernova = page(
        "supernova",
        include_str!("fixtures/cambridge-supernova.html"),
    );
    assert_eq!(
        supernova.entries[0].metadata["images"],
        ["https://dictionary.cambridge.org/images/full/supern_noun_002_36712.jpg"]
    );
    // An unknown word redirects to the home page: no entries.
    assert!(
        page("zzqq", "<html><body>home</body></html>")
            .entries
            .is_empty()
    );
}

fn rendered(
    word: &str,
    html: &str,
    pick: impl Fn(&linguist_core::Sense) -> bool,
) -> std::collections::BTreeMap<String, String> {
    use linguist_core::{LearningContent, LearningDocument, records::*, render};
    let page = page(word, html);
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    doc.target_language = en();
    let sense = page
        .entries
        .iter()
        .flat_map(|e| &e.senses)
        .find(|s| pick(s))
        .unwrap()
        .clone();
    if let LearningContent::Vocabulary(v) = &mut doc.content {
        v.expression = word.into();
        v.reading.clear();
        v.pronunciation = "/x/".into();
        v.sense_key = sense.key.clone();
        v.meaning = sense.definitions.join("; ");
        v.dictionary = page.entries.clone();
        v.examples.clear();
    }
    let input_digest = doc.semantic_digest().unwrap();
    doc.reviews.push(ReviewDecision {
        id: doc.id,
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", doc.id),
        input_digest,
        actor: "reviewer".into(),
        created_at: "2026-10-08T00:00:00Z".into(),
        choice: ReviewChoice::Sense(sense.key),
    });
    render::render(&doc, &Default::default()).unwrap().fields
}

#[test]
fn meaning_shows_the_first_definition_of_each_part_of_speech() {
    let fields = rendered(
        "record",
        include_str!("fixtures/cambridge-record.html"),
        |s| s.definitions[0].starts_with("a flat"),
    );
    let meaning = &fields["Meaning"];
    // verb, noun and adjective firsts, plus the selected later noun sense.
    assert_eq!(meaning.matches("<li").count(), 4, "{meaning}");
    assert_eq!(meaning.matches("lab-selected").count(), 1);
    assert!(meaning.contains("<span class=\"lab-pos\">verb</span>"));
    assert!(meaning.contains("<span class=\"lab-pos\">adjective</span>"));
    assert!(!meaning.contains("record"), "answer masked: {meaning}");
    let back = &rendered(
        "tenacious",
        include_str!("fixtures/cambridge-tenacious.html"),
        |_| true,
    )["UsageExamples"];
    for section in [
        "<h4>Synonyms</h4>",
        "dogged",
        "<h4>Word family</h4>",
        "tenacity (noun)",
        "<h4>Topic: Strong-willed</h4>",
    ] {
        assert!(back.contains(section), "missing {section}: {back}");
    }
}
