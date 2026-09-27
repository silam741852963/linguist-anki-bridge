use linguist_dictionary::{Error, wiktionary::parse_definition};
fn en() -> linguist_core::Language {
    "en".to_owned().try_into().unwrap()
}
fn body() -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "en":[{"language":"English","partOfSpeech":"Verb","sectionExtension":"preserved","definitions":[
            {"definition":"<p>Eat &amp; drink</p><p>&lt;slowly&gt;</p><script>execute()</script>","examples":["<i>I eat.</i>"],"parsedExamples":[{"example":"<b>I eat &amp; drink.</b>","translation":"<i>Translation</i>","note":"original retained"}],"extension":{"unknown":true}},
            {"definition":"Consume a meal","examples":["<span>We eat.</span>"]}]},
            {"language":"English","partOfSpeech":"Noun","definitions":[{"definition":"A meal"}]}],
        "fr":[{"language":"French","partOfSpeech":"Verb","definitions":[{"definition":"Foreign sense stays archived"}]}]
    })).unwrap()
}
#[test]
fn english_sections_senses_examples_and_raw_foreign_data_survive() {
    let bytes = body();
    let page = parse_definition("eat", &en(), &bytes, 10000, 10).unwrap();
    assert_eq!(page.raw_bytes, bytes);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].senses.len(), 2);
    let sense = &page.entries[0].senses[0];
    assert_eq!(sense.definitions, vec!["Eat & drink\n<slowly>\n"]);
    assert!(!sense.definitions[0].contains("execute()"));
    assert_eq!(sense.examples[0].sentence, "I eat & drink.");
    assert_eq!(sense.examples[0].translation, "Translation");
    assert_eq!(
        sense.examples[0].provenance,
        linguist_core::Provenance::Dictionary
    );
    assert_eq!(page.entries[0].senses[1].examples[0].sentence, "We eat.");
    assert!(
        page.entries[0].metadata[&format!("sense:{}:raw_json", sense.key)][0]
            .contains("original retained")
    );
    assert_eq!(
        page.entries[0].metadata["pronunciation_status"],
        vec!["not_exposed_by_definition_response"]
    );
    assert!(
        String::from_utf8(page.raw_bytes.clone())
            .unwrap()
            .contains("Foreign sense stays archived")
    );
    assert_eq!(
        parse_definition("eat", &en(), &bytes, 10000, 10)
            .unwrap()
            .entries,
        page.entries
    );
}
#[test]
fn foreign_language_is_never_an_english_fallback() {
    let page =
        parse_definition("eat", &en(), br#"{"fr":[{"language":"French"}]}"#, 1024, 10).unwrap();
    assert!(page.entries.is_empty());
    assert!(page.exact_matches.is_empty());
    assert_eq!(
        parse_definition(
            "eat",
            &"ja".to_owned().try_into().unwrap(),
            &body(),
            10000,
            10
        )
        .unwrap_err(),
        Error::UnsupportedLanguage
    );
}
#[test]
fn malformed_and_changed_response_shapes_fail_without_invented_facts() {
    for value in [
        serde_json::json!({"en":[{"language":"French","partOfSpeech":"Verb","definitions":[{"definition":"wrong language"}]}]}),
        serde_json::json!({"en":[{"language":"English","partOfSpeech":"Verb","definitions":[{"definition":"<script>code()</script>"}]}]}),
        serde_json::json!({"en":"changed schema"}),
        serde_json::json!({"definitions":[]}),
    ] {
        assert_eq!(
            parse_definition(
                "eat",
                &en(),
                &serde_json::to_vec(&value).unwrap(),
                10000,
                10
            )
            .unwrap_err(),
            Error::Schema
        );
    }
    assert_eq!(
        parse_definition("eat", &en(), br#"{"en":[],"en":[]}"#, 10000, 10).unwrap_err(),
        Error::Schema
    );
    assert_eq!(
        parse_definition("eat", &en(), &body(), 1, 10).unwrap_err(),
        Error::BodyLimit
    );
    assert_eq!(
        parse_definition("eat", &en(), &body(), 10000, 1).unwrap_err(),
        Error::EntryLimit
    );
    assert_eq!(
        parse_definition("eat", &en(), br#"{"type":"not_found"}"#, 10000, 10).unwrap_err(),
        Error::ProviderStatus
    );
}
#[test]
fn source_titles_are_encoded_with_a_fixed_origin() {
    let page = parse_definition("eat/../?attack=true", &en(), &body(), 10000, 10).unwrap();
    let request = url::Url::parse(&page.request_url).unwrap();
    assert_eq!(request.host_str(), Some("en.wiktionary.org"));
    assert!(request.query().is_none());
    assert!(request.path().contains("%2F"));
    assert_eq!(
        parse_definition("..", &en(), &body(), 10000, 10).unwrap_err(),
        Error::InvalidQuery
    );
}
