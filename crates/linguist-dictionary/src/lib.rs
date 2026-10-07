//! Dictionary responses are evidence, never instructions or an automatic sense selection.
use linguist_core::{DictionaryEntry, Language, Sense, canonical};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLimits,
    UnsupportedLanguage,
    InvalidQuery,
    BodyLimit,
    Schema,
    ProviderStatus,
    EntryLimit,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DICTIONARY_{self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Debug)]
pub struct DictionaryPage {
    pub query: String,
    pub request_url: String,
    pub raw_bytes: Vec<u8>,
    pub raw_digest: String,
    pub entries: Vec<DictionaryEntry>,
    pub exact_matches: Vec<usize>,
}
#[derive(Deserialize)]
struct Response {
    meta: Meta,
    data: Vec<RawEntry>,
}
#[derive(Deserialize)]
struct Meta {
    status: u16,
}
#[derive(Deserialize)]
struct RawEntry {
    #[serde(default)]
    slug: Option<String>,
    japanese: Vec<Form>,
    senses: Vec<RawSense>,
    #[serde(default)]
    is_common: bool,
    #[serde(default)]
    jlpt: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
#[derive(Deserialize, serde::Serialize)]
struct Form {
    #[serde(default)]
    word: Option<String>,
    #[serde(default)]
    reading: Option<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
#[derive(Deserialize, serde::Serialize)]
struct RawSense {
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
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
/// All entries are preserved in provider order. Exact matches are indexes, not selections.
pub fn parse_jisho(
    query: &str,
    target: &Language,
    bytes: &[u8],
    max_bytes: u64,
    max_entries: usize,
) -> Result<JishoPage, Error> {
    if max_bytes == 0 || max_bytes > 100 * 1024 * 1024 || !(1..=1000).contains(&max_entries) {
        return Err(Error::InvalidLimits);
    }
    if target.as_str().split('-').next() != Some("ja") {
        return Err(Error::UnsupportedLanguage);
    }
    if query.trim().is_empty()
        || query.chars().count() > 100000
        || query.chars().any(char::is_control)
    {
        return Err(Error::InvalidQuery);
    }
    if bytes.len() as u64 > max_bytes {
        return Err(Error::BodyLimit);
    }
    // The shared parser rejects duplicate keys and unsafe numeric payloads, including extensions.
    let response: Response = canonical::parse(bytes).map_err(|_| Error::Schema)?;
    if response.meta.status != 200 {
        return Err(Error::ProviderStatus);
    }
    if response.data.len() > max_entries {
        return Err(Error::EntryLimit);
    }
    let mut request_url =
        url::Url::parse("https://jisho.org/api/v1/search/words").map_err(|_| Error::Schema)?;
    request_url.query_pairs_mut().append_pair("keyword", query);
    let mut entries = Vec::new();
    let mut exact_matches = Vec::new();
    for raw in response.data {
        if raw.japanese.is_empty() || raw.senses.is_empty() {
            return Err(Error::Schema);
        }
        let mut forms = Vec::new();
        let mut readings = Vec::new();
        for form in &raw.japanese {
            let word = form.word.as_deref().unwrap_or("");
            let reading = form.reading.as_deref().unwrap_or("");
            if word.trim().is_empty() && reading.trim().is_empty() {
                return Err(Error::Schema);
            }
            if !word.is_empty() {
                forms.push(word.to_owned());
            }
            if !reading.is_empty() {
                readings.push(reading.to_owned());
            }
        }
        if forms.iter().chain(&readings).any(|form| form == query) {
            exact_matches.push(entries.len());
        }
        let mut metadata = BTreeMap::from([
            ("definition_language".into(), vec!["en".into()]),
            ("is_common".into(), vec![raw.is_common.to_string()]),
            ("jlpt".into(), raw.jlpt),
            ("tags".into(), raw.tags),
            (
                "written_form_pairs_json".into(),
                vec![serde_json::to_string(&raw.japanese).map_err(|_| Error::Schema)?],
            ),
            (
                "provider_extensions_json".into(),
                vec![serde_json::to_string(&raw.extensions).map_err(|_| Error::Schema)?],
            ),
        ]);
        let slug = raw
            .slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| forms.first().or_else(|| readings.first()).unwrap());
        let mut source = url::Url::parse("https://jisho.org/word/").map_err(|_| Error::Schema)?;
        source
            .path_segments_mut()
            .map_err(|_| Error::Schema)?
            .pop_if_empty()
            .push(slug);
        let mut senses = Vec::new();
        let mut related_entries = Vec::new();
        for sense in raw.senses {
            if sense.english_definitions.is_empty()
                || sense
                    .english_definitions
                    .iter()
                    .any(|s| s.trim().is_empty())
            {
                return Err(Error::Schema);
            }
            let key =
                canonical::digest("jisho-sense", &(slug, &sense)).map_err(|_| Error::Schema)?;
            // Keep restrictions/relationships/info attached to their sense, including unknown extensions.
            metadata.insert(
                format!("sense:{key}:raw_json"),
                vec![serde_json::to_string(&sense).map_err(|_| Error::Schema)?],
            );
            related_entries.extend(sense.see_also.iter().cloned());
            related_entries.extend(sense.antonyms.iter().cloned());
            let labels = sense
                .parts_of_speech
                .into_iter()
                .chain(sense.tags)
                .collect();
            senses.push(Sense {
                key,
                definitions: sense.english_definitions,
                labels,
                examples: vec![],
            });
        }
        entries.push(DictionaryEntry {
            provider: "jisho-api-v1".into(),
            source_url: source.into(),
            language: target.clone(),
            forms,
            readings,
            senses,
            metadata,
            related_entries,
        });
    }
    Ok(JishoPage {
        query: query.into(),
        request_url: request_url.into(),
        raw_bytes: bytes.to_vec(),
        raw_digest: canonical::asset_digest(bytes),
        entries,
        exact_matches,
    })
}

pub mod kanji;
pub mod recording;
pub mod transport;

pub type JishoPage = DictionaryPage;
pub mod wiktionary;
