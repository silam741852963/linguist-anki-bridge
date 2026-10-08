//! WP-19 Meaning field: the selected dictionary entry's senses as escaped,
//! styled data. No provider names, URLs, sense keys or headwords are shown, so
//! the field is safe on Production and Spelling fronts.
use super::escape;
use crate::DictionaryEntry;

/// Escape `text` and hide every occurrence of the answer behind `〜`.
pub(super) fn masked(text: &str, expression: &str) -> String {
    let text = escape(text);
    let expression = escape(expression.trim());
    if expression.is_empty() {
        text
    } else {
        text.replace(&expression, "〜")
    }
}

/// The sense list of the entry holding the selected sense, with that sense
/// marked by the `lab-selected` class. Falls back to the accepted meaning.
pub(super) fn meaning(
    entries: &[DictionaryEntry],
    expression: &str,
    selected_sense_key: &str,
    selected_meaning: &str,
) -> String {
    let entry = entries.iter().find(|entry| {
        !selected_sense_key.is_empty()
            && entry.senses.iter().any(|s| s.key == selected_sense_key)
            && entry
                .forms
                .iter()
                .chain(&entry.readings)
                .any(|form| form == expression)
    });
    let Some(entry) = entry else {
        return if selected_meaning.trim().is_empty() {
            String::new()
        } else {
            format!(
                "<p class=\"lab-gloss\">{}</p>",
                masked(selected_meaning, expression)
            )
        };
    };
    let mut body = String::from("<ol class=\"lab-senses\">");
    for sense in &entry.senses {
        let selected = sense.key == selected_sense_key;
        body.push_str(if selected {
            "<li class=\"lab-sense lab-selected\">"
        } else {
            "<li class=\"lab-sense\">"
        });
        for label in &sense.labels {
            body.push_str(&format!(
                "<span class=\"lab-pos\">{}</span>",
                masked(label, expression)
            ));
        }
        let definitions: Vec<_> = sense.definitions.iter().map(|d| d.trim()).collect();
        body.push_str(&masked(&definitions.join("; "), expression));
        body.push_str("</li>");
    }
    body.push_str("</ol>");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meaning_lists_senses_marks_selection_and_hides_provenance() {
        let entry: DictionaryEntry = serde_json::from_value(serde_json::json!({
            "provider":"jisho-api-v1", "source_url":"https://jisho.org/x", "language":"ja",
            "forms":["生"], "readings":["なま"],
            "senses":[
                {"key":"first", "definitions":["life", "<script>alert(1)</script>"], "labels":["Noun"],
                 "examples":[{"sentence":"生きる。", "translation":"Live.", "provenance":"dictionary"}]},
                {"key":"second", "definitions":["raw (see 生物)"], "labels":[]}
            ],
            "metadata":{"jlpt":["jlpt-n5"]},
            "related_entries":["related word"]
        }))
        .unwrap();
        let mut decoy = entry.clone();
        decoy.forms = vec!["other".into()];
        decoy.readings = vec![];
        let html = meaning(&[decoy, entry], "生", "first", "life");
        assert_eq!(html.matches("lab-selected").count(), 1);
        assert!(html.contains("<span class=\"lab-pos\">Noun</span>life; &lt;script&gt;"));
        assert!(html.contains("raw (see 〜物)"));
        for hidden in [
            "jisho", "https", "first", "second", "生", "なま", "jlpt", "related", "Live.",
        ] {
            assert!(!html.contains(hidden), "shows {hidden}: {html}");
        }
        assert!(!html.contains("<script"));
        assert_eq!(
            meaning(&[], "生", "first", "life <b>"),
            "<p class=\"lab-gloss\">life &lt;b&gt;</p>"
        );
        assert!(meaning(&[], "生", "first", " ").is_empty());
    }
}
