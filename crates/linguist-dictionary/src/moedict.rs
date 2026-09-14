//! Typed parser for the public Moedict JSON shape.

use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoedictEntry {
    pub title: String,
    pub readings: Vec<String>,
    pub definitions: Vec<String>,
}

pub fn parse_json(body: &str) -> Result<MoedictEntry, String> {
    let raw: Raw = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let mut readings = Vec::new();
    let mut definitions = Vec::new();
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
    }
    Ok(MoedictEntry {
        title: raw.title,
        readings,
        definitions,
    })
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    title: String,
    #[serde(default)]
    heteronyms: Vec<Heteronym>,
}
#[derive(Deserialize)]
struct Heteronym {
    #[serde(default)]
    pinyin: String,
    #[serde(default)]
    definitions: Vec<Definition>,
}
#[derive(Deserialize)]
struct Definition {
    #[serde(rename = "def", default)]
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
    }
}
