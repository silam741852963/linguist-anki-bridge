use crate::{LearningContent, LearningDocument};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct FieldMatch {
    pub target_field: String,
    pub target_ordinal: usize,
    pub source_ordinals: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct TemplateMatch {
    pub target_template: String,
    pub target_ordinal: u16,
    pub present: bool,
    pub front_matches: bool,
    pub back_matches: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ModelComparison {
    pub target_model: String,
    pub target_version: u16,
    pub name_matches: bool,
    pub field_order_matches: bool,
    pub fields: Vec<FieldMatch>,
    pub missing_fields: Vec<String>,
    pub unexpected_fields: Vec<String>,
    pub duplicate_source_fields: Vec<String>,
    pub templates: Vec<TemplateMatch>,
    pub unexpected_templates: Vec<String>,
    pub css_matches: bool,
    pub exact_content_match: bool,
    pub source_template_ordinals_verified: bool,
    pub apply_authorized: bool,
}

/// Exact-name comparison only. This is a mapping proposal, never inferred aliases or task history.
pub fn compare(
    source_name: &str,
    fields: &[String],
    templates: &BTreeMap<String, (String, String)>,
    css: &str,
    target: &ManagedModel,
) -> ModelComparison {
    let missing_fields = target
        .fields
        .iter()
        .filter(|field| !fields.contains(field))
        .cloned()
        .collect();
    let unexpected_fields = fields
        .iter()
        .filter(|field| !target.fields.contains(field))
        .cloned()
        .collect();
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for field in fields {
        if !seen.insert(field) {
            duplicates.insert(field.clone());
        }
    }
    let matches: Vec<_> = target
        .templates
        .iter()
        .map(|template| {
            let source = templates.get(&template.name);
            TemplateMatch {
                target_template: template.name.clone(),
                target_ordinal: template.ordinal,
                present: source.is_some(),
                front_matches: source.is_some_and(|(front, _)| *front == template.front),
                back_matches: source.is_some_and(|(_, back)| *back == template.back),
            }
        })
        .collect();
    let unexpected_templates: Vec<_> = templates
        .keys()
        .filter(|name| !target.templates.iter().any(|t| &t.name == *name))
        .cloned()
        .collect();
    let field_order_matches = fields == target.fields;
    let css_matches = css == target.css;
    let exact_content_match = field_order_matches
        && css_matches
        && unexpected_templates.is_empty()
        && matches
            .iter()
            .all(|t| t.present && t.front_matches && t.back_matches);
    ModelComparison {
        target_model: target.name.clone(),
        target_version: target.version,
        name_matches: source_name == target.name,
        field_order_matches,
        fields: target
            .fields
            .iter()
            .enumerate()
            .map(|(ordinal, field)| FieldMatch {
                target_field: field.clone(),
                target_ordinal: ordinal,
                source_ordinals: fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, source)| (source == field).then_some(i))
                    .collect(),
            })
            .collect(),
        missing_fields,
        unexpected_fields,
        duplicate_source_fields: duplicates.into_iter().collect(),
        templates: matches,
        unexpected_templates,
        css_matches,
        exact_content_match,
        source_template_ordinals_verified: false,
        apply_authorized: false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub name: String,
    pub ordinal: u16,
    pub front: String,
    pub back: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagedModel {
    pub name: String,
    pub version: u16,
    pub fields: Vec<String>,
    pub templates: Vec<Template>,
    pub css: String,
}
const STYLE: &str = include_str!("../../../resources/templates/style.css");
fn template(name: &str, ordinal: u16, front: &str, back: &str) -> Template {
    Template {
        name: name.into(),
        ordinal,
        front: front.into(),
        back: back.into(),
    }
}
pub fn vocabulary() -> ManagedModel {
    ManagedModel {
        name: "Linguist Vocabulary v2".into(),
        version: 2,
        fields: [
            "Expression",
            "Reading",
            "Pronunciation",
            "Meaning",
            "Usage",
            "Examples",
            "Picture",
            "Audio",
            "Kanji",
            "PersonalNotes",
            "Source",
            "Language",
            "SenseKey",
            "EnableProduction",
            "EnableSpelling",
            "ProductionPrompt",
            "SpellingPrompt",
            "ExplanationLanguage",
        ]
        .map(str::to_owned)
        .to_vec(),
        templates: vec![
            template(
                "Comprehension",
                0,
                include_str!("../../../resources/templates/vocabulary-comprehension-front.html"),
                include_str!("../../../resources/templates/vocabulary-back.html"),
            ),
            template(
                "Production",
                1,
                include_str!("../../../resources/templates/vocabulary-production-front.html"),
                include_str!("../../../resources/templates/vocabulary-back.html"),
            ),
            template(
                "Spelling",
                2,
                include_str!("../../../resources/templates/vocabulary-spelling-front.html"),
                include_str!("../../../resources/templates/vocabulary-spelling-back.html"),
            ),
        ],
        css: STYLE.into(),
    }
}
pub fn grammar() -> ManagedModel {
    ManagedModel {
        name: "Linguist Grammar v2".into(),
        version: 2,
        fields: [
            "Pattern",
            "Meaning",
            "Formation",
            "Usage",
            "Examples",
            "ExercisePrompt",
            "ExerciseAnswer",
            "Audio",
            "PersonalNotes",
            "Source",
            "Language",
            "EnableApplication",
            "UseKey",
            "RecognitionPrompt",
            "ExplanationLanguage",
        ]
        .map(str::to_owned)
        .to_vec(),
        templates: vec![
            template(
                "Recognition",
                0,
                include_str!("../../../resources/templates/grammar-recognition-front.html"),
                include_str!("../../../resources/templates/grammar-back.html"),
            ),
            template(
                "Application",
                1,
                include_str!("../../../resources/templates/grammar-application-front.html"),
                include_str!("../../../resources/templates/grammar-application-back.html"),
            ),
        ],
        css: STYLE.into(),
    }
}
pub fn for_document(doc: &LearningDocument) -> ManagedModel {
    match doc.content {
        LearningContent::Vocabulary(_) => vocabulary(),
        LearningContent::Grammar(_) => grammar(),
    }
}
