//! Typed local Ollama boundary. Provider JSON stays outside application/UI code.

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct OllamaClient {
    client: reqwest::Client,
    base: reqwest::Url,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OllamaError {
    Url(String),
    Transport(String),
    Http(u16),
    Parse(OllamaParseError),
}
impl std::fmt::Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(error) | Self::Transport(error) => f.write_str(error),
            Self::Http(status) => write!(f, "Ollama HTTP {status}"),
            Self::Parse(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for OllamaError {}
impl OllamaClient {
    pub fn new(base: &str) -> Result<Self, OllamaError> {
        let base =
            reqwest::Url::parse(base).map_err(|error| OllamaError::Url(error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(OllamaError::Url(
                "Ollama URL must be http(s) with a host".into(),
            ));
        }
        Ok(Self {
            client: reqwest::Client::new(),
            base,
        })
    }
    pub async fn models(&self) -> Result<Vec<String>, OllamaError> {
        let response = self
            .client
            .get(self.endpoint("api/tags")?)
            .send()
            .await
            .map_err(|error| OllamaError::Transport(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| OllamaError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(OllamaError::Http(status.as_u16()));
        }
        model_names(&body).map_err(OllamaError::Parse)
    }
    pub async fn generate_vocabulary(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<VocabularyGeneration, OllamaError> {
        match self.generate_vocabulary_once(model, prompt).await {
            Ok(generation) => Ok(generation),
            Err(OllamaError::Parse(_)) => {
                self.generate_vocabulary_once(
                    model,
                    &format!("{prompt}\nReturn only JSON with nuances and examples."),
                )
                .await
            }
            Err(error) => Err(error),
        }
    }
    async fn generate_vocabulary_once(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<VocabularyGeneration, OllamaError> {
        let response = self
            .client
            .post(self.endpoint("api/generate")?)
            .json(&vocabulary_request(model, prompt))
            .send()
            .await
            .map_err(|error| OllamaError::Transport(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| OllamaError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(OllamaError::Http(status.as_u16()));
        }
        let envelope: Value = serde_json::from_str(&body).map_err(|error| {
            OllamaError::Parse(OllamaParseError::InvalidJson(error.to_string()))
        })?;
        let raw = envelope
            .get("response")
            .or_else(|| envelope.get("thinking"))
            .and_then(Value::as_str)
            .ok_or(OllamaError::Parse(OllamaParseError::Missing("response")))?;
        normalize_vocabulary(raw).map_err(OllamaError::Parse)
    }
    fn endpoint(&self, path: &str) -> Result<reqwest::Url, OllamaError> {
        self.base
            .join(path)
            .map_err(|error| OllamaError::Url(error.to_string()))
    }
}
pub fn vocabulary_request(model: &str, prompt: &str) -> Value {
    json!({"model":model,"prompt":prompt,"stream":false,"format":{"type":"object","properties":{"nuances":{"type":"string"},"examples":{"type":"array","items":{"type":"object","properties":{"sentence":{"type":"string"},"translation":{"type":"string"}},"required":["sentence","translation"]}}},"required":["nuances","examples"]},"options":{"temperature":0.2}})
}
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VocabularyGeneration {
    pub nuances: String,
    pub examples: Vec<Example>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Example {
    pub sentence: String,
    pub translation: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OllamaParseError {
    Missing(&'static str),
    InvalidJson(String),
    InvalidShape,
}
impl std::fmt::Display for OllamaParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(key) => write!(f, "Ollama response is missing {key}"),
            Self::InvalidJson(error) => write!(f, "invalid Ollama JSON: {error}"),
            Self::InvalidShape => write!(f, "Ollama response must be a JSON object"),
        }
    }
}
impl std::error::Error for OllamaParseError {}

#[derive(Deserialize)]
struct Tags {
    #[serde(default)]
    models: Vec<Model>,
}
#[derive(Deserialize)]
struct Model {
    name: String,
}
pub fn model_names(body: &str) -> Result<Vec<String>, OllamaParseError> {
    serde_json::from_str::<Tags>(body)
        .map(|tags| tags.models.into_iter().map(|model| model.name).collect())
        .map_err(|error| OllamaParseError::InvalidJson(error.to_string()))
}

/// Accept language/model aliases and nested JSON envelopes, then discard every
/// field except generated nuance and example text.
pub fn normalize_vocabulary(raw: &str) -> Result<VocabularyGeneration, OllamaParseError> {
    let root: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| OllamaParseError::InvalidJson(error.to_string()))?;
    let mut queue = VecDeque::from([root]);
    let mut nuance = None;
    let mut examples = None;
    while let Some(value) = queue.pop_front() {
        let serde_json::Value::Object(object) = value else {
            continue;
        };
        for (key, value) in &object {
            let key = normalized(key);
            if nuance.is_none()
                && matches!(
                    key.as_str(),
                    "nuance" | "nuances" | "usage_nuance" | "usage_notes"
                )
            {
                nuance = Some(value.to_string().trim_matches('"').to_owned());
            }
            if examples.is_none()
                && matches!(key.as_str(), "examples" | "example_sentences" | "sentences")
            {
                examples = Some(value.clone());
            }
            if let serde_json::Value::Object(_) = value {
                queue.push_back(value.clone());
            }
            if let serde_json::Value::String(text) = value {
                if let Ok(nested) = serde_json::from_str(text) {
                    queue.push_back(nested);
                }
            }
        }
    }
    let nuances = nuance.ok_or(OllamaParseError::Missing("nuances"))?;
    let examples = examples.ok_or(OllamaParseError::Missing("examples"))?;
    let serde_json::Value::Array(rows) = examples else {
        return Err(OllamaParseError::InvalidShape);
    };
    Ok(VocabularyGeneration {
        nuances,
        examples: rows.into_iter().filter_map(example).collect(),
    })
}
fn normalized(key: &str) -> String {
    key.to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
        .collect()
}
fn example(value: serde_json::Value) -> Option<Example> {
    match value {
        serde_json::Value::String(sentence) => Some(Example {
            sentence,
            translation: String::new(),
        }),
        serde_json::Value::Object(object) => {
            let map: BTreeMap<_, _> = object
                .into_iter()
                .map(|(key, value)| (normalized(&key), value))
                .collect();
            let text = ["sentence", "example", "japanese", "text"]
                .into_iter()
                .find_map(|key| map.get(key)?.as_str())
                .unwrap_or_default();
            let translation = ["translation", "english", "meaning"]
                .into_iter()
                .find_map(|key| map.get(key)?.as_str())
                .unwrap_or_default();
            (!text.is_empty() || !translation.is_empty()).then(|| Example {
                sentence: text.into(),
                translation: translation.into(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_and_nested_json_normalize() {
        let result = normalize_vocabulary(r#"{"content":"{\"usage_nuance\":\"formal\",\"example_sentences\":[{\"japanese\":\"彼は俳優です。\",\"english\":\"He is an actor.\"}]}"}"#).unwrap();
        assert_eq!(result.nuances, "formal");
        assert_eq!(result.examples[0].translation, "He is an actor.");
    }
    #[test]
    fn tags_are_typed() {
        assert_eq!(
            model_names(r#"{"models":[{"name":"qwen"}]}"#).unwrap(),
            ["qwen"]
        );
    }
    #[test]
    fn generation_request_forces_nonstreaming_schema() {
        let request = vocabulary_request("model", "prompt");
        assert_eq!(request["stream"], false);
        assert_eq!(request["options"]["temperature"], 0.2);
        assert_eq!(
            request["format"]["required"],
            json!(["nuances", "examples"])
        );
    }
}
