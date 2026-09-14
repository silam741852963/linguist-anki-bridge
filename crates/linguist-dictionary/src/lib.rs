//! Jisho transport and parsing, with browser automation held behind a port.

use serde::Deserialize;
use std::{future::Future, pin::Pin, time::Duration};

pub mod cambridge;
pub mod dictcc;
pub mod moedict;

pub type BrowserFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, DictionaryError>> + Send + 'a>>;

/// Browser implementation belongs at the composition edge (e.g. Qt WebEngine),
/// never in the provider parser or application model.
pub trait BrowserFallback: Send + Sync {
    fn search<'a>(&'a self, query: &'a str) -> BrowserFuture<'a>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DictionaryEntry {
    pub word: String,
    pub reading: String,
    pub forms: Vec<WrittenForm>,
    pub senses: Vec<DictionarySense>,
    pub common: bool,
    pub jlpt: Vec<String>,
    pub tags: Vec<String>,
    pub exact: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrittenForm {
    pub word: String,
    pub reading: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DictionarySense {
    pub definitions: Vec<String>,
    pub parts_of_speech: Vec<String>,
    pub tags: Vec<String>,
    pub see_also: Vec<String>,
    pub antonyms: Vec<String>,
    pub info: Vec<String>,
    pub restrictions: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetryClass {
    Retryable,
    Permanent,
}
#[derive(Debug)]
pub enum DictionaryError {
    Url(String),
    Transport(String),
    Http(u16),
    Json(String),
    EmptyResult,
}
impl DictionaryError {
    pub fn retry_class(&self) -> RetryClass {
        match self {
            Self::Transport(_) | Self::Http(408 | 425 | 429 | 500..=599) => RetryClass::Retryable,
            _ => RetryClass::Permanent,
        }
    }
}
impl std::fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(e) | Self::Transport(e) | Self::Json(e) => f.write_str(e),
            Self::Http(s) => write!(f, "Jisho HTTP {s}"),
            Self::EmptyResult => f.write_str("Jisho returned no dictionary entries"),
        }
    }
}
impl std::error::Error for DictionaryError {}

#[derive(Clone, Debug)]
pub struct JishoClient {
    client: reqwest::Client,
    base: reqwest::Url,
    attempts: u8,
    backoff: Duration,
}
impl JishoClient {
    pub fn new() -> Result<Self, DictionaryError> {
        Self::with_config("https://jisho.org/", 3, Duration::from_millis(600))
    }
    pub fn with_config(
        base: &str,
        attempts: u8,
        backoff: Duration,
    ) -> Result<Self, DictionaryError> {
        let base = reqwest::Url::parse(base).map_err(|e| DictionaryError::Url(e.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(DictionaryError::Url(
                "Jisho URL must be http(s) with a host".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|e| DictionaryError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            base,
            attempts: attempts.clamp(1, 5),
            backoff,
        })
    }
    pub async fn search(
        &self,
        query: &str,
        browser: Option<&dyn BrowserFallback>,
    ) -> Result<Vec<DictionaryEntry>, DictionaryError> {
        let mut last = None;
        for attempt in 1..=self.attempts {
            match self.search_once(query).await {
                Ok(entries) if !entries.is_empty() => return Ok(entries),
                Ok(_) => last = Some(DictionaryError::EmptyResult),
                Err(error) if error.retry_class() == RetryClass::Retryable => last = Some(error),
                Err(error) => return Err(error),
            }
            if attempt < self.attempts {
                tokio::time::sleep(self.backoff.saturating_mul(u32::from(attempt))).await;
            }
        }
        if let Some(browser) = browser {
            return parse_jisho(query, &browser.search(query).await?);
        }
        Err(last.unwrap_or(DictionaryError::EmptyResult))
    }
    async fn search_once(&self, query: &str) -> Result<Vec<DictionaryEntry>, DictionaryError> {
        let mut url = self
            .base
            .join("api/v1/search/words")
            .map_err(|e| DictionaryError::Url(e.to_string()))?;
        url.query_pairs_mut().append_pair("keyword", query);
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| DictionaryError::Transport(e.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| DictionaryError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(DictionaryError::Http(status.as_u16()));
        }
        parse_jisho(query, &body)
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    data: Vec<Raw>,
}
#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    japanese: Vec<Japanese>,
    #[serde(default)]
    senses: Vec<Sense>,
    #[serde(default)]
    is_common: bool,
    #[serde(default)]
    jlpt: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}
#[derive(Deserialize)]
struct Japanese {
    #[serde(default)]
    word: String,
    #[serde(default)]
    reading: String,
}
#[derive(Deserialize)]
struct Sense {
    #[serde(default)]
    english_definitions: Vec<String>,
    #[serde(default)]
    parts_of_speech: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    see_also: Vec<String>,
    #[serde(default)]
    antonyms: Vec<String>,
    #[serde(default)]
    info: Vec<String>,
    #[serde(default)]
    restrictions: Vec<String>,
}
pub fn parse_jisho(query: &str, body: &str) -> Result<Vec<DictionaryEntry>, DictionaryError> {
    let response: Response =
        serde_json::from_str(body).map_err(|e| DictionaryError::Json(e.to_string()))?;
    let mut entries = response
        .data
        .into_iter()
        .filter_map(|raw| {
            let forms = raw
                .japanese
                .into_iter()
                .filter(|f| !f.word.is_empty() || !f.reading.is_empty())
                .map(|f| WrittenForm {
                    word: f.word,
                    reading: f.reading,
                })
                .collect::<Vec<_>>();
            let primary = forms.first()?;
            let word = if primary.word.is_empty() {
                primary.reading.clone()
            } else {
                primary.word.clone()
            };
            let reading = primary.reading.clone();
            let senses = raw
                .senses
                .into_iter()
                .filter(|s| !s.english_definitions.is_empty())
                .map(|s| DictionarySense {
                    definitions: s.english_definitions,
                    parts_of_speech: s
                        .parts_of_speech
                        .into_iter()
                        .filter(|v| !v.to_lowercase().contains("wikipedia definition"))
                        .collect(),
                    tags: s.tags,
                    see_also: s.see_also,
                    antonyms: s.antonyms,
                    info: s.info,
                    restrictions: s.restrictions,
                })
                .collect::<Vec<_>>();
            (!senses.is_empty()).then(|| DictionaryEntry {
                exact: forms.iter().any(|f| f.word == query || f.reading == query),
                word,
                reading,
                forms,
                senses,
                common: raw.is_common,
                jlpt: raw
                    .jlpt
                    .into_iter()
                    .map(|v| v.to_uppercase().replace("JLPT-", "JLPT "))
                    .collect(),
                tags: raw.tags.into_iter().map(display_jisho_tag).collect(),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| !entry.exact);
    Ok(entries)
}
fn display_jisho_tag(tag: String) -> String {
    if let Some(level) = tag
        .strip_prefix("wanikani")
        .filter(|n| n.chars().all(char::is_numeric))
    {
        format!("Wanikani level {level}")
    } else {
        tag.replace('_', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_entry_precedes_related() {
        let entries=parse_jisho("食べる",r#"{"data":[{"japanese":[{"word":"食べ物","reading":"たべもの"}],"senses":[{"english_definitions":["food"]}]},{"japanese":[{"word":"食べる","reading":"たべる"}],"is_common":true,"jlpt":["jlpt-n5"],"tags":["wanikani10"],"senses":[{"english_definitions":["to eat"],"parts_of_speech":["Wikipedia definition","verb"]}]}]}"#).unwrap();
        assert_eq!(entries[0].word, "食べる");
        assert_eq!(entries[0].senses[0].parts_of_speech, ["verb"]);
        assert_eq!(entries[0].tags, ["Wanikani level 10"]);
    }
    #[test]
    fn status_classifies_retry() {
        assert_eq!(
            DictionaryError::Http(429).retry_class(),
            RetryClass::Retryable
        );
        assert_eq!(
            DictionaryError::Http(404).retry_class(),
            RetryClass::Permanent
        );
    }
}
