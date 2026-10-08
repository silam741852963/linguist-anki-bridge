use linguist_dictionary::recording::english::parse;

#[test]
fn single_pronunciation_section_yields_ipa_and_a_us_recording() {
    let page = parse(include_str!("fixtures/wiktionary-page-supernova.html"));
    assert_eq!(page.sections, 1);
    let ipa = page.ipa.unwrap();
    assert!(
        ipa.starts_with('/') && ipa.contains("nəʊ") || ipa.contains("noʊ"),
        "{ipa}"
    );
    let audio = page.audio_url.unwrap();
    assert!(audio.starts_with("https://upload.wikimedia.org/") && audio.ends_with(".mp3"));
}

#[test]
fn homographs_leave_the_ipa_and_audio_unset() {
    let page = parse(include_str!("fixtures/wiktionary-page-lead.html"));
    assert_eq!(page.sections, 2);
    assert!(page.ipa.is_none());
    assert!(page.audio_url.is_none());
    assert!(!page.ipa_lines.is_empty());
    assert_eq!(parse("<html></html>").sections, 0);
}
