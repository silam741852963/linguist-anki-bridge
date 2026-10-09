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
/// Escape text-node content exactly as the HTML serializer (and so Anki's
/// editor) writes it back: `&`, NBSP, `<` and `>`. Quotes stay literal, so a
/// note opened and saved in the editor reads back byte-identical. Rendered
/// values never sit inside attributes; media names use `url_filename`.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('\u{a0}', "&nbsp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The Pronunciation (else the Reading) a card shows. A reading equal to the
/// written word (いじめ, ビタミン) adds nothing and would give the Spelling
/// answer away, so it is not shown.
pub fn spoken_cue(v: &crate::Vocabulary) -> &str {
    let spoken = if v.pronunciation.trim().is_empty() {
        &v.reading
    } else {
        &v.pronunciation
    };
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if squash(spoken) == squash(&v.expression) {
        ""
    } else {
        spoken
    }
}

/// Kana-only pronunciation without the spaces older notes put at kanji
/// boundaries (`こわ す` renders `こわす`); any other text is kept as written.
fn compact_kana(text: &str) -> std::borrow::Cow<'_, str> {
    let kana = |c: char| matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}');
    if text.chars().any(kana) && text.chars().all(|c| kana(c) || c.is_whitespace()) {
        text.split_whitespace().collect::<String>().into()
    } else {
        text.into()
    }
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
/// Dictionary neighbours of the selected entry: other spellings, the other
/// entries the lookup returned and the entry's cross-references. Back side
/// only (UsageExamples), so no front shows the answer.
fn related(v: &Vocabulary) -> String {
    const MAX_ENTRIES: usize = 8;
    const MAX_GLOSS: usize = 80;
    let selected = v.dictionary.iter().position(|entry| {
        entry.senses.iter().any(|s| s.key == v.sense_key)
            && entry
                .forms
                .iter()
                .chain(&entry.readings)
                .any(|form| *form == v.expression)
    });
    let mut html = String::new();
    let chips = |values: Vec<&String>| {
        values
            .iter()
            .map(|value| {
                format!(
                    "<li><span class=\"lab-target\">{}</span></li>",
                    escape(value)
                )
            })
            .collect::<String>()
    };
    if let Some(index) = selected {
        let entry = &v.dictionary[index];
        let spellings: Vec<_> = entry
            .forms
            .iter()
            .filter(|form| **form != v.expression)
            .collect();
        if !spellings.is_empty() {
            html.push_str(&format!(
                "<h4>Also written</h4><ul class=\"lab-collocations\">{}</ul>",
                chips(spellings)
            ));
        }
    }
    let cambridge = selected.is_some_and(|i| v.dictionary[i].provider == "cambridge-html-v1");
    let others: Vec<_> = v
        .dictionary
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            Some(*index) != selected
                && !entry.forms.is_empty()
                && !entry.senses.is_empty()
                // Cambridge's other parts of speech are already in Meaning.
                && !(cambridge && entry.forms.contains(&v.expression))
        })
        .take(MAX_ENTRIES)
        .collect();
    if !others.is_empty() {
        html.push_str("<h4>Related words</h4><ul class=\"lab-examples\">");
        for (_, entry) in others {
            let mut gloss = entry.senses[0].definitions.join("; ");
            if gloss.chars().count() > MAX_GLOSS {
                gloss = gloss.chars().take(MAX_GLOSS - 1).collect::<String>() + "…";
            }
            html.push_str(&format!(
                "<li><div class=\"lab-target\"><b>{}</b>{}</div><div class=\"lab-tr\">{}</div></li>",
                highlighted(&entry.forms[0], &v.expression),
                entry
                    .readings
                    .first()
                    .filter(|reading| **reading != entry.forms[0])
                    .map(|reading| format!(" <span class=\"lab-tr\">{}</span>", escape(reading)))
                    .unwrap_or_default(),
                escape(&gloss)
            ));
        }
        html.push_str("</ul>");
    }
    if let Some(index) = selected {
        let entry = &v.dictionary[index];
        let metadata = |key: &str| {
            entry
                .metadata
                .get(key)
                .map(|v| v.iter().collect::<Vec<_>>())
        };
        if let Some(synonyms) = metadata("synonyms") {
            html.push_str(&format!(
                "<h4>Synonyms</h4><ul class=\"lab-collocations\">{}</ul>",
                chips(synonyms)
            ));
        }
        let see: Vec<_> = entry.related_entries.iter().collect();
        if !see.is_empty() {
            html.push_str(&format!(
                "<h4>{}</h4><ul class=\"lab-collocations\">{}</ul>",
                if cambridge { "Word family" } else { "See also" },
                chips(see)
            ));
        }
        if let Some(words) = metadata("smart_vocabulary") {
            const MAX_TOPIC_WORDS: usize = 12;
            let topic = metadata("smart_vocabulary_topic")
                .and_then(|t| t.first().map(|t| t.to_string()))
                .unwrap_or_else(|| "Related".into());
            html.push_str(&format!(
                "<h4>Topic: {}</h4><ul class=\"lab-collocations\">{}</ul>",
                escape(&topic),
                chips(
                    words
                        .into_iter()
                        .filter(|w| **w != v.expression)
                        .take(MAX_TOPIC_WORDS)
                        .collect()
                )
            ));
        }
    }
    html
}
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
/// Grammar v3 Meaning: the meaning in the explanation language, then the
/// source's own meaning line when one was captured.
fn grammar_meaning(g: &crate::document::Grammar) -> String {
    let mut html = format!(
        "<div class=\"lab-gloss\">{}</div>",
        escape(g.meaning.trim())
    );
    if !g.source_meaning.trim().is_empty() {
        html.push_str(&format!(
            "<div class=\"lab-source-meaning\">{}</div>",
            escape(g.source_meaning.trim())
        ));
    }
    html
}
/// The forms a grammar pattern takes in a sentence, longest first: reviewed
/// `forms` when present, else derived from the pattern by dropping the 〜
/// placeholder, word-class slots (N, V, Aい, ...) and `+`, splitting
/// alternatives (／, /, ・) and expanding optional parts in parentheses.
pub fn grammar_forms(g: &crate::document::Grammar) -> Vec<String> {
    let mut forms: Vec<String> = g
        .forms
        .iter()
        .map(|form| form.trim().to_owned())
        .filter(|form| !form.is_empty())
        .collect();
    if forms.is_empty() {
        let pattern = g
            .pattern
            .replace(['（', '〔'], "(")
            .replace(['）', '〕'], ")");
        for alternative in pattern.split(['／', '/', '・']) {
            // Keep only the Japanese pattern text: drop slots (N, V, Aい, Aな,
            // Vる: the class letter, a hyphen and its marker), placeholders
            // and spacing. Other kana after a slot (V-て, V-た) belong to the
            // pattern as it appears in sentences.
            let mut core = String::new();
            let mut after_slot = false;
            for c in alternative.chars() {
                if c.is_ascii_alphabetic() {
                    after_slot = true;
                    continue;
                }
                if after_slot && c == '-' {
                    continue;
                }
                let slot_marker = after_slot && matches!(c, 'い' | 'な' | 'る');
                after_slot = false;
                if slot_marker
                    || c.is_ascii_alphanumeric()
                    || c.is_whitespace()
                    || matches!(c, '〜' | '～' | '~' | '+' | '＋' | '…' | '.')
                {
                    continue;
                }
                core.push(c);
            }
            let expanded = match (core.find('('), core.find(')')) {
                (Some(open), Some(close)) if open < close => vec![
                    format!(
                        "{}{}{}",
                        &core[..open],
                        &core[open + 1..close],
                        &core[close + 1..]
                    ),
                    format!("{}{}", &core[..open], &core[close + 1..]),
                ],
                _ => vec![core.replace(['(', ')'], "")],
            };
            forms.extend(expanded.into_iter().filter(|form| form.chars().count() > 0));
        }
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.chars().count()));
    forms.dedup();
    forms
}
/// Example text with every occurrence of the longest matching form highlighted.
fn highlighted_forms(text: &str, forms: &[String]) -> String {
    match forms.iter().find(|form| text.contains(form.as_str())) {
        Some(form) => highlighted(text, form),
        None => escape(text),
    }
}
/// v3 grammar UsageExamples: usage, nuance against similar patterns and examples.
fn grammar_usage_examples(g: &crate::document::Grammar, forms: &[String]) -> String {
    let mut html = String::new();
    if !g.usage.trim().is_empty() {
        html.push_str(&format!("<h4>Usage</h4><p>{}</p>", escape(g.usage.trim())));
    }
    if !g.nuance.is_empty() {
        html.push_str("<h4>Nuance</h4><dl class=\"lab-nuance\">");
        for contrast in &g.nuance {
            html.push_str(&format!(
                "<div><dt>{}</dt><dd>{}</dd></div>",
                escape(&contrast.expression),
                escape(&contrast.difference)
            ));
        }
        html.push_str("</dl>");
    }
    if !g.examples.is_empty() {
        html.push_str("<h4>Examples</h4><ul class=\"lab-examples\">");
        for example in &g.examples {
            html.push_str(&format!(
                "<li><div class=\"lab-target\">{}</div>{}</li>",
                highlighted_forms(&example.sentence, forms),
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
        LearningContent::Grammar(g) => {
            tags.push("lab::kind::grammar".into());
            for (prefix, value) in [("jlpt", &g.jlpt), ("lesson", &g.lesson)] {
                let value = slug(value.trim_start_matches("jlpt-"));
                if !value.is_empty() {
                    tags.push(format!("lab::{prefix}::{value}"));
                }
            }
        }
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
    // Only the v2 models carry language and provenance fields; v3 models
    // carry them as tags (see `tags`).
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
            fields.insert("Pronunciation".into(), escape(&compact_kana(spoken_cue(v))));
            fields.insert(
                "Meaning".into(),
                dictionary::meaning(&v.dictionary, &v.expression, &v.sense_key, &v.meaning),
            );
            fields.insert("UsageExamples".into(), usage_examples(v) + &related(v));
            // Only the Japanese model has a Kanji section.
            if let Some(field) = fields.get_mut("Kanji") {
                *field = kanji(v, &doc.media);
            }
            for (key, task) in [
                ("EnableProduction", Task::Production),
                ("EnableSpelling", Task::Spelling),
            ] {
                let on = doc.requested_tasks.contains(&task);
                fields.insert(key.into(), if on { "1" } else { "" }.into());
            }
        }
        LearningContent::Grammar(g) => {
            let forms = grammar_forms(g);
            fields.insert("Pattern".into(), escape(&g.pattern));
            fields.insert("Meaning".into(), grammar_meaning(g));
            fields.insert("Formation".into(), block(&g.formation));
            fields.insert(
                "Example".into(),
                g.examples
                    .first()
                    .map(|e| highlighted_forms(&e.sentence, &forms))
                    .unwrap_or_default(),
            );
            fields.insert("UsageExamples".into(), grammar_usage_examples(g, &forms));
            fields.insert("ExercisePrompt".into(), escape(&g.exercise_prompt));
            fields.insert("ExerciseAnswer".into(), escape(&g.exercise_answer));
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
