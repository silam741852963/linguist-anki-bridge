//! Cambridge HTML conversion. Transport/browser concerns stay outside this parser.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CambridgeEntry {
    pub query: String,
    pub headword: String,
    pub definitions: Vec<String>,
    pub audio_url: Option<String>,
}

/// Parse the stable definition and pronunciation URL markers emitted by
/// Cambridge's server-rendered dictionary pages. Missing optional sections are
/// deliberately tolerated: provider markup changes should not discard a card.
pub fn parse_html(query: &str, html: &str) -> CambridgeEntry {
    let definitions = class_texts(html, "def ddef_d db");
    let headword = class_texts(html, "hw dhw")
        .into_iter()
        .next()
        .unwrap_or_else(|| query.into());
    CambridgeEntry {
        query: query.into(),
        headword,
        definitions,
        audio_url: audio_url(html),
    }
}

fn class_texts(html: &str, needle: &str) -> Vec<String> {
    html.match_indices("class=\"")
        .filter_map(|(start, _)| {
            let class_start = start + 7;
            let class_end = html[class_start..].find('"')? + class_start;
            let classes = &html[class_start..class_end];
            if !classes.contains(needle) {
                return None;
            }
            let tag_start = html[..start].rfind('<')?;
            let tag_name = html[tag_start + 1..]
                .split(|ch: char| ch.is_whitespace() || ch == '>')
                .next()?;
            let content_start = html[class_end..].find('>')? + class_end + 1;
            let closing = format!("</{tag_name}");
            let content_end = html[content_start..].find(&closing)? + content_start;
            Some(normalize_html_text(&html[content_start..content_end]))
        })
        .filter(|text| !text.is_empty())
        .collect()
}
fn audio_url(html: &str) -> Option<String> {
    html.match_indices("https://dictionary.cambridge.org/")
        .find_map(|(start, _)| {
            let end = html[start..].find(".mp3")? + start + 4;
            Some(html[start..end].replace("&amp;", "&"))
        })
}
fn normalize_html_text(value: &str) -> String {
    let mut text = String::new();
    let mut inside = false;
    for ch in value.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => text.push(ch),
            _ => {}
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_defs_and_audio() {
        let entry = parse_html(
            "eat",
            r#"<span class="hw dhw">eat</span><div class="def ddef_d db">to <b>put</b> food in the mouth</div><source src="https://dictionary.cambridge.org/media.mp3">"#,
        );
        assert_eq!(entry.headword, "eat");
        assert_eq!(entry.definitions, ["to put food in the mouth"]);
        assert_eq!(
            entry.audio_url.as_deref(),
            Some("https://dictionary.cambridge.org/media.mp3")
        );
    }
}
