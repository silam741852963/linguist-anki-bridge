//! Normalization boundary for user-defined browser extraction schemas.

use std::{future::Future, pin::Pin};

use serde_json::Value;

use crate::DictionaryError;

pub type ExtractionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, DictionaryError>> + Send + 'a>>;

pub trait CustomExtractionPort: Send + Sync {
    fn extract<'a>(&'a self, url: &'a str, schema: &'a Value) -> ExtractionFuture<'a>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomEntry {
    pub word: String,
    pub reading: String,
    pub definition: String,
    pub audio_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CustomDictionary {
    url_template: String,
    schema: Value,
}

impl CustomDictionary {
    pub fn new(url_template: impl Into<String>, schema: Value) -> Result<Self, String> {
        let url_template = url_template.into();
        validate_schema(&url_template, &schema)?;
        Ok(Self {
            url_template,
            schema,
        })
    }

    pub fn request_url(&self, query: &str) -> Result<String, String> {
        let query = query.trim();
        if query.is_empty() {
            return Err("Dictionary query cannot be empty".into());
        }
        let url = self.url_template.replace("{word}", &percent_encode(query));
        let parsed = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err("Dictionary URL must be http(s) with a host".into());
        }
        Ok(url)
    }

    pub async fn search(
        &self,
        query: &str,
        port: &dyn CustomExtractionPort,
    ) -> Result<Option<CustomEntry>, DictionaryError> {
        let url = self.request_url(query).map_err(DictionaryError::Url)?;
        let body = port.extract(&url, &self.schema).await?;
        parse_extracted(query, &body).map_err(DictionaryError::Json)
    }

    pub fn cache_key(&self, query: &str) -> String {
        let canonical_schema = serde_json::to_string(&self.schema).unwrap_or_default();
        stable_key(&self.url_template, &canonical_schema, query)
    }
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn stable_key(template: &str, schema: &str, query: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in template
        .bytes()
        .chain([0])
        .chain(schema.bytes())
        .chain([0])
        .chain(query.trim().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("custom-dictionary-v1-{hash:016x}")
}

pub fn validate_schema(url_template: &str, schema: &Value) -> Result<(), String> {
    if !url_template.contains("{word}") {
        return Err("Dictionary URL template must contain {word}".into());
    }
    let fields = schema
        .get("fields")
        .and_then(Value::as_array)
        .ok_or("Extraction schema requires fields")?;
    if schema
        .get("baseSelector")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return Err("Extraction schema requires a baseSelector".into());
    }
    if fields.is_empty()
        || fields.iter().any(|field| {
            field
                .get("name")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .is_none()
                || field
                    .get("selector")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .is_none()
        })
    {
        return Err("Each extraction field requires name and selector".into());
    }
    Ok(())
}
pub fn parse_extracted(query: &str, body: &str) -> Result<Option<CustomEntry>, String> {
    let values: Vec<Value> = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let Some(value) = values.first() else {
        return Ok(None);
    };
    let text = |key| match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; "),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
        None => String::new(),
    };
    Ok(Some(CustomEntry {
        word: {
            let word = text("word");
            if word.is_empty() { query.into() } else { word }
        },
        reading: text("reading"),
        definition: text("definition"),
        audio_url: {
            let audio = text("audio_url");
            (!audio.is_empty()).then_some(audio)
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_and_normalizes_extraction() {
        let schema =
            json!({"baseSelector":".entry","fields":[{"name":"definition","selector":".def"}]});
        assert!(validate_schema("https://x/{word}", &schema).is_ok());
        let entry = parse_extracted(
            "term",
            r#"[{"reading":"r","definition":["one","two"],"audio_url":"https://x/a.mp3"}]"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(entry.word, "term");
        assert_eq!(entry.definition, "one; two");
    }

    #[test]
    fn safely_builds_urls_and_invalidates_cache_for_schema_changes() {
        let first = CustomDictionary::new(
            "https://example.test/search/{word}",
            json!({"baseSelector":".entry","fields":[{"name":"definition","selector":".def"}]}),
        )
        .unwrap();
        let second = CustomDictionary::new(
            "https://example.test/search/{word}",
            json!({"baseSelector":".result","fields":[{"name":"definition","selector":".def"}]}),
        )
        .unwrap();
        assert_eq!(
            first.request_url("食べる / eat").unwrap(),
            "https://example.test/search/%E9%A3%9F%E3%81%B9%E3%82%8B%20%2F%20eat"
        );
        assert_ne!(first.cache_key("食べる"), second.cache_key("食べる"));
    }
}
