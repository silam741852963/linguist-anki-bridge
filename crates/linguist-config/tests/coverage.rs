//! Registry-to-consumer coverage: no registered setting is silently ignored.
use linguist_config::{
    ConfigFile, Registry, ResolveOptions,
    coverage::{self, ROWS, Status, Unavailable},
    resolve,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_registry_entry_has_exactly_one_coverage_row() {
    let registry = Registry::builtin();
    let keys: BTreeSet<String> = registry.defaults().keys().cloned().collect();
    let mut rows = BTreeSet::new();
    for row in ROWS {
        assert!(rows.insert(row.key), "duplicate row {}", row.key);
        assert!(registry.lookup(row.key).is_ok(), "unknown key {}", row.key);
    }
    let mut entries = BTreeSet::new();
    for key in &keys {
        let entry = registry.lookup(key).unwrap();
        assert!(rows.contains(entry.key.as_str()), "uncovered setting {key}");
        entries.insert(entry.key.clone());
    }
    for pattern in ["profiles.<name>.overrides", "purposes.<purpose>.overrides"] {
        entries.insert(registry.lookup(pattern).unwrap().key.clone());
    }
    assert_eq!(
        rows.len(),
        entries.len(),
        "rows must match registry entries exactly"
    );
    assert_eq!(rows.len(), 157);
}

#[test]
fn every_consumer_site_names_its_setting() {
    for row in ROWS {
        let path = root().join(row.consumer);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{}: missing consumer {}", row.key, row.consumer));
        assert!(
            text.contains(row.needle),
            "{}: {} does not contain {:?}",
            row.key,
            row.consumer,
            row.needle
        );
        if row.status == Status::Gated {
            assert_eq!(row.unavailable, Unavailable::NonDefault, "{}", row.key);
            assert!(!row.feature.is_empty(), "{}", row.key);
        }
    }
}

#[test]
fn defaults_never_need_an_unavailable_feature() {
    let registry = Registry::builtin();
    for purpose in linguist_config::builtin_purposes()
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let effective = resolve(
            &registry,
            &ConfigFile::default(),
            &ResolveOptions {
                purpose,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            coverage::unavailable_settings(&effective),
            Vec::<serde_json::Value>::new()
        );
    }
}

#[test]
fn unavailable_values_and_dormant_settings_are_reported() {
    let registry = Registry::builtin();
    let flags = BTreeMap::from([
        ("browser.max_pages".to_owned(), json!(5)),
        ("dictionary.provider".to_owned(), json!("custom")),
        (
            "dictionary.url_template".to_owned(),
            json!("https://dict.example/{word}"),
        ),
        (
            "dictionary.schema_path".to_owned(),
            json!("/tmp/schema.json"),
        ),
        (
            "network.allowed_remote_service_hosts".to_owned(),
            json!(["dict.example"]),
        ),
        ("selection.duplicate_policy".to_owned(), json!("skip_exact")),
        ("llm.prompts.kanji".to_owned(), json!("/tmp/kanji.txt")),
        ("anki.native_adapter".to_owned(), json!("other-protocol")),
    ]);
    let effective = resolve(
        &registry,
        &ConfigFile::default(),
        &ResolveOptions {
            flags,
            ..Default::default()
        },
    )
    .unwrap();
    let reported: BTreeMap<String, String> = coverage::unavailable_settings(&effective)
        .into_iter()
        .map(|v| {
            (
                v["key"].as_str().unwrap().to_owned(),
                v["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    for (key, status) in [
        ("browser.max_pages", "dormant_unavailable_feature"),
        ("dictionary.provider", "unavailable_value"),
        ("dictionary.url_template", "dormant_unavailable_feature"),
        ("dictionary.schema_path", "dormant_unavailable_feature"),
        ("selection.duplicate_policy", "unavailable_value"),
        ("llm.prompts.kanji", "dormant_unavailable_feature"),
        ("anki.native_adapter", "unavailable_value"),
    ] {
        assert_eq!(
            reported.get(key).map(String::as_str),
            Some(status),
            "{key}: {reported:?}"
        );
    }
    assert!(!reported.contains_key("network.allowed_remote_service_hosts"));
}

#[test]
fn checked_in_coverage_document_is_current() {
    let path = root().join("docs/cli/configuration/setting-coverage.md");
    assert_eq!(
        std::fs::read_to_string(path).unwrap_or_default(),
        coverage::markdown(),
        "run: cargo run --locked -q -p linguist-config --example coverage"
    );
}

#[test]
fn purpose_ocr_mapping_replaces_ocr_languages_but_explicit_overrides_win() {
    let registry = Registry::builtin();
    let file = ConfigFile::parse(
        "[config]\nversion = 2\n[purposes.japanese_vocab]\nocr_languages = [\"jpn\", \"vie\"]\n",
        &registry,
    )
    .unwrap();
    let options = |purpose: &str| ResolveOptions {
        purpose: Some(purpose.into()),
        ..Default::default()
    };
    let mapped = resolve(&registry, &file, &options("japanese_vocab")).unwrap();
    assert_eq!(mapped.values["ocr.languages"], json!(["jpn", "vie"]));
    assert_eq!(mapped.provenance["ocr.languages"], "purpose-mapping");
    let other = resolve(&registry, &file, &options("english_vocab")).unwrap();
    assert_eq!(other.values["ocr.languages"], json!(["eng"]));
    let overridden = ConfigFile::parse(
        "[config]\nversion = 2\n[purposes.japanese_vocab]\nocr_languages = [\"jpn\", \"vie\"]\n[purposes.japanese_vocab.overrides]\n\"ocr.languages\" = [\"jpn\"]\n",
        &registry,
    )
    .unwrap();
    let resolved = resolve(&registry, &overridden, &options("japanese_vocab")).unwrap();
    assert_eq!(resolved.values["ocr.languages"], json!(["jpn"]));
}
