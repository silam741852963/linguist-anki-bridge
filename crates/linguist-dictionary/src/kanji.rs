//! Provider-neutral Kanji summaries and an offline KANJIDIC2 fallback parser.

use serde::Deserialize;
use std::{collections::BTreeSet, future::Future, pin::Pin, time::Duration};

pub type KanjiFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<KanjiSummary>, String>> + Send + 'a>>;

pub trait KanjiLookupPort: Send + Sync {
    fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a>;
}

#[derive(Clone, Debug)]
pub struct KanjiApiClient {
    client: reqwest::Client,
    base: reqwest::Url,
    stroke_media_base: Option<String>,
}

impl KanjiApiClient {
    pub fn new() -> Result<Self, String> {
        Self::with_config(
            "https://kanjiapi.dev/v1/kanji/",
            Duration::from_secs(10),
            Some("https://raw.githubusercontent.com/KanjiVG/kanjivg/master/kanji"),
        )
    }

    pub fn with_config(
        base: &str,
        timeout: Duration,
        stroke_media_base: Option<&str>,
    ) -> Result<Self, String> {
        let base = reqwest::Url::parse(base).map_err(|error| error.to_string())?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err("Kanji API URL must be http(s) with a host".into());
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            base,
            stroke_media_base: stroke_media_base.map(str::to_owned),
        })
    }
}

impl KanjiLookupPort for KanjiApiClient {
    fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a> {
        Box::pin(async move {
            let url = self
                .base
                .join(&character.to_string())
                .map_err(|error| error.to_string())?;
            let response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(format!("Kanji API HTTP {}", response.status().as_u16()));
            }
            let body = response.text().await.map_err(|error| error.to_string())?;
            parse_kanji_api(character, &body, self.stroke_media_base.as_deref()).map(Some)
        })
    }
}

#[derive(Deserialize)]
struct KanjiApiResponse {
    kanji: String,
    #[serde(default)]
    meanings: Vec<String>,
    #[serde(default)]
    kun_readings: Vec<String>,
    #[serde(default)]
    on_readings: Vec<String>,
    stroke_count: Option<u16>,
}

pub fn parse_kanji_api(
    expected: char,
    body: &str,
    media_base: Option<&str>,
) -> Result<KanjiSummary, String> {
    let response: KanjiApiResponse =
        serde_json::from_str(body).map_err(|error| format!("Kanji API JSON: {error}"))?;
    let mut characters = response.kanji.chars();
    if characters.next() != Some(expected) || characters.next().is_some() {
        return Err(format!(
            "Kanji API returned a mismatched character for {expected}"
        ));
    }
    let readings = response
        .kun_readings
        .into_iter()
        .chain(response.on_readings)
        .collect();
    Ok(KanjiSummary {
        character: expected,
        meanings: response.meanings,
        readings,
        strokes: response.stroke_count,
        radical: None,
        stroke_order_url: media_base.and_then(|base| stroke_order_url(base, expected)),
    })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KanjiLookup {
    pub summaries: Vec<KanjiSummary>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KanjiSummary {
    pub character: char,
    pub meanings: Vec<String>,
    pub readings: Vec<String>,
    pub strokes: Option<u16>,
    pub radical: Option<String>,
    pub stroke_order_url: Option<String>,
}

/// Select the first usable provider response for a character. This makes a
/// network source optional: an offline KANJIDIC2 bundle can always be last.
pub fn first_available(
    character: char,
    candidates: impl IntoIterator<Item = Option<KanjiSummary>>,
) -> Option<KanjiSummary> {
    candidates
        .into_iter()
        .flatten()
        .find(|summary| summary.character == character)
}

pub async fn lookup_word<P: KanjiLookupPort + ?Sized>(
    provider: &P,
    word: &str,
    kanjidic2: &str,
    media_base: Option<&str>,
) -> KanjiLookup {
    let characters = word
        .chars()
        .filter(|character| is_cjk(*character))
        .collect::<BTreeSet<_>>();
    let mut result = KanjiLookup::default();
    for character in characters {
        match provider.lookup(character).await {
            Ok(Some(summary)) if summary.character == character => result.summaries.push(summary),
            Ok(_) => {
                if let Some(summary) = parse_kanjidic2(character, kanjidic2, media_base) {
                    result.summaries.push(summary);
                } else {
                    result
                        .warnings
                        .push(format!("No Kanji data for {character}"));
                }
            }
            Err(error) => {
                result
                    .warnings
                    .push(format!("Primary Kanji lookup for {character}: {error}"));
                if let Some(summary) = parse_kanjidic2(character, kanjidic2, media_base) {
                    result.summaries.push(summary);
                }
            }
        }
    }
    result
}

fn is_cjk(character: char) -> bool {
    matches!(u32::from(character), 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff)
}

pub fn cache_key(word: &str, provider_revision: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in provider_revision
        .bytes()
        .chain([0])
        .chain(word.trim().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("kanji-v1-{hash:016x}")
}
pub fn stroke_order_url(base: &str, character: char) -> Option<String> {
    let base = base.trim_end_matches('/');
    (!base.is_empty()).then(|| format!("{base}/{:05x}.svg", u32::from(character)))
}
/// Extract one matching `<character>` record without an XML runtime dependency.
/// KANJIDIC2 is trusted bundled data; malformed or absent fields remain optional.
pub fn parse_kanjidic2(
    character: char,
    xml: &str,
    media_base: Option<&str>,
) -> Option<KanjiSummary> {
    let literal = format!("<literal>{character}</literal>");
    let match_at = xml.find(&literal)?;
    let before = &xml[..match_at];
    let start = before.rfind("<character>")?;
    let after = &xml[match_at..];
    let end = after.find("</character>")? + match_at;
    let record = &xml[start..end];
    let meanings = tag_values(record, "meaning");
    let readings = tag_values(record, "reading");
    let strokes = tag_values(record, "stroke_count")
        .into_iter()
        .find_map(|value| value.parse().ok());
    let radical = tag_values(record, "rad_value").into_iter().next();
    Some(KanjiSummary {
        character,
        meanings,
        readings,
        strokes,
        radical,
        stroke_order_url: media_base.and_then(|base| stroke_order_url(base, character)),
    })
}
fn tag_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = xml;
    let mut values = Vec::new();
    while let Some(index) = rest.find(&open) {
        rest = &rest[index + open.len()..];
        let Some(open_end) = rest.find('>') else {
            break;
        };
        rest = &rest[open_end + 1..];
        let Some(end) = rest.find(&close) else { break };
        let value = rest[..end].trim();
        if !value.is_empty() {
            values.push(value.into());
        }
        rest = &rest[end + close.len()..];
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MissingProvider;
    impl KanjiLookupPort for MissingProvider {
        fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a> {
            Box::pin(async move {
                if character == '食' {
                    Err("offline".into())
                } else {
                    Ok(None)
                }
            })
        }
    }
    #[test]
    fn extracts_offline_fallback() {
        let summary=parse_kanjidic2('学',r#"<character><literal>学</literal><misc><stroke_count>8</stroke_count></misc><reading_meaning><rmgroup><reading r_type="ja_on">ガク</reading><meaning>study</meaning><meaning>learning</meaning></rmgroup></reading_meaning></character>"#,Some("https://assets.example/kanjivg/")).unwrap();
        assert_eq!(summary.strokes, Some(8));
        assert_eq!(summary.meanings, ["study", "learning"]);
        assert_eq!(
            summary.stroke_order_url.as_deref(),
            Some("https://assets.example/kanjivg/05b66.svg")
        );
    }

    #[test]
    fn parses_typed_kanji_api_response_and_rejects_mismatch() {
        let body = r#"{"kanji":"猫","meanings":["cat"],"kun_readings":["ねこ"],"on_readings":["ビョウ"],"stroke_count":11}"#;
        let summary = parse_kanji_api('猫', body, Some("https://raw.example/kanji")).unwrap();
        assert_eq!(summary.meanings, ["cat"]);
        assert_eq!(summary.readings, ["ねこ", "ビョウ"]);
        assert_eq!(summary.strokes, Some(11));
        assert_eq!(
            summary.stroke_order_url.as_deref(),
            Some("https://raw.example/kanji/0732b.svg")
        );
        assert!(parse_kanji_api('犬', body, None).is_err());
    }

    #[tokio::test]
    async fn falls_back_per_unique_character_and_keeps_warnings() {
        let xml = r#"<character><literal>食</literal><misc><stroke_count>9</stroke_count></misc><reading_meaning><rmgroup><meaning>eat</meaning></rmgroup></reading_meaning></character><character><literal>学</literal><reading_meaning><rmgroup><meaning>study</meaning></rmgroup></reading_meaning></character>"#;
        let result = lookup_word(
            &MissingProvider,
            "食学食abc",
            xml,
            Some("https://assets.test"),
        )
        .await;
        assert_eq!(
            result
                .summaries
                .iter()
                .map(|summary| summary.character)
                .collect::<Vec<_>>(),
            ['学', '食']
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result
                .summaries
                .iter()
                .all(|summary| summary.stroke_order_url.is_some())
        );
    }

    #[test]
    fn cache_identity_includes_provider_revision() {
        assert_ne!(cache_key("学", "primary-v1"), cache_key("学", "primary-v2"));
    }
}
