//! ALG-CONFIG: registry-backed configuration. No shell expansion or service discovery.
pub mod coverage;
pub mod legacy;
pub mod resources;
pub mod schema;
pub use linguist_core::records::execution_setting;
use linguist_core::{canonical, document::Language};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub default: Value,
    pub constraints: Value,
    pub scope: String,
    pub consumer: String,
    pub description: String,
    pub sensitive: bool,
}
#[derive(Clone, Debug)]
pub struct Registry {
    pub entries: BTreeMap<String, Entry>,
}
fn name_valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
impl Registry {
    pub fn builtin() -> Self {
        let entries: Vec<Entry> =
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/registry.json")))
                .expect("compiled registry");
        Self {
            entries: entries.into_iter().map(|e| (e.key.clone(), e)).collect(),
        }
    }
    pub fn lookup(&self, key: &str) -> Result<&Entry> {
        if let Some(e) = self.entries.get(key) {
            return Ok(e);
        }
        for prefix in ["purposes", "profiles"] {
            let parts: Vec<_> = key.split('.').collect();
            if parts.len() == 3 && parts[0] == prefix && name_valid(parts[1]) {
                let pattern = format!(
                    "{prefix}.{}.{}",
                    if prefix == "purposes" {
                        "<purpose>"
                    } else {
                        "<name>"
                    },
                    parts[2]
                );
                if let Some(e) = self.entries.get(&pattern) {
                    return Ok(e);
                }
            }
        }
        Err(format!("UNKNOWN_SETTING: {key}"))
    }
    /// Return up to three nearby registered names; concrete profile/purpose
    /// spelling is retained when the lookup was for a named mapping.
    pub fn suggestions(&self, key: &str) -> Vec<String> {
        if key.len() > 128 {
            return Vec::new();
        }
        let parts: Vec<_> = key.split('.').collect();
        let (pattern, name) = if parts.len() == 3
            && matches!(parts[0], "purposes" | "profiles")
            && name_valid(parts[1])
        {
            (
                format!(
                    "{}.<{}>.{}",
                    parts[0],
                    if parts[0] == "purposes" {
                        "purpose"
                    } else {
                        "name"
                    },
                    parts[2]
                ),
                Some(parts[1]),
            )
        } else {
            (key.to_owned(), None)
        };
        let mut candidates: Vec<_> = self
            .entries
            .keys()
            .map(|candidate| {
                (
                    edit_distance(pattern.as_bytes(), candidate.as_bytes()),
                    candidate
                        .replace("<purpose>", name.unwrap_or("<purpose>"))
                        .replace("<name>", name.unwrap_or("<name>")),
                )
            })
            .collect();
        candidates.sort();
        candidates.into_iter().take(3).map(|(_, key)| key).collect()
    }
    pub fn parse_value(&self, key: &str, input: &str) -> Result<Value> {
        let entry = self.lookup(key)?;
        let value = match entry.value_type.as_str() {
            "string" | "string|null" | "enum" => Value::String(input.into()),
            _ => canonical::parse(input.as_bytes())
                .map_err(|_| format!("INVALID_TYPED_VALUE: {key}"))?,
        };
        self.validate_value(key, &value)?;
        Ok(value)
    }
    pub fn validate_value(&self, key: &str, value: &Value) -> Result<()> {
        let e = self.lookup(key)?;
        let fail = || {
            format!(
                "INVALID_SETTING: {key} expects {} within {:?}",
                e.value_type, e.constraints
            )
        };
        if value.is_null() {
            return if e.value_type == "string|null" {
                Ok(())
            } else {
                Err(fail())
            };
        }
        let typed = match e.value_type.as_str() {
            "boolean" => value.is_boolean(),
            "integer" => value.as_i64().is_some(),
            "number" => value.as_f64().is_some_and(f64::is_finite),
            "string" | "string|null" | "enum" => value.is_string(),
            "string[]" => value
                .as_array()
                .is_some_and(|a| a.len() <= 4096 && a.iter().all(Value::is_string)),
            "field_map" | "task_map" | "override_map" => value.is_object(),
            _ => false,
        };
        if !typed {
            return Err(fail());
        }
        if let Some(number) = value.as_f64()
            && (e
                .constraints
                .get("min")
                .and_then(Value::as_f64)
                .is_some_and(|v| number < v)
                || e.constraints
                    .get("max")
                    .and_then(Value::as_f64)
                    .is_some_and(|v| number > v))
        {
            return Err(fail());
        }
        if let Some(choices) = e.constraints.get("values").and_then(Value::as_array)
            && e.value_type == "enum"
            && !choices.contains(value)
        {
            return Err(fail());
        }
        if let Some(text) = value.as_str() {
            let format = e
                .constraints
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("text");
            check_format(format, text).map_err(|_| fail())?;
            if format == "url_template" {
                check_url_template(key, text).map_err(|_| fail())?;
            }
        }
        if let Some(items) = value.as_array() {
            if e.constraints["unique"] == true
                && items
                    .iter()
                    .map(Value::to_string)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != items.len()
            {
                return Err(fail());
            }
            if let Some(allowed) = e.constraints.get("items_values").and_then(Value::as_array)
                && items.iter().any(|item| !allowed.contains(item))
            {
                return Err(fail());
            }
            for item in items {
                check_format(
                    e.constraints
                        .get("items_format")
                        .and_then(Value::as_str)
                        .unwrap_or("text"),
                    item.as_str().unwrap(),
                )
                .map_err(|_| fail())?;
            }
        }
        if let Some(map) = value.as_object() {
            if map.len() > 4096 {
                return Err(fail());
            }
            match e.value_type.as_str() {
                "override_map" => {
                    for (k, v) in map {
                        let entry = self.lookup(k)?;
                        if entry.scope != "purpose" {
                            return Err(format!("INVALID_OVERRIDE_SCOPE: {k}"));
                        }
                        self.validate_value(k, v)?;
                    }
                }
                "field_map" => {
                    for (k, v) in map {
                        if !e.constraints["keys"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(k))
                            || !v
                                .as_str()
                                .is_some_and(|s| !s.trim().is_empty() && !s.contains('\0'))
                        {
                            return Err(fail());
                        }
                    }
                }
                "task_map" => {
                    let mut tasks = BTreeSet::new();
                    for (k, v) in map {
                        if k.is_empty()
                            || !k.bytes().all(|b| b.is_ascii_digit())
                            || (k.len() > 1 && k.starts_with('0'))
                            || k.parse::<u16>().is_err()
                            || !e.constraints["values"].as_array().unwrap().contains(v)
                            || !tasks.insert(v.to_string())
                        {
                            return Err(fail());
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn defaults(&self) -> BTreeMap<String, Value> {
        let mut values: BTreeMap<_, _> = self
            .entries
            .values()
            .filter(|e| !e.key.contains('<'))
            .map(|e| (e.key.clone(), e.default.clone()))
            .collect();
        for purpose in presets()["presets"].as_object().unwrap().keys() {
            for e in self
                .entries
                .values()
                .filter(|e| e.key.starts_with("purposes.<purpose>."))
            {
                values.insert(e.key.replace("<purpose>", purpose), e.default.clone());
            }
        }
        values
    }
}
fn edit_distance(left: &[u8], right: &[u8]) -> usize {
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, &a) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, &b) in right.iter().enumerate() {
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + usize::from(a != b));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}
fn check_url_template(key: &str, template: &str) -> Result<()> {
    let placeholder = match key {
        "dictionary.url_template" => "{word}",
        "kanji.url_template" => "{char}",
        _ => return Err("undeclared URL template".into()),
    };
    if !template.contains(placeholder) {
        return Err("required placeholder missing".into());
    }
    let authority = template
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(rest))
        .ok_or("invalid URL template")?;
    if authority.contains(['{', '}']) {
        return Err("placeholder in URL authority".into());
    }
    let resolved = template.replace(placeholder, "x");
    if resolved.contains(['{', '}']) {
        return Err("unknown URL placeholder".into());
    }
    check_format("url", &resolved)
}
fn check_format(format: &str, s: &str) -> Result<()> {
    if s.contains('\0') || s.len() > 1024 * 1024 {
        return Err("invalid string".into());
    }
    let valid = match format {
        "text" => true,
        "nonempty" => !s.trim().is_empty(),
        "language" => Language::try_from(s.to_owned()).is_ok(),
        "env_name" => {
            !s.is_empty()
                && (s.as_bytes()[0].is_ascii_alphabetic() || s.starts_with('_'))
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        "path" | "resource_ref" => !s.trim().is_empty() && !s.contains(['\n', '\r']),
        "executable" => {
            !s.is_empty()
                && !s.chars().any(char::is_whitespace)
                && !s.contains(['$', '`', ';', '|', '&'])
        }
        "url" | "url_template" => {
            let u = url::Url::parse(s).map_err(|_| "invalid URL")?;
            matches!(u.scheme(), "http" | "https")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && u.fragment().is_none()
        }
        "host" => {
            let u = url::Url::parse(&format!("https://{s}/")).map_err(|_| "invalid host")?;
            u.host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case(s.trim_matches(['[', ']'])))
                && u.port().is_none()
                && u.username().is_empty()
                && u.password().is_none()
                && u.path() == "/"
        }
        "duration" => {
            s == "-1"
                || s == "0"
                || ["ms", "s", "m", "h"].iter().any(|suffix| {
                    s.strip_suffix(suffix)
                        .is_some_and(|n| n.parse::<f64>().is_ok_and(|v| v.is_finite() && v >= 0.0))
                })
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid format".into())
    }
}
fn presets() -> Value {
    serde_json::from_str(include_str!(
        "../../../docs/cli/configuration/purpose-defaults.json"
    ))
    .expect("compiled presets")
}
pub fn builtin_purposes() -> Vec<String> {
    presets()["presets"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}
#[derive(Clone, Debug, Default)]
pub struct ConfigFile {
    pub values: BTreeMap<String, Value>,
}
impl ConfigFile {
    pub fn parse(text: &str, registry: &Registry) -> Result<Self> {
        let table: toml::Table = toml::from_str(text)
            .map_err(|_| "INVALID_TOML: malformed or duplicate configuration keys".to_owned())?;
        let mut values = BTreeMap::new();
        flatten(
            "",
            &serde_json::to_value(table).map_err(|e| e.to_string())?,
            registry,
            &mut values,
        )?;
        if values.get("config.version") != Some(&json!(2)) {
            return Err("UNSUPPORTED_CONFIG_VERSION: config.version = 2 is required".into());
        }
        for (k, v) in &values {
            registry.validate_value(k, v)?;
        }
        Ok(Self { values })
    }
    pub fn read(path: &Path, registry: &Registry) -> Result<Self> {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|_| "CONFIG_IO: unable to read selected configuration".to_owned())?;
        if !file.metadata().map_err(|_| "CONFIG_IO")?.is_file() {
            return Err("CONFIG_NOT_REGULAR_FILE".into());
        }
        let mut data = Vec::new();
        file.take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|_| "CONFIG_IO: unable to read selected configuration".to_owned())?;
        if data.len() > 4 * 1024 * 1024 {
            return Err("CONFIG_TOO_LARGE".into());
        }
        Self::parse(
            std::str::from_utf8(&data).map_err(|_| "CONFIG_ENCODING")?,
            registry,
        )
    }
}
fn flatten(
    prefix: &str,
    value: &Value,
    registry: &Registry,
    out: &mut BTreeMap<String, Value>,
) -> Result<()> {
    if let Ok(entry) = registry.lookup(prefix) {
        let result = if entry.value_type == "override_map" {
            let mut nested = BTreeMap::new();
            flatten_overrides("", value, registry, &mut nested)?;
            serde_json::to_value(nested).unwrap()
        } else {
            value.clone()
        };
        if out.insert(prefix.into(), result).is_some() {
            return Err(format!("DUPLICATE_SETTING: {prefix}"));
        }
        return Ok(());
    }
    if let Some(table) = value.as_object() {
        if table.is_empty() && !prefix.is_empty() {
            return Err(format!("UNKNOWN_SETTING: {prefix}"));
        }
        for (k, v) in table {
            flatten(
                &if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                },
                v,
                registry,
                out,
            )?;
        }
        Ok(())
    } else {
        Err(format!("UNKNOWN_SETTING: {prefix}"))
    }
}
fn flatten_overrides(
    prefix: &str,
    value: &Value,
    registry: &Registry,
    out: &mut BTreeMap<String, Value>,
) -> Result<()> {
    if let Ok(entry) = registry.lookup(prefix) {
        if entry.scope != "purpose" {
            return Err(format!("INVALID_OVERRIDE_SCOPE: {prefix}"));
        }
        if out.insert(prefix.into(), value.clone()).is_some() {
            return Err(format!("DUPLICATE_SETTING: {prefix}"));
        }
        return Ok(());
    }
    if let Some(table) = value.as_object() {
        if table.is_empty()
            && !prefix.is_empty()
            && !registry
                .entries
                .values()
                .any(|e| e.scope == "purpose" && e.key.starts_with(&format!("{prefix}.")))
        {
            return Err(format!("UNKNOWN_SETTING: {prefix}"));
        }
        for (k, v) in table {
            flatten_overrides(
                &if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                },
                v,
                registry,
                out,
            )?;
        }
        Ok(())
    } else {
        Err(format!("UNKNOWN_SETTING: {prefix}"))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Effective {
    pub version: u16,
    pub values: BTreeMap<String, Value>,
    pub provenance: BTreeMap<String, String>,
    pub fingerprint: String,
    pub semantic_fingerprint: String,
    pub execution_fingerprint: String,
}
pub fn setting_fingerprints(values: &BTreeMap<String, Value>) -> Result<(String, String)> {
    linguist_core::records::setting_fingerprints(values).map_err(|e| e.to_string())
}
#[derive(Default)]
pub struct ResolveOptions {
    pub profile: Option<String>,
    pub purpose: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub flags: BTreeMap<String, Value>,
}
pub fn resolve(
    registry: &Registry,
    file: &ConfigFile,
    options: &ResolveOptions,
) -> Result<Effective> {
    let mut values = registry.defaults();
    let mut provenance: BTreeMap<_, _> = values
        .keys()
        .map(|k| (k.clone(), "builtin".into()))
        .collect();
    let presets = presets();
    if let Some(purpose) = &options.purpose {
        let preset = presets["presets"]
            .get(purpose)
            .ok_or_else(|| format!("UNSUPPORTED_PURPOSE: {purpose}"))?;
        values.insert(
            format!("purposes.{purpose}.target_language"),
            preset["target_language"].clone(),
        );
        provenance.insert(
            format!("purposes.{purpose}.target_language"),
            "purpose-preset".into(),
        );
        for (k, v) in preset["overrides"].as_object().unwrap() {
            values.insert(k.clone(), v.clone());
            provenance.insert(k.clone(), "purpose-preset".into());
        }
    }
    for (k, v) in &file.values {
        values.insert(k.clone(), v.clone());
        provenance.insert(k.clone(), "file".into());
    }
    let profile = options
        .profile
        .as_ref()
        .or_else(|| options.environment.get("LAB_PROFILE"))
        .cloned()
        .or_else(|| {
            values
                .get("config.default_profile")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    // A non-empty purpose OCR mapping replaces ocr.languages for that purpose;
    // explicit purpose overrides, environment and flags still win.
    let mut ocr_mapping = None;
    if let Some(purpose) = &options.purpose
        && let Some(languages) = file
            .values
            .get(&format!("purposes.{purpose}.ocr_languages"))
            .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
    {
        ocr_mapping = Some(languages.clone());
    }
    for (key, origin) in profile
        .as_ref()
        .map(|p| (format!("profiles.{p}.overrides"), "profile"))
        .into_iter()
        .chain(
            options
                .purpose
                .as_ref()
                .map(|p| (format!("purposes.{p}.overrides"), "purpose")),
        )
    {
        if !name_valid(key.split('.').nth(1).unwrap()) {
            return Err("INVALID_RECORD_NAME".into());
        }
        let record = file.values.get(&key);
        if origin == "profile" && record.is_none() {
            return Err("UNKNOWN_PROFILE".into());
        }
        if origin == "purpose"
            && let Some(languages) = ocr_mapping.take()
        {
            values.insert("ocr.languages".into(), languages);
            provenance.insert("ocr.languages".into(), "purpose-mapping".into());
        }
        if let Some(record) = record {
            for (k, v) in record.as_object().unwrap() {
                values.insert(k.clone(), v.clone());
                provenance.insert(k.clone(), origin.into());
            }
        }
    }
    // RI-01: only `LAB_SECTION__KEY` names are setting overrides. Other
    // `LAB_*` variables (tool variables such as LAB_ANKI_PYTHON, or a
    // credential named by a `*.api_key_env` setting) are not settings.
    let credential_names: BTreeSet<String> = values
        .iter()
        .chain(options.flags.iter())
        .filter(|(key, _)| key.ends_with(".api_key_env"))
        .filter_map(|(_, value)| value.as_str().map(str::to_owned))
        .collect();
    for (name, text) in &options.environment {
        if !name.starts_with("LAB_")
            || !name[4..].contains("__")
            || matches!(name.as_str(), "LAB_CONFIG" | "LAB_PROFILE")
            || credential_names.contains(name)
        {
            continue;
        }
        let key = name[4..].to_ascii_lowercase().replace("__", ".");
        let e = registry.lookup(&key)?;
        if e.scope == "mapping" || key.contains('<') {
            return Err(format!("ENV_MAPPING_UNSUPPORTED: {name}"));
        }
        values.insert(key.clone(), registry.parse_value(&key, text)?);
        provenance.insert(key, format!("environment:{name}"));
    }
    for (k, v) in &options.flags {
        registry.validate_value(k, v)?;
        values.insert(k.clone(), v.clone());
        provenance.insert(k.clone(), "flag".into());
    }
    validate_effective(registry, &values)?;
    let fingerprint = canonical::digest("resolved-settings", &values).map_err(|e| e.to_string())?;
    let (semantic_fingerprint, execution_fingerprint) = setting_fingerprints(&values)?;
    Ok(Effective {
        version: 2,
        values,
        provenance,
        fingerprint,
        semantic_fingerprint,
        execution_fingerprint,
    })
}
const NUMERIC_RELATIONS: [(&str, &str, bool); 3] = [
    ("jobs.heartbeat_seconds", "jobs.lease_seconds", true),
    (
        "retry.initial_backoff_seconds",
        "retry.max_backoff_seconds",
        false,
    ),
    (
        "learning.examples_min",
        "learning.generated_examples_max",
        false,
    ),
];
fn provider_requirements() -> Vec<(&'static str, Value, &'static str)> {
    vec![
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
    ]
}
/// Machine-readable cross-field rules involving a registered setting.
pub fn cross_field_checks(key: &str) -> Vec<Value> {
    let mut checks = Vec::new();
    for (left, right, third) in NUMERIC_RELATIONS {
        if key == left || key == right {
            checks.push(json!({"kind":"numeric_relation","left":left,"operator":if third {"less_than_one_third_of"} else {"at_most"},"right":right}));
        }
    }
    if matches!(key, "llm.max_output_tokens" | "llm.context_tokens") {
        checks.push(json!({"kind":"numeric_relation","left":"llm.max_output_tokens","operator":"less_than","right":"llm.context_tokens"}));
    }
    for (selector, equals, required) in provider_requirements() {
        if key == selector || key == required {
            checks.push(json!({"kind":"required_when","selector":selector,"equals":equals,"required":required}));
        }
    }
    if matches!(
        key,
        "anki.endpoint"
            | "llm.endpoint"
            | "audio.endpoint"
            | "images.custom_endpoint"
            | "dictionary.url_template"
            | "kanji.url_template"
            | "network.allowed_remote_service_hosts"
            | "network.offline"
    ) {
        checks.push(json!({"kind":"remote_host_policy","allowed_hosts":"network.allowed_remote_service_hosts","offline":"network.offline"}));
    }
    checks
}
fn validate_effective(registry: &Registry, values: &BTreeMap<String, Value>) -> Result<()> {
    for (k, v) in values {
        registry.validate_value(k, v)?;
    }
    let n = |key: &str| values.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    for (a, b, strict) in NUMERIC_RELATIONS {
        let right = if strict { n(b) / 3.0 } else { n(b) };
        if (strict && n(a) >= right) || (!strict && n(a) > right) {
            return Err(format!(
                "CROSS_FIELD_CONSTRAINT: {a} must be {} {b}",
                if strict {
                    "less than one third of"
                } else {
                    "at most"
                }
            ));
        }
    }
    if n("llm.max_output_tokens") >= n("llm.context_tokens") {
        return Err(
            "CROSS_FIELD_CONSTRAINT: llm.max_output_tokens must leave context capacity for input"
                .into(),
        );
    }
    let present = |key: &str| {
        values
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    };
    for (selector, selected, required) in provider_requirements() {
        if values.get(selector) == Some(&selected) && !present(required) {
            return Err(format!(
                "PROVIDER_SETTING_REQUIRED: {selector} requires {required}"
            ));
        }
    }
    let hosts: Vec<_> = values["network.allowed_remote_service_hosts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut endpoint_keys = vec![
        "anki.endpoint",
        "llm.endpoint",
        "audio.endpoint",
        "images.custom_endpoint",
    ];
    if values["dictionary.provider"] == "custom" {
        endpoint_keys.push("dictionary.url_template");
    }
    if values.get("kanji.url_template") != Some(&registry.lookup("kanji.url_template")?.default) {
        endpoint_keys.push("kanji.url_template");
    }
    for key in endpoint_keys {
        if let Some(endpoint) = values.get(key).and_then(Value::as_str) {
            let u = url::Url::parse(endpoint).map_err(|_| format!("INVALID_ENDPOINT: {key}"))?;
            let host = u.host_str().unwrap();
            let loopback = host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
            if !loopback
                && (values["network.offline"] == true
                    || !hosts.iter().any(|h| h.eq_ignore_ascii_case(host)))
            {
                return Err(format!("REMOTE_ENDPOINT_NOT_ALLOWED: {key}"));
            }
        }
    }
    Ok(())
}
pub fn config_path(
    explicit: Option<&Path>,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_owned());
    }
    if let Some(p) = environment.get("LAB_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let root = environment
        .get("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("HOME")
                .map(|s| PathBuf::from(s).join(".config"))
        })
        .ok_or("CONFIG_PATH_UNAVAILABLE")?;
    if !root.is_absolute() {
        return Err("XDG_CONFIG_HOME_MUST_BE_ABSOLUTE".into());
    }
    Ok(root.join("linguist-anki-bridge/config.toml"))
}
/// Publish a minimal private configuration without replacing any existing path.
pub fn initialize(path: &Path) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut directory = std::fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(parent).map_err(|_| "CONFIG_PARENT_IO")?;
    let temp = parent.join(format!(".lab-config-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&temp).map_err(|_| "CONFIG_TEMP_IO")?;
        f.write_all(b"[config]\nversion = 2\n")
            .map_err(|_| "CONFIG_WRITE_IO")?;
        f.sync_all().map_err(|_| "CONFIG_SYNC_IO")?;
        std::fs::hard_link(&temp, path).map_err(|_| "CONFIG_EXISTS_OR_PUBLISH_FAILED")?;
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "CONFIG_DIRECTORY_SYNC_FAILED")?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temp);
    result.map_err(str::to_owned)
}
/// Expand only documented HOME/XDG tokens; this never invokes a shell.
/// Call when binding a filesystem consumer, then freeze the resulting absolute path.
pub fn expand_path(input: &str, environment: &BTreeMap<String, String>) -> Result<PathBuf> {
    let home = environment.get("HOME").filter(|s| !s.is_empty());
    let mut roots = BTreeMap::new();
    if let Some(home) = home {
        roots.insert("HOME", home.clone());
    }
    for (name, fallback) in [
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_STATE_HOME", ".local/state"),
        ("XDG_CACHE_HOME", ".cache"),
        ("XDG_DATA_HOME", ".local/share"),
    ] {
        if let Some(root) = environment.get(name).filter(|s| !s.is_empty()) {
            if !Path::new(root).is_absolute() {
                return Err(format!("INVALID_XDG_ROOT: {name}"));
            }
            roots.insert(name, root.clone());
        } else if let Some(home) = home {
            roots.insert(
                name,
                Path::new(home)
                    .join(fallback)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    let mut output = String::new();
    let mut rest = input;
    if rest == "~" || rest.starts_with("~/") {
        output.push_str(home.ok_or("HOME_UNAVAILABLE")?);
        rest = &rest[1..];
    }
    while let Some(index) = rest.find('$') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if !rest.starts_with("${") {
            return Err("UNSUPPORTED_PATH_EXPANSION".into());
        }
        let end = rest.find('}').ok_or("UNSUPPORTED_PATH_EXPANSION")?;
        let token = &rest[2..end];
        output.push_str(
            roots
                .get(token)
                .ok_or("UNAVAILABLE_OR_UNKNOWN_PATH_TOKEN")?,
        );
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    if output.contains('\0') || output.starts_with('~') {
        return Err("UNSUPPORTED_PATH_EXPANSION".into());
    }
    Ok(PathBuf::from(output))
}

pub mod edit;
