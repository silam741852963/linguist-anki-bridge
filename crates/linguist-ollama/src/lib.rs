//! Typed local Ollama boundary. Provider JSON stays outside application/UI code.

use serde::Deserialize;
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
}
