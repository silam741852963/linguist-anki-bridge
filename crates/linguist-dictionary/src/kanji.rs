//! Provider-neutral Kanji summaries and an offline KANJIDIC2 fallback parser.

use base64::Engine;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::{collections::HashSet, future::Future, pin::Pin, time::Duration};

pub type KanjiFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<KanjiSummary>, String>> + Send + 'a>>;

pub trait KanjiLookupPort: Send + Sync {
    fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a>;
}

#[derive(Clone, Debug)]
pub struct KanjiMediaFetcher {
    client: reqwest::Client,
    base: reqwest::Url,
}

impl KanjiMediaFetcher {
    pub fn new() -> Result<Self, String> {
        Self::with_config(
            "https://raw.githubusercontent.com/mistval/kanji_images/master/gifs/",
            Duration::from_secs(10),
        )
    }

    pub fn with_config(base: &str, timeout: Duration) -> Result<Self, String> {
        let base = reqwest::Url::parse(base).map_err(|error| error.to_string())?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err("Kanji media URL must be http(s) with a host".into());
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("LinguistAnkiBridge/0.1")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client, base })
    }

    pub async fn gif_data_uri(&self, character: char) -> Result<String, String> {
        const MAX_GIF_BYTES: usize = 1024 * 1024;
        let url = self
            .base
            .join(&format!("{:x}.gif", u32::from(character)))
            .map_err(|error| error.to_string())?;
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Kanji stroke-order media HTTP {}",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_GIF_BYTES as u64)
        {
            return Err("Kanji stroke-order GIF exceeds 1 MiB".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
            if bytes.len() + chunk.len() > MAX_GIF_BYTES {
                return Err("Kanji stroke-order GIF exceeds 1 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
            return Err("Kanji stroke-order asset is not a GIF".into());
        }
        Ok(format!(
            "data:image/gif;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }
}

#[derive(Clone, Debug)]
pub struct JishoKanjiClient {
    client: reqwest::Client,
    base: reqwest::Url,
    stroke_media_base: Option<String>,
}

impl JishoKanjiClient {
    pub fn new() -> Result<Self, String> {
        Self::with_config(
            "https://jisho.org/search/",
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
            return Err("Jisho Kanji URL must be http(s) with a host".into());
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

impl KanjiLookupPort for JishoKanjiClient {
    fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a> {
        Box::pin(async move {
            let url = self
                .base
                .join(&format!("{character}%23kanji"))
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
                return Err(format!("Jisho Kanji HTTP {}", response.status().as_u16()));
            }
            let body = response.text().await.map_err(|error| error.to_string())?;
            Ok(parse_jisho_kanji(
                character,
                &body,
                self.stroke_media_base.as_deref(),
            ))
        })
    }
}

#[derive(Clone, Debug)]
pub struct HvdicKanjiClient {
    client: reqwest::Client,
    base: reqwest::Url,
    stroke_media_base: Option<String>,
}

impl HvdicKanjiClient {
    pub fn new() -> Result<Self, String> {
        Self::with_config(
            "https://hvdic.thivien.net/whv/",
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
            return Err("HVDic Kanji URL must be http(s) with a host".into());
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

impl KanjiLookupPort for HvdicKanjiClient {
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
                return Err(format!("HVDic Kanji HTTP {}", response.status().as_u16()));
            }
            let body = response.text().await.map_err(|error| error.to_string())?;
            Ok(parse_hvdic_kanji(
                character,
                &body,
                self.stroke_media_base.as_deref(),
            ))
        })
    }
}

pub fn parse_hvdic_kanji(
    character: char,
    body: &str,
    media_base: Option<&str>,
) -> Option<KanjiSummary> {
    let document = Html::parse_document(body);
    let results = Selector::parse("div.hvres[data-hvres-idx]").expect("static CSS selector");
    let spell = Selector::parse(".hvres-spell").expect("static CSS selector");
    let meaning = Selector::parse(".hvres-meaning").expect("static CSS selector");
    let mut readings = Vec::new();
    let mut meanings = Vec::new();
    for result in document.select(&results).take(32) {
        for node in result.select(&spell) {
            let value = node
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !value.is_empty() && !readings.contains(&value) {
                readings.push(value);
            }
        }
        for node in result.select(&meaning) {
            let value = node
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !value.is_empty() && !meanings.contains(&value) {
                meanings.push(value);
            }
        }
    }
    if meanings.is_empty() && readings.is_empty() {
        return None;
    }
    Some(KanjiSummary {
        character,
        meanings,
        readings,
        strokes: None,
        radical: None,
        parts: None,
        stroke_order_url: media_base.and_then(|base| stroke_order_url(base, character)),
    })
}

pub fn parse_jisho_kanji(
    character: char,
    body: &str,
    media_base: Option<&str>,
) -> Option<KanjiSummary> {
    let document = Html::parse_document(body);
    let select = |css: &str| Selector::parse(css).expect("static CSS selector");
    let details = document.select(&select(".kanji.details, .kanji")).next()?;
    let text = |css: &str| -> Vec<String> {
        details
            .select(&select(css))
            .map(|node| {
                node.text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|value| !value.is_empty())
            .collect()
    };
    let meanings = text(".kanji-details__main-meanings");
    let readings = text(".kanji-details__main-readings dd");
    let strokes = text(".kanji-details__stroke_count strong")
        .first()
        .and_then(|value| value.parse().ok());
    let mut radical = None;
    let mut parts = None;
    for (label, value) in text(".radicals dt").into_iter().zip(text(".radicals dd")) {
        if label.to_ascii_lowercase().contains("radical") {
            radical = Some(value);
        } else if label.to_ascii_lowercase().contains("parts") {
            parts = Some(value);
        }
    }
    if meanings.is_empty()
        && readings.is_empty()
        && strokes.is_none()
        && radical.is_none()
        && parts.is_none()
    {
        return None;
    }
    Some(KanjiSummary {
        character,
        meanings,
        readings,
        strokes,
        radical,
        parts,
        stroke_order_url: media_base.and_then(|base| stroke_order_url(base, character)),
    })
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
        parts: None,
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
    pub parts: Option<String>,
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

pub async fn lookup_word<P: KanjiLookupPort>(
    provider: &P,
    word: &str,
    kanjidic2: &str,
    media_base: Option<&str>,
) -> KanjiLookup {
    lookup_word_with_sources(&[provider], word, kanjidic2, media_base).await
}

pub async fn lookup_word_with_sources(
    providers: &[&dyn KanjiLookupPort],
    word: &str,
    kanjidic2: &str,
    media_base: Option<&str>,
) -> KanjiLookup {
    let characters = word
        .chars()
        .filter(|character| is_cjk(*character))
        .collect::<Vec<_>>();
    let mut result = KanjiLookup::default();
    let mut seen = HashSet::new();
    for character in characters {
        if !seen.insert(character) {
            continue;
        }
        let mut found = None;
        for (index, provider) in providers.iter().enumerate() {
            match provider.lookup(character).await {
                Ok(Some(summary)) if summary.character == character => {
                    found = Some(summary);
                    break;
                }
                Ok(_) => {}
                Err(error) => result.warnings.push(format!(
                    "Kanji source {} for {character}: {error}",
                    index + 1
                )),
            }
        }
        if let Some(summary) = found.or_else(|| parse_kanjidic2(character, kanjidic2, media_base)) {
            result.summaries.push(summary);
        } else {
            result
                .warnings
                .push(format!("No Kanji data for {character}"));
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

pub fn render_kanji_summaries(summaries: &[KanjiSummary]) -> String {
    let mut html = String::new();
    for summary in summaries {
        html.push_str(&format!(
            "<section data-kanji=\"{}\"><b>Kanji {}</b>",
            escape_html(&summary.character.to_string()),
            escape_html(&summary.character.to_string())
        ));
        if let Some(url) = &summary.stroke_order_url {
            html.push_str(&format!("<div><b>Stroke order:</b><br/><img src=\"{}\" alt=\"Stroke order for {}\" style=\"max-width:220px\"/></div>", escape_html(url), escape_html(&summary.character.to_string())));
        }
        let fields = [
            ("Meanings", summary.meanings.join(", ")),
            ("Readings", summary.readings.join(" · ")),
            (
                "Stroke count",
                summary
                    .strokes
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            ("Radical", summary.radical.clone().unwrap_or_default()),
            ("Parts", summary.parts.clone().unwrap_or_default()),
        ];
        for (label, value) in fields {
            if !value.is_empty() {
                html.push_str(&format!(
                    "<div><b>{label}:</b> {}</div>",
                    escape_html(&value)
                ));
            }
        }
        html.push_str("</section>");
    }
    html
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        parts: None,
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    struct FallbackProvider;
    impl KanjiLookupPort for FallbackProvider {
        fn lookup<'a>(&'a self, character: char) -> KanjiFuture<'a> {
            Box::pin(async move {
                Ok(Some(KanjiSummary {
                    character,
                    meanings: vec!["fallback".into()],
                    readings: Vec::new(),
                    strokes: None,
                    radical: None,
                    parts: None,
                    stroke_order_url: None,
                }))
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
            ['食', '学']
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result
                .summaries
                .iter()
                .all(|summary| summary.stroke_order_url.is_some())
        );
    }

    #[tokio::test]
    async fn falls_back_to_second_provider_in_word_order() {
        let result =
            lookup_word_with_sources(&[&MissingProvider, &FallbackProvider], "食学食", "", None)
                .await;
        assert_eq!(
            result
                .summaries
                .iter()
                .map(|summary| summary.character)
                .collect::<Vec<_>>(),
            ['食', '学']
        );
        assert!(
            result
                .summaries
                .iter()
                .all(|summary| summary.meanings == ["fallback"])
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("offline"))
        );
    }

    #[test]
    fn cache_identity_includes_provider_revision() {
        assert_ne!(cache_key("学", "primary-v1"), cache_key("学", "primary-v2"));
    }

    #[test]
    fn parses_jisho_details_and_escapes_rendered_html() {
        let body = r#"<div class="kanji details"><div class="kanji-details__main-meanings">study &amp; learn</div><div class="kanji-details__main-readings"><dl><dd>ガク</dd></dl></div><div class="kanji-details__stroke_count"><strong>8</strong></div><div class="radicals"><dl><dt>Radical</dt><dd>子</dd><dt>Parts</dt><dd>冖</dd></dl></div></div>"#;
        let mut summary = parse_jisho_kanji('学', body, Some("https://assets.test/kanji")).unwrap();
        assert_eq!(summary.meanings, ["study & learn"]);
        assert_eq!(summary.readings, ["ガク"]);
        assert_eq!(summary.strokes, Some(8));
        assert_eq!(summary.radical.as_deref(), Some("子"));
        assert_eq!(summary.parts.as_deref(), Some("冖"));
        summary.meanings.push("<script>alert(1)</script>".into());
        let rendered = render_kanji_summaries(&[summary]);
        assert!(rendered.contains("&lt;script&gt;"));
        assert!(!rendered.contains("<script>"));
        assert!(rendered.contains("<img src=\"https://assets.test/kanji/05b66.svg\""));
    }

    #[test]
    fn parses_hvdic_definition_blocks_without_unrelated_results() {
        let body = r#"<div class="hvres" data-hvres-idx="1"><span class="hvres-spell">học</span><div class="hvres-meaning">học tập</div></div><div class="hvres"><div class="hvres-meaning">unrelated</div></div>"#;
        let summary = parse_hvdic_kanji('学', body, None).unwrap();
        assert_eq!(summary.readings, ["học"]);
        assert_eq!(summary.meanings, ["học tập"]);
        assert!(parse_hvdic_kanji('学', "<div class='hvres'>other</div>", None).is_none());
    }

    #[tokio::test]
    async fn embeds_validated_stroke_order_gif() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let count = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET /gifs/5b66.gif "));
            let body = b"GIF89aimage";
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        let fetcher = KanjiMediaFetcher::with_config(
            &format!("http://{address}/gifs/"),
            Duration::from_secs(2),
        )
        .unwrap();
        let uri = fetcher.gif_data_uri('学').await.unwrap();
        assert!(uri.starts_with("data:image/gif;base64,R0lGODlh"));
        server.await.unwrap();
    }
}
