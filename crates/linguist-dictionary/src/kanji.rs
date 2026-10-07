//! Jisho kanji detail pages (`builtin:jisho-kanji-v2`) as rich, inert evidence.
//!
//! Page text is extracted with fixed selectors only; markup, links and scripts are
//! never executed or rendered. A page without kanji details is "not found", never
//! invented data.
use linguist_config::Effective;
use linguist_provider::{ReadError, Reader, Service, sha256_hex};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "builtin:jisho-kanji-v2";
/// Bound the number of characters looked up for one expression.
pub const MAX_CHARACTERS: usize = 32;
const SETTINGS: [&str; 5] = [
    "kanji.enabled",
    "kanji.explanation_language",
    "kanji.url_template",
    "kanji.schema",
    "kanji.stroke_order",
];
/// Stroke-order animations (KanjiVG-derived, CC BY-SA 3.0), one GIF per code point.
pub const STROKE_ORDER_URL: &str =
    "https://raw.githubusercontent.com/mistval/kanji_images/master/gifs/{codepoint}.gif";
pub const STROKE_ORDER_HOST: &str = "raw.githubusercontent.com";
pub const STROKE_ORDER_ATTRIBUTION: &str =
    "Stroke order animation from mistval/kanji_images, based on KanjiVG by Ulrich Apel";
pub const STROKE_ORDER_LICENSE: &str = "CC-BY-SA-3.0";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KanjiEntry {
    pub character: String,
    pub meanings: Vec<String>,
    pub kun_readings: Vec<String>,
    pub on_readings: Vec<String>,
    pub strokes: Option<u32>,
    pub radical: Option<String>,
    pub parts: Vec<String>,
    pub grade: Option<String>,
    pub jlpt: Option<String>,
    pub frequency: Option<u32>,
    pub schema: &'static str,
    pub source_url: String,
    pub raw_digest: String,
    pub fetched_at: u64,
    pub from_cache: bool,
    /// Exact page bytes for archival; excluded from serialized summaries.
    #[serde(skip)]
    pub raw_bytes: Vec<u8>,
}

/// GIF signature check; any other bytes are refused.
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

/// Applicable characters: CJK unified ideographs, extensions and compatibility forms.
pub fn is_kanji(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF)
}

/// Unique applicable characters in first-occurrence order.
pub fn characters(expression: &str) -> Vec<char> {
    let mut seen = Vec::new();
    for c in expression.chars().filter(|c| is_kanji(*c)) {
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Utf8,
    CharacterMismatch,
    Schema,
}

/// Parse one detail page. `Ok(None)` means the page has no kanji details.
pub fn parse_page(character: char, bytes: &[u8]) -> Result<Option<KanjiFacts>, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError::Utf8)?;
    let document = Html::parse_document(text);
    let select = |css: &str| Selector::parse(css).expect("static selector");
    let Some(details) = document.select(&select(".kanji.details")).next() else {
        return Ok(None);
    };
    let clean = visible_text;
    let all = |css: &str| -> Vec<String> {
        details
            .select(&select(css))
            .map(clean)
            .filter(|v| !v.is_empty())
            .collect()
    };
    let first = |css: &str| all(css).into_iter().next();
    let heading = first("h1.character").ok_or(ParseError::Schema)?;
    if heading != character.to_string() {
        return Err(ParseError::CharacterMismatch);
    }
    let meanings = first(".kanji-details__main-meanings")
        .map(|m| {
            m.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut facts = KanjiFacts {
        meanings,
        kun_readings: all(".kanji-details__main-readings .kun_yomi dd a"),
        on_readings: all(".kanji-details__main-readings .on_yomi dd a"),
        strokes: first(".kanji-details__stroke_count strong")
            .map(|s| s.parse().map_err(|_| ParseError::Schema))
            .transpose()?,
        radical: None,
        parts: Vec::new(),
        grade: first(".kanji_stats .grade strong"),
        jlpt: first(".kanji_stats .jlpt strong"),
        frequency: first(".kanji_stats .frequency strong")
            .map(|s| s.parse().map_err(|_| ParseError::Schema))
            .transpose()?,
    };
    for section in details.select(&select(".radicals dl")) {
        let label = section
            .select(&select("dt"))
            .next()
            .map(clean)
            .unwrap_or_default();
        let Some(value) = section.select(&select("dd")).next() else {
            continue;
        };
        match label.to_ascii_lowercase().trim_end_matches(':') {
            "radical" => {
                // Keep the radical form and its meaning as separate visible text.
                let meaning = value
                    .select(&select(".radical_meaning"))
                    .next()
                    .map(clean)
                    .unwrap_or_default();
                let whole = clean(value);
                let form = whole
                    .strip_prefix(&meaning)
                    .unwrap_or(&whole)
                    .trim()
                    .to_owned();
                facts.radical = Some(if meaning.is_empty() {
                    form
                } else {
                    format!("{form} — {meaning}")
                });
            }
            "parts" => facts.parts = value.select(&select("a")).map(clean).collect(),
            _ => {}
        }
    }
    if facts.meanings.is_empty() && facts.kun_readings.is_empty() && facts.on_readings.is_empty() {
        return Err(ParseError::Schema);
    }
    Ok(Some(facts))
}

/// Whitespace-normalized text, excluding script/style/template content.
fn visible_text(node: ElementRef) -> String {
    let mut parts = Vec::new();
    for descendant in node.descendants() {
        if let Some(text) = descendant.value().as_text() {
            let hidden = descendant
                .ancestors()
                .take_while(|a| a.id() != node.id())
                .any(|a| {
                    a.value()
                        .as_element()
                        .is_some_and(|e| matches!(e.name(), "script" | "style" | "template"))
                });
            if !hidden {
                parts.push(&**text);
            }
        }
    }
    parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct KanjiFacts {
    pub meanings: Vec<String>,
    pub kun_readings: Vec<String>,
    pub on_readings: Vec<String>,
    pub strokes: Option<u32>,
    pub radical: Option<String>,
    pub parts: Vec<String>,
    pub grade: Option<String>,
    pub jlpt: Option<String>,
    pub frequency: Option<u32>,
}

#[derive(Clone)]
pub struct KanjiClient {
    reader: Reader,
    template: String,
    stroke_order: bool,
}

impl KanjiClient {
    /// Only the pinned Jisho schema and English explanations are available;
    /// other schemas, hosts or languages are explicitly unavailable.
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, ReadError> {
        let registry = linguist_config::Registry::builtin();
        for key in SETTINGS {
            registry
                .validate_value(key, settings.values.get(key).ok_or(ReadError::Policy)?)
                .map_err(|_| ReadError::Policy)?;
        }
        let v = &settings.values;
        if v["kanji.enabled"] != true
            || v["kanji.schema"] != SCHEMA
            || v["kanji.explanation_language"]
                .as_str()
                .map(|l| l.split('-').next())
                != Some(Some("en"))
        {
            return Err(ReadError::Unavailable);
        }
        let template = v["kanji.url_template"].as_str().unwrap().to_owned();
        let probe =
            url::Url::parse(&template.replacen("{char}", "x", 1)).map_err(|_| ReadError::Policy)?;
        if template.matches("{char}").count() != 1
            || probe.scheme() != "https"
            || probe.host_str() != Some("jisho.org")
        {
            return Err(ReadError::Unavailable);
        }
        let stroke_order = v["kanji.stroke_order"] == true;
        let hosts: &[&str] = if stroke_order {
            &["jisho.org", STROKE_ORDER_HOST]
        } else {
            &["jisho.org"]
        };
        let reader = Reader::from_settings(settings, environment, Service::Kanji, hosts, &[])?;
        Ok(Self {
            reader,
            template,
            stroke_order,
        })
    }

    pub fn with_reader(reader: Reader, template: &str) -> Self {
        Self {
            reader,
            template: template.into(),
            stroke_order: false,
        }
    }

    /// The stroke-order GIF for one kanji. `Ok(None)`: disabled or not published.
    pub fn stroke_order(&self, character: char) -> Result<Option<Vec<u8>>, ReadError> {
        if !self.stroke_order {
            return Ok(None);
        }
        if !is_kanji(character) {
            return Err(ReadError::Policy);
        }
        let url = url::Url::parse(
            &STROKE_ORDER_URL.replace("{codepoint}", &format!("{:x}", character as u32)),
        )
        .map_err(|_| ReadError::Policy)?;
        match self.reader.get(&url, &["image/gif"]) {
            Ok(fetched) if is_gif(&fetched.bytes) => Ok(Some(fetched.bytes)),
            Ok(_) => Err(ReadError::Schema),
            Err(ReadError::Http(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn url(&self, character: char) -> Result<url::Url, ReadError> {
        let encoded = percent_encoding::utf8_percent_encode(
            &character.to_string(),
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        url::Url::parse(&self.template.replacen("{char}", &encoded, 1))
            .map_err(|_| ReadError::Policy)
    }

    /// Look up one character. `Ok(None)` is an ordinary not-found result.
    pub fn lookup(&self, character: char) -> Result<Option<KanjiEntry>, ReadError> {
        if !is_kanji(character) {
            return Err(ReadError::Policy);
        }
        let url = self.url(character)?;
        let fetched = self.reader.get(&url, &["text/html"])?;
        let Some(facts) = parse_page(character, &fetched.bytes).map_err(|_| ReadError::Schema)?
        else {
            return Ok(None);
        };
        Ok(Some(KanjiEntry {
            character: character.to_string(),
            meanings: facts.meanings,
            kun_readings: facts.kun_readings,
            on_readings: facts.on_readings,
            strokes: facts.strokes,
            radical: facts.radical,
            parts: facts.parts,
            grade: facts.grade,
            jlpt: facts.jlpt,
            frequency: facts.frequency,
            schema: SCHEMA,
            source_url: fetched.final_url.to_string(),
            raw_digest: sha256_hex(&fetched.bytes),
            fetched_at: fetched.fetched_at,
            from_cache: fetched.from_cache,
            raw_bytes: fetched.bytes,
        }))
    }

    /// Look up every applicable character in order; any failure fails the item.
    pub fn lookup_expression(
        &self,
        expression: &str,
    ) -> Result<Vec<Option<KanjiEntry>>, ReadError> {
        let characters = characters(expression);
        if characters.len() > MAX_CHARACTERS {
            return Err(ReadError::ResponseLimit);
        }
        characters.into_iter().map(|c| self.lookup(c)).collect()
    }
}

/// Plain-text reference for the Kanji field; rendering escapes it later.
pub fn render_reference(entries: &[KanjiEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let mut lines = vec![format!(
                "{} — {}",
                entry.character,
                entry.meanings.join(", ")
            )];
            if !entry.on_readings.is_empty() {
                lines.push(format!("On: {}", entry.on_readings.join("、")));
            }
            if !entry.kun_readings.is_empty() {
                lines.push(format!("Kun: {}", entry.kun_readings.join("、")));
            }
            if let Some(strokes) = entry.strokes {
                lines.push(format!("Strokes: {strokes}"));
            }
            if let Some(radical) = &entry.radical {
                lines.push(format!("Radical: {radical}"));
            }
            if let Some(jlpt) = &entry.jlpt {
                lines.push(format!("JLPT: {jlpt}"));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
