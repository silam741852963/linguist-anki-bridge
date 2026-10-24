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
/// WP-27: each sense and part-of-speech chip carries `lab-pos-<class>`, and a
/// sense outside the card's `main` class is marked `lab-other`.
pub(super) fn meaning(
    language: &str,
    main: Option<&str>,
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
                .any(|form| crate::review::same_spelling(form, expression))
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
    // Cambridge: the first definition of each part of speech of the word,
    // plus the selected one; other providers list the selected entry.
    let senses: Vec<(&DictionaryEntry, usize)> = if entry.provider == "cambridge-html-v1" {
        entries
            .iter()
            .filter(|e| e.provider == entry.provider && e.forms.iter().any(|f| f == expression))
            .flat_map(|e| {
                let mut picked = vec![(e, 0)];
                if let Some(selected) = e
                    .senses
                    .iter()
                    .skip(1)
                    .position(|s| s.key == selected_sense_key)
                {
                    picked.push((e, selected + 1));
                }
                picked
            })
            .collect()
    } else {
        (0..entry.senses.len())
            .map(|index| (entry, index))
            .collect()
    };
    let mut body = String::from("<ol class=\"lab-senses\">");
    for (entry, index) in senses {
        let sense = &entry.senses[index];
        let selected = sense.key == selected_sense_key;
        let classes = crate::profile::sense_classes(language, entry, index);
        let mut class = String::from("lab-sense");
        if selected {
            class.push_str(" lab-selected");
        }
        if let Some(first) = classes.first() {
            class.push_str(&format!(" lab-pos-{first}"));
            if !selected && main.is_some_and(|main| !classes.contains(&main)) {
                class.push_str(" lab-other");
            }
        }
        body.push_str(&format!("<li class=\"{class}\">"));
        for label in &sense.labels {
            let chip = match crate::profile::label_class(language, label) {
                Some(class) => format!("lab-pos lab-pos-{class}"),
                None => "lab-pos".into(),
            };
            body.push_str(&format!(
                "<span class=\"{chip}\">{}</span>",
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
                {"key":"second", "definitions":["raw (see 生物)"], "labels":[]},
                {"key":"third", "definitions":["to grow"], "labels":["Godan verb with 'u' ending"]}
            ],
            "metadata":{"jlpt":["jlpt-n5"]},
            "related_entries":["related word"]
        }))
        .unwrap();
        let mut decoy = entry.clone();
        decoy.forms = vec!["other".into()];
        decoy.readings = vec![];
        let html = meaning("ja", Some("noun"), &[decoy, entry], "生", "first", "life");
        assert_eq!(html.matches("lab-selected").count(), 1);
        assert!(html.contains(
            "<li class=\"lab-sense lab-selected lab-pos-noun\"><span class=\"lab-pos lab-pos-noun\">Noun</span>life; &lt;script&gt;"
        ));
        // A sense without labels takes the one before; another class is marked.
        assert!(html.contains("<li class=\"lab-sense lab-pos-noun\">raw"));
        assert!(html.contains("<li class=\"lab-sense lab-pos-verb lab-other\">"));
        assert!(html.contains("raw (see 〜物)"));
        for hidden in [
            "jisho", "https", "first", "second", "生", "なま", "jlpt", "related", "Live.",
        ] {
            assert!(!html.contains(hidden), "shows {hidden}: {html}");
        }
        assert!(!html.contains("<script"));
        assert_eq!(
            meaning("ja", None, &[], "生", "first", "life <b>"),
            "<p class=\"lab-gloss\">life &lt;b&gt;</p>"
        );
        assert!(meaning("ja", None, &[], "生", "first", " ").is_empty());
    }
}
