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
    let values = flat_yaml(contents)?;
    let mut config = NativeConfig {
        version: NATIVE_CONFIG_VERSION,
        anki_url: value(&values, "anki.url"),
        ollama_url: value(&values, "llm.ollama_url"),
        ollama_model: optional(&values, "llm.model"),
        dictionary_preset: value(&values, "dictionary.preset"),
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
pub fn native_config_path(config_root: &Path) -> PathBuf {
    config_root
        .join("linguist-anki-bridge")
        .join("native-config-v1.json")
}
fn flat_yaml(contents: &str) -> Result<BTreeMap<String, String>, ConfigError> {
    let mut result = BTreeMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (number, line) in contents.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let Some((key, raw)) = trimmed.split_once(':') else {
            return Err(ConfigError::Parse(format!(
                "line {} is not a YAML mapping",
                number + 1
            )));
        };
        while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
            stack.pop();
        }
        let key = key.trim();
        if key.is_empty() {
            return Err(ConfigError::Parse(format!(
                "line {} has an empty key",
                number + 1
            )));
        }
        let value = raw.trim().trim_matches('"').trim_matches('\'').to_owned();
        let mut path = stack.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>();
        path.push(key.into());
        if value.is_empty() {
            stack.push((indent, key.into()))
        } else {
            result.insert(path.join("."), value);
        }
    }
    Ok(result)
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
}
