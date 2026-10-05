//! ALG-CAPTURE: source discovery only: original field bytes remain owned by the capture archive.
use linguist_core::validation::safe_media_name;
use scraper::{Html, Node};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaReference {
    pub field: String,
    pub filename: String,
    pub syntax: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureIssue {
    pub field: String,
    pub code: String,
    pub syntax: String,
}
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct MediaDiscovery {
    pub references: Vec<MediaReference>,
    pub issues: Vec<CaptureIssue>,
}
impl MediaDiscovery {
    fn issue(&mut self, field: &str, code: &str, syntax: &str) {
        self.issues.push(CaptureIssue {
            field: field.into(),
            code: code.into(),
            syntax: syntax.into(),
        });
    }
    fn reference(&mut self, field: &str, value: &str, syntax: &str, url_encoded: bool) {
        let decoded;
        let filename = if url_encoded {
            decoded = match percent_encoding::percent_decode_str(value).decode_utf8() {
                Ok(value) => value,
                Err(_) => {
                    self.issue(field, "INVALID_MEDIA_ENCODING", syntax);
                    return;
                }
            };
            decoded.as_ref()
        } else {
            value
        };
        if !safe_media_name(filename) || filename.trim().is_empty() {
            self.issue(field, "UNSAFE_OR_REMOTE_MEDIA_REFERENCE", syntax);
            return;
        }
        self.references.push(MediaReference {
            field: field.into(),
            filename: filename.into(),
            syntax: syntax.into(),
        });
    }
}

/// Bound the complete raw input before HTML parsing. No mutation, fetching or normalization.
/// Associations (including repeated references) are retained in deterministic field order.
pub fn discover_media(
    fields: &BTreeMap<String, String>,
    max_bytes: u64,
    max_references: usize,
) -> Result<MediaDiscovery, String> {
    if !(1..=100 * 1024 * 1024).contains(&max_bytes) || !(1..=10000).contains(&max_references) {
        return Err("CAPTURE_INVALID_LIMITS".into());
    }
    let size = fields
        .iter()
        .try_fold(0u64, |size, (key, value)| {
            size.checked_add(key.len() as u64)?
                .checked_add(value.len() as u64)
        })
        .ok_or("CAPTURE_INPUT_LIMIT")?;
    if size > max_bytes {
        return Err("CAPTURE_INPUT_LIMIT".into());
    }
    let mut result = MediaDiscovery::default();
    for (field, raw) in fields {
        // Anki sound markers are field syntax, independent of HTML text-node boundaries.
        let mut rest = raw.as_str();
        while let Some(start) = rest.find("[sound:") {
            rest = &rest[start + 7..];
            match rest.find(']') {
                Some(end) => {
                    result.reference(field, &rest[..end], "sound", false);
                    rest = &rest[end + 1..];
                }
                None => {
                    result.issue(field, "MALFORMED_SOUND_REFERENCE", "sound");
                    break;
                }
            }
            if result.references.len() + result.issues.len() > max_references {
                return Err("CAPTURE_REFERENCE_LIMIT".into());
            }
        }
        let html = Html::parse_fragment(raw);
        for node in html.tree.nodes() {
            let Node::Element(element) = node.value() else {
                continue;
            };
            let name = element.name();
            let attributes: &[&str] = match name {
                "img" | "audio" | "source" | "embed" => &["src"],
                "video" => &["src", "poster"],
                "object" => &["data"],
                _ => &[],
            };
            for attribute in attributes {
                if let Some(value) = element.attr(attribute) {
                    result.reference(field, value, &format!("{name}.{attribute}"), true);
                }
            }
            if element.attr("srcset").is_some()
                || element.attr("background").is_some()
                || element.attr("style").is_some()
                || name == "style"
                || matches!(name, "iframe" | "image" | "script" | "link" | "svg" | "use")
                || (name == "input"
                    && element
                        .attr("type")
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("image")))
            {
                result.issue(field, "UNSUPPORTED_MEDIA_SYNTAX", name);
            }
            if result.references.len() + result.issues.len() > max_references {
                return Err("CAPTURE_REFERENCE_LIMIT".into());
            }
        }
    }
    // A case-insensitive collision is unsafe on supported Anki desktop platforms.
    let mut seen = BTreeMap::<String, BTreeSet<String>>::new();
    for reference in &result.references {
        seen.entry(reference.filename.to_lowercase())
            .or_default()
            .insert(reference.filename.clone());
    }
    for reference in result.references.clone() {
        if seen[&reference.filename.to_lowercase()].len() > 1 {
            result.issue(
                &reference.field,
                "SOURCE_MEDIA_CASE_COLLISION",
                &reference.syntax,
            );
        }
    }
    if result.references.len() + result.issues.len() > max_references {
        return Err("CAPTURE_REFERENCE_LIMIT".into());
    }
    Ok(result)
}
