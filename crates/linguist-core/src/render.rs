//! ALG-RENDER: render a validated v2 document into the fixed managed model fields.
mod dictionary;
use crate::canonical::ContractError;
use crate::{document::*, model, records::MediaRole, validation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderedNote {
    pub document_id: uuid::Uuid,
    pub model: model::ManagedModel,
    pub fields: BTreeMap<String, String>,
    pub tasks: Vec<Task>,
    pub media_digests: Vec<String>,
    pub digest: String,
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn sanitize_reference(s: &str) -> String {
    ammonia::Builder::default()
        .tags(HashSet::from([
            "p",
            "br",
            "b",
            "strong",
            "i",
            "em",
            "ul",
            "ol",
            "li",
            "blockquote",
            "code",
            "span",
        ]))
        .generic_attributes(HashSet::new())
        .url_schemes(HashSet::new())
        .clean(s)
        .to_string()
}
fn block(s: &str) -> String {
    if s.trim().is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>", escape(s))
    }
}
pub(crate) fn examples(values: &[Example]) -> String {
    values
        .iter()
        .map(|e| {
            format!(
                "<p>{}<br>{}<br><span class=\"lab-label\">{:?}</span></p>",
                escape(&e.sentence),
                escape(&e.translation),
                e.provenance
            )
        })
        .collect()
}
fn url_filename(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
/// Expression occurrences in example text, highlighted after escaping.
fn highlighted(text: &str, expression: &str) -> String {
    let text = escape(text);
    let expression = escape(expression.trim());
    if expression.is_empty() {
        text
    } else {
        text.replace(
            &expression,
            &format!("<b class=\"lab-hl\">{expression}</b>"),
        )
    }
}
/// v3 UsageExamples: usage, nuance, collocations and examples in one fixed layout.
fn usage_examples(v: &Vocabulary) -> String {
    let mut html = String::new();
    if !v.usage.trim().is_empty() {
        html.push_str(&format!("<h4>Usage</h4><p>{}</p>", escape(v.usage.trim())));
    }
    if !v.nuance.is_empty() {
        html.push_str("<h4>Nuance</h4><dl class=\"lab-nuance\">");
        for contrast in &v.nuance {
            html.push_str(&format!(
                "<div><dt>{}</dt><dd>{}</dd></div>",
                escape(&contrast.expression),
                escape(&contrast.difference)
            ));
        }
        html.push_str("</dl>");
    }
    if !v.collocations.is_empty() {
        html.push_str("<h4>Collocations</h4><ul class=\"lab-collocations\">");
        for collocation in &v.collocations {
            html.push_str(&format!(
                "<li><span class=\"lab-target\">{}</span>{}</li>",
                highlighted(&collocation.phrase, &v.expression),
                if collocation.gloss.trim().is_empty() {
                    String::new()
                } else {
                    format!(
                        "<span class=\"lab-tr\">{}</span>",
                        escape(&collocation.gloss)
                    )
                }
            ));
        }
        html.push_str("</ul>");
    }
    if !v.examples.is_empty() {
        html.push_str("<h4>Examples</h4><ul class=\"lab-examples\">");
        for example in &v.examples {
            html.push_str(&format!(
                "<li><div class=\"lab-target\">{}</div>{}</li>",
                highlighted(&example.sentence, &v.expression),
                if example.translation.trim().is_empty() {
                    String::new()
                } else {
                    format!(
                        "<div class=\"lab-tr\">{}</div>",
                        escape(&example.translation)
                    )
                }
            ));
        }
        html.push_str("</ul>");
    }
    html
}
/// v3 Kanji: one card per character with its stroke-order animation when staged.
fn kanji(v: &Vocabulary, media: &[crate::records::MediaAsset]) -> String {
    if v.kanji_details.is_empty() {
        return String::new();
    }
    let mut html = String::from("<div class=\"lab-kanji-grid\">");
    for detail in &v.kanji_details {
        let stroke = detail.stroke_digest.as_deref().and_then(|digest| {
            media
                .iter()
                .find(|m| m.role == MediaRole::KanjiStroke && m.digest == digest)
        });
        let art = match stroke {
            Some(asset) => format!("<img src=\"{}\">", url_filename(&asset.filename)),
            None => format!(
                "<span class=\"lab-kanji-char\">{}</span>",
                escape(&detail.character)
            ),
        };
        let readings = |label: &str, values: &[String]| {
            if values.is_empty() {
                String::new()
            } else {
                format!(
                    "<div class=\"lab-kanji-readings\"><b>{label}</b>{}</div>",
                    escape(&values.join("、"))
                )
            }
        };
        let mut meta = Vec::new();
        if let Some(strokes) = detail.strokes {
            meta.push(format!("{strokes} strokes"));
        }
        if let Some(radical) = &detail.radical {
            meta.push(format!("radical {}", escape(radical)));
        }
        if !detail.parts.is_empty() {
            meta.push(format!("parts {}", escape(&detail.parts.join(" "))));
        }
        if let Some(jlpt) = &detail.jlpt {
            meta.push(escape(jlpt));
        }
        html.push_str(&format!(
            "<div class=\"lab-kanji\"><div class=\"lab-kanji-art\">{art}</div><div class=\"lab-kanji-info\"><div class=\"lab-kanji-meanings\">{} {}</div>{}{}<div class=\"lab-kanji-meta\">{}</div></div></div>",
            escape(&detail.character),
            escape(&detail.meanings.join("; ")),
            readings("ON", &detail.on_readings),
            readings("KUN", &detail.kun_readings),
            meta.join(" · ")
        ));
    }
    html.push_str("</div>");
    html
}
fn slug(value: &str) -> String {
    let mut out = String::new();
    for c in value.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}
/// WP-19 derived tags: language, explanation language, kind, tasks, and the
/// selected entry's JLPT level, commonness and parts of speech.
pub fn tags(doc: &LearningDocument) -> Vec<String> {
    let mut tags = vec![
        format!("lab::lang::{}", slug(doc.target_language.as_str())),
        format!("lab::explain::{}", slug(doc.explanation_language.as_str())),
    ];
    match &doc.content {
        LearningContent::Vocabulary(v) => {
            tags.push("lab::kind::vocabulary".into());
            let entry = v.dictionary.iter().find(|entry| {
                entry.senses.iter().any(|s| s.key == v.sense_key)
                    && entry
                        .forms
                        .iter()
                        .chain(&entry.readings)
                        .any(|form| *form == v.expression)
            });
            if let Some(entry) = entry {
                for level in entry.metadata.get("jlpt").into_iter().flatten() {
                    let level = slug(level.trim_start_matches("jlpt-"));
                    if !level.is_empty() {
                        tags.push(format!("lab::jlpt::{level}"));
                    }
                }
                if entry
                    .metadata
                    .get("is_common")
                    .is_some_and(|values| values.iter().any(|value| value == "true"))
                {
                    tags.push("lab::common".into());
                }
                if let Some(sense) = entry.senses.iter().find(|s| s.key == v.sense_key) {
                    for label in &sense.labels {
                        let label = slug(label);
                        if !label.is_empty() {
                            tags.push(format!("lab::pos::{label}"));
                        }
                    }
                }
            }
            if !v.kanji_details.is_empty() {
                tags.push("lab::has::kanji".into());
            }
        }
        LearningContent::Grammar(_) => tags.push("lab::kind::grammar".into()),
    }
    for task in &doc.requested_tasks {
        tags.push(format!("lab::task::{}", slug(&format!("{task:?}"))));
    }
    if doc.media.iter().any(|m| m.role == MediaRole::Picture) {
        tags.push("lab::has::picture".into());
    }
    if doc.media.iter().any(|m| m.role == MediaRole::Audio) {
        tags.push("lab::has::audio".into());
    }
    tags.sort();
    tags.dedup();
    tags
}
pub fn render(
    doc: &LearningDocument,
    source_fields: &BTreeMap<String, String>,
) -> Result<RenderedNote, ContractError> {
    let issues = validation::validate(doc);
    if issues
        .iter()
        .any(|i| i.severity != validation::Severity::Warning)
    {
        return Err(ContractError(format!(
            "DOCUMENT_NOT_READY: {}",
            issues
                .iter()
                .map(|i| i.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let model = model::for_document(doc);
    let mut fields: BTreeMap<String, String> = model
        .fields
        .iter()
        .map(|s| (s.clone(), String::new()))
        .collect();
    // Grammar v2 still carries language and provenance fields; vocabulary v3
    // carries them as tags (see `tags`).
    for (key, value) in [
        ("Language", doc.target_language.to_string()),
        ("ExplanationLanguage", doc.explanation_language.to_string()),
        ("PersonalNotes", block(&doc.personal_notes)),
        ("Source", block(&doc.source_summary)),
    ] {
        if let Some(field) = fields.get_mut(key) {
            *field = value;
        }
    }
    match &doc.content {
        LearningContent::Vocabulary(v) => {
            fields.insert("Expression".into(), escape(&v.expression));
            let spoken = if v.pronunciation.trim().is_empty() {
                &v.reading
            } else {
                &v.pronunciation
            };
            fields.insert("Pronunciation".into(), escape(spoken));
            fields.insert(
                "Meaning".into(),
                dictionary::meaning(&v.dictionary, &v.expression, &v.sense_key, &v.meaning),
            );
            fields.insert("UsageExamples".into(), usage_examples(v));
            fields.insert("Kanji".into(), kanji(v, &doc.media));
            for (key, task) in [
                ("EnableProduction", Task::Production),
                ("EnableSpelling", Task::Spelling),
            ] {
                let on = doc.requested_tasks.contains(&task);
                fields.insert(key.into(), if on { "1" } else { "" }.into());
            }
        }
        LearningContent::Grammar(g) => {
            for (key, value) in [
                ("Pattern", &g.pattern),
                ("UseKey", &g.use_key),
                ("RecognitionPrompt", &g.recognition_prompt),
                ("ExercisePrompt", &g.exercise_prompt),
                ("ExerciseAnswer", &g.exercise_answer),
            ] {
                fields.insert(key.into(), escape(value));
            }
            for (key, value) in [
                ("Meaning", &g.meaning),
                ("Formation", &g.formation),
                ("Usage", &g.usage),
            ] {
                fields.insert(key.into(), block(value));
            }
            fields.insert("Examples".into(), examples(&g.examples));
            fields.insert(
                "EnableApplication".into(),
                if doc.requested_tasks.contains(&Task::Application) {
                    "1"
                } else {
                    ""
                }
                .into(),
            );
        }
    }
    for asset in &doc.media {
        match asset.role {
            MediaRole::Picture => {
                if let Some(p) = fields.get_mut("Picture") {
                    p.push_str(&format!("<img src=\"{}\">", url_filename(&asset.filename)))
                }
            }
            MediaRole::Audio => fields
                .get_mut("Audio")
                .unwrap()
                .push_str(&format!("[sound:{}]", asset.filename)),
            MediaRole::Archive | MediaRole::KanjiStroke => {}
        }
    }
    for (key, intent) in &doc.edits {
        let value = intent.resolve(source_fields.get(key))?;
        fields.insert(
            key.clone(),
            value.map(|s| sanitize_reference(&s)).unwrap_or_default(),
        );
    }
    // String edits cannot bypass effective task prerequisite checks.
    for key in ["Meaning", "Formation"] {
        if fields.contains_key(key)
            && ammonia::Builder::default()
                .tags(HashSet::new())
                .clean(&fields[key])
                .to_string()
                .replace("&nbsp;", " ")
                .trim()
                .is_empty()
        {
            return Err(ContractError(format!(
                "EFFECTIVE_REQUIRED_FIELD_EMPTY:{key}"
            )));
        }
    }
    let media_digests: Vec<_> = doc
        .media
        .iter()
        .filter(|m| m.role != MediaRole::Archive)
        .map(|m| m.digest.clone())
        .collect();
    let digest = crate::canonical::digest(
        "rendered-note",
        &(
            doc.id,
            &model,
            &fields,
            &doc.requested_tasks,
            &media_digests,
        ),
    )?;
    Ok(RenderedNote {
        document_id: doc.id,
        model,
        fields,
        tasks: doc.requested_tasks.clone(),
        media_digests,
        digest,
    })
}
