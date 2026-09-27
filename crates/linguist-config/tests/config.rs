use linguist_config::*;
use serde_json::json;
use std::collections::BTreeMap;
fn file(text: &str) -> ConfigFile {
    ConfigFile::parse(text, &Registry::builtin()).unwrap()
}
#[test]
fn every_registry_default_validates_and_example_resolves() {
    let r = Registry::builtin();
    assert_eq!(r.entries.len(), 150);
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
