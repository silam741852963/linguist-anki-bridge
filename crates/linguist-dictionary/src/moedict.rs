//! Typed parser for the public Moedict JSON shape.

use serde::Deserialize;
use std::time::Duration;

use crate::DictionaryError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoedictEntry {
    pub title: String,
    pub readings: Vec<String>,
    pub definitions: Vec<String>,
    pub audio_urls: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MoedictClient {
    client: reqwest::Client,
    base: reqwest::Url,
}

impl MoedictClient {
    pub fn new() -> Result<Self, DictionaryError> {
        Self::with_config("https://www.moedict.tw/", Duration::from_secs(10))
    }

    pub fn with_config(base: &str, timeout: Duration) -> Result<Self, DictionaryError> {
        let base =
            reqwest::Url::parse(base).map_err(|error| DictionaryError::Url(error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(DictionaryError::Url(
                "Moedict URL must be http(s) with a host".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        Ok(Self { client, base })
    }

    pub async fn search(&self, query: &str) -> Result<MoedictEntry, DictionaryError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| DictionaryError::Url("Moedict base URL cannot be a base".into()))?
            .extend(["a", &format!("{query}.json")]);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(DictionaryError::Http(status.as_u16()));
        }
        let body = response
            .text()
            .await
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        let entry = parse_json(&body).map_err(DictionaryError::Json)?;
        if entry.definitions.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        Ok(entry)
    }
}

pub fn cache_key(query: &str) -> String {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("moedict-v1-{hash:016x}")
}

pub fn parse_json(body: &str) -> Result<MoedictEntry, String> {
    let raw: Raw = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let mut readings = Vec::new();
    let mut definitions = Vec::new();
    let mut audio_urls = Vec::new();
    for heteronym in raw.heteronyms {
        if !heteronym.pinyin.is_empty() {
            readings.push(heteronym.pinyin);
        }
        definitions.extend(
            heteronym
                .definitions
                .into_iter()
                .map(|definition| definition.definition),
        );
        if !heteronym.audio_id.is_empty() {
            audio_urls.push(format!(
                "https://t.moedict.tw/mp3/{}.mp3",
                heteronym.audio_id
            ));
        }
    }
    Ok(MoedictEntry {
        title: raw.title,
        readings,
        definitions,
        audio_urls,
    })
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default, alias = "t")]
    title: String,
    #[serde(default, alias = "h")]
    heteronyms: Vec<Heteronym>,
}
#[derive(Deserialize)]
struct Heteronym {
    #[serde(default, alias = "T", alias = "p")]
    pinyin: String,
    #[serde(default, alias = "d")]
    definitions: Vec<Definition>,
    #[serde(default, rename = "_")]
    audio_id: String,
}
#[derive(Deserialize)]
struct Definition {
    #[serde(rename = "def", alias = "f", default)]
    definition: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_each_pronunciation_and_definition() {
        let entry=parse_json(r#"{"title":"學","heteronyms":[{"pinyin":"xué","definitions":[{"def":"學習。"}]},{"pinyin":"xiào","definitions":[{"def":"學校的簡稱。"}]}]}"#).unwrap();
        assert_eq!(entry.readings, ["xué", "xiào"]);
        assert_eq!(entry.definitions.len(), 2);
        assert!(entry.audio_urls.is_empty());
        let compact =
            parse_json(r#"{"t":"食","h":[{"T":"tsia̍h","_":"123","d":[{"f":"吃。"}]}]}"#).unwrap();
        assert_eq!(compact.readings, ["tsia̍h"]);
        assert_eq!(compact.definitions, ["吃。"]);
        assert_eq!(compact.audio_urls, ["https://t.moedict.tw/mp3/123.mp3"]);
    }

    #[test]
    fn cache_identity_normalizes_spacing() {
        assert_eq!(cache_key("  學  校 "), cache_key("學 校"));
        assert_ne!(cache_key("學"), cache_key("學校"));
    }
}
