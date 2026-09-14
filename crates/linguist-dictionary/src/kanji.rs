//! Provider-neutral Kanji summaries and an offline KANJIDIC2 fallback parser.

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
pub fn stroke_order_url(base: &str, character: char) -> Option<String> {
    base.strip_suffix('/')
        .map(|base| format!("{base}/{:05x}.svg", u32::from(character)))
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
}
