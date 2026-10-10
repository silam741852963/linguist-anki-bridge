//! ALG-VALIDATE: offline semantic validation of one v2 document (issues, cues, leakage, evidence).
use crate::{
    canonical,
    document::*,
    records::{EvidenceTarget, MediaOwner, MediaRole, ReviewChoice, TargetModelKind},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

/// Stored stage observations that stay in the document after resolution and
/// reopen as review unless a decision still matches the current content.
pub fn reopenable(issue: &Issue) -> bool {
    match issue.stage.as_str() {
        "capture" => matches!(
            issue.code.as_str(),
            "SOURCE_HTML_TEXT_REVIEW"
                | "SOURCE_UNMAPPED_FIELD_REVIEW"
                | "SOURCE_NATIVE_HISTORY_REVIEW"
                | "SOURCE_MEDIA_MISSING_REVIEW"
                | "SOURCE_EXAMPLES_REVIEW"
                | "SOURCE_MEDIA_CONTENT_REVIEW"
                | "SOURCE_MEDIA_FORMAT_REVIEW"
                | "SOURCE_AUDIO_COMPLETENESS_REVIEW"
        ),
        "ocr" => crate::review::OCR_REVIEW_CODES.contains(&issue.code.as_str()),
        "enrichment" => crate::review::CANDIDATE_REVIEW_CODES.contains(&issue.code.as_str()),
        "duplicates" => issue.code == "COLLECTION_DUPLICATE_REVIEW",
        "segmentation" => issue.code == "GRAMMAR_SEGMENTATION_REVIEW",
        _ => false,
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
    // Validation-stage issues are always recomputed; only stage observations
    // (capture, OCR, dictionary, enrichment, generation, ...) are stored input.
    let mut issues: Vec<Issue> = doc
        .issues
        .iter()
        .filter(|issue| issue.stage != "validation")
        .cloned()
        .collect();
    // Resolved capture observations remain in the document as warning records.
    // Re-open them unless a decision still proves the exact current content/evidence.
    for issue in &mut issues {
        if reopenable(issue) {
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
            // One note holding several words (私 / 僕 / 俺) or readings
            // (おどかす / おびやかす) is split, one note per unit.
            let japanese = doc.target_language.as_str().split('-').next() == Some("ja");
            let readings = vocabulary_units(&v.pronunciation);
            if japanese
                && (vocabulary_units(&v.expression).len() > 1
                    || readings.len() > 1 && readings.iter().all(|r| is_kana(r)))
            {
                add(
                    "VOCAB_SPLIT_REQUIRED",
                    Severity::Error,
                    Some("expression"),
                    "This note holds several words or readings; split it with plans split-vocab.",
                )
            }
            // v3 fronts show fields, not text cues: Production shows Picture and
            // Meaning (masked), Spelling shows Audio, Pronunciation and Meaning.
            if tasks.contains(&Task::Spelling) {
                // A reading equal to the word (a kana word) is not shown; the
                // front is then the recording and the meaning.
                let spoken = crate::render::spoken_cue(v);
                let has_audio = doc.media.iter().any(|m| m.role == MediaRole::Audio);
                if spoken.trim().is_empty() && !has_audio {
                    add(
                        "SPELLING_CUE_MISSING",
                        Severity::Error,
                        Some("pronunciation"),
                        "Spelling needs a pronunciation or audio on its front.",
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
            if tasks.contains(&Task::Recognition)
                && !g.recognition_prompt.trim().is_empty()
                && answer_leaks(&g.recognition_prompt, &g.meaning, &doc.explanation_language)
            {
                add(
                    "ANSWER_LEAK",
                    Severity::Error,
                    Some("recognition_prompt"),
                    "Recognition cue exposes the meaning it asks for.",
                )
            }
            // Grammar v4 (WP-23) has no Application card.
            if tasks.contains(&Task::Application) {
                add(
                    "GRAMMAR_APPLICATION_RETIRED",
                    Severity::Error,
                    Some("requested_tasks"),
                    "Grammar cards have no Application task; request Recognition only.",
                )
            }
            (
                vec![
                    ("pattern", &g.pattern),
                    ("meaning", &g.meaning),
                    ("formation", &g.formation),
                    ("use_key", &g.use_key),
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
    // One name with the same bytes is one file: re-revamping a managed note
    // archives its stroke GIF under the name enrichment renders again.
    let mut filenames = BTreeMap::new();
    for asset in &doc.media {
        if !safe_media_name(&asset.filename) {
            add(
                "UNSAFE_MEDIA_NAME",
                Severity::Error,
                Some("media"),
                "Media name is unsafe for local Anki references.",
            )
        }
        if filenames
            .insert(asset.filename.to_lowercase(), &asset.digest)
            .is_some_and(|digest| *digest != asset.digest)
        {
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
            MediaRole::KanjiStroke => {
                asset.mime == "image/gif"
                    && matches!(&doc.content, LearningContent::Vocabulary(v)
                        if v.kanji_details.iter().any(|k| k.stroke_digest.as_deref() == Some(asset.digest.as_str())))
            }
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
        if source.text.as_ref().is_some_and(|text| {
            let digest = canonical::asset_digest(text.as_bytes());
            !doc.archives.iter().any(|archive| {
                archive.source_id == source.id && archive.asset_digests.contains(&digest)
            })
        }) {
            add(
                "SOURCE_TEXT_ASSET_REQUIRED",
                Severity::Error,
                Some("archives"),
                "Full source text requires its exact bytes in the source archive.",
            );
        }
        if source.captured_at_unix_seconds == Some(0) {
            add(
                "SOURCE_CAPTURE_TIME_INVALID",
                Severity::Error,
                Some("sources"),
                "Capture time must be a positive Unix timestamp when present.",
            );
        }
        if source.template_manifest.as_ref().is_some_and(|digest| {
            digest.len() != 64
                || !digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || !doc.archives.iter().any(|archive| {
                    archive.source_id == source.id && archive.asset_digests.contains(digest)
                })
        }) {
            add(
                "SOURCE_TEMPLATE_ARCHIVE_REQUIRED",
                Severity::Error,
                Some("archives"),
                "Template manifest requires a valid digest retained in its source archive.",
            );
        }
        if !doc.archives.iter().any(|a| {
            a.source_id == source.id
                && a.original_fields == source.fields
                && a.original_text == source.text
        }) {
            add(
                "SOURCE_ARCHIVE_REQUIRED",
                Severity::Error,
                Some("archives"),
                "Every captured source requires a matching full original-text and field archive.",
            )
        }
        for name in &source.media_refs {
            let acknowledged_missing = doc.reviews.iter().any(|r| {
                matches!(&r.choice, ReviewChoice::MissingMedia { source_id, filename }
                    if *source_id == source.id && filename == name)
            });
            if !acknowledged_missing
                && !doc.media.iter().any(|m| {
                    m.source_id == Some(source.id)
                        && (m.original_filename.as_ref() == Some(name) || &m.filename == name)
                })
            {
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
    let mut mapped_sources = BTreeSet::new();
    for map in &doc.task_maps {
        let source = doc.sources.iter().find(|source| source.id == map.source_id);
        let kind_matches = matches!(
            (&doc.content, map.target_model),
            (LearningContent::Vocabulary(_), TargetModelKind::Vocabulary)
                | (LearningContent::Grammar(_), TargetModelKind::Grammar)
        );
        if map.validate().is_err()
            || !mapped_sources.insert(map.source_id)
            || !kind_matches
            || !source.is_some_and(|source| {
                source.kind == "anki_read_capture_v2"
                    && source.model_manifest == map.source_model_digest
                    && source.cards.iter().all(|card| {
                        map.entries
                            .iter()
                            .any(|entry| entry.target_task == card.task)
                    })
            })
            || map
                .entries
                .iter()
                .any(|entry| !tasks.contains(&entry.target_task))
        {
            add(
                "SOURCE_TASK_MAP_INVALID",
                Severity::Error,
                Some("task_maps"),
                "Task map must bind one captured source model to unique, requested target tasks and fixed template ordinals.",
            );
        }
    }
    for asset in &doc.media {
        if asset
            .source_id
            .is_some_and(|id| !doc.sources.iter().any(|source| source.id == id))
        {
            add(
                "MEDIA_SOURCE_MISSING",
                Severity::Error,
                Some("media"),
                "Media references a source absent from this document.",
            );
        }
        if asset.owner == MediaOwner::Source
            && !asset.source_id.is_some_and(|id| {
                doc.archives.iter().any(|archive| {
                    archive.source_id == id && archive.asset_digests.contains(&asset.digest)
                })
            })
        {
            add(
                "SOURCE_MEDIA_ARCHIVE_REQUIRED",
                Severity::Error,
                Some("media"),
                "Source-owned media requires its source and retained bytes in that source's archive.",
            );
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
        if let Some(span) = &evidence.source_span {
            let valid = evidence.source_id.is_some_and(|source_id| {
                doc.sources
                    .iter()
                    .find(|source| source.id == source_id)
                    .and_then(|source| source.text.as_deref())
                    .is_some_and(|text| {
                        let start = span.start_byte as usize;
                        let end = span.end_byte as usize;
                        start < end
                            && end <= text.len()
                            && text.is_char_boundary(start)
                            && text.is_char_boundary(end)
                    })
            });
            if !valid {
                add(
                    "EVIDENCE_SOURCE_SPAN_INVALID",
                    Severity::Error,
                    Some("evidence"),
                    "Source span must select complete UTF-8 characters within retained source text.",
                );
            }
        }
        let target_valid = match &evidence.target {
            None => true,
            Some(EvidenceTarget::DictionarySense {
                entry_index,
                sense_index,
            }) => matches!(&doc.content, LearningContent::Vocabulary(v)
                if evidence.provenance == Provenance::Dictionary
                    && v.dictionary.get(*entry_index).is_some_and(|entry| entry.senses.get(*sense_index).is_some())),
            Some(EvidenceTarget::Example { index }) => match &doc.content {
                LearningContent::Vocabulary(v) => v.examples.get(*index),
                LearningContent::Grammar(g) => g.examples.get(*index),
            }
            .is_some_and(|example| {
                example.provenance == evidence.provenance
                    && example.evidence_ids.contains(&evidence.id)
            }),
            Some(EvidenceTarget::GrammarFormation) => {
                evidence.field == "formation" && matches!(&doc.content, LearningContent::Grammar(_))
            }
            Some(EvidenceTarget::MediaAsset { digest }) => doc.media.iter().any(|asset| {
                asset.digest == digest.as_str() && asset.source_id == evidence.source_id
            }),
        };
        if !target_valid {
            add(
                "EVIDENCE_TARGET_INVALID",
                Severity::Error,
                Some("evidence"),
                "Evidence target must match a current sense, example, formation or media asset.",
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
    // Only an entry whose written form or reading is the word itself offers a
    // sense to select; partial matches (副 for 副委員長) leave the meaning to
    // the author.
    if let LearningContent::Vocabulary(vocab) = &doc.content
        && vocab.dictionary.iter().any(|entry| {
            entry.forms.contains(&vocab.expression) || entry.readings.contains(&vocab.expression)
        })
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
    // A reviewed multi-unit segmentation must be carried out by a split.
    if doc.reviews.iter().any(|review| {
        matches!(&review.choice, ReviewChoice::Segmentation(ids) if ids.len() > 1)
            && review.issue_id.starts_with("GRAMMAR_SEGMENTATION_REVIEW:")
    }) {
        issues.push(Issue::new(
            "GRAMMAR_SPLIT_PENDING",
            Severity::Error,
            Some("pattern"),
            "The reviewed segmentation has several units; publish them with plans split-grammar.",
        ));
    }
    // Independent source/user claims for one field must agree or be reviewed.
    for field in ["pattern", "meaning", "formation", "usage", "reading"] {
        let claims: Vec<_> = doc
            .evidence
            .iter()
            .filter(|e| {
                e.field == field && matches!(e.provenance, Provenance::Source | Provenance::User)
            })
            .collect();
        let distinct: BTreeSet<String> = claims
            .iter()
            .map(|e| e.claim.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|claim| !claim.is_empty())
            .collect();
        if distinct.len() > 1 {
            let mut issue = Issue::new(
                "SOURCE_CLAIM_CONFLICT",
                Severity::Review,
                Some(field),
                "Sources disagree about this field; verify the kept value against every claim.",
            );
            let mut refs: Vec<String> = claims.iter().map(|e| e.id.to_string()).collect();
            refs.sort();
            issue.id = format!("SOURCE_CLAIM_CONFLICT:{field}");
            issue.source_refs = refs;
            issues.push(issue);
        }
    }
    // A mapped enable field (`enable_production`, `enable_spelling`,
    // `enable_application`) is a task candidate until the reviewed native task
    // map for that source retains the task; its card history then decides.
    issues.retain(|issue| {
        if issue.code != "SOURCE_TASK_MAPPING_REVIEW" {
            return true;
        }
        let task = match issue.field.as_deref() {
            Some("enable_production") => Task::Production,
            Some("enable_spelling") => Task::Spelling,
            Some("enable_application") => Task::Application,
            _ => return true,
        };
        !doc.task_maps.iter().any(|map| {
            issue.source_refs == [map.source_id.to_string()]
                && map.validate().is_ok()
                && map.entries.iter().any(|entry| entry.target_task == task)
                && doc.requested_tasks.contains(&task)
        })
    });
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
                    ReviewChoice::SourceMediaRole { .. } => crate::review::source_media_matches(doc, issue, &r.choice, true),
                    ReviewChoice::SourceContentVerified { source_id, evidence_ids } =>
                        crate::review::source_content_verified(doc, issue, *source_id, evidence_ids),
                    ReviewChoice::SourceFieldDropped { source_id, field } =>
                        crate::review::source_field_dropped(doc, issue, *source_id, field),
                    ReviewChoice::Media(digest) => crate::review::candidate_choice_matches(doc, issue, digest),
                    ReviewChoice::Segmentation(ids) => issue.code == "GRAMMAR_SEGMENTATION_REVIEW"
                        && crate::review::segmentation_valid(issue, ids)
                        && (ids.len() > 1 || matches!(&doc.content, LearningContent::Grammar(g) if !g.pattern.trim().is_empty())),
                    ReviewChoice::Duplicate { note_id, action } =>
                        crate::review::duplicate_choice_matches(issue, note_id, action),
                    ReviewChoice::NativeHistory { .. } =>
                        crate::review::native_history_matches(doc, issue, &r.choice),
                    ReviewChoice::MissingMedia { .. } =>
                        crate::review::missing_media_matches(doc, issue, &r.choice),
                    ReviewChoice::Anchor(anchor) => issue.code == "GRAMMAR_SPLIT_NATIVE_REVIEW"
                        && crate::review::split_anchor_matches(doc, *anchor),
                    ReviewChoice::ContentVerified { evidence_ids } => {
                        matches!(issue.code.as_str(), "GENERATED_FACT_REVIEW" | "SOURCE_CLAIM_CONFLICT")
                            && !issue.source_refs.is_empty()
                            && issue
                                .source_refs
                                .iter()
                                .all(|s| evidence_ids.iter().any(|id| id.to_string() == *s))
                    }
                    ReviewChoice::Sense(key) | ReviewChoice::SenseWithReading { key, .. } => {
                        issue.code == "DICTIONARY_SENSE_REVIEW"
                            && match &r.choice {
                                ReviewChoice::SenseWithReading { reading, .. } =>
                                    matches!(&doc.content, LearningContent::Vocabulary(vocab) if vocab.reading == *reading),
                                _ => true,
                            }
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

/// The units of a field that lists several words or readings: one per line,
/// or separated by a full-width slash (ふし／せつ). Whitespace inside a unit
/// is dropped (older notes space kana at kanji boundaries).
pub fn vocabulary_units(text: &str) -> Vec<String> {
    text.split(['\n', '／'])
        .map(|unit| unit.split_whitespace().collect::<String>())
        .filter(|unit| !unit.is_empty())
        .collect()
}

fn is_kana(text: &str) -> bool {
    text.chars()
        .all(|c| matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}'))
}
