//! WP-15 final purpose presets. The four builtin purposes resolve to the
//! learning/provider decisions (docs/cli/decisions/learning-and-providers.md):
//! japanese_vocab ja/en/jpn+eng/Jisho, japanese_grammar ja/vi/jpn+eng+vie,
//! english_vocab en/en/eng/Wiktionary and english_grammar en/en/eng. Opt-in
//! tasks stay off, and no private deck, model or field mapping is builtin.
use linguist_config::{ConfigFile, Registry, ResolveOptions, builtin_purposes, resolve};
use serde_json::{Value, json};

fn resolved(purpose: &str) -> std::collections::BTreeMap<String, Value> {
    resolve(
        &Registry::builtin(),
        &ConfigFile::default(),
        &ResolveOptions {
            purpose: Some(purpose.into()),
            ..Default::default()
        },
    )
    .unwrap()
    .values
}

#[test]
fn builtin_purposes_are_the_four_final_presets() {
    let mut purposes = builtin_purposes();
    purposes.sort();
    assert_eq!(
        purposes,
        [
            "english_grammar",
            "english_vocab",
            "japanese_grammar",
            "japanese_vocab"
        ]
    );
    let file: Value = serde_json::from_slice(include_bytes!(
        "../../../docs/cli/configuration/purpose-defaults.json"
    ))
    .unwrap();
    assert_eq!(file["schema_version"], 1);
    // Presets are data and override only language, OCR packs and dictionary.
    for (purpose, preset) in file["presets"].as_object().unwrap() {
        for key in preset["overrides"].as_object().unwrap().keys() {
            assert!(
                [
                    "learning.explanation_language",
                    "ocr.languages",
                    "dictionary.provider"
                ]
                .contains(&key.as_str()),
                "{purpose}: {key}"
            );
        }
    }
}

#[test]
fn each_purpose_resolves_to_its_final_values() {
    let expected = [
        ("japanese_vocab", "ja", "en", json!(["jpn", "eng"]), "jisho"),
        (
            "japanese_grammar",
            "ja",
            "vi",
            json!(["jpn", "eng", "vie"]),
            "auto",
        ),
        ("english_vocab", "en", "en", json!(["eng"]), "wiktionary"),
        ("english_grammar", "en", "en", json!(["eng"]), "auto"),
    ];
    for (purpose, target, explanation, ocr, dictionary) in expected {
        let values = resolved(purpose);
        assert_eq!(
            values[&format!("purposes.{purpose}.target_language")],
            target,
            "{purpose}"
        );
        assert_eq!(
            values["learning.explanation_language"], explanation,
            "{purpose}"
        );
        assert_eq!(values["ocr.languages"], ocr, "{purpose}");
        assert_eq!(values["dictionary.provider"], dictionary, "{purpose}");
        // New vocabulary is Comprehension only; new grammar Recognition only.
        for key in [
            "learning.vocabulary.production",
            "learning.vocabulary.spelling",
            "learning.grammar.application",
        ] {
            assert_eq!(values[key], false, "{purpose}: {key}");
        }
        assert_eq!(values["learning.examples_min"], 3);
        assert_eq!(values["learning.generated_examples_max"], 5);
        // Private deck names, source models and mappings are never builtin.
        for suffix in ["source_deck", "target_deck", "source_model"] {
            assert!(
                values[&format!("purposes.{purpose}.{suffix}")].is_null(),
                "{purpose}: {suffix}"
            );
        }
        for suffix in ["fields", "card_tasks", "overrides"] {
            assert_eq!(
                values[&format!("purposes.{purpose}.{suffix}")],
                json!({}),
                "{purpose}: {suffix}"
            );
        }
        // Generation is opt-in per invocation evidence, with a pinned candidate.
        assert_eq!(values["llm.model"], "gemma4:12b", "{purpose}");
        assert_eq!(values["llm.temperature"], 0.0, "{purpose}");
        assert_eq!(values["browser.enabled"], false, "{purpose}");
    }
}

#[test]
fn explicit_user_values_override_presets() {
    let values = resolve(
        &Registry::builtin(),
        &ConfigFile::default(),
        &ResolveOptions {
            purpose: Some("japanese_grammar".into()),
            flags: [("learning.explanation_language".to_owned(), json!("en"))].into(),
            ..Default::default()
        },
    )
    .unwrap()
    .values;
    assert_eq!(values["learning.explanation_language"], "en");
    assert_eq!(values["ocr.languages"], json!(["jpn", "eng", "vie"]));
}
