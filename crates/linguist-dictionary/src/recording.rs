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
        let hosts: Vec<&str> = HOSTS.iter().chain(&english::HOSTS).copied().collect();
        let reader = Reader::from_settings(settings, environment, Service::Tts, &hosts, &[])?;
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

/// What a Wiktionary page yields for one English word.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnglishLookup {
    pub page_url: String,
    pub page_sha256: String,
    pub pronunciation: english::Pronunciation,
    pub recording: Option<Recording>,
}

impl RecordingClient {
    /// IPA and pronunciation recording for one English word, or `Ok(None)`
    /// when Wiktionary has no page.
    pub fn english(&self, word: &str) -> Result<Option<EnglishLookup>, ReadError> {
        let word = word.trim();
        if word.is_empty() || word.chars().any(char::is_control) {
            return Err(ReadError::Policy);
        }
        let title = word.replace(' ', "_");
        let encoded =
            percent_encoding::utf8_percent_encode(&title, percent_encoding::NON_ALPHANUMERIC);
        let url = url::Url::parse(&format!("{}{encoded}", english::PAGE_URL))
            .map_err(|_| ReadError::Policy)?;
        let page = match self.reader.get(&url, &["text/html"]) {
            Ok(page) => page,
            Err(ReadError::Http(404)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let html = std::str::from_utf8(&page.bytes).map_err(|_| ReadError::Schema)?;
        let pronunciation = english::parse(html);
        let recording = match &pronunciation.audio_url {
            Some(audio) => {
                let audio = url::Url::parse(audio).map_err(|_| ReadError::Schema)?;
                let fetched = self.reader.get(&audio, &["audio/mpeg"])?;
                if !is_mp3(&fetched.bytes) {
                    return Err(ReadError::Schema);
                }
                Some(Recording {
                    provider: "wikimedia_commons",
                    source_url: fetched.final_url.to_string(),
                    sha256: sha256_hex(&fetched.bytes),
                    fetched_at: fetched.fetched_at,
                    from_cache: fetched.from_cache,
                    bytes: fetched.bytes,
                })
            }
            None => None,
        };
        Ok(Some(EnglishLookup {
            page_url: page.final_url.to_string(),
            page_sha256: sha256_hex(&page.bytes),
            pronunciation,
            recording,
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

/// English pronunciation from a Wiktionary page (Parsoid HTML of
/// `en.wiktionary.org/api/rest_v1/page/html/{word}`): the IPA and the
/// pronunciation recording of the English section.
pub mod english {
    use scraper::{ElementRef, Html, Selector};

    pub const PAGE_URL: &str = "https://en.wiktionary.org/api/rest_v1/page/html/";
    pub const HOSTS: [&str; 2] = ["en.wiktionary.org", "upload.wikimedia.org"];
    pub const ATTRIBUTION: &str =
        "Pronunciation recording from Wikimedia Commons via Wiktionary (CC BY-SA)";

    #[derive(Debug, Clone, PartialEq, serde::Serialize)]
    pub struct Pronunciation {
        /// The US/General American IPA, else the first listed one; `None`
        /// when several pronunciation sections exist (homographs).
        pub ipa: Option<String>,
        /// Every IPA line seen, for evidence.
        pub ipa_lines: Vec<(String, String)>,
        pub sections: usize,
        /// Transcoded MP3 of the preferred recording (US first).
        pub audio_url: Option<String>,
    }

    fn select(css: &str) -> Selector {
        Selector::parse(css).expect("static selector")
    }
    fn text(node: ElementRef) -> String {
        node.text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn heading_id(section: ElementRef) -> Option<String> {
        section
            .children()
            .filter_map(ElementRef::wrap)
            .find(|child| matches!(child.value().name(), "h2" | "h3" | "h4" | "h5"))
            .and_then(|heading| heading.value().attr("id").map(str::to_owned))
    }

    /// The IPA span texts and the plain text that belong to this list item
    /// itself, excluding nested lists.
    fn own_parts(item: ElementRef) -> (Vec<String>, String) {
        let mine = |node: &ElementRef| {
            node.ancestors()
                .filter_map(ElementRef::wrap)
                .find(|a| a.value().name() == "li")
                .is_some_and(|li| li.id() == item.id())
        };
        let spans = item
            .select(&select("span.IPA"))
            .filter(|span| mine(span))
            .map(text)
            .collect();
        let mut label = Vec::new();
        for node in item.descendants() {
            if let Some(t) = node.value().as_text()
                && let Some(parent) = node.parent().and_then(ElementRef::wrap)
                && (parent.id() == item.id() || mine(&parent))
            {
                label.push(t.to_string());
            }
        }
        let label = label
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (spans, label)
    }

    pub fn parse(html: &str) -> Pronunciation {
        let document = Html::parse_document(html);
        let english = document
            .select(&select("section"))
            .find(|section| heading_id(*section).as_deref() == Some("English"));
        let mut result = Pronunciation {
            ipa: None,
            ipa_lines: vec![],
            sections: 0,
            audio_url: None,
        };
        let Some(english) = english else {
            return result;
        };
        let sections: Vec<_> = english
            .select(&select("section"))
            .filter(|section| {
                heading_id(*section).is_some_and(|id| id.starts_with("Pronunciation"))
            })
            .collect();
        result.sections = sections.len();
        let Some(first) = sections.first() else {
            return result;
        };
        for item in first.select(&select("li")) {
            let (spans, label) = own_parts(item);
            let ipa = spans
                .into_iter()
                .find(|ipa| ipa.starts_with('/') || ipa.starts_with('['));
            // IPA lines only; audio lines repeat a transcription.
            if let Some(ipa) = ipa.filter(|_| label.contains("IPA"))
                && !result.ipa_lines.iter().any(|(_, seen)| *seen == ipa)
            {
                result.ipa_lines.push((label, ipa));
            }
        }
        if sections.len() == 1 {
            let american = |label: &str| {
                ["General American", "US", "GA"]
                    .iter()
                    .any(|accent| label.contains(accent))
            };
            result.ipa = result
                .ipa_lines
                .iter()
                .find(|(label, _)| american(label))
                .or(result.ipa_lines.first())
                .map(|(_, ipa)| ipa.clone());
            let audio: Vec<String> = first
                .select(&select("source"))
                .filter_map(|source| source.value().attr("src"))
                .filter(|src| src.contains("/transcoded/") && src.ends_with(".mp3"))
                .map(|src| format!("https:{}", src.trim_start_matches("https:")))
                .collect();
            result.audio_url = audio
                .iter()
                .find(|src| src.to_ascii_lowercase().contains("en-us"))
                .or(audio.first())
                .cloned();
        }
        result
    }
}
