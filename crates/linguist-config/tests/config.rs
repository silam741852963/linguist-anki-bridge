use linguist_config::*;
use serde_json::json;
use std::collections::BTreeMap;
fn file(text: &str) -> ConfigFile {
    ConfigFile::parse(text, &Registry::builtin()).unwrap()
}
#[test]
fn every_registry_default_validates_and_example_resolves() {
    let r = Registry::builtin();
    assert_eq!(r.entries.len(), 156);
    for e in r.entries.values() {
        r.validate_value(&e.key, &e.default)
            .unwrap_or_else(|err| panic!("{}: {err}", e.key));
    }
    let config = file(include_str!(
        "../../../docs/cli/configuration/config.example.toml"
    ));
    resolve(&r, &config, &ResolveOptions::default()).unwrap();
}
#[test]
fn execution_settings_change_only_execution_fingerprint() {
    let registry = Registry::builtin();
    let config = ConfigFile::default();
    let base = resolve(&registry, &config, &ResolveOptions::default()).unwrap();
    let mut options = ResolveOptions::default();
    options.flags.extend([
        ("output.format".into(), json!("json")),
        ("retry.read_attempts".into(), json!(4)),
    ]);
    let execution = resolve(&registry, &config, &options).unwrap();
    assert_eq!(base.semantic_fingerprint, execution.semantic_fingerprint);
    assert_ne!(base.execution_fingerprint, execution.execution_fingerprint);
    assert_ne!(base.fingerprint, execution.fingerprint);
    options.flags.clear();
    options.flags.insert("llm.temperature".into(), json!(0.4));
    let semantic = resolve(&registry, &config, &options).unwrap();
    assert_ne!(base.semantic_fingerprint, semantic.semantic_fingerprint);
    assert_eq!(base.execution_fingerprint, semantic.execution_fingerprint);
}
#[test]
fn describe_rule_metadata_and_nearby_keys_track_the_resolver() {
    let registry = Registry::builtin();
    assert_eq!(registry.suggestions("llm.modle")[0], "llm.model");
    assert_eq!(
        registry.suggestions("purposes.study.fileds")[0],
        "purposes.study.fields"
    );
    assert!(
        cross_field_checks("llm.model")
            .iter()
            .any(|rule| rule["kind"] == "required_when" && rule["selector"] == "llm.enabled")
    );
    assert!(
        cross_field_checks("jobs.heartbeat_seconds")
            .iter()
            .any(|rule| rule["operator"] == "less_than_one_third_of")
    );
    assert!(
        cross_field_checks("anki.endpoint")
            .iter()
            .any(|rule| rule["kind"] == "remote_host_policy")
    );
}
#[test]
fn purpose_presets_inherit_but_explicit_base_wins() {
    let r = Registry::builtin();
    let options = ResolveOptions {
        purpose: Some("japanese_grammar".into()),
        ..Default::default()
    };
    let v = resolve(&r, &file("[config]\nversion=2"), &options).unwrap();
    assert_eq!(v.values["learning.explanation_language"], "vi");
    assert_eq!(
        v.provenance["learning.explanation_language"],
        "purpose-preset"
    );
    let v = resolve(
        &r,
        &file("[config]\nversion=2\n[learning]\nexplanation_language='en'"),
        &options,
    )
    .unwrap();
    assert_eq!(v.values["learning.explanation_language"], "en");
}
#[test]
fn all_override_layers_follow_precedence() {
    let r = Registry::builtin();
    let config = file(
        "[config]\nversion=2\n[llm]\ntemperature=0.1\n[profiles.study.overrides.llm]\ntemperature=0.2\n[purposes.japanese_vocab.overrides]\n'llm.temperature'=0.3\n",
    );
    let mut options = ResolveOptions {
        profile: Some("study".into()),
        purpose: Some("japanese_vocab".into()),
        ..Default::default()
    };
    assert_eq!(
        resolve(&r, &config, &options).unwrap().values["llm.temperature"],
        0.3
    );
    options
        .environment
        .insert("LAB_LLM__TEMPERATURE".into(), "0.4".into());
    assert_eq!(
        resolve(&r, &config, &options).unwrap().values["llm.temperature"],
        0.4
    );
    options.flags.insert("llm.temperature".into(), json!(0.5));
    let v = resolve(&r, &config, &options).unwrap();
    assert_eq!(v.values["llm.temperature"], 0.5);
    assert_eq!(v.provenance["llm.temperature"], "flag");
}
#[test]
fn rejects_unknown_duplicate_coercion_and_illegal_scope() {
    let r = Registry::builtin();
    for text in [
        "[config]\nversion=2\nwat=1",
        "[config]\nversion=2\n[profiles.x.overrides.jobs]\nprepare_workers=3",
        "[config]\nversion=2\n[learning]\nexamples_min='3'",
        "[config]\nversion=2\n[profiles.x.overrides]\n'llm.model'='a'\n[profiles.x.overrides.llm]\nmodel='b'",
    ] {
        assert!(ConfigFile::parse(text, &r).is_err(), "{text}");
    }
    assert!(r.parse_value("network.offline", "True").is_err());
    assert!(r.parse_value("ocr.languages", r#"["eng","eng"]"#).is_err());
    assert!(
        r.validate_value("purposes.x.card_tasks", &json!({"00":"comprehension"}))
            .is_err()
    );
    assert!(
        r.validate_value("purposes.x.fields", &json!({"invented":"Foo"}))
            .is_err()
    );
}
#[test]
fn url_templates_accept_only_declared_path_or_query_placeholders() {
    let registry = Registry::builtin();
    for (key, value) in [
        (
            "dictionary.url_template",
            "https://example.org/search/{word}",
        ),
        ("dictionary.url_template", "https://example.org/?q={word}"),
        (
            "kanji.url_template",
            "https://jisho.org/search/{char}%23kanji",
        ),
    ] {
        registry.validate_value(key, &json!(value)).unwrap();
    }
    for (key, value) in [
        ("dictionary.url_template", "https://example.org/search/term"),
        (
            "dictionary.url_template",
            "https://example.org/search/{char}",
        ),
        (
            "dictionary.url_template",
            "https://example.org/{word}/{other}",
        ),
        (
            "dictionary.url_template",
            "https://{word}.example.org/search",
        ),
        ("kanji.url_template", "https://jisho.org/search/{word}"),
        (
            "kanji.url_template",
            "https://jisho.org/search/{char}/{secret}",
        ),
    ] {
        assert!(
            registry.validate_value(key, &json!(value)).is_err(),
            "{key}: {value}"
        );
    }
}
#[test]
fn endpoint_policy_rejects_credentials_remote_and_offline_opt_in() {
    let r = Registry::builtin();
    let config = ConfigFile::default();
    let mut options = ResolveOptions::default();
    for endpoint in ["https://user:secret@localhost:8765", "https://example.com"] {
        options
            .flags
            .insert("anki.endpoint".into(), json!(endpoint));
        let error = resolve(&r, &config, &options).unwrap_err();
        assert!(!error.contains("secret"));
    }
    options.flags.insert(
        "network.allowed_remote_service_hosts".into(),
        json!(["example.com"]),
    );
    resolve(&r, &config, &options).unwrap();
    options.flags.insert("network.offline".into(), json!(true));
    assert!(resolve(&r, &config, &options).is_err());
}
#[test]
fn custom_template_hosts_require_explicit_network_permission() {
    let registry = Registry::builtin();
    let config = ConfigFile::default();
    let mut options = ResolveOptions::default();
    options.flags.extend([
        ("dictionary.provider".into(), json!("custom")),
        (
            "dictionary.url_template".into(),
            json!("https://example.org/search/{word}"),
        ),
        ("dictionary.schema_path".into(), json!("/tmp/schema.json")),
    ]);
    assert!(
        resolve(&registry, &config, &options)
            .unwrap_err()
            .contains("REMOTE_ENDPOINT_NOT_ALLOWED: dictionary.url_template")
    );
    options.flags.insert(
        "network.allowed_remote_service_hosts".into(),
        json!(["example.org"]),
    );
    resolve(&registry, &config, &options).unwrap();
    options.flags.insert("network.offline".into(), json!(true));
    assert!(resolve(&registry, &config, &options).is_err());

    options.flags.clear();
    options.flags.insert(
        "kanji.url_template".into(),
        json!("https://example.org/kanji/{char}"),
    );
    assert!(
        resolve(&registry, &config, &options)
            .unwrap_err()
            .contains("REMOTE_ENDPOINT_NOT_ALLOWED: kanji.url_template")
    );
    options.flags.insert(
        "network.allowed_remote_service_hosts".into(),
        json!(["example.org"]),
    );
    resolve(&registry, &config, &options).unwrap();
    options.flags.insert("network.offline".into(), json!(true));
    options.flags.insert(
        "kanji.url_template".into(),
        json!("http://127.0.0.1:1234/kanji/{char}"),
    );
    resolve(&registry, &config, &options).unwrap();
}
#[test]
fn cross_field_settings_and_env_errors_are_not_ignored() {
    let r = Registry::builtin();
    let mut options = ResolveOptions::default();
    options
        .flags
        .insert("jobs.heartbeat_seconds".into(), json!(20));
    assert!(resolve(&r, &ConfigFile::default(), &options).is_err());
    options.flags.clear();
    options
        .environment
        .insert("LAB_UNKNOWN__SETTING".into(), "secret".into());
    assert!(resolve(&r, &ConfigFile::default(), &options).is_err());
}
#[test]
fn selected_providers_require_their_declared_settings() {
    let registry = Registry::builtin();
    for (selector, selected, required) in [
        ("llm.enabled", json!(true), "llm.model"),
        (
            "dictionary.provider",
            json!("custom"),
            "dictionary.url_template",
        ),
        (
            "dictionary.provider",
            json!("custom"),
            "dictionary.schema_path",
        ),
        ("images.provider", json!("custom"), "images.custom_endpoint"),
        ("audio.provider", json!("custom"), "audio.endpoint"),
        ("audio.provider", json!("piper"), "audio.executable"),
        ("audio.provider", json!("piper"), "audio.voice_resource"),
        ("ocr.engine", json!("ollama"), "llm.vision_model"),
        ("ocr.engine", json!("paddleocr"), "ocr.resource_path"),
        ("browser.enabled", json!(true), "browser.executable"),
    ] {
        let mut options = ResolveOptions::default();
        options.flags.insert(selector.into(), selected);
        if required == "dictionary.schema_path" {
            options.flags.insert(
                "dictionary.url_template".into(),
                json!("http://127.0.0.1:1234/{word}"),
            );
        }
        if required == "audio.voice_resource" {
            options
                .flags
                .insert("audio.executable".into(), json!("piper"));
        }
        options.flags.insert(required.into(), json!(null));
        let error = resolve(&registry, &ConfigFile::default(), &options).unwrap_err();
        assert!(error.starts_with("PROVIDER_SETTING_REQUIRED"), "{error}");
        assert!(error.contains(required), "{error}");
    }
    let mut options = ResolveOptions::default();
    options.flags.extend([
        ("dictionary.provider".into(), json!("custom")),
        (
            "dictionary.url_template".into(),
            json!("http://127.0.0.1:1234/{word}"),
        ),
        ("dictionary.schema_path".into(), json!("/tmp/schema.json")),
        ("images.provider".into(), json!("custom")),
        (
            "images.custom_endpoint".into(),
            json!("http://127.0.0.1:1234"),
        ),
        ("audio.provider".into(), json!("piper")),
        ("audio.executable".into(), json!("piper")),
        ("audio.voice_resource".into(), json!("/tmp/voice.onnx")),
        ("ocr.engine".into(), json!("ollama")),
        ("llm.vision_model".into(), json!("vision:model")),
        ("browser.enabled".into(), json!(true)),
        ("browser.executable".into(), json!("firefox")),
    ]);
    resolve(&registry, &ConfigFile::default(), &options).unwrap();
}
#[test]
fn config_init_is_private_atomic_and_never_replaces() {
    let root = std::env::temp_dir().join(format!("lab-config-test-{}", uuid::Uuid::new_v4()));
    let path = root.join("config.toml");
    initialize(&path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[config]\nversion = 2\n"
    );
    assert!(initialize(&path).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn xdg_explicit_and_environment_path_precedence() {
    let env = BTreeMap::from([
        ("HOME".into(), "/home/test".into()),
        ("XDG_CONFIG_HOME".into(), "/tmp/config".into()),
    ]);
    assert_eq!(
        config_path(None, &env).unwrap(),
        std::path::Path::new("/tmp/config/linguist-anki-bridge/config.toml")
    );
    let mut env = env;
    env.insert("LAB_CONFIG".into(), "custom.toml".into());
    assert_eq!(
        config_path(None, &env).unwrap(),
        std::path::Path::new("custom.toml")
    );
    assert_eq!(
        config_path(Some(std::path::Path::new("explicit.toml")), &env).unwrap(),
        std::path::Path::new("explicit.toml")
    );
}
#[test]
fn path_expansion_is_explicit_and_never_shell_evaluated() {
    let env = BTreeMap::from([("HOME".into(), "/home/test".into())]);
    assert_eq!(
        expand_path("${XDG_STATE_HOME}/lab", &env).unwrap(),
        std::path::Path::new("/home/test/.local/state/lab")
    );
    assert_eq!(
        expand_path("~/a", &env).unwrap(),
        std::path::Path::new("/home/test/a")
    );
    for input in [
        "$(touch /tmp/never)",
        "${UNKNOWN}/a",
        "~someone/file",
        "$HOME/a",
    ] {
        assert!(expand_path(input, &env).is_err());
    }
    assert!(
        expand_path(
            "${XDG_CACHE_HOME}",
            &BTreeMap::from([("XDG_CACHE_HOME".into(), "relative".into())])
        )
        .is_err()
    );
}

#[test]
fn only_section_key_environment_names_are_overrides() {
    // RI-01: tool and credential variables that merely start with LAB_ are not settings.
    let r = Registry::builtin();
    let mut options = ResolveOptions::default();
    for (name, value) in [
        ("LAB_ANKI_PYTHON", "/usr/bin/python3.14"),
        ("LAB_SECRET", "s3cret"),
        ("LAB_ANKI__KEY", "credential"),
        ("LAB_LLM__TEMPERATURE", "0.4"),
    ] {
        options.environment.insert(name.into(), value.into());
    }
    options
        .flags
        .insert("anki.api_key_env".into(), json!("LAB_ANKI__KEY"));
    let effective = resolve(&r, &ConfigFile::default(), &options).unwrap();
    assert_eq!(effective.values["llm.temperature"], json!(0.4));
    assert!(
        effective
            .provenance
            .values()
            .all(|origin| !origin.contains("LAB_ANKI__KEY") && !origin.contains("LAB_SECRET"))
    );
    options
        .environment
        .insert("LAB_ANKI__ENDPONT".into(), "http://127.0.0.1:1".into());
    assert!(
        resolve(&r, &ConfigFile::default(), &options).is_err(),
        "typos still fail"
    );
}
