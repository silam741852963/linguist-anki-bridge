//! Cambridge Dictionary entry pages (`cambridge-html-v1`) as inert evidence.
//!
//! American English first: the American dictionary block (`cacd`) supplies the
//! entries, senses, US IPA and US audio. Every other block on the page (for
//! example the British `cald4`) only adds word-level extras by part of speech:
//! synonyms, related words, SMART Vocabulary and illustrations. Page text is
//! read with fixed selectors; links and scripts are never followed.
use crate::{DictionaryPage, Error};
use linguist_core::{DictionaryEntry, Example, Language, Provenance, Sense, canonical};
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;

pub const PROVIDER: &str = "cambridge-html-v1";
pub const ORIGIN: &str = "https://dictionary.cambridge.org";
pub const PAGE: &str = "https://dictionary.cambridge.org/dictionary/english/";
/// Bounds on word-level extras kept per entry.
const MAX_WORDS: usize = 24;

fn select(css: &str) -> Selector {
    Selector::parse(css).expect("static selector")
}
fn text(node: ElementRef) -> String {
    node.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn first(node: ElementRef, css: &str) -> Option<String> {
    node.select(&select(css))
        .next()
        .map(text)
        .filter(|t| !t.is_empty())
}
fn all(node: ElementRef, css: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in node.select(&select(css)).map(text) {
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}
fn absolute(path: &str) -> Option<String> {
    let path = path.trim();
    if path.starts_with('/') && !path.starts_with("//") {
        Some(format!(
            "{ORIGIN}{}",
            path.split('?').next().unwrap_or(path)
        ))
    } else {
        None
    }
}

/// Word-level extras of one part of speech, from any dictionary block.
#[derive(Default)]
struct Extras {
    synonyms: Vec<String>,
    related: Vec<String>,
    smart_topic: Vec<String>,
    smart_words: Vec<String>,
    images: Vec<String>,
    ipa_us: Vec<String>,
    ipa_uk: Vec<String>,
    audio_us: Vec<String>,
}
fn merge(into: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !into.contains(&value) && into.len() < MAX_WORDS {
            into.push(value);
        }
    }
}

pub fn request_url(query: &str) -> Result<url::Url, Error> {
    let mut url = url::Url::parse(PAGE).map_err(|_| Error::Schema)?;
    url.path_segments_mut()
        .map_err(|_| Error::Schema)?
        .pop_if_empty()
        .push(&query.replace(' ', "-"));
    Ok(url)
}

/// Parse one entry page. A page without dictionary blocks (Cambridge
/// redirects unknown words to its home page) yields no entries.
pub fn parse_page(
    query: &str,
    target: &Language,
    bytes: &[u8],
    max_bytes: u64,
    max_entries: usize,
) -> Result<DictionaryPage, Error> {
    if max_bytes == 0 || max_entries == 0 {
        return Err(Error::InvalidLimits);
    }
    if target.as_str().split('-').next() != Some("en") {
        return Err(Error::UnsupportedLanguage);
    }
    let query = query.trim();
    if query.is_empty() || query.chars().any(char::is_control) {
        return Err(Error::InvalidQuery);
    }
    if bytes.len() as u64 > max_bytes {
        return Err(Error::BodyLimit);
    }
    let html = std::str::from_utf8(bytes).map_err(|_| Error::Schema)?;
    let document = Html::parse_document(html);
    let mut blocks: Vec<_> = document.select(&select("div.pr.dictionary")).collect();
    let id = |block: &ElementRef| block.value().attr("data-id").unwrap_or("").to_owned();
    // American dictionary first, so its IPA and audio lead every extra.
    if let Some(index) = blocks.iter().position(|block| id(block) == "cacd") {
        let american = blocks.remove(index);
        blocks.insert(0, american);
    }
    // Extras by part of speech across all blocks.
    let mut extras: BTreeMap<String, Extras> = BTreeMap::new();
    for block in &blocks {
        for entry in block.select(&select(".entry-body__el")) {
            let pos = first(entry, ".posgram .pos").unwrap_or_default();
            let slot = extras.entry(pos).or_default();
            merge(&mut slot.synonyms, all(entry, ".xref.synonyms .x-h"));
            merge(&mut slot.related, all(entry, ".xref.related_word .x-h"));
            for runon in entry.select(&select(".runon")) {
                if let Some(word) = first(runon, ".runon-title .w") {
                    let pos = first(runon, ".pos")
                        .map(|p| format!(" ({p})"))
                        .unwrap_or_default();
                    merge(&mut slot.related, vec![format!("{word}{pos}")]);
                }
            }
            // One SMART Vocabulary topic per part of speech, with its own words.
            if slot.smart_topic.is_empty()
                && let Some(smart) = entry.select(&select(".smartt")).next()
            {
                merge(
                    &mut slot.smart_topic,
                    all(smart, ".daccord_lt a").into_iter().take(1).collect(),
                );
                merge(&mut slot.smart_words, all(smart, ".daccord_lb .hw"));
            }
            let images: Vec<String> = entry
                .select(&select(".dimg amp-img"))
                .filter_map(|img| img.value().attr("src"))
                .filter_map(absolute)
                .map(|src| src.replacen("/images/thumb/", "/images/full/", 1))
                .collect();
            merge(&mut slot.images, images);
            if let Some(header) = entry.select(&select(".pos-header")).next() {
                merge(&mut slot.ipa_us, all(header, ".us .ipa"));
                merge(&mut slot.ipa_uk, all(header, ".uk .ipa"));
                let audio: Vec<String> = header
                    .select(&select(".us source[type=\"audio/mpeg\"]"))
                    .filter_map(|s| s.value().attr("src"))
                    .filter_map(absolute)
                    .collect();
                merge(&mut slot.audio_us, audio);
            }
        }
    }
    // The primary (American) block's entries, then any part of speech only
    // another block defines (American run-ons carry no definition).
    let request = request_url(query)?;
    let mut source = request.clone();
    source.set_fragment(None);
    let mut entries = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for (rank, block) in blocks.iter().enumerate() {
        let mut found_here: Vec<String> = Vec::new();
        for entry in block.select(&select(".entry-body__el")) {
            let Some(headword) = first(entry, ".di-title .hw, .headword .hw") else {
                continue;
            };
            let pos = first(entry, ".posgram .pos").unwrap_or_default();
            if rank > 0 && (covered.contains(&pos) || pos.is_empty()) {
                continue;
            }
            found_here.push(pos.clone());
            let mut senses = Vec::new();
            for sense in entry.select(&select(".dsense")) {
                let guideword = first(sense, ".dsense_h .guideword").map(|g| {
                    g.trim_matches(|c| c == '(' || c == ')' || c == ' ')
                        .to_lowercase()
                });
                for def in sense.select(&select(".def-block")) {
                    let Some(definition) = first(def, ".ddef_h .def") else {
                        continue;
                    };
                    let definition = definition.trim_end_matches(':').trim().to_owned();
                    let mut labels = vec![];
                    if !pos.is_empty() {
                        labels.push(pos.clone());
                    }
                    if let Some(guideword) = &guideword {
                        labels.push(guideword.clone());
                    }
                    let examples = all(def, ".def-body .examp .eg")
                        .into_iter()
                        .map(|sentence| Example {
                            sentence,
                            translation: String::new(),
                            provenance: Provenance::Dictionary,
                            evidence_ids: vec![],
                        })
                        .collect();
                    let key = canonical::digest(
                        "cambridge-sense",
                        &(id(block), &headword, &pos, &definition),
                    )
                    .map_err(|_| Error::Schema)?;
                    senses.push(Sense {
                        key,
                        definitions: vec![definition],
                        labels,
                        examples,
                    });
                }
            }
            if senses.is_empty() {
                continue;
            }
            let slot = extras.get(&pos);
            let mut metadata = BTreeMap::from([
                ("definition_language".into(), vec!["en".into()]),
                ("dictionary".into(), vec![id(block)]),
                ("part_of_speech".into(), vec![pos.clone()]),
            ]);
            if let Some(slot) = slot {
                for (key, values) in [
                    ("synonyms", &slot.synonyms),
                    ("smart_vocabulary_topic", &slot.smart_topic),
                    ("smart_vocabulary", &slot.smart_words),
                    ("images", &slot.images),
                    ("ipa_us", &slot.ipa_us[..slot.ipa_us.len().min(1)].to_vec()),
                    ("ipa_uk", &slot.ipa_uk[..slot.ipa_uk.len().min(1)].to_vec()),
                    (
                        "audio_us",
                        &slot.audio_us[..slot.audio_us.len().min(1)].to_vec(),
                    ),
                ] {
                    if !values.is_empty() {
                        metadata.insert(key.into(), values.clone());
                    }
                }
            }
            entries.push(DictionaryEntry {
                provider: PROVIDER.into(),
                source_url: source.to_string(),
                language: target.clone(),
                forms: vec![headword],
                readings: vec![],
                senses,
                metadata,
                related_entries: slot.map(|s| s.related.clone()).unwrap_or_default(),
            });
        }
        covered.extend(found_here);
    }
    if entries.len() > max_entries {
        return Err(Error::EntryLimit);
    }
    let exact_matches = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.forms.iter().any(|f| f.eq_ignore_ascii_case(query)))
        .map(|(i, _)| i)
        .collect();
    Ok(DictionaryPage {
        query: query.into(),
        request_url: request.to_string(),
        raw_digest: canonical::asset_digest(bytes),
        raw_bytes: bytes.to_vec(),
        entries,
        exact_matches,
    })
}

/// Reads Cambridge media (US pronunciation MP3s, entry illustrations) named
/// by a parsed page. Only `dictionary.cambridge.org` is reachable.
pub struct MediaClient {
    reader: linguist_provider::Reader,
}
impl MediaClient {
    pub fn from_settings(
        settings: &linguist_config::Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, linguist_provider::ReadError> {
        let reader = linguist_provider::Reader::from_settings(
            settings,
            environment,
            linguist_provider::Service::Dictionary,
            &["dictionary.cambridge.org"],
            &[],
        )?;
        Ok(Self { reader })
    }
    pub fn fetch(
        &self,
        url: &str,
        accept: &[&str],
    ) -> Result<Vec<u8>, linguist_provider::ReadError> {
        let url = url::Url::parse(url).map_err(|_| linguist_provider::ReadError::Policy)?;
        if url.origin().ascii_serialization() != ORIGIN {
            return Err(linguist_provider::ReadError::Policy);
        }
        Ok(self.reader.get(&url, accept)?.bytes)
    }
}
