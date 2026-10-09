//! A dictionary-style entry for a word or phrase the dictionary does not list
//! (WP-20: 副委員長, 折り目をつける, 太陽に雲がかかる). The generation provider
//! chain (`llm.provider`, `llm.fallback`) writes readings, parts of speech and
//! numbered senses in the dictionary's own style, so the card's Meaning renders
//! like every other card. The entry is labelled generated, archived with the
//! provider's response and identity, and needs a reviewer's verification.
use crate::{agents, generation::GenerationRequest};
use linguist_config::Effective;
use linguist_core::{DictionaryEntry, Language, Sense, canonical};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub use linguist_core::document::GENERATED_DICTIONARY_PROVIDER as PROVIDER;

const PROMPT: &str = "You write one dictionary entry in the style of Jisho.org (JMdict) for a Japanese word or phrase that the dictionary does not list. \
Give its reading(s) in kana (the whole phrase, no spaces), keeping katakana where the word is written in katakana (エンジンがかかる, not えんじんがかかる), then 1 to 5 senses, most common first. \
Each sense has 1 to 6 short English definitions, as JMdict writes them (\"to make a crease\", \"vice-chairperson\"; no full sentences), \
and its parts of speech, chosen exactly from the allowed labels, as Jisho lists them: a phrase gets \"Expressions (phrases, clauses, etc.)\" \
followed by the conjugation class and transitivity of its final verb (for example \"Godan verb with 'ru' ending\", \"Intransitive verb\"); \
a compound noun gets \"Noun\". Use the reading hint and the learner's own note text when they are given; never invent rare meanings.";

/// The part-of-speech labels Jisho.org shows (JMdict entities), so a generated
/// entry renders like a dictionary one.
pub const PARTS_OF_SPEECH: &[&str] = &[
    "Noun",
    "Pronoun",
    "Noun which may take the genitive case particle 'no'",
    "Noun, used as a suffix",
    "Noun, used as a prefix",
    "Temporal noun",
    "Adverbial noun (fukushitekimeishi)",
    "Suru verb",
    "Suru verb - included",
    "Suru verb - special class",
    "Ichidan verb",
    "Godan verb with 'u' ending",
    "Godan verb with 'ku' ending",
    "Godan verb with 'gu' ending",
    "Godan verb with 'su' ending",
    "Godan verb with 'tsu' ending",
    "Godan verb with 'nu' ending",
    "Godan verb with 'bu' ending",
    "Godan verb with 'mu' ending",
    "Godan verb with 'ru' ending",
    "Godan verb - Iku/Yuku special class",
    "Kuru verb - special class",
    "Transitive verb",
    "Intransitive verb",
    "I-adjective (keiyoushi)",
    "I-Adjective (keiyoushi) - yoi/ii class",
    "Na-adjective (keiyodoshi)",
    "Pre-noun adjectival (rentaishi)",
    "Noun or verb acting prenominally",
    "Adverb (fukushi)",
    "Adverb taking the 'to' particle",
    "Auxiliary verb",
    "Auxiliary adjective",
    "Expressions (phrases, clauses, etc.)",
    "Conjunction",
    "Interjection (kandoushi)",
    "Particle",
    "Counter",
    "Prefix",
    "Suffix",
    "Numeric",
];

fn parts_of_speech_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "minItems": 1,
        "maxItems": 4,
        "items": {"type": "string", "enum": PARTS_OF_SPEECH},
    })
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedSense {
    pub definitions: Vec<String>,
    #[schemars(schema_with = "parts_of_speech_schema")]
    pub parts_of_speech: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedEntry {
    pub readings: Vec<String>,
    pub senses: Vec<GeneratedSense>,
}

/// The written entry and the evidence to archive with it.
#[derive(Debug)]
pub struct Written {
    pub entry: DictionaryEntry,
    pub provider: String,
    pub identity: Value,
    pub request: Value,
    pub raw: Vec<u8>,
}

fn kana(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}'))
}

/// Validate the provider's entry and build the dictionary entry from it.
pub fn entry_from(
    expression: &str,
    target: &Language,
    content: &str,
    provider: &str,
) -> Result<DictionaryEntry, String> {
    let invalid = || "DICTIONARY_ENTRY_INVALID".to_owned();
    let written: GeneratedEntry = serde_json::from_str(content).map_err(|_| invalid())?;
    let readings: Vec<String> = written
        .readings
        .iter()
        .map(|r| r.split_whitespace().collect::<String>())
        .collect();
    let line = |s: &String, max: usize| {
        !s.trim().is_empty() && s.chars().count() <= max && !s.contains(['\n', '\r'])
    };
    if !(1..=3).contains(&readings.len())
        || readings.iter().any(|r| !kana(r))
        || !(1..=5).contains(&written.senses.len())
        || written.senses.iter().any(|s| {
            !(1..=6).contains(&s.definitions.len())
                || s.definitions.iter().any(|d| !line(d, 120))
                || !(1..=4).contains(&s.parts_of_speech.len())
                || s.parts_of_speech
                    .iter()
                    .any(|p| !PARTS_OF_SPEECH.contains(&p.trim()))
        })
    {
        return Err(invalid());
    }
    // Kana written in the word stays as written in the reading (katakana
    // stays katakana).
    let katakana: String = expression
        .chars()
        .filter(|c| matches!(c, '\u{30A1}'..='\u{30FA}'))
        .collect();
    if !katakana.is_empty()
        && readings.iter().any(|r| {
            r.chars()
                .filter(|c| matches!(c, '\u{30A1}'..='\u{30FA}'))
                .collect::<String>()
                != katakana
        })
    {
        return Err("DICTIONARY_ENTRY_INVALID: the reading changes the word's katakana".into());
    }
    // A Godan label names the verb's final kana; it must be this word's.
    let last = expression.chars().last().unwrap_or_default();
    fn ending(label: &str, last: char) -> Option<(&str, char)> {
        label
            .strip_prefix("Godan verb with '")
            .and_then(|rest| rest.strip_suffix("' ending"))
            .map(|romaji| (romaji, last))
    }
    let godan_matches = |(romaji, kana): (&str, char)| {
        matches!(
            (romaji, kana),
            ("u", 'う')
                | ("ku", 'く')
                | ("gu", 'ぐ')
                | ("su", 'す')
                | ("tsu", 'つ')
                | ("nu", 'ぬ')
                | ("bu", 'ぶ')
                | ("mu", 'む')
                | ("ru", 'る')
        )
    };
    if written.senses.iter().any(|s| {
        s.parts_of_speech
            .iter()
            .filter_map(|p| ending(p.trim(), last))
            .any(|e| !godan_matches(e))
    }) {
        return Err("DICTIONARY_ENTRY_INVALID: the Godan ending does not match the word".into());
    }
    let senses = written
        .senses
        .into_iter()
        .enumerate()
        .map(|(index, sense)| {
            let definitions: Vec<String> = sense
                .definitions
                .iter()
                .map(|d| d.trim().to_owned())
                .collect();
            Ok(Sense {
                key: canonical::digest(
                    "generated-dictionary-sense",
                    &(expression, index, &definitions),
                )
                .map_err(|e| e.to_string())?,
                definitions,
                labels: sense
                    .parts_of_speech
                    .iter()
                    .map(|p| p.trim().to_owned())
                    .collect(),
                examples: vec![],
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DictionaryEntry {
        provider: PROVIDER.into(),
        source_url: String::new(),
        language: target.clone(),
        forms: vec![expression.to_owned()],
        readings,
        senses,
        metadata: BTreeMap::from([("generated_by".into(), vec![provider.to_owned()])]),
        related_entries: vec![],
    })
}

/// Write the entry with the first provider of the chain that succeeds.
pub fn write(
    expression: &str,
    reading_hint: &str,
    note_text: &str,
    target: &Language,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Written, String> {
    let schema =
        serde_json::to_value(schemars::schema_for!(GeneratedEntry)).map_err(|e| e.to_string())?;
    let user = json!({
        "expression": expression,
        "reading_hint": reading_hint,
        "learner_note": note_text,
        "target_language": target,
        "definition_language": "en",
    });
    let user_json = String::from_utf8(canonical::bytes(&user).map_err(|e| e.to_string())?)
        .map_err(|_| "DICTIONARY_ENTRY_ENCODING")?;
    let request = GenerationRequest {
        system_prompt: PROMPT.into(),
        user_json: user_json.clone(),
        output_schema: schema.clone(),
        prompt_digest: canonical::asset_digest(PROMPT.as_bytes()),
        schema_digest: canonical::asset_digest(
            &canonical::bytes(&schema).map_err(|e| e.to_string())?,
        ),
        input_digest: canonical::asset_digest(user_json.as_bytes()),
        allowed_fields: vec![],
        examples_requested: 0,
    };
    let recorded =
        json!({"system_prompt": PROMPT, "user_json": user_json, "output_schema": schema});
    let mut errors = Vec::new();
    for provider in agents::chain(settings)? {
        let attempt = match provider {
            agents::Provider::Ollama => {
                crate::ollama::transport::Client::from_settings(settings, environment)
                    .and_then(|client| client.complete(request_copy(&request)))
                    .map(|candidate| {
                        Some((
                            candidate.completion.content.clone(),
                            json!({"provider": "ollama", "model": candidate.evidence.identity.name,
                                   "digest": candidate.evidence.identity.digest}),
                            candidate.completion.raw.clone(),
                        ))
                    })
            }
            other => agents::generate(other, &request, settings, environment)
                .map(|e| e.map(|e| (e.content, e.identity, e.raw))),
        };
        match attempt {
            Ok(Some((content, identity, raw))) => {
                match entry_from(expression, target, &content, provider.name()) {
                    Ok(entry) => {
                        return Ok(Written {
                            entry,
                            provider: provider.name().into(),
                            identity,
                            request: recorded,
                            raw,
                        });
                    }
                    Err(error) => errors.push(format!("{}: {error}", provider.name())),
                }
            }
            Ok(None) => {}
            Err(error) => errors.push(format!("{}: {error}", provider.name())),
        }
    }
    Err(if errors.is_empty() {
        "DICTIONARY_ENTRY_UNAVAILABLE: no generation provider is available".into()
    } else {
        format!("DICTIONARY_ENTRY_FAILED: {}", errors.join("; "))
    })
}

fn request_copy(request: &GenerationRequest) -> GenerationRequest {
    GenerationRequest {
        system_prompt: request.system_prompt.clone(),
        user_json: request.user_json.clone(),
        output_schema: request.output_schema.clone(),
        prompt_digest: request.prompt_digest.clone(),
        schema_digest: request.schema_digest.clone(),
        input_digest: request.input_digest.clone(),
        allowed_fields: request.allowed_fields.clone(),
        examples_requested: request.examples_requested,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_entry_is_validated_and_shaped_like_a_dictionary_entry() {
        let ja: Language = "ja".to_owned().try_into().unwrap();
        let content = r#"{"readings":["ふく いいんちょう"],"senses":[{"definitions":["vice-chairperson","deputy chair"],"parts_of_speech":["Noun"]}]}"#;
        let entry = entry_from("副委員長", &ja, content, "claude_code").unwrap();
        assert_eq!(entry.provider, PROVIDER);
        assert_eq!(entry.forms, ["副委員長"]);
        assert_eq!(entry.readings, ["ふくいいんちょう"]);
        assert_eq!(entry.senses[0].labels, ["Noun"]);
        assert_eq!(entry.metadata["generated_by"], ["claude_code"]);
        for bad in [
            r#"{"readings":["fuku"],"senses":[{"definitions":["x"],"parts_of_speech":["Noun"]}]}"#,
            r#"{"readings":["ふく"],"senses":[{"definitions":["x"],"parts_of_speech":["Verb"]}]}"#,
            r#"{"readings":["めいわくがかかる"],"senses":[{"definitions":["x"],"parts_of_speech":["Godan verb with 'u' ending"]}]}"#,
            r#"{"readings":["ふく"],"senses":[]}"#,
            r#"{"readings":["ふく"],"senses":[{"definitions":["two\nlines"],"parts_of_speech":["Noun"]}]}"#,
            r#"{"readings":["ふく"],"senses":[{"definitions":["x"],"parts_of_speech":["Noun"]}],"extra":1}"#,
        ] {
            assert!(entry_from("副", &ja, bad, "x").is_err(), "{bad}");
        }
        let phrase = r#"{"readings":["めいわくがかかる"],"senses":[{"definitions":["to be troubled"],"parts_of_speech":["Expressions (phrases, clauses, etc.)","Godan verb with 'ru' ending"]}]}"#;
        assert!(entry_from("迷惑がかかる", &ja, phrase, "x").is_ok());
        let wrong = phrase.replace("'ru'", "'u'");
        assert!(entry_from("迷惑がかかる", &ja, &wrong, "x").is_err());
        let engine = r#"{"readings":["えんじんがかかる"],"senses":[{"definitions":["the engine starts"],"parts_of_speech":["Expressions (phrases, clauses, etc.)"]}]}"#;
        assert!(entry_from("エンジンがかかる", &ja, engine, "x").is_err());
        let engine = engine.replace("えんじん", "エンジン");
        assert!(entry_from("エンジンがかかる", &ja, &engine, "x").is_ok());
    }
}
