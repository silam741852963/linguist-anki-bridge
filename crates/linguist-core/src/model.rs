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
/// RI-05: the one canonical model manifest projection. Fields keep Anki field
/// order, templates are sorted by ordinal, and the managed version is not part
/// of it (a collection note type has no version). Read capture, apply and the
/// companion (`addons/linguist_bridge/manifest.py`) all use this digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestProjection {
    pub name: String,
    pub fields: Vec<String>,
    pub templates: Vec<Template>,
    pub css: String,
}
impl ManifestProjection {
    pub fn new(name: &str, fields: &[String], templates: &[Template], css: &str) -> Self {
        let mut templates = templates.to_vec();
        templates.sort_by_key(|template| template.ordinal);
        Self {
            name: name.into(),
            fields: fields.to_vec(),
            templates,
            css: css.into(),
        }
    }
    /// RFC 8785 bytes; the digest is their lowercase SHA-256.
    pub fn bytes(&self) -> Result<Vec<u8>, crate::canonical::ContractError> {
        crate::canonical::bytes(self)
    }
    pub fn digest(&self) -> Result<String, crate::canonical::ContractError> {
        Ok(crate::canonical::asset_digest(&self.bytes()?))
    }
}
impl ManagedModel {
    pub fn projection(&self) -> ManifestProjection {
        ManifestProjection::new(&self.name, &self.fields, &self.templates, &self.css)
    }
    pub fn manifest_digest(&self) -> Result<String, crate::canonical::ContractError> {
        self.projection().digest()
    }
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
const VOCABULARY_STYLE: &str = include_str!("../../../resources/templates/vocabulary-v3.css");
/// Grammar v3 shares the vocabulary v3 visual system plus a few pattern rules.
const GRAMMAR_STYLE: &str = concat!(
    include_str!("../../../resources/templates/vocabulary-v3.css"),
    include_str!("../../../resources/templates/grammar-v3.css")
);
/// WP-19 vocabulary model: fields only, no text cues and no provenance fields.
/// Language and explanation language are note tags (`lab::lang::*`).
pub fn vocabulary() -> ManagedModel {
    let back = include_str!("../../../resources/templates/vocabulary-v3-back.html");
    ManagedModel {
        name: "Linguist Vocabulary v3".into(),
        version: 3,
        fields: [
            "Expression",
            "Pronunciation",
            "Meaning",
            "UsageExamples",
            "Picture",
            "Audio",
            "Kanji",
            "EnableProduction",
            "EnableSpelling",
        ]
        .map(str::to_owned)
        .to_vec(),
        templates: vec![
            template(
                "Comprehension",
                0,
                include_str!("../../../resources/templates/vocabulary-v3-comprehension-front.html"),
                back,
            ),
            template(
                "Production",
                1,
                include_str!("../../../resources/templates/vocabulary-v3-production-front.html"),
                back,
            ),
            template(
                "Spelling",
                2,
                include_str!("../../../resources/templates/vocabulary-v3-spelling-front.html"),
                include_str!("../../../resources/templates/vocabulary-v3-spelling-back.html"),
            ),
        ],
        css: VOCABULARY_STYLE.into(),
    }
}
/// WP-19 English vocabulary model: the v3 card without the Kanji section.
/// English cards get IPA in Pronunciation and Wiktionary audio.
pub fn english_vocabulary() -> ManagedModel {
    let back = include_str!("../../../resources/templates/english-vocabulary-v1-back.html");
    ManagedModel {
        name: "Linguist English Vocabulary v1".into(),
        version: 1,
        fields: [
            "Expression",
            "Pronunciation",
            "Meaning",
            "UsageExamples",
            "Picture",
            "Audio",
            "EnableProduction",
            "EnableSpelling",
        ]
        .map(str::to_owned)
        .to_vec(),
        templates: vec![
            template(
                "Comprehension",
                0,
                include_str!("../../../resources/templates/vocabulary-v3-comprehension-front.html"),
                back,
            ),
            template(
                "Production",
                1,
                include_str!("../../../resources/templates/vocabulary-v3-production-front.html"),
                back,
            ),
            template(
                "Spelling",
                2,
                include_str!("../../../resources/templates/vocabulary-v3-spelling-front.html"),
                include_str!(
                    "../../../resources/templates/english-vocabulary-v1-spelling-back.html"
                ),
            ),
        ],
        css: VOCABULARY_STYLE.into(),
    }
}
/// Every managed model, in a fixed order.
pub fn managed() -> Vec<ManagedModel> {
    vec![vocabulary(), english_vocabulary(), grammar()]
}
pub fn by_name(name: &str) -> Option<ManagedModel> {
    managed().into_iter().find(|model| model.name == name)
}
/// WP-22 grammar model: fields only, by analogy with vocabulary v3.
/// Languages, JLPT level and lesson travel as tags; the Recognition front
/// shows the pattern and one example with the pattern highlighted.
pub fn grammar() -> ManagedModel {
    ManagedModel {
        name: "Linguist Grammar v3".into(),
        version: 3,
        fields: [
            "Pattern",
            "Meaning",
            "Formation",
            "Example",
            "UsageExamples",
            "ExercisePrompt",
            "ExerciseAnswer",
            "Audio",
            "EnableApplication",
        ]
        .map(str::to_owned)
        .to_vec(),
        templates: vec![
            template(
                "Recognition",
                0,
                include_str!("../../../resources/templates/grammar-v3-recognition-front.html"),
                include_str!("../../../resources/templates/grammar-v3-back.html"),
            ),
            template(
                "Application",
                1,
                include_str!("../../../resources/templates/grammar-v3-application-front.html"),
                include_str!("../../../resources/templates/grammar-v3-application-back.html"),
            ),
        ],
        css: GRAMMAR_STYLE.into(),
    }
}
/// The WP-17 grammar model, kept so its notes can be read as revamp sources.
pub fn grammar_v2() -> ManagedModel {
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
        LearningContent::Vocabulary(_)
            if doc.target_language.as_str().split('-').next() == Some("en") =>
        {
            english_vocabulary()
        }
        LearningContent::Vocabulary(_) => vocabulary(),
        LearningContent::Grammar(_) => grammar(),
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn projection_digest_matches_the_companion_vector_and_ignores_version() {
        let template = Template {
            name: "Recognition".into(),
            ordinal: 0,
            front: "{{Expression}}".into(),
            back: "{{Meaning}}".into(),
        };
        let projection = ManifestProjection::new(
            "Linguist Vocabulary v2",
            &["Expression".into(), "Meaning".into()],
            &[template],
            ".card { color: black; }",
        );
        // Same vector as addons/linguist_bridge/tests/test_payloads.py.
        assert_eq!(
            projection.digest().unwrap(),
            "3ebd381fb2210421a3a1be2416450552fa26f2282d5eef997ad2340e903fc029"
        );
        let mut managed = vocabulary();
        let digest = managed.manifest_digest().unwrap();
        managed.version += 1;
        assert_eq!(managed.manifest_digest().unwrap(), digest);
        managed.templates.reverse();
        assert_eq!(managed.manifest_digest().unwrap(), digest);
        managed.fields.reverse();
        assert_ne!(managed.manifest_digest().unwrap(), digest);
    }
}
