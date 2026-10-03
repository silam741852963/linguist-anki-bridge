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
fn examples(values: &[Example]) -> String {
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
    fields.insert("Language".into(), doc.target_language.to_string());
    fields.insert(
        "ExplanationLanguage".into(),
        doc.explanation_language.to_string(),
    );
    fields.insert("PersonalNotes".into(), block(&doc.personal_notes));
    fields.insert("Source".into(), block(&doc.source_summary));
    match &doc.content {
        LearningContent::Vocabulary(v) => {
            for (key, value) in [
                ("Expression", &v.expression),
                ("Reading", &v.reading),
                ("Pronunciation", &v.pronunciation),
                ("SenseKey", &v.sense_key),
                ("ProductionPrompt", &v.production_prompt),
                ("SpellingPrompt", &v.spelling_prompt),
            ] {
                fields.insert(key.into(), escape(value));
            }
            for (key, value) in [
                ("Meaning", &v.meaning),
                ("Usage", &v.usage),
                ("Kanji", &v.kanji),
            ] {
                fields.insert(key.into(), block(value));
            }
            fields.insert("Examples".into(), examples(&v.examples));
            let reference =
                dictionary::reference(&v.dictionary, &v.expression, &v.sense_key, &v.meaning);
            if !reference.is_empty() {
                fields.get_mut("Meaning").unwrap().push_str(&format!(
                    "<section class=\"lab-reference\">{reference}</section>"
                ))
            }
            fields.insert(
                "EnableProduction".into(),
                if doc.requested_tasks.contains(&Task::Production) {
                    "1"
                } else {
                    ""
                }
                .into(),
            );
            fields.insert(
                "EnableSpelling".into(),
                if doc.requested_tasks.contains(&Task::Spelling) {
                    "1"
                } else {
                    ""
                }
                .into(),
            );
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
            MediaRole::Archive => {}
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
