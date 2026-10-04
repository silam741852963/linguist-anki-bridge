//! Read-only import of legacy Python YAML and native JSON configuration (OP-09).
//!
//! The source is parsed with a restricted YAML loader (no tags, anchors,
//! aliases, merge keys, complex keys or multiple documents) and every source key
//! receives a report record. Nothing here writes files: the caller publishes the
//! candidate, report and exported prompt/schema resources.
use crate::{ConfigFile, Registry, ResolveOptions, builtin_purposes, presets, resolve};
use linguist_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, String>;

pub const REPORT_SCHEMA_VERSION: u32 = 1;
const SOURCE_LIMIT: usize = 4 * 1024 * 1024;
const DEPTH_LIMIT: usize = 64;
const NODE_LIMIT: usize = 100_000;
const PYTHON_DEFAULTS: &str = include_str!("../../../resources/legacy/python-config-defaults.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Same meaning and value under a new key.
    Transferred,
    /// Converted value or merged with another source key.
    Transformed,
    /// Equals the effective v2 builtin/preset value, so no override is written.
    Defaulted,
    /// Not carried; needs an explicit decision before activation.
    Unresolved,
    /// Intentionally not carried; behavior replaced or no longer configurable.
    Retired,
    /// Preserved only in the report (unvalidated purpose or language).
    Unsupported,
    /// Not in the migration table.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedResource {
    /// File name inside the candidate's resource directory.
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyRecord {
    pub legacy_key: String,
    pub status: Status,
    pub code: String,
    pub targets: Vec<String>,
    /// Activation requires `--accept-unresolved LEGACY_KEY` for this record.
    pub blocking: bool,
    /// The key was absent from the source; the legacy runtime default applied.
    pub implicit_default: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_value: Option<Value>,
    /// Override written to the candidate for this record, if any.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub candidate_values: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<ExportedResource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub source_format: String,
    pub source_version: Option<i64>,
    pub source_sha256: String,
    pub candidate_sha256: String,
    pub records: Vec<KeyRecord>,
    pub counts: BTreeMap<String, u64>,
    /// Legacy keys that must be accepted explicitly before activation.
    pub blocking_keys: Vec<String>,
    pub activation_blocked: bool,
    /// Non-blocking choices that remain after activation (deck targets, models).
    pub follow_up: Vec<String>,
    /// Directory (next to the candidate) holding exported prompt/schema files.
    #[serde(default)]
    pub resource_directory: Option<String>,
}

pub struct Import {
    pub candidate: ConfigFile,
    pub candidate_bytes: Vec<u8>,
    pub report: Report,
    pub resources: Vec<(ExportedResource, Vec<u8>)>,
}

/// Parse a restricted YAML document. Tags, anchors, aliases, merge keys,
/// non-string or complex keys, duplicate keys, multiple documents and
/// ambiguous YAML 1.1 scalars (octal, hex, sexagesimal, dates, non-finite) fail.
pub fn parse_yaml(text: &str) -> Result<Value> {
    use yaml_rust2::parser::{Event, Parser};
    use yaml_rust2::scanner::TScalarStyle;
    enum Frame {
        Seq(Vec<Value>),
        Map(Map<String, Value>, Option<String>),
    }
    fn place(stack: &mut [Frame], root: &mut Option<Value>, value: Value, key: bool) -> Result<()> {
        match stack.last_mut() {
            None => {
                if root.replace(value).is_some() {
                    return Err("LEGACY_YAML_UNSAFE: multiple root values".into());
                }
            }
            Some(Frame::Seq(items)) => items.push(value),
            Some(Frame::Map(map, pending)) => match pending.take() {
                None => {
                    let Value::String(name) = value else {
                        return Err("LEGACY_YAML_UNSAFE: mapping keys must be plain strings".into());
                    };
                    if !key {
                        return Err("LEGACY_YAML_UNSAFE: complex mapping keys".into());
                    }
                    if name == "<<" {
                        return Err("LEGACY_YAML_UNSAFE: merge keys are not accepted".into());
                    }
                    if map.contains_key(&name) {
                        return Err(format!("LEGACY_YAML_DUPLICATE_KEY: {name}"));
                    }
                    *pending = Some(name);
                }
                Some(name) => {
                    map.insert(name, value);
                }
            },
        }
        Ok(())
    }
    let mut parser = Parser::new_from_str(text);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root = None;
    let mut documents = 0;
    let mut nodes = 0usize;
    loop {
        let (event, mark) = parser
            .next_token()
            .map_err(|e| format!("LEGACY_YAML_INVALID: {e}"))?;
        let at = || format!("line {}", mark.line());
        nodes += 1;
        if nodes > NODE_LIMIT {
            return Err("LEGACY_YAML_TOO_LARGE: node limit".into());
        }
        match event {
            Event::Nothing | Event::StreamStart | Event::DocumentEnd => {}
            Event::StreamEnd => break,
            Event::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    return Err("LEGACY_YAML_UNSAFE: multiple documents".into());
                }
            }
            Event::Alias(_) => {
                return Err(format!("LEGACY_YAML_UNSAFE: alias at {}", at()));
            }
            Event::Scalar(text, style, anchor, tag) => {
                if anchor != 0 {
                    return Err(format!("LEGACY_YAML_UNSAFE: anchor at {}", at()));
                }
                if let Some(tag) = tag {
                    return Err(format!(
                        "LEGACY_YAML_UNSAFE: tag {}{} at {}",
                        tag.handle,
                        tag.suffix,
                        at()
                    ));
                }
                let value = if matches!(style, TScalarStyle::Plain) {
                    plain_scalar(&text).map_err(|e| format!("{e} at {}", at()))?
                } else {
                    Value::String(text)
                };
                place(&mut stack, &mut root, value, true)?;
            }
            Event::SequenceStart(ref anchor, ref tag)
            | Event::MappingStart(ref anchor, ref tag) => {
                let (anchor, tag) = (*anchor, tag.clone());
                if anchor != 0 {
                    return Err(format!("LEGACY_YAML_UNSAFE: anchor at {}", at()));
                }
                if let Some(tag) = tag {
                    return Err(format!(
                        "LEGACY_YAML_UNSAFE: tag {}{} at {}",
                        tag.handle,
                        tag.suffix,
                        at()
                    ));
                }
                if stack.len() >= DEPTH_LIMIT {
                    return Err("LEGACY_YAML_TOO_DEEP".into());
                }
                if let Some(Frame::Map(_, None)) = stack.last() {
                    return Err("LEGACY_YAML_UNSAFE: complex mapping keys".into());
                }
                stack.push(if matches!(event, Event::SequenceStart(..)) {
                    Frame::Seq(Vec::new())
                } else {
                    Frame::Map(Map::new(), None)
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let value = match stack.pop() {
                    Some(Frame::Seq(items)) => Value::Array(items),
                    Some(Frame::Map(map, None)) => Value::Object(map),
                    _ => return Err("LEGACY_YAML_INVALID: unbalanced collection".into()),
                };
                place(&mut stack, &mut root, value, false)?;
            }
        }
    }
    root.ok_or_else(|| "LEGACY_YAML_EMPTY".to_owned())
}

/// PyYAML `safe_load` plain-scalar resolution, restricted to unambiguous forms.
fn plain_scalar(text: &str) -> Result<Value> {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Ok(Value::Null),
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Ok(Value::Bool(true));
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Ok(Value::Bool(false));
        }
        _ => {}
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let lowered = unsigned.to_ascii_lowercase();
    if matches!(lowered.as_str(), ".inf" | ".nan") {
        return Err("LEGACY_YAML_AMBIGUOUS_SCALAR: non-finite number".into());
    }
    let digits_only = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    if lowered.starts_with("0x")
        || lowered.starts_with("0o")
        || lowered.starts_with("0b")
        || (unsigned.len() > 1 && unsigned.starts_with('0') && digits_only(unsigned))
        || (unsigned.contains(':')
            && unsigned
                .split(':')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
    {
        return Err(
            "LEGACY_YAML_AMBIGUOUS_SCALAR: octal, hexadecimal, binary or sexagesimal number".into(),
        );
    }
    let bytes = unsigned.as_bytes();
    if bytes.len() >= 8
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
    {
        return Err("LEGACY_YAML_AMBIGUOUS_SCALAR: timestamp".into());
    }
    if digits_only(unsigned) && unsigned.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
        let clean = text.replace('_', "");
        return clean
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "LEGACY_YAML_AMBIGUOUS_SCALAR: integer out of range".into());
    }
    let float_shaped = {
        let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
            Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
            None => (unsigned, None),
        };
        mantissa.contains('.')
            && mantissa
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'_' || b == b'.')
            && mantissa.bytes().filter(|b| *b == b'.').count() == 1
            && mantissa.bytes().any(|b| b.is_ascii_digit())
            && exponent.is_none_or(|e| {
                let e = e.strip_prefix(['-', '+']).unwrap_or(e);
                !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
            })
    };
    if float_shaped {
        let number: f64 = text
            .replace('_', "")
            .parse()
            .map_err(|_| "LEGACY_YAML_AMBIGUOUS_SCALAR: number")?;
        return serde_json::Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| "LEGACY_YAML_AMBIGUOUS_SCALAR: non-finite number".into());
    }
    Ok(Value::String(text.to_owned()))
}

struct Proposal {
    record: KeyRecord,
    settings: Vec<(String, Value)>,
}

struct Context<'a> {
    registry: &'a Registry,
    proposals: Vec<Proposal>,
    resources: BTreeMap<String, (ExportedResource, Vec<u8>)>,
    implicit: bool,
}

fn record(key: &str, status: Status, code: &str, detail: impl Into<String>) -> KeyRecord {
    KeyRecord {
        legacy_key: key.to_owned(),
        status,
        code: code.to_owned(),
        targets: Vec::new(),
        blocking: matches!(status, Status::Unresolved | Status::Unknown),
        implicit_default: false,
        detail: detail.into(),
        legacy_value: None,
        candidate_values: BTreeMap::new(),
        resource: None,
    }
}

impl Context<'_> {
    fn push(
        &mut self,
        mut record: KeyRecord,
        value: Option<&Value>,
        settings: Vec<(String, Value)>,
    ) {
        record.implicit_default = self.implicit;
        if self.implicit {
            // Absent keys were never a user choice: report them without blocking.
            record.blocking = false;
        }
        if record.legacy_value.is_none() && record.resource.is_none() {
            record.legacy_value = value.map(redact);
        }
        if record.targets.is_empty() {
            record.targets = settings.iter().map(|(k, _)| k.clone()).collect();
        }
        self.proposals.push(Proposal { record, settings });
    }
    fn simple(&mut self, key: &str, value: &Value, status: Status, code: &str, detail: &str) {
        self.push(record(key, status, code, detail), Some(value), Vec::new());
    }
    fn transfer(&mut self, key: &str, value: &Value, target: &str, converted: Value) {
        let status = if *value == converted {
            Status::Transferred
        } else {
            Status::Transformed
        };
        let detail = format!("{key} maps to {target}");
        let mut r = record(key, status, "LEGACY_MAPPED", detail);
        r.targets = vec![target.into()];
        self.push(r, Some(value), vec![(target.into(), converted)]);
    }
    fn unresolved(&mut self, key: &str, value: &Value, code: &str, detail: &str, targets: &[&str]) {
        let mut r = record(key, Status::Unresolved, code, detail);
        r.targets = targets.iter().map(|t| (*t).to_owned()).collect();
        self.push(r, Some(value), Vec::new());
    }
    fn export(&mut self, value: &Value, extension: &str, media_type: &str) -> ExportedResource {
        let bytes = match value {
            Value::String(text) => text.as_bytes().to_vec(),
            other => {
                let mut bytes =
                    canonical::bytes(other).unwrap_or_else(|_| other.to_string().into_bytes());
                bytes.push(b'\n');
                bytes
            }
        };
        let sha256 = canonical::asset_digest(&bytes);
        let resource = ExportedResource {
            file: format!("{sha256}.{extension}"),
            sha256,
            bytes: bytes.len() as u64,
            media_type: media_type.into(),
        };
        self.resources
            .insert(resource.file.clone(), (resource.clone(), bytes));
        resource
    }
}

/// Replace URL credentials; long text is summarized by its digest elsewhere.
fn redact(value: &Value) -> Value {
    if let Some(text) = value.as_str()
        && let Ok(url) = url::Url::parse(text)
        && (!url.username().is_empty() || url.password().is_some())
    {
        return json!("<redacted URL with credentials>");
    }
    value.clone()
}

fn language(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "english" | "en" => Some("en"),
        "vietnamese" | "vi" | "tiếng việt" => Some("vi"),
        "japanese" | "ja" => Some("ja"),
        _ => None,
    }
}

fn legacy_python_defaults() -> Value {
    serde_json::from_str::<Value>(PYTHON_DEFAULTS).expect("embedded legacy defaults")["defaults"]
        .clone()
}

fn flatten(prefix: &str, value: &Value, leaves: &[&str], out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) if !leaves.contains(&prefix) && prefix != "decks" => {
            if map.is_empty() && !prefix.is_empty() {
                out.insert(prefix.into(), value.clone());
            }
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, leaves, out);
            }
        }
        _ => {
            out.insert(prefix.into(), value.clone());
        }
    }
}

const PYTHON_LEAVES: [&str; 2] = ["dictionary.schema", "kanji.schema"];

/// Import legacy bytes into a validated v2 candidate and per-key report.
pub fn import(source: &[u8], environment: &BTreeMap<String, String>) -> Result<Import> {
    if source.len() > SOURCE_LIMIT {
        return Err("LEGACY_CONFIG_TOO_LARGE".into());
    }
    let text = std::str::from_utf8(source).map_err(|_| "LEGACY_CONFIG_ENCODING")?;
    let registry = Registry::builtin();
    let mut context = Context {
        registry: &registry,
        proposals: Vec::new(),
        resources: BTreeMap::new(),
        implicit: false,
    };
    let native = serde_json::from_str::<Value>(text)
        .ok()
        .filter(|v| v.get("version").is_some() || v.get("anki_url").is_some());
    let (format, version) = if let Some(document) = native {
        let version = document.get("version").and_then(Value::as_i64);
        if version != Some(1) {
            return Err(format!(
                "LEGACY_CONFIG_VERSION_UNSUPPORTED: native config version {:?}; only version 1 is known",
                document.get("version")
            ));
        }
        import_native(&mut context, &document)?;
        ("native_json", version)
    } else {
        let document = parse_yaml(text)?;
        if !document.is_object() {
            return Err("LEGACY_CONFIG_NOT_MAPPING".into());
        }
        let version = match document.get("config_version") {
            None => None,
            Some(v) => match v.as_i64() {
                Some(n @ (1 | 2)) => Some(n),
                _ => {
                    return Err(format!(
                        "LEGACY_CONFIG_VERSION_UNSUPPORTED: Python config_version {v}; known versions are 1 and 2"
                    ));
                }
            },
        };
        import_python(&mut context, &document)?;
        ("python_yaml", version)
    };
    let mut proposals = std::mem::take(&mut context.proposals);
    let resources: Vec<_> = context.resources.into_values().collect();
    let (candidate, candidate_bytes) = build_candidate(&registry, &mut proposals, environment)?;
    let records: Vec<KeyRecord> = proposals.into_iter().map(|p| p.record).collect();
    let mut counts = BTreeMap::new();
    for r in &records {
        *counts
            .entry(
                serde_json::to_value(r.status)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
            .or_insert(0) += 1;
    }
    let blocking_keys: Vec<String> = records
        .iter()
        .filter(|r| r.blocking)
        .map(|r| r.legacy_key.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let follow_up = records
        .iter()
        .filter(|r| !r.blocking && matches!(r.status, Status::Unresolved | Status::Unsupported))
        .map(|r| format!("{}: {}", r.legacy_key, r.code))
        .collect();
    let report = Report {
        schema_version: REPORT_SCHEMA_VERSION,
        source_format: format.into(),
        source_version: version,
        source_sha256: canonical::asset_digest(source),
        candidate_sha256: canonical::asset_digest(&candidate_bytes),
        activation_blocked: !blocking_keys.is_empty(),
        blocking_keys,
        records,
        counts,
        follow_up,
        resource_directory: None,
    };
    Ok(Import {
        candidate,
        candidate_bytes,
        report,
        resources,
    })
}

/// Whether `value` differs from what v2 would resolve without an override for
/// any supported purpose (builtin default or purpose preset).
fn needs_override(registry: &Registry, key: &str, value: &Value) -> bool {
    if key.starts_with("purposes.") {
        return true;
    }
    let Ok(entry) = registry.lookup(key) else {
        return true;
    };
    if entry.default != *value {
        return true;
    }
    let presets = presets();
    presets["presets"]
        .as_object()
        .unwrap()
        .values()
        .any(|preset| preset["overrides"].get(key).is_some_and(|v| v != value))
}

fn build_candidate(
    registry: &Registry,
    proposals: &mut [Proposal],
    environment: &BTreeMap<String, String>,
) -> Result<(ConfigFile, Vec<u8>)> {
    let mut candidate = ConfigFile::default();
    candidate.values.insert("config.version".into(), json!(2));
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    for proposal in proposals.iter_mut() {
        let mut written = BTreeMap::new();
        let mut failure = None;
        for (key, value) in &proposal.settings {
            if let Err(e) = registry.validate_value(key, value) {
                failure = Some(e);
                break;
            }
            if let Some(owner) = owners.get(key)
                && candidate.values.get(key) != Some(value)
            {
                failure = Some(format!(
                    "LEGACY_TARGET_CONFLICT: {key} is already set from {owner}"
                ));
                break;
            }
            if needs_override(registry, key, value) {
                written.insert(key.clone(), value.clone());
            }
        }
        if failure.is_none() && !written.is_empty() {
            let mut trial = candidate.clone();
            trial.values.extend(written.clone());
            match crate::edit::validate_candidate(registry, &trial, environment) {
                Ok(()) => candidate = trial,
                Err(e) => failure = Some(e),
            }
        }
        let record = &mut proposal.record;
        if let Some(error) = failure {
            let code = error
                .split(':')
                .next()
                .unwrap_or("INVALID_SETTING")
                .trim()
                .to_owned();
            record.status = Status::Unresolved;
            record.code = format!("LEGACY_VALUE_REJECTED_{code}");
            record.detail = format!("{}; candidate keeps the v2 value: {error}", record.detail);
            record.blocking = !record.implicit_default;
            continue;
        }
        for key in written.keys() {
            owners.insert(key.clone(), record.legacy_key.clone());
        }
        if written.is_empty()
            && !proposal.settings.is_empty()
            && matches!(record.status, Status::Transferred | Status::Transformed)
        {
            record.status = Status::Defaulted;
            record.code = "LEGACY_EQUALS_V2_DEFAULT".into();
            record.detail = format!(
                "{}; value equals the v2 builtin/preset value, no override written",
                record.detail
            );
        }
        record.candidate_values = written;
    }
    crate::edit::validate_candidate(registry, &candidate, environment)?;
    let bytes = crate::edit::serialize_config(&candidate)?;
    // Round-trip the serialized form exactly as activation will read it.
    let parsed = ConfigFile::parse(
        std::str::from_utf8(&bytes).map_err(|_| "CONFIG_ENCODING")?,
        registry,
    )?;
    resolve(registry, &parsed, &ResolveOptions::default())?;
    Ok((parsed, bytes))
}

fn endpoint(context: &mut Context, key: &str, value: &Value, target: &str) {
    match value.as_str() {
        Some(text) => match url::Url::parse(text) {
            Ok(url) if !url.username().is_empty() || url.password().is_some() => context.unresolved(
                key,
                value,
                "LEGACY_URL_CREDENTIALS",
                "credentials embedded in URLs are rejected; move them to an environment variable named by the matching *.api_key_env setting",
                &[target],
            ),
            Ok(_) => context.transfer(key, value, target, json!(text.trim_end_matches('/'))),
            Err(_) => context.unresolved(key, value, "LEGACY_URL_INVALID", "not an absolute http(s) URL", &[target]),
        },
        None => context.unresolved(key, value, "LEGACY_TYPE_MISMATCH", "expected a URL string", &[target]),
    }
}

fn number(context: &mut Context, key: &str, value: &Value, target: &str) {
    if value.is_number() {
        context.transfer(key, value, target, value.clone());
    } else {
        context.unresolved(
            key,
            value,
            "LEGACY_TYPE_MISMATCH",
            "expected a number",
            &[target],
        );
    }
}

fn boolean(context: &mut Context, key: &str, value: &Value, target: &str) {
    if value.is_boolean() {
        context.transfer(key, value, target, value.clone());
    } else {
        context.unresolved(
            key,
            value,
            "LEGACY_TYPE_MISMATCH",
            "expected a boolean",
            &[target],
        );
    }
}

fn integer(context: &mut Context, key: &str, value: &Value, target: &str) {
    match value.as_f64() {
        Some(n) if n.fract() == 0.0 && (0.0..1e9).contains(&n) => {
            context.transfer(key, value, target, json!(n as i64))
        }
        _ => context.unresolved(
            key,
            value,
            "LEGACY_TYPE_MISMATCH",
            "expected an integer",
            &[target],
        ),
    }
}

/// Export a prompt or schema; the legacy default is retired in favour of the
/// builtin v2 resource, a custom one is retained but not activated.
fn pinned(
    context: &mut Context,
    key: &str,
    value: &Value,
    legacy_default: Option<&Value>,
    target: &str,
    schema: bool,
) {
    let empty = value.is_null()
        || value.as_str().is_some_and(|s| s.trim().is_empty())
        || value.as_object().is_some_and(Map::is_empty);
    if empty {
        context.simple(
            key,
            value,
            Status::Retired,
            "LEGACY_EMPTY_RESOURCE",
            "empty legacy value; nothing to export",
        );
        return;
    }
    if (schema && !value.is_object()) || (!schema && !value.is_string()) {
        context.unresolved(
            key,
            value,
            "LEGACY_TYPE_MISMATCH",
            if schema {
                "expected a schema mapping"
            } else {
                "expected prompt text"
            },
            &[target],
        );
        return;
    }
    let resource = if schema {
        context.export(value, "json", "application/json")
    } else {
        context.export(value, "txt", "text/plain; charset=utf-8")
    };
    let mut r = if Some(value) == legacy_default {
        record(
            key,
            Status::Retired,
            "LEGACY_DEFAULT_RESOURCE_REPLACED",
            format!(
                "legacy builtin {} exported for reference; v2 uses its builtin resource for {target}",
                if schema { "schema" } else { "prompt" }
            ),
        )
    } else {
        record(
            key,
            Status::Unresolved,
            if schema {
                "LEGACY_SCHEMA_CUSTOM"
            } else {
                "LEGACY_PROMPT_CUSTOM"
            },
            format!(
                "custom legacy {} exported unchanged; its output contract is not validated for v2, so the candidate keeps the builtin {target}. After review, set {target} to the exported file path",
                if schema { "schema" } else { "prompt" }
            ),
        )
    };
    r.targets = vec![target.into()];
    r.resource = Some(resource);
    context.push(r, None, Vec::new());
}

fn import_python(context: &mut Context, document: &Value) -> Result<()> {
    let defaults = legacy_python_defaults();
    let mut source = BTreeMap::new();
    flatten("", document, &PYTHON_LEAVES, &mut source);
    let mut implicit = BTreeMap::new();
    flatten("", &defaults, &PYTHON_LEAVES, &mut implicit);
    implicit.retain(|k, _| !source.contains_key(k) && k != "decks" && !k.starts_with("decks."));
    let lookup = |key: &str| -> Option<Value> {
        source
            .get(key)
            .cloned()
            .or_else(|| implicit.get(key).cloned())
    };
    let default_of = |key: &str| -> Option<Value> {
        let mut out = BTreeMap::new();
        flatten("", &defaults, &PYTHON_LEAVES, &mut out);
        out.remove(key)
    };
    for (pass, values) in [(false, &source), (true, &implicit)] {
        context.implicit = pass;
        for (key, value) in values {
            if key == "decks" {
                continue;
            }
            python_key(context, key, value, &lookup, &default_of)?;
        }
    }
    context.implicit = false;
    if let Some(decks) = document.get("decks") {
        import_decks(
            context,
            decks,
            "note_type",
            "ocr_langs",
            Some(&defaults["decks"]),
        )?;
    }
    Ok(())
}

fn python_key(
    context: &mut Context,
    key: &str,
    value: &Value,
    lookup: &dyn Fn(&str) -> Option<Value>,
    default_of: &dyn Fn(&str) -> Option<Value>,
) -> Result<()> {
    let retired = |context: &mut Context, code: &str, detail: &str| {
        context.simple(key, value, Status::Retired, code, detail)
    };
    match key {
        "config_version" => {
            let mut r = record(key, Status::Transformed, "LEGACY_VERSION_REPLACED", "Python config_version is not copied; the candidate declares config.version = 2");
            r.targets = vec!["config.version".into()];
            context.push(r, Some(value), Vec::new());
        }
        "anki.url" => endpoint(context, key, value, "anki.endpoint"),
        "anki.backup_dir" => match value.as_str() {
            Some(path) if path.starts_with('/') && !path.contains(['\0', '\n', '\r']) => {
                context.transfer(key, value, "storage.backup_dir", json!(path))
            }
            Some(path) if path.starts_with("${HOME}") || path.starts_with("~/") => {
                context.transfer(key, value, "storage.backup_dir", json!(path))
            }
            _ => context.unresolved(key, value, "LEGACY_PATH_INVALID", "backup directory must be an absolute path", &["storage.backup_dir"]),
        },
        "anki.auto_backup_before_write" => retired(context, "LEGACY_RETIRED_SETTING", "whole-deck automatic backups are replaced by per-operation snapshots and explicit verified checkpoints"),
        "llm.ollama_url" => endpoint(context, key, value, "llm.endpoint"),
        "llm.model" => match value {
            Value::Null => {
                let mut r = record(key, Status::Unresolved, "LEGACY_MODEL_AUTODETECT_RETIRED", "legacy auto-detected the first installed model; v2 never auto-selects, so the builtin llm.model applies until you set one explicitly");
                r.blocking = false;
                r.targets = vec!["llm.model".into()];
                context.push(r, Some(value), Vec::new());
            }
            Value::String(name) if !name.trim().is_empty() => context.transfer(key, value, "llm.model", json!(name)),
            _ => context.unresolved(key, value, "LEGACY_TYPE_MISMATCH", "expected a model name", &["llm.model"]),
        },
        "llm.translation_language" => match value.as_str().and_then(language) {
            Some(code) => {
                let mut r = record(key, Status::Transformed, "LEGACY_LANGUAGE_NORMALIZED", format!("normalized to {code}; a base-file value overrides the builtin purpose presets (japanese_grammar defaults to vi)"));
                r.targets = vec!["learning.explanation_language".into()];
                context.push(r, Some(value), vec![("learning.explanation_language".into(), json!(code))]);
            }
            None => context.unresolved(key, value, "LEGACY_LANGUAGE_UNSUPPORTED", "only English, Vietnamese and Japanese labels are recognized", &["learning.explanation_language"]),
        },
        "llm.system_prompt_vocab" => pinned(context, key, value, default_of(key).as_ref(), "llm.prompts.vocabulary", false),
        "llm.system_prompt_grammar" => pinned(context, key, value, default_of(key).as_ref(), "llm.prompts.grammar", false),
        "ocr.method" => match value.as_str() {
            Some("tesseract") => context.transfer(key, value, "ocr.engine", json!("tesseract")),
            Some("ollama") => context.unresolved(key, value, "LEGACY_OCR_ENGINE_UNAVAILABLE", "Ollama vision OCR is not available in this build; the candidate keeps ocr.engine = tesseract", &["ocr.engine", "llm.vision_model"]),
            _ => context.unresolved(key, value, "LEGACY_OCR_ENGINE_UNKNOWN", "unknown legacy OCR method", &["ocr.engine"]),
        },
        "ocr.preprocess" => boolean(context, key, value, "ocr.preprocess"),
        "ocr.ollama_model" => {
            if lookup("ocr.method").as_ref().and_then(Value::as_str) == Some("ollama") {
                let mut r = record(key, Status::Unresolved, "LEGACY_VISION_MODEL_NOT_CARRIED", "vision OCR is unavailable; llm.vision_model is not set");
                r.blocking = false;
                r.targets = vec!["llm.vision_model".into()];
                context.push(r, Some(value), Vec::new());
            } else {
                retired(context, "LEGACY_DORMANT_SETTING", "only used by legacy Ollama OCR, which was not selected; llm.vision_model is not set")
            }
        }
        "ocr.ollama_url" => {
            let llm = lookup("llm.ollama_url");
            let same = llm.as_ref().and_then(Value::as_str).map(|s| s.trim_end_matches('/'))
                == value.as_str().map(|s| s.trim_end_matches('/'));
            if same {
                let mut r = record(key, Status::Transformed, "LEGACY_ENDPOINT_MERGED", "same endpoint as llm.ollama_url; merged into llm.endpoint");
                r.targets = vec!["llm.endpoint".into()];
                context.push(r, Some(value), Vec::new());
            } else {
                context.unresolved(key, value, "LEGACY_ENDPOINT_CONFLICT", "OCR and LLM endpoints differ; v2 has one llm.endpoint per purpose. The candidate keeps llm.ollama_url; set a purpose override if OCR needs another server", &["llm.endpoint"]);
            }
        }
        "image_classification.decision_threshold" => number(context, key, value, "classification.decision_threshold"),
        "image_classification.confirmation_margin" => number(context, key, value, "classification.confirmation_margin"),
        "image_classification.llm_accept_confidence" => number(context, key, value, "classification.vision_accept_confidence"),
        "image_classification.llm_adjudication" => match value {
            Value::Bool(true) => context.unresolved(key, value, "LEGACY_VISION_ADJUDICATION_UNAVAILABLE", "vision adjudication is not available in this build; the candidate keeps classification.vision_adjudication = false and ambiguous images stay for review", &["classification.vision_adjudication"]),
            _ => boolean(context, key, value, "classification.vision_adjudication"),
        },
        "image_classification.vision_model" => {
            let other = lookup("ocr.ollama_model");
            let detail = if other.is_some() && other.as_ref() != Some(value) {
                "differs from ocr.ollama_model; v2 has one llm.vision_model and no vision adapter, so neither is carried"
            } else {
                "vision adjudication is unavailable; llm.vision_model is not set"
            };
            if lookup("image_classification.llm_adjudication") == Some(json!(true)) {
                let mut r = record(key, Status::Unresolved, "LEGACY_VISION_MODEL_NOT_CARRIED", detail);
                r.blocking = false;
                r.targets = vec!["llm.vision_model".into()];
                context.push(r, Some(value), Vec::new());
            } else {
                retired(context, "LEGACY_DORMANT_SETTING", detail)
            }
        }
        "image_classification.dictionary_threshold" | "image_classification.visual_threshold" => retired(context, "LEGACY_RETIRED_SETTING", "superseded by the calibrated decision threshold in the legacy release"),
        "kanji.enabled" => boolean(context, key, value, "kanji.enabled"),
        "kanji.source_lang" => match value.as_str().and_then(language) {
            Some(code @ ("en" | "vi")) => {
                let mut r = record(key, Status::Transformed, "LEGACY_LANGUAGE_NORMALIZED", format!("normalized to {code}"));
                r.targets = vec!["kanji.explanation_language".into()];
                context.push(r, Some(value), vec![("kanji.explanation_language".into(), json!(code))]);
            }
            _ => context.unresolved(key, value, "LEGACY_LANGUAGE_UNSUPPORTED", "kanji explanations support english or vietnamese", &["kanji.explanation_language"]),
        },
        "kanji.url_template" => match value.as_str() {
            Some(text) => context.transfer(key, value, "kanji.url_template", json!(text)),
            None => context.unresolved(key, value, "LEGACY_TYPE_MISMATCH", "expected a URL template", &["kanji.url_template"]),
        },
        "kanji.prompt_en" | "kanji.prompt_vi" => pinned(context, key, value, default_of(key).as_ref(), "llm.prompts.kanji", false),
        "kanji.schema" => pinned(context, key, value, default_of(key).as_ref(), "kanji.schema", true),
        "filters.remove_parentheses" => boolean(context, key, value, "filters.remove_parentheses"),
        "filters.clean_word_only" => boolean(context, key, value, "filters.clean_word_only"),
        "image_search.enabled_for_empty" => boolean(context, key, value, "images.search_when_missing"),
        "image_search.suffix" => match value {
            Value::String(text) => context.transfer(key, value, "images.query_suffix", json!(text)),
            Value::Null => context.transfer(key, value, "images.query_suffix", json!("")),
            _ => context.unresolved(key, value, "LEGACY_TYPE_MISMATCH", "expected text", &["images.query_suffix"]),
        },
        "dictionary.preset" => match value.as_str() {
            Some("jisho") => {
                let mut r = record(key, Status::Transformed, "LEGACY_DICTIONARY_ROUTED_BY_PURPOSE", "legacy used jisho for every deck; v2 purpose presets select jisho for Japanese and wiktionary for English, so no global override is written");
                r.targets = vec!["dictionary.provider".into()];
                context.push(r, Some(value), Vec::new());
            }
            Some("wiktionary") => {
                let mut r = record(key, Status::Transformed, "LEGACY_DICTIONARY_ROUTED_BY_PURPOSE", "v2 purpose presets select wiktionary for English and jisho for Japanese; no global override is written");
                r.targets = vec!["dictionary.provider".into()];
                context.push(r, Some(value), Vec::new());
            }
            Some("custom") => context.unresolved(key, value, "LEGACY_CUSTOM_DICTIONARY_UNAVAILABLE", "the custom dictionary adapter is not available in this build; the candidate keeps purpose routing", &["dictionary.provider", "dictionary.url_template", "dictionary.schema_path"]),
            _ => context.unresolved(key, value, "LEGACY_DICTIONARY_UNKNOWN", "unknown legacy dictionary preset", &["dictionary.provider"]),
        },
        "dictionary.url_template" => {
            let custom = lookup("dictionary.preset").as_ref().and_then(Value::as_str) == Some("custom");
            if value.as_str().is_none_or(|s| s.trim().is_empty()) {
                retired(context, "LEGACY_EMPTY_SETTING", "empty legacy template");
            } else if custom {
                context.unresolved(key, value, "LEGACY_CUSTOM_DICTIONARY_UNAVAILABLE", "custom dictionary templates are not used until the custom adapter exists", &["dictionary.url_template"]);
            } else {
                retired(context, "LEGACY_DORMANT_SETTING", "only used by the custom preset, which was not selected");
            }
        }
        "dictionary.schema" => {
            let custom = lookup("dictionary.preset").as_ref().and_then(Value::as_str) == Some("custom");
            if custom {
                pinned(context, key, value, None, "dictionary.schema_path", true);
            } else if value.is_null() || value.as_object().is_some_and(Map::is_empty) {
                retired(context, "LEGACY_EMPTY_SETTING", "empty legacy schema");
            } else {
                let resource = context.export(value, "json", "application/json");
                let mut r = record(key, Status::Retired, "LEGACY_DORMANT_SETTING", "exported for reference; only used by the custom preset, which was not selected");
                r.resource = Some(resource);
                context.push(r, None, Vec::new());
            }
        }
        "dictionary.retry_count" => integer(context, key, value, "retry.read_attempts"),
        "dictionary.retry_backoff_seconds" => number(context, key, value, "retry.initial_backoff_seconds"),
        "dictionary.browser_fallback" => match value {
            Value::Bool(true) => context.unresolved(key, value, "LEGACY_BROWSER_UNAVAILABLE", "the controlled browser helper is not available in this build; the candidate keeps dictionary.browser_fallback = false", &["dictionary.browser_fallback"]),
            _ => boolean(context, key, value, "dictionary.browser_fallback"),
        },
        "batch.max_attempts" => integer(context, key, value, "jobs.max_item_attempts"),
        "batch.retry_backoff_seconds" => {
            let read = lookup("dictionary.retry_backoff_seconds");
            if Some(value) == default_of(key).as_ref() {
                retired(context, "LEGACY_RETRY_DEFAULT_NOT_CARRIED", "legacy job backoff default is not carried; retry.initial_backoff_seconds comes from dictionary.retry_backoff_seconds");
            } else if read.as_ref() == Some(value) {
                let mut r = record(key, Status::Transformed, "LEGACY_RETRY_MERGED", "same value as dictionary.retry_backoff_seconds; merged into retry.initial_backoff_seconds");
                r.targets = vec!["retry.initial_backoff_seconds".into()];
                context.push(r, Some(value), Vec::new());
            } else {
                context.unresolved(key, value, "LEGACY_RETRY_CONFLICT", "custom job backoff conflicts with dictionary.retry_backoff_seconds; v2 has one retry.initial_backoff_seconds, which the candidate takes from the dictionary setting", &["retry.initial_backoff_seconds"]);
            }
        }
        "batch.commit_interval_seconds" => number(context, key, value, "anki.commit_interval_seconds"),
        "dry_run" => retired(context, "LEGACY_DRY_RUN_RETIRED", "every preparation command is non-writing and each write needs an explicit --apply on that invocation, whatever this legacy value was"),
        _ if key.starts_with("batch.service_intervals.") => {
            let service = &key["batch.service_intervals.".len()..];
            if ["dictionary", "ollama", "kanji", "image", "tts"].contains(&service) {
                number(context, key, value, &format!("services.{service}.min_interval_seconds"));
            } else {
                context.unresolved(key, value, "LEGACY_UNKNOWN_KEY", "unknown legacy service interval", &[]);
            }
        }
        _ if ["theme", "omarchy", "tui", "appearance", "ui"]
            .iter()
            .any(|p| key == *p || key.starts_with(&format!("{p}."))) =>
        {
            retired(context, "LEGACY_APPEARANCE_RETIRED", "terminal/desktop appearance is not a CLI setting");
        }
        _ => {
            let mut r = record(key, Status::Unknown, "LEGACY_UNKNOWN_KEY", "not in the migration table; review it before activation");
            r.blocking = true;
            context.push(r, Some(value), Vec::new());
        }
    }
    Ok(())
}

const FIELD_ROLES: [(&str, &str); 4] = [
    ("meaning_image", "picture"),
    ("meaning_text", "meaning"),
    ("kanji_construction", "kanji"),
    ("word", "expression"),
];

fn import_decks(
    context: &mut Context,
    decks: &Value,
    model_key: &str,
    ocr_key: &str,
    defaults: Option<&Value>,
) -> Result<()> {
    let Some(decks) = decks.as_object() else {
        context.unresolved(
            "decks",
            decks,
            "LEGACY_TYPE_MISMATCH",
            "expected a mapping of deck purposes",
            &[],
        );
        return Ok(());
    };
    let supported = builtin_purposes();
    let roles: Vec<String> = context
        .registry
        .lookup("purposes.<purpose>.fields")
        .map_or_else(
            |_| Vec::new(),
            |e| {
                e.constraints["keys"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            },
        );
    for (name, entry) in decks {
        // The legacy loader renamed bare language keys to *_vocab.
        let renamed = if ["japanese", "english", "taiwanese", "german"].contains(&name.as_str())
            && !decks.contains_key(&format!("{name}_vocab"))
        {
            format!("{name}_vocab")
        } else {
            name.clone()
        };
        let base = format!("decks.{name}");
        let Some(entry) = entry.as_object() else {
            context.unresolved(
                &base,
                entry,
                "LEGACY_TYPE_MISMATCH",
                "expected a deck mapping",
                &[],
            );
            continue;
        };
        let configured = entry
            .get("deck_name")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !supported.contains(&renamed) {
            for (sub, value) in entry {
                let mut r = record(
                    &format!("{base}.{sub}"),
                    Status::Unsupported,
                    "LEGACY_PURPOSE_UNSUPPORTED",
                    format!(
                        "{renamed} is not a validated v2 purpose; preserved in this report only and never mapped to another language"
                    ),
                );
                r.blocking = configured;
                context.push(r, Some(value), Vec::new());
            }
            continue;
        }
        let default_entry = defaults.and_then(|d| d.get(&renamed));
        let unchanged = |sub: &str| default_entry.and_then(|d| d.get(sub)) == entry.get(sub);
        let purpose = format!("purposes.{renamed}");
        for (sub, value) in entry {
            let key = format!("{base}.{sub}");
            match sub.as_str() {
                "deck_name" => match value.as_str().filter(|s| !s.trim().is_empty()) {
                    Some(deck) => {
                        let mut r = record(
                            &key,
                            Status::Transformed,
                            "LEGACY_DECK_SOURCE_ONLY",
                            format!(
                                "source deck for {renamed}; the destination deck is never inferred: run `decks map {renamed}` to choose purposes.{renamed}.target_deck"
                            ),
                        );
                        r.targets = vec![format!("{purpose}.source_deck")];
                        context.push(
                            r,
                            Some(value),
                            vec![(format!("{purpose}.source_deck"), json!(deck))],
                        );
                    }
                    None => context.simple(
                        &key,
                        value,
                        Status::Retired,
                        "LEGACY_DECK_UNCONFIGURED",
                        "no legacy deck selected for this purpose",
                    ),
                },
                s if s == model_key => match value.as_str().filter(|s| !s.trim().is_empty()) {
                    Some(model) if configured || !unchanged(sub) => context.transfer(
                        &key,
                        value,
                        &format!("{purpose}.source_model"),
                        json!(model),
                    ),
                    Some(_) => context.simple(
                        &key,
                        value,
                        Status::Retired,
                        "LEGACY_DECK_UNCONFIGURED",
                        "legacy default for an unconfigured deck",
                    ),
                    None => context.simple(
                        &key,
                        value,
                        Status::Retired,
                        "LEGACY_EMPTY_SETTING",
                        "no legacy note type",
                    ),
                },
                s if s == ocr_key => match value.as_str() {
                    Some(langs) if configured || !unchanged(sub) => {
                        let list: Vec<String> = langs
                            .split('+')
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let mut r = record(
                            &key,
                            Status::Transformed,
                            "LEGACY_OCR_LANGUAGES_SPLIT",
                            format!("{langs} split into a language array"),
                        );
                        r.targets = vec![format!("{purpose}.ocr_languages")];
                        context.push(
                            r,
                            Some(value),
                            vec![(format!("{purpose}.ocr_languages"), json!(list))],
                        );
                    }
                    Some(_) => context.simple(
                        &key,
                        value,
                        Status::Retired,
                        "LEGACY_DECK_UNCONFIGURED",
                        "legacy default for an unconfigured deck",
                    ),
                    None => context.unresolved(
                        &key,
                        value,
                        "LEGACY_TYPE_MISMATCH",
                        "expected '+'-separated language codes",
                        &[],
                    ),
                },
                "fields" => {
                    let Some(fields) = value.as_object() else {
                        context.unresolved(
                            &key,
                            value,
                            "LEGACY_TYPE_MISMATCH",
                            "expected a role to field-name mapping",
                            &[],
                        );
                        continue;
                    };
                    if !configured && unchanged(sub) {
                        context.simple(
                            &key,
                            value,
                            Status::Retired,
                            "LEGACY_DECK_UNCONFIGURED",
                            "legacy default field mapping for an unconfigured deck",
                        );
                        continue;
                    }
                    let mut map = Map::new();
                    let mut problems = Vec::new();
                    for (role, field) in fields {
                        let target = FIELD_ROLES
                            .iter()
                            .find(|(old, _)| old == role)
                            .map(|(_, new)| (*new).to_owned())
                            .unwrap_or_else(|| role.clone());
                        if !roles.contains(&target) {
                            problems.push(format!("unknown role {role}"));
                        } else if !field.is_string() {
                            problems.push(format!("{role} is not a field name"));
                        } else if map.insert(target.clone(), field.clone()).is_some() {
                            problems.push(format!("two legacy roles map to {target}"));
                        }
                    }
                    if problems.is_empty() {
                        let mut r = record(
                            &key,
                            Status::Transformed,
                            "LEGACY_FIELD_ROLES_RENAMED",
                            "meaning_image→picture, meaning_text→meaning, kanji_construction→kanji; field names preserved exactly. Card task maps are not inferred",
                        );
                        r.targets = vec![format!("{purpose}.fields")];
                        context.push(
                            r,
                            Some(value),
                            vec![(format!("{purpose}.fields"), Value::Object(map))],
                        );
                    } else {
                        context.unresolved(
                            &key,
                            value,
                            "LEGACY_FIELD_ROLES_UNRESOLVED",
                            &problems.join("; "),
                            &[],
                        );
                    }
                }
                _ => {
                    let mut r = record(
                        &key,
                        Status::Unknown,
                        "LEGACY_UNKNOWN_KEY",
                        "unknown deck setting",
                    );
                    r.blocking = true;
                    context.push(r, Some(value), Vec::new());
                }
            }
        }
        if renamed != *name {
            let mut r = record(
                &base,
                Status::Transformed,
                "LEGACY_PURPOSE_RENAMED",
                format!("legacy key {name} is read as {renamed}, as the legacy loader did"),
            );
            r.blocking = false;
            context.push(r, None, Vec::new());
        }
    }
    Ok(())
}

fn import_native(context: &mut Context, document: &Value) -> Result<()> {
    let map = document.as_object().ok_or("LEGACY_CONFIG_NOT_MAPPING")?;
    for (key, value) in map {
        match key.as_str() {
            "version" => {
                let mut r = record(key, Status::Transformed, "LEGACY_VERSION_REPLACED", "native version 1 is not copied; the candidate declares config.version = 2");
                r.targets = vec!["config.version".into()];
                context.push(r, Some(value), Vec::new());
            }
            "anki_url" => endpoint(context, key, value, "anki.endpoint"),
            "ollama_url" => endpoint(context, key, value, "llm.endpoint"),
            "ollama_model" => match value {
                Value::Null => {
                    let mut r = record(key, Status::Unresolved, "LEGACY_MODEL_AUTODETECT_RETIRED", "no model was chosen; v2 never auto-selects, so the builtin llm.model applies until you set one explicitly");
                    r.blocking = false;
                    r.targets = vec!["llm.model".into()];
                    context.push(r, Some(value), Vec::new());
                }
                Value::String(name) if !name.trim().is_empty() => context.transfer(key, value, "llm.model", json!(name)),
                _ => context.unresolved(key, value, "LEGACY_TYPE_MISMATCH", "expected a model name", &["llm.model"]),
            },
            "dictionary_preset" => python_key(context, "dictionary.preset", value, &|k| (k == "dictionary.preset").then(|| value.clone()), &|_| None).map(|_| {
                if let Some(last) = context.proposals.last_mut() {
                    last.record.legacy_key = key.clone();
                }
            })?,
            "dictionary_url_template" => {
                let custom = map.get("dictionary_preset").and_then(Value::as_str) == Some("custom");
                if value.as_str().is_none_or(|s| s.trim().is_empty()) {
                    context.simple(key, value, Status::Retired, "LEGACY_EMPTY_SETTING", "empty legacy template");
                } else if custom {
                    context.unresolved(key, value, "LEGACY_CUSTOM_DICTIONARY_UNAVAILABLE", "custom dictionary templates are not used until the custom adapter exists", &["dictionary.url_template"]);
                } else {
                    context.simple(key, value, Status::Retired, "LEGACY_DORMANT_SETTING", "only used by the custom preset, which was not selected");
                }
            }
            "dictionary_schema" => {
                if value.is_null() || value.as_object().is_some_and(Map::is_empty) {
                    context.simple(key, value, Status::Retired, "LEGACY_EMPTY_SETTING", "empty legacy schema");
                } else {
                    pinned(context, key, value, None, "dictionary.schema_path", true);
                }
            }
            "kanji_source_lang" => match value.as_str().and_then(language) {
                Some(code @ ("en" | "vi")) => {
                    let mut r = record(key, Status::Transformed, "LEGACY_LANGUAGE_NORMALIZED", format!("normalized to {code}"));
                    r.targets = vec!["kanji.explanation_language".into()];
                    context.push(r, Some(value), vec![("kanji.explanation_language".into(), json!(code))]);
                }
                _ if value.as_str() == Some("") => context.simple(key, value, Status::Retired, "LEGACY_EMPTY_SETTING", "empty legacy language"),
                _ => context.unresolved(key, value, "LEGACY_LANGUAGE_UNSUPPORTED", "kanji explanations support english or vietnamese", &["kanji.explanation_language"]),
            },
            "dry_run" => context.simple(key, value, Status::Retired, "LEGACY_DRY_RUN_RETIRED", "every preparation command is non-writing and each write needs an explicit --apply on that invocation, whatever this legacy value was"),
            "decks" => import_decks(context, value, "model_name", "ocr_languages", None)?,
            _ => {
                let mut r = record(key, Status::Unknown, "LEGACY_UNKNOWN_KEY", "not in the migration table; review it before activation");
                r.blocking = true;
                context.push(r, Some(value), Vec::new());
            }
        }
    }
    Ok(())
}

/// Paths written by [`write_outputs`].
#[derive(Debug, Serialize)]
pub struct Written {
    pub candidate: std::path::PathBuf,
    pub report: std::path::PathBuf,
    pub resource_directory: std::path::PathBuf,
    pub resources: Vec<String>,
    pub replaced: bool,
}

pub fn report_path(candidate: &std::path::Path) -> std::path::PathBuf {
    let mut name = candidate.file_name().unwrap_or_default().to_os_string();
    name.push(".import.json");
    candidate.with_file_name(name)
}

fn resource_dir(candidate: &std::path::Path) -> std::path::PathBuf {
    let mut name = candidate.file_name().unwrap_or_default().to_os_string();
    name.push(".resources");
    candidate.with_file_name(name)
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    let normal = |p: &std::path::Path| {
        p.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(p.file_name())
            .map(|(parent, name)| parent.join(name))
    };
    match (normal(a), normal(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

fn private_write(path: &std::path::Path, bytes: &[u8], replace: bool) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let temp = parent.join(format!(".lab-import-{}.tmp", uuid::Uuid::new_v4()));
    let outcome = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&temp).map_err(|_| "IMPORT_OUTPUT_IO")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "IMPORT_OUTPUT_IO")?;
        if replace {
            if std::fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
                return Err("IMPORT_OUTPUT_NOT_REGULAR_FILE".into());
            }
            std::fs::rename(&temp, path).map_err(|_| "IMPORT_OUTPUT_IO")?;
        } else {
            std::fs::hard_link(&temp, path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "IMPORT_OUTPUT_EXISTS: pass --replace to overwrite this import's candidate files".to_owned()
                } else {
                    "IMPORT_OUTPUT_IO".to_owned()
                }
            })?;
        }
        std::fs::File::open(parent)
            .and_then(|d| d.sync_all())
            .map_err(|_| "IMPORT_OUTPUT_SYNC".into())
    })();
    let _ = std::fs::remove_file(&temp);
    outcome
}

/// Publish the candidate, its report and exported resources. The legacy source
/// and the live configuration are never written; only the three output paths
/// owned by this import may be replaced with `replace`.
pub fn write_outputs(
    import: &mut Import,
    source: &std::path::Path,
    output: &std::path::Path,
    live_config: &std::path::Path,
    replace: bool,
) -> Result<Written> {
    if output.file_name().is_none() {
        return Err("IMPORT_OUTPUT_INVALID".into());
    }
    if same_file(output, source) {
        return Err("IMPORT_OUTPUT_IS_SOURCE: the legacy file is never written".into());
    }
    if same_file(output, live_config) {
        return Err("IMPORT_OUTPUT_IS_LIVE_CONFIG: import never activates; use `config migrate --from-import` after review".into());
    }
    let report = report_path(output);
    let resources = resource_dir(output);
    if same_file(&report, source) || same_file(&report, live_config) {
        return Err(
            "IMPORT_OUTPUT_INVALID: report path collides with source or live config".into(),
        );
    }
    if !replace {
        for path in [output, report.as_path()] {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err("IMPORT_OUTPUT_EXISTS: pass --replace to overwrite this import's candidate files".into());
            }
        }
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(parent).map_err(|_| "IMPORT_OUTPUT_IO")?;
    let mut names = Vec::new();
    if !import.resources.is_empty() {
        builder.create(&resources).map_err(|_| "IMPORT_OUTPUT_IO")?;
        let meta = std::fs::symlink_metadata(&resources).map_err(|_| "IMPORT_OUTPUT_IO")?;
        if !meta.is_dir() || meta.is_symlink() {
            return Err("IMPORT_OUTPUT_NOT_DIRECTORY".into());
        }
        for (resource, bytes) in &import.resources {
            let path = resources.join(&resource.file);
            match std::fs::read(&path) {
                Ok(existing) if existing == *bytes => {}
                Ok(_) => return Err("IMPORT_RESOURCE_DIGEST_MISMATCH".into()),
                Err(_) => private_write(&path, bytes, false)?,
            }
            names.push(resource.file.clone());
        }
    }
    import.report.resource_directory = Some(
        resources
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    );
    let mut report_bytes = serde_json::to_vec_pretty(&import.report).map_err(|e| e.to_string())?;
    report_bytes.push(b'\n');
    // The candidate is published last; activation reads it with its report.
    private_write(&report, &report_bytes, replace)?;
    private_write(output, &import.candidate_bytes, replace)?;
    Ok(Written {
        candidate: output.to_owned(),
        report,
        resource_directory: resources,
        resources: names,
        replaced: replace,
    })
}

/// Load an import's candidate and report and check that every blocking legacy
/// key was explicitly accepted. Returns the candidate bytes and accepted keys.
pub fn check_activation(
    candidate: &std::path::Path,
    accepted: &[String],
) -> Result<(Vec<u8>, Report, Vec<String>)> {
    let read = |path: &std::path::Path| -> Result<Vec<u8>> {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path).map_err(|_| "IMPORT_CANDIDATE_IO")?;
        if !file
            .metadata()
            .map_err(|_| "IMPORT_CANDIDATE_IO")?
            .is_file()
        {
            return Err("IMPORT_CANDIDATE_NOT_REGULAR_FILE".into());
        }
        let mut bytes = Vec::new();
        file.take(SOURCE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "IMPORT_CANDIDATE_IO")?;
        if bytes.len() > SOURCE_LIMIT {
            return Err("IMPORT_CANDIDATE_TOO_LARGE".into());
        }
        Ok(bytes)
    };
    let bytes = read(candidate)?;
    let report: Report = serde_json::from_slice(&read(&report_path(candidate))?)
        .map_err(|_| "IMPORT_REPORT_INVALID")?;
    if report.schema_version != REPORT_SCHEMA_VERSION {
        return Err("IMPORT_REPORT_VERSION_UNSUPPORTED".into());
    }
    if canonical::asset_digest(&bytes) != report.candidate_sha256 {
        return Err("IMPORT_CANDIDATE_CHANGED: the candidate no longer matches its import report; re-run config import, activate, then edit with config set".into());
    }
    let accepted: BTreeSet<&String> = accepted.iter().collect();
    for key in &accepted {
        if !report.blocking_keys.contains(key) {
            return Err(format!(
                "IMPORT_ACCEPTANCE_UNKNOWN: {key} is not a blocking key of this import"
            ));
        }
    }
    let missing: Vec<&String> = report
        .blocking_keys
        .iter()
        .filter(|k| !accepted.contains(k))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "IMPORT_ACTIVATION_BLOCKED: accept each unresolved legacy key explicitly with --accept-unresolved KEY: {}",
            missing
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok((bytes, report, accepted.into_iter().cloned().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_scalars_follow_pyyaml_without_ambiguous_forms() {
        assert_eq!(plain_scalar("yes").unwrap(), json!(true));
        assert_eq!(plain_scalar("Off").unwrap(), json!(false));
        assert_eq!(plain_scalar("~").unwrap(), Value::Null);
        assert_eq!(plain_scalar("12_000").unwrap(), json!(12000));
        assert_eq!(plain_scalar("-0.25").unwrap(), json!(-0.25));
        assert_eq!(
            plain_scalar("2. Picture Words").unwrap(),
            json!("2. Picture Words")
        );
        assert_eq!(plain_scalar("1.0e3").unwrap(), json!(1000.0));
        for bad in ["0x1f", "017", "1:30", ".inf", "-.NaN", "2024-01-02"] {
            assert!(
                plain_scalar(bad)
                    .unwrap_err()
                    .starts_with("LEGACY_YAML_AMBIGUOUS_SCALAR"),
                "{bad}"
            );
        }
    }
}
