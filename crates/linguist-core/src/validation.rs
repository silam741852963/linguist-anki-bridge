use crate::{
    document::*,
    records::{MediaRole, ReviewChoice},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
    Review,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub id: String,
    pub code: String,
    pub severity: Severity,
    pub stage: String,
    pub field: Option<String>,
    pub source_refs: Vec<String>,
    pub message: String,
}
impl Issue {
    pub fn new(
        code: &str,
        severity: Severity,
        field: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("{code}:{}", field.unwrap_or("document")),
            code: code.into(),
            severity,
            stage: "validation".into(),
            field: field.map(str::to_owned),
            source_refs: Vec::new(),
            message: message.into(),
        }
    }
}

pub fn safe_media_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 240
        && !s.starts_with('.')
        && !s.contains("..")
        && !s.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '/' | '\\' | '<' | '>' | '"' | '\'' | '[' | ']' | ':' | '?' | '#'
                )
        })
}
pub fn answer_leaks(prompt: &str, answer: &str, language: &Language) -> bool {
    let p = prompt.to_lowercase();
    let a = answer.trim().to_lowercase();
    if a.is_empty() {
        return false;
    }
    if language.as_str().starts_with("ja") {
        return p.contains(&a);
    }
    p.match_indices(&a).any(|(i, _)| {
        let left = p[..i].chars().next_back();
        let right = p[i + a.len()..].chars().next();
        !left.is_some_and(|c| c.is_alphanumeric()) && !right.is_some_and(|c| c.is_alphanumeric())
    })
}
pub fn validate(doc: &LearningDocument) -> Vec<Issue> {
    let mut issues = doc.issues.clone();
    // Resolved capture observations remain in the document as warning records.
    // Re-open them unless a decision still proves the exact current content/evidence.
    for issue in &mut issues {
        if issue.stage == "capture"
            && matches!(
                issue.code.as_str(),
                "SOURCE_HTML_TEXT_REVIEW" | "SOURCE_EXAMPLES_REVIEW"
            )
        {
            issue.severity = Severity::Review;
        }
    }
    let mut add = |code: &str, severity: Severity, field: Option<&str>, message: &str| {
        issues.push(Issue::new(code, severity, field, message))
    };
    if doc.schema_version != 2 {
        add(
            "UNSUPPORTED_DOCUMENT_VERSION",
            Severity::Error,
            None,
            "Only learning document v2 is supported.",
        )
    }
    if !doc.target_language.is_target_supported() {
        add(
            "UNSUPPORTED_LANGUAGE",
            Severity::Error,
            Some("target_language"),
            "Initial validated targets are Japanese and English.",
        )
    }
    let tasks: BTreeSet<_> = doc.requested_tasks.iter().copied().collect();
    if tasks.is_empty() {
        add(
            "NO_TASKS",
            Severity::Error,
            Some("requested_tasks"),
            "Choose at least one learning task.",
        )
    }
    if tasks.len() != doc.requested_tasks.len() {
        add(
            "DUPLICATE_TASK",
            Severity::Error,
            Some("requested_tasks"),
            "Learning tasks must be unique.",
        )
    }
    let (required, answer, allowed): (Vec<(&str, &str)>, &str, &[Task]) = match &doc.content {
        LearningContent::Vocabulary(v) => {
            if tasks.contains(&Task::Production) {
                if v.production_prompt.trim().is_empty() {
                    add(
                        "MISSING_CUE",
                        Severity::Error,
                        Some("production_prompt"),
                        "Production requires a reviewed specific cue.",
                    )
                }
                if answer_leaks(&v.production_prompt, &v.expression, &doc.target_language) {
                    add(
                        "ANSWER_LEAK",
                        Severity::Error,
                        Some("production_prompt"),
                        "Production cue exposes the answer.",
                    )
                }
            }
            if tasks.contains(&Task::Spelling) {
                if v.spelling_prompt.trim().is_empty() {
                    add(
                        "MISSING_CUE",
                        Severity::Error,
                        Some("spelling_prompt"),
                        "Spelling requires an appropriate text/audio cue.",
                    )
                }
                if answer_leaks(&v.spelling_prompt, &v.expression, &doc.target_language) {
                    add(
                        "ANSWER_LEAK",
                        Severity::Error,
                        Some("spelling_prompt"),
                        "Spelling cue exposes the exact answer.",
                    )
                }
            }
            (
                vec![
                    ("expression", &v.expression),
                    ("meaning", &v.meaning),
                    ("sense_key", &v.sense_key),
                ],
                v.expression.as_str(),
                &[Task::Comprehension, Task::Production, Task::Spelling],
            )
        }
        LearningContent::Grammar(g) => {
            if g.examples.is_empty() {
                add(
                    "GRAMMAR_EXAMPLE_REQUIRED",
                    Severity::Error,
                    Some("examples"),
                    "Grammar requires at least one validated use example.",
                )
            }
            if tasks.contains(&Task::Application) {
                if g.exercise_prompt.trim().is_empty() || g.exercise_answer.trim().is_empty() {
                    add(
                        "MISSING_EXERCISE",
                        Severity::Error,
                        Some("exercise_prompt"),
                        "Application requires both prompt and answer.",
                    )
                }
                if answer_leaks(&g.exercise_prompt, &g.exercise_answer, &doc.target_language) {
                    add(
                        "ANSWER_LEAK",
                        Severity::Error,
                        Some("exercise_prompt"),
                        "Exercise cue exposes its answer.",
                    )
                }
            }
            (
                vec![
                    ("pattern", &g.pattern),
                    ("meaning", &g.meaning),
                    ("formation", &g.formation),
                    ("use_key", &g.use_key),
                    ("recognition_prompt", &g.recognition_prompt),
                ],
                g.pattern.as_str(),
                &[Task::Recognition, Task::Application],
            )
        }
    };
    for (field, value) in required {
        if value.trim().is_empty() {
            add(
                "REQUIRED_CONTENT",
                Severity::Error,
                Some(field),
                "Required learning content is empty.",
            )
        }
    }
    for task in &tasks {
        if !allowed.contains(task) {
            add(
                "TASK_KIND_MISMATCH",
                Severity::Error,
                Some("requested_tasks"),
                "Task is incompatible with document kind.",
            )
        }
    }
    let expected_model = crate::model::for_document(doc);
    for key in doc.edits.keys() {
        if !expected_model.fields.iter().any(|s| s == key) {
            add(
                "UNKNOWN_FIELD",
                Severity::Error,
                Some(key),
                "Field edit is outside the fixed target model.",
            )
        }
    }
    // Identity/task controls are typed domain properties, not generic string field edits.
    for key in doc.edits.keys() {
        if matches!(
            key.as_str(),
            "Picture"
                | "Audio"
                | "Examples"
                | "Expression"
                | "Pattern"
                | "SenseKey"
                | "UseKey"
                | "Language"
                | "ExplanationLanguage"
                | "EnableProduction"
                | "EnableSpelling"
                | "EnableApplication"
                | "ProductionPrompt"
                | "SpellingPrompt"
                | "RecognitionPrompt"
                | "ExercisePrompt"
                | "ExerciseAnswer"
        ) {
            add(
                "TYPED_EDIT_REQUIRED",
                Severity::Error,
                Some(key),
                "Edit identity/cues/tasks through typed document properties, then revalidate.",
            )
        }
    }
    let mut filenames = BTreeSet::new();
    for asset in &doc.media {
        if !safe_media_name(&asset.filename) {
            add(
                "UNSAFE_MEDIA_NAME",
                Severity::Error,
                Some("media"),
                "Media name is unsafe for local Anki references.",
            )
        }
        if !filenames.insert(asset.filename.to_lowercase()) {
            add(
                "MEDIA_NAME_COLLISION",
                Severity::Error,
                Some("media"),
                "Case-insensitive media filenames collide.",
            )
        }
        if asset.digest.len() != 64
            || !asset
                .digest
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || asset.size_bytes == 0
        {
            add(
                "INVALID_ASSET_MANIFEST",
                Severity::Error,
                Some("media"),
                "Asset requires SHA-256 bytes digest and positive size.",
            )
        }
        let valid = match asset.role {
            MediaRole::Picture => matches!(
                asset.mime.as_str(),
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            ),
            MediaRole::Audio => matches!(
                asset.mime.as_str(),
                "audio/mpeg" | "audio/ogg" | "audio/wav"
            ),
            MediaRole::Archive => true,
        };
        if !valid {
            add(
                "INVALID_MEDIA_TYPE",
                Severity::Error,
                Some("media"),
                "Media MIME does not match its rendered role.",
            )
        }
    }
    for source in &doc.sources {
        if !doc
            .archives
            .iter()
            .any(|a| a.source_id == source.id && a.original_fields == source.fields)
        {
            add(
                "SOURCE_ARCHIVE_REQUIRED",
                Severity::Error,
                Some("archives"),
                "Every captured source requires a matching full original-field archive.",
            )
        }
        for name in &source.media_refs {
            if !doc.media.iter().any(|m| {
                m.source_id == Some(source.id)
                    && (m.original_filename.as_ref() == Some(name) || &m.filename == name)
            }) {
                add(
                    "SOURCE_MEDIA_MISSING",
                    Severity::Error,
                    Some("archives"),
                    "Referenced source media must remain archived.",
                )
            }
        }
        for card in &source.cards {
            if !tasks.contains(&card.task) {
                add(
                    "RETAINED_TASK_REMOVAL",
                    Severity::Error,
                    Some("requested_tasks"),
                    "Existing task/history cannot be removed to satisfy new-note defaults.",
                )
            }
        }
    }
    let examples = match &doc.content {
        LearningContent::Vocabulary(v) => &v.examples,
        LearningContent::Grammar(g) => &g.examples,
    };
    let translation_required = doc.target_language.as_str().split('-').next()
        != doc.explanation_language.as_str().split('-').next();
    for e in examples {
        if e.sentence.trim().is_empty() || (translation_required && e.translation.trim().is_empty())
        {
            add(
                "INCOMPLETE_EXAMPLE",
                Severity::Error,
                Some("examples"),
                "Example requires a sentence and, for different languages, an explanation-language translation.",
            )
        }
    }
    let _ = answer;
    let source_ids: BTreeSet<_> = doc.sources.iter().map(|s| s.id).collect();
    let region_ids: BTreeSet<_> = doc.regions.iter().map(|s| s.id).collect();
    let evidence_ids: BTreeSet<_> = doc.evidence.iter().map(|s| s.id).collect();
    if source_ids.len() != doc.sources.len()
        || region_ids.len() != doc.regions.len()
        || evidence_ids.len() != doc.evidence.len()
    {
        add(
            "DUPLICATE_RECORD_ID",
            Severity::Error,
            None,
            "Evidence/source/region IDs must be unique within their record type.",
        );
    }
    for region in &doc.regions {
        if !source_ids.contains(&region.source_id)
            || region
                .confidence
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            || region.bounds[2] == 0
            || region.bounds[3] == 0
        {
            add(
                "INVALID_SOURCE_REGION",
                Severity::Error,
                Some("regions"),
                "Region requires an existing source, nonempty bounds and confidence in [0,1].",
            );
        }
    }
    for evidence in &doc.evidence {
        if evidence
            .source_id
            .is_some_and(|id| !source_ids.contains(&id))
            || evidence
                .region_id
                .is_some_and(|id| !region_ids.contains(&id))
        {
            add(
                "EVIDENCE_REFERENCE_MISSING",
                Severity::Error,
                Some("evidence"),
                "Evidence references a missing source or region.",
            );
        }
    }
    for example in examples {
        if example
            .evidence_ids
            .iter()
            .any(|id| !evidence_ids.contains(id))
        {
            add(
                "EVIDENCE_REFERENCE_MISSING",
                Severity::Error,
                Some("examples"),
                "Example references missing evidence.",
            );
        }
        if example.provenance == Provenance::Generated && example.evidence_ids.is_empty() {
            add(
                "GENERATED_EXAMPLE_EVIDENCE_REQUIRED",
                Severity::Error,
                Some("examples"),
                "Generated examples require explicit evidence and a content verification decision.",
            );
        }
    }
    for evidence in &doc.evidence {
        if evidence.provenance == Provenance::Generated
            || examples.iter().any(|example| {
                example.provenance == Provenance::Generated
                    && example.evidence_ids.contains(&evidence.id)
            })
        {
            let mut issue = Issue::new(
                "GENERATED_FACT_REVIEW",
                Severity::Review,
                Some(&evidence.field),
                "Verify novel generated facts against source evidence.",
            );
            issue.id = format!("GENERATED_FACT_REVIEW:{}", evidence.id);
            issue.source_refs.push(evidence.id.to_string());
            issues.push(issue);
        }
    }
    if let LearningContent::Vocabulary(vocab) = &doc.content
        && !vocab.dictionary.is_empty()
    {
        let mut issue = Issue::new(
            "DICTIONARY_SENSE_REVIEW",
            Severity::Review,
            Some("sense_key"),
            "Select the intended exact-match dictionary sense with a typed sense decision.",
        );
        issue.id = format!("DICTIONARY_SENSE_REVIEW:{}", doc.id);
        issue.source_refs = vocab
            .dictionary
            .iter()
            .flat_map(|entry| entry.senses.iter().map(|sense| sense.key.clone()))
            .collect();
        issues.push(issue);
        for field in ["Meaning", "Reading", "Pronunciation"] {
            if doc.edits.contains_key(field) {
                issues.push(Issue::new("DICTIONARY_FACT_OVERRIDE",Severity::Error,Some(field),"Dictionary facts require a typed content correction, not a generic rendered-field edit."));
            }
        }
    }
    let input = doc.semantic_digest().ok();
    issues.retain(|issue| {
        if issue.severity != Severity::Review {
            return true;
        }
        !doc.reviews.iter().any(|r| {
            r.issue_id == issue.id
                && Some(&r.input_digest) == input.as_ref()
                && !r.actor.trim().is_empty()
                && match &r.choice {
                    ReviewChoice::SourceContentVerified { source_id, evidence_ids } =>
                        crate::review::source_content_verified(doc, issue, *source_id, evidence_ids),
                    ReviewChoice::ContentVerified { evidence_ids } => {
                        issue.code == "GENERATED_FACT_REVIEW"
                            && !issue.source_refs.is_empty()
                            && issue
                                .source_refs
                                .iter()
                                .all(|s| evidence_ids.iter().any(|id| id.to_string() == *s))
                    }
                    ReviewChoice::Sense(key) => {
                        issue.code == "DICTIONARY_SENSE_REVIEW"
                            && matches!(&doc.content, LearningContent::Vocabulary(vocab) if vocab.sense_key == *key
                                && vocab.dictionary.iter().any(|entry| entry.forms.iter().chain(&entry.readings).any(|form|form == &vocab.expression) && crate::review::dictionary_readings(entry,&vocab.expression).is_ok_and(|readings|readings.contains(&vocab.reading) || doc.target_language.as_str().split('-').next()==Some("en") && readings.is_empty() && vocab.reading.is_empty()) && entry.senses.iter().any(|sense| sense.key == *key && vocab.meaning == sense.definitions.join("; "))))
                    }
                    _ => false,
                }
        })
    });
    issues
}
pub fn ready(doc: &LearningDocument) -> bool {
    validate(doc)
        .iter()
        .all(|i| i.severity == Severity::Warning)
}
