//! OP-09/OP-10 legacy configuration import and guarded activation.
use linguist_config::legacy::{self, Status};
use std::collections::{BTreeMap, BTreeSet};

const FULL: &[u8] = include_bytes!("fixtures/legacy/python-full.yaml");

fn env() -> BTreeMap<String, String> {
    BTreeMap::from([("HOME".to_owned(), "/home/fixture".to_owned())])
}

fn record<'a>(import: &'a legacy::Import, key: &str) -> &'a legacy::KeyRecord {
    import
        .report
        .records
        .iter()
        .find(|r| r.legacy_key == key)
        .unwrap_or_else(|| panic!("no record for {key}"))
}

fn temp() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("lab-legacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    path
}

fn leaves(prefix: &str, value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value.as_object() {
        Some(map)
            if !["dictionary.schema", "kanji.schema"].contains(&prefix)
                && !prefix.ends_with(".fields") =>
        {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                leaves(&key, v, out);
            }
        }
        _ => {
            out.insert(prefix.to_owned());
        }
    }
}

#[test]
fn every_legacy_key_is_accounted_and_the_candidate_validates() {
    let import = legacy::import(FULL, &env()).unwrap();
    let text = std::str::from_utf8(&import.candidate_bytes).unwrap();
    let parsed =
        linguist_config::ConfigFile::parse(text, &linguist_config::Registry::builtin()).unwrap();
    assert_eq!(parsed.values, import.candidate.values);
    let source = legacy::parse_yaml(std::str::from_utf8(FULL).unwrap()).unwrap();
    let mut keys = BTreeSet::new();
    leaves("", &source, &mut keys);
    let recorded: BTreeSet<String> = import
        .report
        .records
        .iter()
        .filter(|r| !r.implicit_default)
        .map(|r| r.legacy_key.clone())
        .collect();
    for key in &keys {
        assert!(recorded.contains(key), "unaccounted legacy key {key}");
    }
    let values = &import.candidate.values;
    assert_eq!(values["config.version"], 2);
    assert_eq!(
        values["filters.remove_parentheses"], true,
        "explicit legacy true stays explicit"
    );
    assert_eq!(values["llm.model"], "gemma3:12b");
    assert_eq!(values["storage.backup_dir"], "/home/fixture/AnkiBackups");
    assert_eq!(values["purposes.japanese_vocab.source_deck"], "森の言葉");
    assert_eq!(
        values["purposes.japanese_grammar.fields"]["meaning"],
        "Explanation"
    );
    assert_eq!(
        values["purposes.japanese_vocab.fields"]["picture"],
        "Picture"
    );
    assert_eq!(
        values["purposes.japanese_vocab.ocr_languages"],
        serde_json::json!(["jpn", "eng", "vie"])
    );
    assert!(
        !values.contains_key("purposes.japanese_vocab.target_deck"),
        "destination never inferred"
    );
    assert!(
        !values.contains_key("dictionary.provider"),
        "no global jisho for English"
    );
    assert!(
        !values
            .keys()
            .any(|k| k.contains("taiwanese") || k.contains("german"))
    );
    assert_eq!(record(&import, "dry_run").status, Status::Retired);
    assert_eq!(
        record(&import, "decks.taiwanese_vocab.ocr_langs").status,
        Status::Unsupported
    );
    assert_eq!(
        record(&import, "config_version").status,
        Status::Transformed
    );
    // Unavailable features enabled in the legacy file need explicit acceptance.
    assert_eq!(
        import.report.blocking_keys,
        vec![
            "dictionary.browser_fallback",
            "image_classification.llm_adjudication"
        ]
    );
    assert!(import.report.activation_blocked);
    assert_eq!(values.get("dictionary.browser_fallback"), None);
    let counted: u64 = import.report.counts.values().sum();
    assert_eq!(counted as usize, import.report.records.len());
}

#[test]
fn legacy_default_prompts_are_retired_and_custom_prompts_retained_unchanged() {
    let import = legacy::import(FULL, &env()).unwrap();
    let vocab = record(&import, "llm.system_prompt_vocab");
    assert_eq!(vocab.status, Status::Retired);
    assert!(
        vocab.resource.is_some(),
        "the original default is exported for reference"
    );
    let custom_prompt = "Custom prompt: return {\"x\": 1}\nline two\twith tab";
    let text = format!(
        "config_version: 2\nllm:\n  system_prompt_vocab: {}\nkanji:\n  schema:\n    name: mine\n    fields: []\n",
        serde_json::to_string(custom_prompt).unwrap()
    );
    let import = legacy::import(text.as_bytes(), &env()).unwrap();
    let prompt = record(&import, "llm.system_prompt_vocab");
    assert_eq!(prompt.status, Status::Unresolved);
    assert!(prompt.blocking);
    assert_eq!(prompt.code, "LEGACY_PROMPT_CUSTOM");
    let resource = prompt.resource.clone().unwrap();
    let (_, bytes) = import
        .resources
        .iter()
        .find(|(r, _)| r.file == resource.file)
        .unwrap();
    assert_eq!(
        bytes,
        custom_prompt.as_bytes(),
        "original prompt text is retained byte-exactly"
    );
    assert_eq!(
        resource.sha256,
        linguist_core::canonical::asset_digest(custom_prompt.as_bytes())
    );
    assert!(
        !import
            .candidate
            .values
            .contains_key("llm.prompts.vocabulary")
    );
    let schema = record(&import, "kanji.schema");
    assert_eq!(schema.code, "LEGACY_SCHEMA_CUSTOM");
    assert!(schema.resource.as_ref().unwrap().file.ends_with(".json"));
    assert!(!import.candidate.values.contains_key("kanji.schema"));
}

#[test]
fn conflicting_retry_and_endpoint_defaults_are_unresolved() {
    let text = b"config_version: 2\ndictionary:\n  retry_backoff_seconds: 0.6\nbatch:\n  retry_backoff_seconds: 2.0\nllm:\n  ollama_url: http://127.0.0.1:11434\nocr:\n  ollama_url: http://127.0.0.1:11999\n";
    let import = legacy::import(text, &env()).unwrap();
    let retry = record(&import, "batch.retry_backoff_seconds");
    assert_eq!(
        (retry.status, retry.code.as_str()),
        (Status::Unresolved, "LEGACY_RETRY_CONFLICT")
    );
    assert!(retry.blocking);
    let endpoint = record(&import, "ocr.ollama_url");
    assert_eq!(endpoint.code, "LEGACY_ENDPOINT_CONFLICT");
    assert!(endpoint.blocking);
    assert_eq!(
        import.candidate.values.get("llm.endpoint"),
        None,
        "loopback default equals the v2 default"
    );
    // Matching values merge instead.
    let merged = legacy::import(
        b"dictionary:\n  retry_backoff_seconds: 2.0\nbatch:\n  retry_backoff_seconds: 2.0\n",
        &env(),
    )
    .unwrap();
    assert_eq!(
        record(&merged, "batch.retry_backoff_seconds").code,
        "LEGACY_RETRY_MERGED"
    );
    assert_eq!(
        merged.candidate.values["retry.initial_backoff_seconds"],
        2.0
    );
}

#[test]
fn unknown_keys_block_and_unsafe_yaml_is_refused() {
    let import = legacy::import(
        b"anki:\n  url: http://127.0.0.1:8765\nmystery:\n  knob: 3\n",
        &env(),
    )
    .unwrap();
    let unknown = record(&import, "mystery.knob");
    assert_eq!(unknown.status, Status::Unknown);
    assert!(unknown.blocking);
    assert!(
        import
            .report
            .blocking_keys
            .contains(&"mystery.knob".to_owned())
    );
    for (yaml, code) in [
        (
            "dry_run: !!python/object/apply:os.system ['true']\n",
            "LEGACY_YAML_UNSAFE",
        ),
        ("anki: !custom {url: x}\n", "LEGACY_YAML_UNSAFE"),
        ("base: &a {x: 1}\nother: *a\n", "LEGACY_YAML_UNSAFE"),
        ("base:\n  <<: {x: 1}\n", "LEGACY_YAML_UNSAFE"),
        ("? [a, b]\n: 1\n", "LEGACY_YAML_UNSAFE"),
        ("a: 1\na: 2\n", "LEGACY_YAML_DUPLICATE_KEY"),
        ("a: 1\n---\nb: 2\n", "LEGACY_YAML_UNSAFE"),
        (
            "batch:\n  max_attempts: 0x10\n",
            "LEGACY_YAML_AMBIGUOUS_SCALAR",
        ),
        ("- just\n- a list\n", "LEGACY_CONFIG_NOT_MAPPING"),
        ("config_version: 7\n", "LEGACY_CONFIG_VERSION_UNSUPPORTED"),
    ] {
        let error = legacy::import(yaml.as_bytes(), &env())
            .err()
            .unwrap_or_default();
        assert!(error.starts_with(code), "{yaml:?}: {error}");
    }
}

#[test]
fn credentials_and_remote_endpoints_are_not_carried() {
    let import = legacy::import(
        b"anki:\n  url: http://user:secret@127.0.0.1:8765\nllm:\n  ollama_url: https://ollama.example.org\n",
        &env(),
    )
    .unwrap();
    let anki = record(&import, "anki.url");
    assert_eq!(anki.code, "LEGACY_URL_CREDENTIALS");
    let report = serde_json::to_string(&import.report).unwrap();
    assert!(
        !report.contains("secret"),
        "credentials never reach the report"
    );
    let llm = record(&import, "llm.ollama_url");
    assert_eq!(llm.status, Status::Unresolved);
    assert!(
        llm.code
            .starts_with("LEGACY_VALUE_REJECTED_REMOTE_ENDPOINT_NOT_ALLOWED"),
        "{}",
        llm.code
    );
    assert!(!import.candidate.values.contains_key("llm.endpoint"));
}

#[test]
fn native_json_config_is_imported_with_the_same_accounting() {
    let native = serde_json::json!({
        "version": 1,
        "anki_url": "http://127.0.0.1:8765",
        "ollama_url": "http://127.0.0.1:11434",
        "ollama_model": "qwen3:8b",
        "dictionary_preset": "jisho",
        "dictionary_url_template": "",
        "dictionary_schema": null,
        "kanji_source_lang": "vietnamese",
        "dry_run": false,
        "decks": {"japanese": {"deck_name": "森", "model_name": "Words", "ocr_languages": "jpn+eng", "fields": {"expression": "Word", "meaning_text": "Meaning"}}},
        "future_field": 1
    });
    let import = legacy::import(native.to_string().as_bytes(), &env()).unwrap();
    assert_eq!(import.report.source_format, "native_json");
    let values = &import.candidate.values;
    assert_eq!(values["llm.model"], "qwen3:8b");
    assert_eq!(values["kanji.explanation_language"], "vi");
    assert_eq!(
        values["purposes.japanese_vocab.source_deck"], "森",
        "legacy bare name renamed"
    );
    assert_eq!(
        values["purposes.japanese_vocab.fields"]["meaning"],
        "Meaning"
    );
    assert_eq!(record(&import, "dry_run").status, Status::Retired);
    assert_eq!(
        record(&import, "dictionary_preset").status,
        Status::Transformed
    );
    assert_eq!(import.report.blocking_keys, vec!["future_field"]);
    let bad = serde_json::json!({"version": 2, "anki_url": "x"});
    assert!(
        legacy::import(bad.to_string().as_bytes(), &env())
            .err()
            .unwrap()
            .starts_with("LEGACY_CONFIG_VERSION_UNSUPPORTED")
    );
}

#[test]
fn outputs_never_touch_source_or_live_config_and_activation_is_guarded() {
    let dir = temp();
    let source = dir.join("config.yaml");
    std::fs::write(&source, FULL).unwrap();
    let before = std::fs::metadata(&source).unwrap().modified().unwrap();
    let live = dir.join("live/config.toml");
    let mut import = legacy::import(&std::fs::read(&source).unwrap(), &env()).unwrap();
    for bad in [source.clone(), live.clone()] {
        assert!(legacy::write_outputs(&mut import, &source, &bad, &live, true).is_err());
    }
    let candidate = dir.join("out/candidate.toml");
    let written = legacy::write_outputs(&mut import, &source, &candidate, &live, false).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), FULL);
    assert_eq!(
        std::fs::metadata(&source).unwrap().modified().unwrap(),
        before
    );
    assert!(!live.exists(), "import never activates");
    assert!(
        written
            .resources
            .iter()
            .all(|f| written.resource_directory.join(f).is_file())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&candidate).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(
        legacy::write_outputs(&mut import, &source, &candidate, &live, false)
            .unwrap_err()
            .starts_with("IMPORT_OUTPUT_EXISTS")
    );
    legacy::write_outputs(&mut import, &source, &candidate, &live, true).unwrap();
    // Every blocking key must be accepted explicitly; unknown acceptances fail.
    let error = legacy::check_activation(&candidate, &[]).unwrap_err();
    assert!(error.starts_with("IMPORT_ACTIVATION_BLOCKED"), "{error}");
    assert!(
        legacy::check_activation(&candidate, &["dry_run".into()])
            .unwrap_err()
            .starts_with("IMPORT_ACCEPTANCE_UNKNOWN")
    );
    let accepted = vec![
        "dictionary.browser_fallback".to_owned(),
        "image_classification.llm_adjudication".to_owned(),
    ];
    let (bytes, _, keys) = legacy::check_activation(&candidate, &accepted).unwrap();
    assert_eq!(keys, accepted);
    let preview = linguist_config::edit::activate(&live, &bytes, false, &env()).unwrap();
    assert!(!preview.executed && !live.exists());
    let receipt = linguist_config::edit::activate(&live, &bytes, true, &env()).unwrap();
    assert!(receipt.executed && receipt.created && receipt.backup.is_none());
    assert_eq!(std::fs::read(&live).unwrap(), bytes);
    // A second activation over an existing live file keeps a byte-exact backup.
    std::fs::write(&live, b"[llm]\nmodel = \"old\"\n[config]\nversion = 2\n").unwrap();
    let receipt = linguist_config::edit::activate(&live, &bytes, true, &env()).unwrap();
    let backup = receipt.backup.unwrap();
    assert_eq!(
        std::fs::read(backup).unwrap(),
        b"[llm]\nmodel = \"old\"\n[config]\nversion = 2\n"
    );
    assert!(receipt.changed_keys.contains(&"llm.model".to_owned()));
    // A tampered candidate no longer matches its report.
    std::fs::write(&candidate, b"[config]\nversion = 2\n").unwrap();
    assert!(
        legacy::check_activation(&candidate, &accepted)
            .unwrap_err()
            .starts_with("IMPORT_CANDIDATE_CHANGED")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn activation_cannot_relocate_nonempty_state() {
    let dir = temp();
    let state = dir.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("state.sqlite3"), b"x").unwrap();
    let live = dir.join("config.toml");
    std::fs::write(
        &live,
        format!(
            "[config]\nversion = 2\n[storage]\nstate_dir = \"{}\"\n",
            state.display()
        ),
    )
    .unwrap();
    let candidate = b"[config]\nversion = 2\n";
    let error = linguist_config::edit::activate(&live, candidate, true, &env()).unwrap_err();
    assert!(error.starts_with("STORAGE_RELOCATION_BLOCKED"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}
