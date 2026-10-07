//! Native-speaker pronunciation recordings (`audio.provider=dictionary`).
//!
//! Japanese only: the JapanesePod101 dictionary audio endpoint, keyed by the
//! written form and its kana reading. A word without a recording returns a
//! fixed placeholder clip, which is reported as "not found", never staged.
use linguist_config::Effective;
use linguist_provider::{ReadError, Reader, Service, sha256_hex};
use std::collections::BTreeMap;

pub const ENDPOINT: &str = "https://assets.languagepod101.com/dictionary/japanese/audiomp3.php";
/// The endpoint host and the CDN it redirects recordings to.
pub const HOSTS: [&str; 2] = ["assets.languagepod101.com", "cdn.innovativelanguage.com"];
/// SHA-256 of the "this word has no recording yet" placeholder clip.
pub const PLACEHOLDER_SHA256: &str =
    "ae6398b5a27bc8c0a771df6c907ade794be15518174773c58c7c7ddd17098906";
pub const ATTRIBUTION: &str = "Native speaker recording from the JapanesePod101 dictionary";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Recording {
    pub provider: &'static str,
    pub source_url: String,
    pub sha256: String,
    pub fetched_at: u64,
    pub from_cache: bool,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// MPEG audio frame sync or an ID3v2 header.
pub fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || (bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
}

pub struct RecordingClient {
    reader: Reader,
}

impl RecordingClient {
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, ReadError> {
        if settings
            .values
            .get("audio.provider")
            .and_then(|v| v.as_str())
            != Some("dictionary")
        {
            return Err(ReadError::Unavailable);
        }
        let reader = Reader::from_settings(settings, environment, Service::Tts, &HOSTS, &[])?;
        Ok(Self { reader })
    }

    pub fn with_reader(reader: Reader) -> Self {
        Self { reader }
    }

    pub fn url(expression: &str, kana: &str) -> Result<url::Url, ReadError> {
        let mut url = url::Url::parse(ENDPOINT).map_err(|_| ReadError::Policy)?;
        url.query_pairs_mut()
            .append_pair("kanji", expression)
            .append_pair("kana", kana);
        Ok(url)
    }

    /// One recording, or `Ok(None)` when the dictionary has none for this word.
    pub fn japanese(&self, expression: &str, kana: &str) -> Result<Option<Recording>, ReadError> {
        let (expression, kana) = (expression.trim(), kana.trim());
        if expression.is_empty() || kana.is_empty() {
            return Err(ReadError::Policy);
        }
        let url = Self::url(expression, kana)?;
        let fetched = self.reader.get(&url, &["audio/mpeg"])?;
        if !is_mp3(&fetched.bytes) {
            return Err(ReadError::Schema);
        }
        let sha256 = sha256_hex(&fetched.bytes);
        if sha256 == PLACEHOLDER_SHA256 {
            return Ok(None);
        }
        Ok(Some(Recording {
            provider: "japanesepod101",
            source_url: fetched.final_url.to_string(),
            sha256,
            fetched_at: fetched.fetched_at,
            from_cache: fetched.from_cache,
            bytes: fetched.bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_encodes_both_keys_and_mp3_signatures_are_recognised() {
        let url = RecordingClient::url("歩留まり", "ぶどまり").unwrap();
        assert_eq!(url.host_str(), Some(HOSTS[0]));
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs[0].1, "歩留まり");
        assert_eq!(pairs[1].1, "ぶどまり");
        assert!(is_mp3(b"ID3\x04"));
        assert!(is_mp3(&[0xFF, 0xFB, 0x90]));
        assert!(!is_mp3(b"<html>"));
    }
}
