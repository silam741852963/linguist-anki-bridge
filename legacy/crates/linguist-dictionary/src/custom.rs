//! Normalization boundary for user-defined browser extraction schemas.

use std::{future::Future, pin::Pin, time::Duration};

use scraper::{Html, Selector};
use serde_json::Value;

use crate::DictionaryError;

pub type ExtractionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, DictionaryError>> + Send + 'a>>;

pub trait CustomExtractionPort: Send + Sync {
    fn extract<'a>(&'a self, url: &'a str, schema: &'a Value) -> ExtractionFuture<'a>;
}

/// Static-page adapter. Browser-rendered pages can use another port.
#[derive(Clone, Debug)]
pub struct HttpCssExtraction {
    client: reqwest::Client,
}

impl HttpCssExtraction {
    pub fn new() -> Result<Self, DictionaryError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        Ok(Self { client })
    }
}

impl CustomExtractionPort for HttpCssExtraction {
    fn extract<'a>(&'a self, url: &'a str, schema: &'a Value) -> ExtractionFuture<'a> {
        Box::pin(async move {
            const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
            let mut response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|error| DictionaryError::Transport(error.to_string()))?;
            if !response.status().is_success() {
                return Err(DictionaryError::Http(response.status().as_u16()));
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_HTML_BYTES as u64)
            {
                return Err(DictionaryError::Json(
                    "Dictionary page exceeds 2 MiB".into(),
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| DictionaryError::Transport(error.to_string()))?
            {
                if bytes.len() + chunk.len() > MAX_HTML_BYTES {
                    return Err(DictionaryError::Json(
                        "Dictionary page exceeds 2 MiB".into(),
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            let html = String::from_utf8(bytes)
                .map_err(|error| DictionaryError::Json(error.to_string()))?;
            extract_html(schema, &html).map_err(DictionaryError::Json)
        })
    }
}

pub fn extract_html(schema: &Value, html: &str) -> Result<String, String> {
    let base = schema
        .get("baseSelector")
        .and_then(Value::as_str)
        .ok_or("Extraction schema requires a baseSelector")?;
    let base = Selector::parse(base).map_err(|error| format!("Invalid base selector: {error}"))?;
    let fields = schema
        .get("fields")
        .and_then(Value::as_array)
        .ok_or("Extraction schema requires fields")?;
    let document = Html::parse_document(html);
    let mut entries = Vec::new();
    for root in document.select(&base).take(32) {
        let mut entry = serde_json::Map::new();
        for field in fields {
            let name = field
                .get("name")
                .and_then(Value::as_str)
                .ok_or("Field name missing")?;
            let css = field
                .get("selector")
                .and_then(Value::as_str)
                .ok_or("Field selector missing")?;
            let selector =
                Selector::parse(css).map_err(|error| format!("Invalid field selector: {error}"))?;
            let attribute = if field.get("type").and_then(Value::as_str) == Some("attribute") {
                Some(
                    field
                        .get("attribute")
                        .and_then(Value::as_str)
                        .ok_or("Attribute field requires attribute name")?,
                )
            } else {
                None
            };
            let values = root
                .select(&selector)
                .take(64)
                .filter_map(|element| {
                    let value = match attribute {
                        Some(attribute) => element.value().attr(attribute)?.trim().to_owned(),
                        None => element
                            .text()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_owned(),
                    };
                    (!value.is_empty()).then_some(Value::String(value))
                })
                .collect::<Vec<_>>();
            let value = if field.get("multiple").and_then(Value::as_bool) == Some(true) {
                Value::Array(values)
            } else {
                values.into_iter().next().unwrap_or(Value::Null)
            };
            entry.insert(name.to_owned(), value);
        }
        entries.push(Value::Object(entry));
    }
    serde_json::to_string(&entries).map_err(|error| error.to_string())
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
        let dictionary = Self {
            url_template,
            schema,
        };
        dictionary.request_url("validation")?;
        Ok(dictionary)
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
    let Some(base) = schema
        .get("baseSelector")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    else {
        return Err("Extraction schema requires a baseSelector".into());
    };
    Selector::parse(base).map_err(|error| format!("Invalid base selector: {error}"))?;
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
    for field in fields {
        let selector = field["selector"].as_str().expect("validated above");
        Selector::parse(selector).map_err(|error| format!("Invalid field selector: {error}"))?;
        match field.get("type").and_then(Value::as_str).unwrap_or("text") {
            "text" => {}
            "attribute"
                if field
                    .get("attribute")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.is_empty()) => {}
            "attribute" => return Err("Attribute field requires attribute name".into()),
            other => return Err(format!("Unsupported extraction field type: {other}")),
        }
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
        Some(Value::Null) => String::new(),
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        assert_eq!(
            parse_extracted("term", r#"[{"audio_url":null}]"#)
                .unwrap()
                .unwrap()
                .audio_url,
            None
        );
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
        assert!(
            CustomDictionary::new(
                "file:///tmp/{word}",
                json!({"baseSelector":".entry","fields":[{"name":"definition","selector":".def"}]})
            )
            .is_err()
        );
    }

    #[test]
    fn extracts_text_lists_and_attributes_from_html() {
        let schema = json!({"baseSelector":".entry","fields":[
            {"name":"word","selector":".head"},
            {"name":"definition","selector":".sense","multiple":true},
            {"name":"audio_url","selector":"audio","type":"attribute","attribute":"src"}
        ]});
        let html = r#"<article class="entry"><h2 class="head">sample</h2><p class="sense">first</p><p class="sense">second</p><audio src="https://example.test/a.mp3"></audio></article>"#;
        let extracted = extract_html(&schema, html).unwrap();
        let entry = parse_extracted("fallback", &extracted).unwrap().unwrap();
        assert_eq!(entry.word, "sample");
        assert_eq!(entry.definition, "first; second");
        assert_eq!(
            entry.audio_url.as_deref(),
            Some("https://example.test/a.mp3")
        );
        assert_eq!(
            parse_extracted("missing", &extract_html(&schema, "<p>none</p>").unwrap()).unwrap(),
            None
        );
    }

    #[test]
    fn rejects_invalid_css_and_field_types() {
        assert!(
            validate_schema(
                "https://x/{word}",
                &json!({"baseSelector":"[","fields":[{"name":"definition","selector":".def"}]})
            )
            .is_err()
        );
        assert!(
            validate_schema(
                "https://x/{word}",
                &json!({"baseSelector":".entry","fields":[{"name":"definition","selector":"["}]})
            )
            .is_err()
        );
        assert!(validate_schema("https://x/{word}", &json!({"baseSelector":".entry","fields":[{"name":"audio_url","selector":"audio","type":"attribute"}]})).is_err());
    }

    #[tokio::test]
    async fn searches_custom_page_through_http_adapter() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let count = stream.read(&mut request).await.unwrap();
            assert!(
                String::from_utf8_lossy(&request[..count]).starts_with("GET /search/ice%20cream ")
            );
            let body = "<article class='entry'><h2>ice cream</h2><p class='def'>frozen dessert</p></article>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let dictionary = CustomDictionary::new(
            format!("http://{address}/search/{{word}}"),
            json!({"baseSelector":".entry","fields":[
                {"name":"word","selector":"h2"},
                {"name":"definition","selector":".def"}
            ]}),
        )
        .unwrap();
        let entry = dictionary
            .search("ice cream", &HttpCssExtraction::new().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.word, "ice cream");
        assert_eq!(entry.definition, "frozen dessert");
        server.await.unwrap();
    }
}
