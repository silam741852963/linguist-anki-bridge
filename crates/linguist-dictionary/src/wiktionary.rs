//! Versioned Wiktionary definition-response parser. Transport compatibility is checked separately.
use crate::{DictionaryPage, Error};
use linguist_core::{DictionaryEntry, Example, Language, Provenance, Sense, canonical};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Section {
    part_of_speech: String,
    language: String,
    definitions: Vec<Definition>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Definition {
    definition: String,
    #[serde(default)]
    examples: Vec<String>,
    #[serde(default)]
    parsed_examples: Vec<ParsedExample>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize)]
struct ParsedExample {
    example: String,
    #[serde(default)]
    translation: Option<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
fn plain(value: &str) -> String {
    let cleaned = ammonia::Builder::default()
        .tags(HashSet::from(["br", "p", "div", "li"]))
        .generic_attributes(HashSet::new())
        .tag_attributes(std::collections::HashMap::new())
        .clean(value)
        .to_string();
    let text = cleaned
        .replace("<br>", "\n")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("<div>", "")
        .replace("</div>", "\n")
        .replace("<li>", "")
        .replace("</li>", "\n");
    html_escape::decode_html_entities(&text).into_owned()
}
pub fn parse_definition(
    query: &str,
    target: &Language,
    bytes: &[u8],
    max_bytes: u64,
    max_senses: usize,
) -> Result<DictionaryPage, Error> {
    if max_bytes == 0 || max_bytes > 100 * 1024 * 1024 || !(1..=1000).contains(&max_senses) {
        return Err(Error::InvalidLimits);
    }
    if target.as_str().split('-').next() != Some("en") {
        return Err(Error::UnsupportedLanguage);
    }
    if query.trim().is_empty()
        || query.chars().count() > 100000
        || query.chars().any(char::is_control)
        || matches!(query, "." | "..")
    {
        return Err(Error::InvalidQuery);
    }
    if bytes.len() as u64 > max_bytes {
        return Err(Error::BodyLimit);
    }
    // Preserve every language's original bytes, but never use another language as English fallback.
    let response: BTreeMap<String, Value> = canonical::parse(bytes).map_err(|_| Error::Schema)?;
    if response.contains_key("type") || response.contains_key("error") {
        return Err(Error::ProviderStatus);
    }
    for (language, value) in &response {
        Language::try_from(language.clone()).map_err(|_| Error::Schema)?;
        if !value.is_array() {
            return Err(Error::Schema);
        }
    }
    let sections: Vec<Section> = match response.get("en") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| Error::Schema)?,
        None => vec![],
    };
    if sections.len() > max_senses
        || sections.iter().map(|s| s.definitions.len()).sum::<usize>() > max_senses
    {
        return Err(Error::EntryLimit);
    }
    let mut request = url::Url::parse("https://en.wiktionary.org/api/rest_v1/page/definition/")
        .map_err(|_| Error::Schema)?;
    request
        .path_segments_mut()
        .map_err(|_| Error::Schema)?
        .pop_if_empty()
        .push(query);
    let mut source =
        url::Url::parse("https://en.wiktionary.org/wiki/").map_err(|_| Error::Schema)?;
    source
        .path_segments_mut()
        .map_err(|_| Error::Schema)?
        .pop_if_empty()
        .push(query);
    source.set_fragment(Some("English"));
    let mut entries = Vec::new();
    for section in sections {
        if section.language != "English"
            || section.part_of_speech.trim().is_empty()
            || section.definitions.is_empty()
        {
            return Err(Error::Schema);
        }
        let mut metadata = BTreeMap::from([
            ("definition_language".into(), vec!["en".into()]),
            ("language_section".into(), vec![section.language]),
            (
                "part_of_speech".into(),
                vec![section.part_of_speech.clone()],
            ),
            (
                "section_extensions_json".into(),
                vec![serde_json::to_string(&section.extensions).map_err(|_| Error::Schema)?],
            ),
            (
                "pronunciation_status".into(),
                vec!["not_exposed_by_definition_response".into()],
            ),
            ("attribution".into(), vec!["Wiktionary contributors".into()]),
            (
                "license_evidence".into(),
                vec!["not_supplied_by_definition_response".into()],
            ),
        ]);
        let mut senses = Vec::new();
        for definition in section.definitions {
            let text = plain(&definition.definition);
            if text.trim().is_empty() {
                return Err(Error::Schema);
            }
            let key = canonical::digest(
                "wiktionary-definition-v0.8",
                &(query, &section.part_of_speech, &definition),
            )
            .map_err(|_| Error::Schema)?;
            metadata.insert(
                format!("sense:{key}:raw_json"),
                vec![serde_json::to_string(&definition).map_err(|_| Error::Schema)?],
            );
            let examples = if definition.parsed_examples.is_empty() {
                definition
                    .examples
                    .iter()
                    .map(|example| Example {
                        sentence: plain(example),
                        translation: String::new(),
                        provenance: Provenance::Dictionary,
                        evidence_ids: vec![],
                    })
                    .collect::<Vec<_>>()
            } else {
                definition
                    .parsed_examples
                    .iter()
                    .map(|example| Example {
                        sentence: plain(&example.example),
                        translation: example
                            .translation
                            .as_deref()
                            .map(plain)
                            .unwrap_or_default(),
                        provenance: Provenance::Dictionary,
                        evidence_ids: vec![],
                    })
                    .collect()
            };
            if examples
                .iter()
                .any(|example| example.sentence.trim().is_empty())
            {
                return Err(Error::Schema);
            }
            senses.push(Sense {
                key,
                definitions: vec![text],
                labels: vec![section.part_of_speech.clone()],
                examples,
            });
        }
        entries.push(DictionaryEntry {
            provider: "wiktionary-definition-v0.8".into(),
            source_url: source.to_string(),
            language: target.clone(),
            forms: vec![query.into()],
            readings: vec![],
            senses,
            metadata,
            related_entries: vec![],
        });
    }
    Ok(DictionaryPage {
        query: query.into(),
        request_url: request.into(),
        raw_bytes: bytes.to_vec(),
        raw_digest: canonical::asset_digest(bytes),
        exact_matches: (0..entries.len()).collect(),
        entries,
    })
}
