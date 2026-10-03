use linguist_application::selector::require_explicit_matches;

#[test]
fn explicit_ids_require_full_unique_match_and_empty_query_is_safe() {
    let requested = vec!["12".into(), "2".into()];
    assert!(require_explicit_matches(&requested, &["2".into(), "12".into()]).is_ok());
    assert_eq!(
        require_explicit_matches(&requested, &["2".into()]).unwrap_err(),
        "NOTE_SELECTOR_MISSING_OR_CHANGED_ID"
    );
    assert_eq!(
        require_explicit_matches(&["2".into(), "2".into()], &["2".into()]).unwrap_err(),
        "NOTE_SELECTOR_DUPLICATE_ID"
    );
    assert!(require_explicit_matches(&[], &[]).is_ok());
}
