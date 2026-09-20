//! Read-only import of the Python YAML config into a versioned native file.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub const NATIVE_CONFIG_VERSION: u16 = 1;
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeConfig {
    pub version: u16,
    pub anki_url: String,
    pub ollama_url: String,
    pub ollama_model: Option<String>,
    pub dictionary_preset: String,
    #[serde(default)]
    pub dictionary_url_template: String,
    #[serde(default)]
    pub dictionary_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub kanji_source_lang: String,
    pub dry_run: bool,
    pub decks: BTreeMap<String, DeckConfig>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeckConfig {
    pub deck_name: Option<String>,
    pub model_name: Option<String>,
    pub fields: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportReport {
    pub config: NativeConfig,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
pub enum ConfigError {
    Read(String),
    Parse(String),
    UnsupportedVersion(u16),
    NativeExists(PathBuf),
}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(v) | Self::Parse(v) => f.write_str(v),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported native config version {version}")
            }
            Self::NativeExists(v) => write!(f, "native config already exists: {}", v.display()),
        }
    }
}
pub fn load_native(path: &Path) -> Result<NativeConfig, ConfigError> {
    let bytes = fs::read(path).map_err(|error| ConfigError::Read(error.to_string()))?;
    let config: NativeConfig =
        serde_json::from_slice(&bytes).map_err(|error| ConfigError::Parse(error.to_string()))?;
    if config.version != NATIVE_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(config.version));
    }
    Ok(config)
}
impl std::error::Error for ConfigError {}
pub fn import_legacy_yaml(contents: &str) -> Result<ImportReport, ConfigError> {
    let document: serde_json::Value =
        serde_yaml_ng::from_str(contents).map_err(|error| ConfigError::Parse(error.to_string()))?;
    if !document.is_object() {
        return Err(ConfigError::Parse(
            "legacy config must be a YAML mapping".into(),
        ));
    }
    let mut values = BTreeMap::new();
    flatten_values(&document, "", &mut values);
    let mut config = NativeConfig {
        version: NATIVE_CONFIG_VERSION,
        anki_url: value(&values, "anki.url"),
        ollama_url: value(&values, "llm.ollama_url"),
        ollama_model: optional(&values, "llm.model"),
        dictionary_preset: value(&values, "dictionary.preset"),
        dictionary_url_template: value(&values, "dictionary.url_template"),
        dictionary_schema: document
            .pointer("/dictionary/schema")
            .filter(|schema| !schema.is_null() && **schema != serde_json::json!({}))
            .cloned(),
        kanji_source_lang: value(&values, "kanji.source_lang"),
        dry_run: matches!(value(&values, "dry_run").as_str(), "true" | "True" | "TRUE"),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    for (path, value) in &values {
        let parts = path.split('.').collect::<Vec<_>>();
        if parts.len() >= 3 && parts[0] == "decks" {
            let deck = config.decks.entry(parts[1].into()).or_default();
            match parts[2] {
                "deck_name" => deck.deck_name = nonempty(value),
                "note_type" => deck.model_name = nonempty(value),
                "fields" if parts.len() == 4 => {
                    deck.fields.insert(parts[3].into(), value.clone());
                }
                _ => {}
            }
        }
    }
    if config.anki_url.is_empty() {
        warnings.push("Legacy anki.url is missing".into())
    }
    if config.decks.is_empty() {
        warnings.push("Legacy config has no deck mappings".into())
    }
    Ok(ImportReport { config, warnings })
}
/// Write only a native file. The legacy YAML input is never opened for write.
pub fn save_native_new(path: &Path, config: &NativeConfig) -> Result<(), ConfigError> {
    if path.exists() {
        return Err(ConfigError::NativeExists(path.into()));
    }
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| ConfigError::Parse(e.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Read("native config has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|e| ConfigError::Read(e.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|e| ConfigError::Read(e.to_string()))?;
    fs::rename(temporary, path).map_err(|e| ConfigError::Read(e.to_string()))
}
/// Atomically replace only the native config. Legacy input is never opened here.
pub fn save_native_replace(path: &Path, config: &NativeConfig) -> Result<(), ConfigError> {
    if config.version != NATIVE_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(config.version));
    }
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| ConfigError::Parse(e.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Read("native config has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|e| ConfigError::Read(e.to_string()))?;
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let mut file = fs::File::create(&temporary).map_err(|e| ConfigError::Read(e.to_string()))?;
    use std::io::Write;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| ConfigError::Read(e.to_string()))?;
    fs::rename(&temporary, path).map_err(|e| ConfigError::Read(e.to_string()))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| ConfigError::Read(e.to_string()))
}
pub fn native_config_path(config_root: &Path) -> PathBuf {
    config_root
        .join("linguist-anki-bridge")
        .join("native-config-v1.json")
}
fn flatten_values(value: &serde_json::Value, path: &str, result: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::Object(mapping) => {
            for (key, value) in mapping {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten_values(value, &child, result);
            }
        }
        serde_json::Value::String(value) => {
            result.insert(path.into(), value.clone());
        }
        serde_json::Value::Null => {}
        value if value.is_boolean() || value.is_number() => {
            result.insert(path.into(), value.to_string());
        }
        _ => {}
    }
}
fn value(values: &BTreeMap<String, String>, key: &str) -> String {
    values.get(key).cloned().unwrap_or_default()
}
fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty() && !matches!(value, "null" | "None" | "~")).then(|| value.into())
}
fn optional(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    values.get(key).and_then(|value| nonempty(value))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn imports_known_legacy_settings_without_mutating_yaml() {
        let source = "anki:\n  url: http://localhost:8765\nllm:\n  ollama_url: http://localhost:11434\n  model: llama\ndictionary:\n  preset: jisho\ndry_run: true\ndecks:\n  japanese_vocab:\n    deck_name: Japanese\n    note_type: Picture Words\n    fields:\n      expression: Word\n";
        let report = import_legacy_yaml(source).unwrap();
        assert_eq!(report.config.version, 1);
        assert_eq!(
            report.config.decks["japanese_vocab"].fields["expression"],
            "Word"
        );
        assert_eq!(source.lines().count(), 14);
    }
    #[test]
    fn imports_custom_dictionary_schema_and_preserves_old_native_files() {
        let source = "dictionary:\n  preset: custom\n  url_template: 'https://example.test/search/{word}'\n  schema:\n    baseSelector: .entry\n    fields:\n      - name: definition\n        selector: .sense\n        multiple: true\n      - name: audio_url\n        selector: audio\n        type: attribute\n        attribute: src\n";
        let report = import_legacy_yaml(source).unwrap();
        assert_eq!(report.config.dictionary_preset, "custom");
        assert_eq!(
            report.config.dictionary_url_template,
            "https://example.test/search/{word}"
        );
        let schema = report.config.dictionary_schema.unwrap();
        assert_eq!(schema["baseSelector"], ".entry");
        assert_eq!(schema["fields"][0]["multiple"], true);
        assert_eq!(schema["fields"][1]["attribute"], "src");
        let old = r#"{"version":1,"anki_url":"","ollama_url":"","ollama_model":null,"dictionary_preset":"jisho","dry_run":false,"decks":{}}"#;
        let loaded: NativeConfig = serde_json::from_str(old).unwrap();
        assert!(loaded.dictionary_schema.is_none());
        assert!(loaded.dictionary_url_template.is_empty());
        assert!(loaded.kanji_source_lang.is_empty());
    }
    #[test]
    fn imports_kanji_language_choice() {
        let report = import_legacy_yaml("kanji:\n  source_lang: vietnamese\n").unwrap();
        assert_eq!(report.config.kanji_source_lang, "vietnamese");
    }
    #[test]
    fn rejects_malformed_and_never_overwrites_native() {
        assert!(import_legacy_yaml("- list").is_err());
        let path = std::env::temp_dir().join(format!(
            "config-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = NativeConfig {
            version: 1,
            ..Default::default()
        };
        save_native_new(&path, &config).unwrap();
        assert_eq!(load_native(&path).unwrap(), config);
        assert!(matches!(
            save_native_new(&path, &config),
            Err(ConfigError::NativeExists(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn native_loader_rejects_unknown_schema_versions() {
        let path = std::env::temp_dir().join(format!(
            "config-version-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, r#"{"version":99,"anki_url":"","ollama_url":"","ollama_model":null,"dictionary_preset":"","dry_run":true,"decks":{}}"#).unwrap();
        assert!(matches!(
            load_native(&path),
            Err(ConfigError::UnsupportedVersion(99))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn native_replace_is_versioned_and_replaces_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "config-replace-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = NativeConfig {
            version: NATIVE_CONFIG_VERSION,
            ..Default::default()
        };
        save_native_replace(&path, &config).unwrap();
        config.anki_url = "http://localhost:8765".into();
        save_native_replace(&path, &config).unwrap();
        assert_eq!(load_native(&path).unwrap(), config);
        config.version = 99;
        assert!(matches!(
            save_native_replace(&path, &config),
            Err(ConfigError::UnsupportedVersion(99))
        ));
        let _ = fs::remove_file(path);
    }
}
