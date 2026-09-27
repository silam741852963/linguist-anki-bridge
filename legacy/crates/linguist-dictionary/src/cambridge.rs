//! Cambridge HTML conversion. Transport/browser concerns stay outside this parser.

use std::time::Duration;

use crate::DictionaryError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CambridgeEntry {
    pub query: String,
    pub headword: String,
    pub definitions: Vec<String>,
    pub audio_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CambridgeClient {
    client: reqwest::Client,
    base: reqwest::Url,
}

impl CambridgeClient {
    pub fn new() -> Result<Self, DictionaryError> {
        Self::with_config("https://dictionary.cambridge.org/", Duration::from_secs(10))
    }

    pub fn with_config(base: &str, timeout: Duration) -> Result<Self, DictionaryError> {
        let base =
            reqwest::Url::parse(base).map_err(|error| DictionaryError::Url(error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(DictionaryError::Url(
                "Cambridge URL must be http(s) with a host".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        Ok(Self { client, base })
    }

    pub async fn search(&self, query: &str) -> Result<CambridgeEntry, DictionaryError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| DictionaryError::Url("Cambridge base URL cannot be a base".into()))?
            .extend(["dictionary", "english", query]);
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
        let entry = parse_html(query, &body);
        if entry.definitions.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        Ok(entry)
    }
}

pub fn cache_key(query: &str) -> String {
    provider_cache_key("cambridge-v1", query)
}

fn provider_cache_key(provider: &str, query: &str) -> String {
    let normalized = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in provider.bytes().chain([0]).chain(normalized.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{provider}-{hash:016x}")
}

/// Parse the stable definition and pronunciation URL markers emitted by
/// Cambridge's server-rendered dictionary pages. Missing optional sections are
/// deliberately tolerated: provider markup changes should not discard a card.
pub fn parse_html(query: &str, html: &str) -> CambridgeEntry {
    let definitions = class_texts(html, "def ddef_d db");
    let headword = class_texts(html, "hw dhw")
        .into_iter()
        .next()
        .unwrap_or_else(|| query.into());
    CambridgeEntry {
        query: query.into(),
        headword,
        definitions,
        audio_url: audio_url(html),
    }
}

fn class_texts(html: &str, needle: &str) -> Vec<String> {
    html.match_indices("class=\"")
        .filter_map(|(start, _)| {
            let class_start = start + 7;
            let class_end = html[class_start..].find('"')? + class_start;
            let classes = &html[class_start..class_end];
            if !classes.contains(needle) {
                return None;
            }
            let tag_start = html[..start].rfind('<')?;
            let tag_name = html[tag_start + 1..]
                .split(|ch: char| ch.is_whitespace() || ch == '>')
                .next()?;
            let content_start = html[class_end..].find('>')? + class_end + 1;
            let closing = format!("</{tag_name}");
            let content_end = html[content_start..].find(&closing)? + content_start;
            Some(normalize_html_text(&html[content_start..content_end]))
        })
        .filter(|text| !text.is_empty())
        .collect()
}
fn audio_url(html: &str) -> Option<String> {
    html.match_indices("https://dictionary.cambridge.org/")
        .find_map(|(start, _)| {
            let end = html[start..].find(".mp3")? + start + 4;
            Some(html[start..end].replace("&amp;", "&"))
        })
}
fn normalize_html_text(value: &str) -> String {
    let mut text = String::new();
    let mut inside = false;
    for ch in value.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => text.push(ch),
            _ => {}
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_defs_and_audio() {
        let entry = parse_html(
            "eat",
            r#"<span class="hw dhw">eat</span><div class="def ddef_d db">to <b>put</b> food in the mouth</div><source src="https://dictionary.cambridge.org/media.mp3">"#,
        );
        assert_eq!(entry.headword, "eat");
        assert_eq!(entry.definitions, ["to put food in the mouth"]);
        assert_eq!(
            entry.audio_url.as_deref(),
            Some("https://dictionary.cambridge.org/media.mp3")
        );
    }

    #[test]
    fn cache_identity_normalizes_case_and_spacing() {
        assert_eq!(cache_key("  Take  Off "), cache_key("take off"));
        assert_ne!(cache_key("take off"), cache_key("take on"));
    }

    #[test]
    fn rejects_invalid_transport_configuration() {
        assert!(CambridgeClient::with_config("file:///tmp", Duration::from_secs(1)).is_err());
    }
}
