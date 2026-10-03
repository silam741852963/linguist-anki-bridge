//! JSON Schema for ConfigFile.values after strict TOML flattening.
//! Runtime resolution still enforces cross-field and service-specific rules.
use crate::{Entry, Registry};
use serde_json::{Map, Value, json};

const NAME: &str = "[A-Za-z][A-Za-z0-9_-]{0,63}";
const TASK_ORDINAL: &str =
    "^(?:0|[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5])$";

fn format_schema(format: &str, schema: &mut Map<String, Value>) {
    schema.insert("x-runtime-format".into(), json!(format));
    match format {
        "nonempty" => {
            schema.insert("pattern".into(), json!(r"\S"));
        }
        "env_name" => {
            schema.insert("pattern".into(), json!(r"^[A-Za-z_][A-Za-z0-9_]*$"));
        }
        "path" | "resource_ref" => {
            schema.insert("pattern".into(), json!(r"^[^\u0000\r\n]*\S[^\u0000\r\n]*$"));
        }
        "executable" => {
            schema.insert("pattern".into(), json!(r"^[^\s$`;&|]+$"));
        }
        "url" | "url_template" => {
            schema.insert("pattern".into(), json!(r"^[Hh][Tt][Tt][Pp][Ss]?://"));
        }
        _ => {}
    }
}

fn value_schema(entry: &Entry, registry: &Registry) -> Value {
    let mut schema = Map::new();
    schema.insert("description".into(), json!(entry.description));
    schema.insert("default".into(), entry.default.clone());
    schema.insert("x-scope".into(), json!(entry.scope));
    schema.insert("x-consumer".into(), json!(entry.consumer));
    schema.insert("x-sensitive".into(), json!(entry.sensitive));
    match entry.value_type.as_str() {
        "boolean" => {
            schema.insert("type".into(), json!("boolean"));
        }
        "integer" | "number" => {
            schema.insert("type".into(), json!(entry.value_type));
            if let Some(min) = entry.constraints.get("min") {
                schema.insert("minimum".into(), min.clone());
            }
            if let Some(max) = entry.constraints.get("max") {
                schema.insert("maximum".into(), max.clone());
            }
        }
        "string" | "string|null" | "enum" => {
            schema.insert(
                "type".into(),
                if entry.value_type == "string|null" {
                    json!(["string", "null"])
                } else {
                    json!("string")
                },
            );
            schema.insert("maxLength".into(), json!(1024 * 1024));
            if let Some(format) = entry.constraints.get("format").and_then(Value::as_str) {
                format_schema(format, &mut schema);
            }
            if let Some(values) = entry.constraints.get("values") {
                schema.insert("enum".into(), values.clone());
            }
        }
        "string[]" => {
            let mut item = Map::from_iter([
                ("type".into(), json!("string")),
                ("maxLength".into(), json!(1024 * 1024)),
            ]);
            if let Some(format) = entry
                .constraints
                .get("items_format")
                .and_then(Value::as_str)
            {
                format_schema(format, &mut item);
            }
            schema.insert("type".into(), json!("array"));
            schema.insert("items".into(), Value::Object(item));
            schema.insert("maxItems".into(), json!(4096));
            if entry.constraints["unique"] == true {
                schema.insert("uniqueItems".into(), json!(true));
            }
        }
        "field_map" => {
            let keys = entry.constraints["keys"]
                .as_array()
                .expect("field-map keys");
            let properties: Map<String, Value> = keys
                .iter()
                .map(|key| {
                    (
                        key.as_str().expect("field-map key").to_owned(),
                        json!({"type":"string","pattern":r"\S","not":{"pattern":r"\u0000"}}),
                    )
                })
                .collect();
            schema.insert("type".into(), json!("object"));
            schema.insert("properties".into(), Value::Object(properties));
            schema.insert("additionalProperties".into(), json!(false));
            schema.insert("maxProperties".into(), json!(4096));
        }
        "task_map" => {
            schema.insert("type".into(), json!("object"));
            schema.insert(
                "patternProperties".into(),
                json!({TASK_ORDINAL:{"type":"string","enum":entry.constraints["values"]}}),
            );
            schema.insert("additionalProperties".into(), json!(false));
            schema.insert("maxProperties".into(), json!(4096));
            schema.insert(
                "x-runtime-constraint".into(),
                json!("target task values must be unique"),
            );
        }
        "override_map" => {
            let properties: Map<String, Value> = registry
                .entries
                .values()
                .filter(|candidate| candidate.scope == "purpose" && !candidate.key.contains('<'))
                .map(|candidate| (candidate.key.clone(), value_schema(candidate, registry)))
                .collect();
            schema.insert("type".into(), json!("object"));
            schema.insert("properties".into(), Value::Object(properties));
            schema.insert("additionalProperties".into(), json!(false));
            schema.insert("maxProperties".into(), json!(4096));
        }
        other => panic!("unknown registry type: {other}"),
    }
    Value::Object(schema)
}

/// Strict shape for the flattened ConfigFile.values representation, not raw TOML syntax.
pub fn normalized_file(registry: &Registry) -> Value {
    let mut properties = Map::new();
    let mut patterns = Map::new();
    for entry in registry.entries.values() {
        let schema = value_schema(entry, registry);
        if entry.key.contains('<') {
            let pattern = entry
                .key
                .replace('.', r"\.")
                .replace("<purpose>", NAME)
                .replace("<name>", NAME);
            patterns.insert(format!("^{pattern}$"), schema);
        } else {
            properties.insert(entry.key.clone(), schema);
        }
    }
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Linguist normalized TOML v2 configuration",
        "description":"Validates ConfigFile.values after TOML flattening; the Rust resolver additionally validates cross-field relationships, URL details, language tags, and selected provider resources.",
        "type":"object",
        "properties":properties,
        "patternProperties":patterns,
        "additionalProperties":false,
        "required":["config.version"],
        "x-runtime-cross-field-rules":[
            "jobs.heartbeat_seconds < jobs.lease_seconds / 3",
            "retry.initial_backoff_seconds <= retry.max_backoff_seconds",
            "learning.examples_min <= learning.generated_examples_max",
            "llm.max_output_tokens < llm.context_tokens"
        ]
    })
}
