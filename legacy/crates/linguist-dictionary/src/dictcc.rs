//! dict.cc result-table parser; callers own locale and transport selection.

use std::time::Duration;

use crate::DictionaryError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Translation {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug)]
pub struct DictCcClient {
    client: reqwest::Client,
    base: reqwest::Url,
}

impl DictCcClient {
    pub fn new(base: &str) -> Result<Self, DictionaryError> {
        Self::with_config(base, Duration::from_secs(10))
    }

    pub fn with_config(base: &str, timeout: Duration) -> Result<Self, DictionaryError> {
        let base =
            reqwest::Url::parse(base).map_err(|error| DictionaryError::Url(error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(DictionaryError::Url(
                "dict.cc URL must be http(s) with a host".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| DictionaryError::Transport(error.to_string()))?;
        Ok(Self { client, base })
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Translation>, DictionaryError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        let mut url = self.base.clone();
        url.query_pairs_mut().append_pair("s", query);
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
        let entries = parse_html(&body);
        if entries.is_empty() {
            return Err(DictionaryError::EmptyResult);
        }
        Ok(entries)
    }
}

pub fn cache_key(locale_base: &str, query: &str) -> String {
    let normalized = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in locale_base.bytes().chain([0]).chain(normalized.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("dictcc-v1-{hash:016x}")
}

pub fn parse_html(html: &str) -> Vec<Translation> {
    let cells = td_texts(html);
    cells
        .chunks_exact(2)
        .filter(|pair| !pair[0].is_empty() && !pair[1].is_empty())
        .map(|pair| Translation {
            source: pair[0].clone(),
            target: pair[1].clone(),
        })
        .collect()
}
fn td_texts(html: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<td") {
        rest = &rest[start..];
        let Some(open) = rest.find('>') else { break };
        rest = &rest[open + 1..];
        let Some(end) = rest.find("</td>") else { break };
        values.push(text(&rest[..end]));
        rest = &rest[end + 5..];
    }
    values
}
fn text(value: &str) -> String {
    let mut result = String::new();
    let mut tag = false;
    for ch in value.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => result.push(ch),
            _ => {}
        }
    }
    result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairs_result_columns() {
        assert_eq!(
            parse_html(
                r#"<tr><td>Haus</td><td><a>house</a></td></tr><tr><td>Heim</td><td>home</td></tr>"#
            ),
            vec![
                Translation {
                    source: "Haus".into(),
                    target: "house".into()
                },
                Translation {
                    source: "Heim".into(),
                    target: "home".into()
                }
            ]
        );
    }

    #[test]
    fn cache_identity_includes_locale() {
        assert_eq!(
            cache_key("https://deen.dict.cc/", "  Haus "),
            cache_key("https://deen.dict.cc/", "haus")
        );
        assert_ne!(
            cache_key("https://deen.dict.cc/", "Haus"),
            cache_key("https://defr.dict.cc/", "Haus")
        );
    }
}
