use linguist_application::mapping::*;
use std::collections::BTreeMap;
fn fields() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Combined".into(),
            "<b>食べる</b><br>たべる [sound:meal.mp3]".into(),
        ),
        ("Meaning".into(), "to eat".into()),
        ("Unused".into(), "  private original\n".into()),
    ])
}
#[test]
fn exact_mapping_preserves_shared_and_unmapped_original_fields_without_claiming_facts() {
    let fields = fields();
    let original = fields.clone();
    let map = BTreeMap::from([
        ("expression".into(), "Combined".into()),
        ("reading".into(), "Combined".into()),
        ("meaning".into(), "Meaning".into()),
    ]);
    let result = map_fields(SourceKind::Vocabulary, &fields, &map, 10000, 10000).unwrap();
    assert_eq!(fields, original);
    assert_eq!(result.roles["expression"].raw_value, fields["Combined"]);
    assert_eq!(
        result.shared_fields["Combined"],
        vec!["expression", "reading"]
    );
    assert_eq!(result.unmapped_fields, vec!["Unused"]);
    assert!(result.missing_required_roles.is_empty());
    assert!(!result.normalized_facts_verified && !result.apply_authorized);
    let mut changed = fields.clone();
    changed.insert("Unused".into(), "changed".into());
    let next = map_fields(SourceKind::Vocabulary, &changed, &map, 10000, 10000).unwrap();
    assert_ne!(result.source_digest, next.source_digest);
    assert_eq!(result.mapping_digest, next.mapping_digest);
}
#[test]
fn absent_empty_and_wrong_kind_mappings_are_explicit() {
    let fields = fields();
    let empty = map_fields(SourceKind::Grammar, &fields, &BTreeMap::new(), 10000, 10000).unwrap();
    assert_eq!(
        empty.missing_required_roles,
        vec!["pattern", "meaning", "formation"]
    );
    for (role, name, expected) in [
        ("expression", "combined", "SOURCE_MAPPING_FIELD_MISSING"),
        ("pattern", "Combined", "SOURCE_MAPPING_ROLE_INVALID"),
        ("unknown", "Meaning", "SOURCE_MAPPING_ROLE_INVALID"),
    ] {
        let map = BTreeMap::from([(role.into(), name.into())]);
        assert_eq!(
            map_fields(SourceKind::Vocabulary, &fields, &map, 10000, 10000).unwrap_err(),
            expected
        );
    }
    let mut fields = fields;
    fields.insert("Meaning".into(), " \n".into());
    let map = BTreeMap::from([("meaning".into(), "Meaning".into())]);
    assert_eq!(
        map_fields(SourceKind::Vocabulary, &fields, &map, 10000, 10000)
            .unwrap()
            .missing_required_roles,
        vec!["expression", "meaning"]
    );
}
#[test]
fn source_limits_include_unmapped_data_and_unicode_character_counts() {
    let fields = fields();
    assert_eq!(
        map_fields(SourceKind::Vocabulary, &fields, &BTreeMap::new(), 1, 10000).unwrap_err(),
        "SOURCE_MAPPING_INPUT_LIMIT"
    );
    let text = BTreeMap::from([("Text".into(), "猫犬鳥".into())]);
    assert!(map_fields(SourceKind::Vocabulary, &text, &BTreeMap::new(), 100, 3).is_ok());
    assert!(map_fields(SourceKind::Vocabulary, &text, &BTreeMap::new(), 100, 2).is_err());
    assert!(map_fields(SourceKind::Vocabulary, &text, &BTreeMap::new(), 0, 3).is_err());
}

#[test]
fn purpose_mapping_consumes_resolved_overrides_and_expected_source_model() {
    use linguist_config::*;
    let registry = Registry::builtin();
    for purpose in [
        "japanese_vocab",
        "english_vocab",
        "japanese_grammar",
        "english_grammar",
    ] {
        let role = if purpose.ends_with("vocab") {
            "expression"
        } else {
            "pattern"
        };
        let mut options = ResolveOptions {
            purpose: Some(purpose.into()),
            ..Default::default()
        };
        options.flags.insert(
            format!("purposes.{purpose}.fields"),
            serde_json::json!({role:"Combined"}),
        );
        options.flags.insert(
            format!("purposes.{purpose}.source_model"),
            serde_json::json!("Legacy"),
        );
        let config = resolve(&registry, &ConfigFile::default(), &options).unwrap();
        let mapped = map_purpose_fields(&config, purpose, "Legacy", &fields()).unwrap();
        assert_eq!(mapped.roles[role].source_field, "Combined");
        assert_eq!(mapped.roles[role].raw_value, fields()["Combined"]);
        assert_eq!(
            map_purpose_fields(&config, purpose, "Different", &fields()).unwrap_err(),
            "SOURCE_MAPPING_MODEL_CONFLICT"
        );
    }
}
