use std::collections::BTreeMap;

use linguist_core::{CardDocument, japanese_vocab_spec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardFace {
    Front,
    Back,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedCard {
    pub template_name: String,
    pub face: CardFace,
    pub html: String,
    pub css: String,
    pub audio: Vec<String>,
    pub images: Vec<String>,
}

/// Render one managed Anki card without a network-capable browser. Rich fields
/// remain rich HTML, but remote links and media URLs are never carried forward.
pub fn render_managed_card(
    document: &CardDocument,
    template_index: usize,
    face: CardFace,
) -> Option<RenderedCard> {
    let spec = japanese_vocab_spec();
    let template = spec.templates.get(template_index)?;
    let mut fields = BTreeMap::from([
        ("Expression", escape_html(&document.expression)),
        (
            "Picture",
            sanitize_markup(document.values.meaning_image.as_deref().unwrap_or_default()),
        ),
        (
            "Meaning",
            sanitize_markup(document.values.meaning_text.as_deref().unwrap_or_default()),
        ),
        (
            "Kanji",
            sanitize_markup(
                document
                    .values
                    .kanji_construction
                    .as_deref()
                    .unwrap_or_default(),
            ),
        ),
        (
            "Audio",
            render_audio(document.values.audio.as_deref().unwrap_or_default()),
        ),
    ]);
    let source = match face {
        CardFace::Front => &template.front,
        CardFace::Back => &template.back,
    };
    let html = render_template(source, &mut fields);
    let images = image_urls(&html);
    let audio = audio_files(document.values.audio.as_deref().unwrap_or_default());
    Some(RenderedCard {
        template_name: template.name.clone(),
        face,
        html,
        css: spec.css,
        audio,
        images,
    })
}

pub fn local_media_url(filename: &str) -> Option<String> {
    valid_local_filename(filename).then(|| format!("linguist-media:///{filename}"))
}

pub fn audio_media_url(value: &str, index: usize) -> Option<String> {
    audio_files(value)
        .get(index)
        .and_then(|filename| local_media_url(filename))
}

fn render_template(template: &str, fields: &mut BTreeMap<&str, String>) -> String {
    let mut rendered = template.to_owned();
    for field in ["Expression", "Picture", "Meaning", "Kanji", "Audio"] {
        let value = fields.get(field).cloned().unwrap_or_default();
        let opening = format!("{{{{#{field}}}}}");
        let closing = format!("{{{{/{field}}}}}");
        while let Some(start) = rendered.find(&opening) {
            let body_start = start + opening.len();
            let Some(end_offset) = rendered[body_start..].find(&closing) else {
                break;
            };
            let end = body_start + end_offset;
            let replacement =
                (!value.trim().is_empty()).then(|| rendered[body_start..end].to_owned());
            let after = end + closing.len();
            rendered.replace_range(start..after, replacement.as_deref().unwrap_or_default());
        }
        rendered = rendered.replace(&format!("{{{{type:{field}}}}}"), &type_answer(field));
        rendered = rendered.replace(&format!("{{{{{field}}}}}"), &value);
    }
    format!("<style>{}</style>{rendered}", japanese_vocab_spec().css)
}

fn type_answer(field: &str) -> String {
    format!("<input id=\"typeans\" aria-label=\"Type {field}\" disabled>")
}

fn sanitize_markup(value: &str) -> String {
    let value = sanitize_attribute(value, "src", |url| local_media_url(url).unwrap_or_default());
    sanitize_attribute(&value, "href", |url| {
        if url.starts_with('#') {
            url.into()
        } else {
            String::new()
        }
    })
}

fn sanitize_attribute(value: &str, attribute: &str, map: impl Fn(&str) -> String) -> String {
    let mut safe = String::with_capacity(value.len());
    let marker = format!("{attribute}=");
    let mut remaining = value;
    while let Some(offset) = remaining.find(&marker) {
        safe.push_str(&remaining[..offset + marker.len()]);
        remaining = &remaining[offset + marker.len()..];
        let Some(quote) = remaining
            .chars()
            .next()
            .filter(|quote| matches!(quote, '\'' | '\"'))
        else {
            continue;
        };
        safe.push(quote);
        remaining = &remaining[quote.len_utf8()..];
        let Some(end) = remaining.find(quote) else {
            break;
        };
        safe.push_str(&map(&remaining[..end]));
        safe.push(quote);
        remaining = &remaining[end + quote.len_utf8()..];
    }
    safe.push_str(remaining);
    safe
}

fn render_audio(value: &str) -> String {
    audio_files(value)
        .into_iter()
        .map(|filename| {
            format!(
                "<span class=\"lab-audio\" data-local-audio=\"{filename}\">🔊 {filename}</span>"
            )
        })
        .collect::<Vec<_>>()
        .join("<br/>")
}

fn audio_files(value: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("[sound:") {
        remaining = &remaining[start + "[sound:".len()..];
        let Some(end) = remaining.find(']') else {
            break;
        };
        let filename = &remaining[..end];
        if valid_local_filename(filename) {
            files.push(filename.into());
        }
        remaining = &remaining[end + 1..];
    }
    if files.is_empty() {
        files.extend(
            value
                .split(['\n', '<', '>'])
                .map(str::trim)
                .filter(|filename| valid_local_filename(filename))
                .map(str::to_owned),
        );
    }
    files
}

fn image_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut remaining = html;
    while let Some(start) = remaining.find("src=\"") {
        remaining = &remaining[start + "src=\"".len()..];
        let Some(end) = remaining.find('\"') else {
            break;
        };
        if let Some(url) = remaining[..end].strip_prefix("linguist-media:///") {
            urls.push(url.into());
        }
        remaining = &remaining[end + 1..];
    }
    urls
}

fn valid_local_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use linguist_core::{LogicalFields, MediaAsset};

    fn document() -> CardDocument {
        CardDocument {
            schema_version: linguist_core::CONTRACT_VERSION,
            expression: "食べる".into(),
            values: LogicalFields {
                meaning_image: Some("<img src='linguist-taberu.jpg'>".into()),
                meaning_text: Some("to eat <a href='https://unsafe.example'>more</a>".into()),
                kanji_construction: Some("食 · eat".into()),
                audio: Some("たべる [sound:linguist-taberu.mp3]".into()),
            },
            media: vec![MediaAsset {
                filename: "linguist-taberu.jpg".into(),
                data_base64: "aW1hZ2U=".into(),
            }],
            obsolete_media: vec![],
            issues: vec![],
            tags: vec![],
            provenance: BTreeMap::new(),
        }
    }

    #[test]
    fn managed_templates_render_all_faces_with_local_media_only() {
        let document = document();
        for template in 0..3 {
            for face in [CardFace::Front, CardFace::Back] {
                let rendered = render_managed_card(&document, template, face).unwrap();
                assert!(rendered.html.contains("lab-shell"));
                assert!(rendered.css.contains("lab-expression"));
                assert!(!rendered.html.contains("https://unsafe.example"));
                if face == CardFace::Back || template == 2 {
                    assert!(
                        rendered
                            .html
                            .contains("linguist-media:///linguist-taberu.jpg")
                    );
                }
            }
        }
    }

    #[test]
    fn remote_and_path_traversal_media_are_blocked() {
        assert_eq!(local_media_url("../secret.jpg"), None);
        assert_eq!(local_media_url("https://unsafe.example/a.jpg"), None);
        assert_eq!(
            local_media_url("safe-name.jpg"),
            Some("linguist-media:///safe-name.jpg".into())
        );
        assert_eq!(
            audio_media_url("word [sound:safe-name.mp3]", 0),
            Some("linguist-media:///safe-name.mp3".into())
        );
        assert_eq!(
            audio_media_url("safe-name.mp3", 0),
            Some("linguist-media:///safe-name.mp3".into())
        );
    }
}
