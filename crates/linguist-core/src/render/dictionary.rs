//! Dictionary reference content is escaped data, never provider HTML or active URLs.
use super::{block, escape, examples};
use crate::{DictionaryEntry, canonical};

fn list(label: &str, values: &[String]) -> String {
    if values.is_empty() {
        return String::new();
    }
    format!(
        "<p><strong>{}</strong>: {}</p>",
        escape(label),
        values
            .iter()
            .map(|value| escape(value))
            .collect::<Vec<_>>()
            .join("; ")
    )
}
fn structured(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Object(values) => {
            let entries: String = values
                .iter()
                .map(|(key, value)| {
                    let body = structured(value);
                    if body.is_empty() {
                        String::new()
                    } else {
                        format!("<dt>{}</dt><dd>{body}</dd>", escape(&key.replace('_', " ")))
                    }
                })
                .collect();
            if entries.is_empty() {
                "{}".into()
            } else {
                format!("<dl>{entries}</dl>")
            }
        }
        serde_json::Value::Array(values) => {
            let entries: String = values
                .iter()
                .map(structured)
                .filter(|body| !body.is_empty())
                .map(|body| format!("<li>{body}</li>"))
                .collect();
            if entries.is_empty() {
                "[]".into()
            } else {
                format!("<ul>{entries}</ul>")
            }
        }
        serde_json::Value::String(text) if text.is_empty() => "&quot;&quot;".into(),
        serde_json::Value::String(text) => escape(text),
        value => escape(&value.to_string()),
    }
}
fn metadata(label: &str, values: &[String]) -> String {
    let content: String = values
        .iter()
        .map(|raw| {
            if label.ends_with("json") {
                canonical::parse::<serde_json::Value>(raw.as_bytes())
                    .map(|value| structured(&value))
                    .unwrap_or_else(|_| block(raw))
            } else {
                block(raw)
            }
        })
        .collect();
    if content.is_empty() {
        return content;
    }
    let label = label
        .strip_suffix("_json")
        .unwrap_or(label)
        .replace('_', " ");
    format!(
        "<details><summary>{}</summary>{content}</details>",
        escape(&label)
    )
}
pub(super) fn reference(
    entries: &[DictionaryEntry],
    expression: &str,
    selected_sense_key: &str,
    selected_meaning: &str,
) -> String {
    entries
        .iter()
        .map(|entry| {
            let mut body = format!(
                "<article class=\"lab-dictionary-entry\"><p class=\"lab-label\">{} ({})</p>",
                escape(&entry.provider),
                escape(entry.language.as_str())
            );
            body.push_str(&list("Forms", &entry.forms));
            body.push_str(&list("Readings", &entry.readings));
            body.push_str(&list("Source", std::slice::from_ref(&entry.source_url)));
            for (label, values) in &entry.metadata {
                if !entry
                    .senses
                    .iter()
                    .any(|sense| *label == format!("sense:{}:raw_json", sense.key))
                {
                    body.push_str(&metadata(label, values));
                }
            }
            body.push_str("<ol>");
            for sense in &entry.senses {
                let selected = !selected_sense_key.is_empty()
                    && sense.key == selected_sense_key
                    && sense.definitions.join("; ") == selected_meaning
                    && entry
                        .forms
                        .iter()
                        .chain(&entry.readings)
                        .any(|form| form == expression);
                body.push_str(&format!(
                    "<li><p class=\"lab-label\">Sense: {}{}</p><p>{}</p>{}{}",
                    escape(&sense.key),
                    if selected { " (selected)" } else { "" },
                    sense
                        .definitions
                        .iter()
                        .map(|definition| escape(definition))
                        .collect::<Vec<_>>()
                        .join("; "),
                    list("Labels", &sense.labels),
                    examples(&sense.examples)
                ));
                if let Some(values) = entry.metadata.get(&format!("sense:{}:raw_json", sense.key)) {
                    body.push_str(&metadata("Sense information_json", values));
                }
                body.push_str("</li>");
            }
            body.push_str("</ol>");
            body.push_str(&list("Related entries", &entry.related_entries));
            body.push_str("</article>");
            body
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_preserve_entry_and_sense_data_as_inert_text() {
        let entry: DictionaryEntry = serde_json::from_value(serde_json::json!({
            "provider":"fixture <script>", "source_url":"javascript:alert(1)", "language":"ja",
            "forms":["生", "<img src=x onerror=alert(1)>"], "readings":["なま", "せい"],
            "senses":[
                {"key":"first", "definitions":["life", "<script>alert(1)</script>"], "labels":["Noun"],
                 "examples":[{"sentence":"生きる。", "translation":"Live.", "provenance":"dictionary"}]},
                {"key":"second", "definitions":["raw"], "labels":[]},
                {"key":"<img src=x onerror=alert(1)>", "definitions":["unsafe key"], "labels":[]}
            ],
            "metadata":{"jlpt":["jlpt-n5"],"sense:first:raw_json":["{\"restrictions\":[\"なま\"],\"antonyms\":[\"<iframe>\"]}"],
                "provider_extensions_json":["{\"unknown_field\":{\"value\":\"retained\",\"missing\":null,\"empty_list\":[],\"empty_object\":{},\"empty_text\":\"\"}}"],
                "malformed_json":["<svg onload=alert(1)>"]},
            "related_entries":["related word"]
        })).unwrap();
        let mut decoy = entry.clone();
        decoy.forms = vec!["other".into()];
        let html = reference(
            &[decoy, entry],
            "生",
            "first",
            "life; <script>alert(1)</script>",
        );
        assert_eq!(html.matches("(selected)").count(), 1);
        for text in [
            "生",
            "なま",
            "せい",
            "life",
            "raw",
            "Noun",
            "生きる。",
            "Live.",
            "jlpt-n5",
            "restrictions",
            "antonyms",
            "related word",
            "unknown field",
            "retained",
            "Sense: first (selected)",
            "Sense: second",
            "&lt;img src=x onerror=alert(1)&gt;",
            "<dt>missing</dt><dd>null</dd>",
            "<dt>empty list</dt><dd>[]</dd>",
            "<dt>empty object</dt><dd>{}</dd>",
            "<dt>empty text</dt><dd>&quot;&quot;</dd>",
        ] {
            assert!(html.contains(text), "missing {text}: {html}");
        }
        for tag in ["<script", "<img", "<iframe", "<svg", "href=", "src=\""] {
            assert!(!html.contains(tag), "active markup {tag}: {html}");
        }
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("&lt;svg onload=alert(1)&gt;"));
        assert!(html.find("life").unwrap() < html.find("raw</p>").unwrap());
        assert!(reference(&[], "生", "first", "life").is_empty());
    }
}
