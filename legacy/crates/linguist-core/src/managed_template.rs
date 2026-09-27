//! Read-only managed-model validation and refresh planning.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelTemplate {
    pub name: String,
    pub front: String,
    pub back: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedModelSpec {
    pub model_name: String,
    pub fields: Vec<String>,
    pub templates: Vec<ModelTemplate>,
    pub css: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedModel {
    pub model_name: String,
    pub fields: Vec<String>,
    pub templates: Vec<ModelTemplate>,
    pub css: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedTemplatePlan {
    Create {
        spec: ManagedModelSpec,
    },
    UpgradeLegacy {
        rename: (String, String),
        add: Vec<ModelTemplate>,
        refresh_templates: bool,
        refresh_css: bool,
    },
    Refresh {
        templates: bool,
        css: bool,
    },
    NoChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedTemplateError {
    ForeignModel {
        expected: String,
        actual: String,
    },
    UnsafeFieldOrder {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    UnexpectedTemplates {
        expected: Vec<String>,
        actual: Vec<String>,
    },
}

impl std::fmt::Display for ManagedTemplateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignModel { expected, actual } => {
                write!(
                    formatter,
                    "expected managed model {expected:?}, found {actual:?}"
                )
            }
            Self::UnsafeFieldOrder { expected, actual } => {
                write!(
                    formatter,
                    "managed fields differ: expected {expected:?}, found {actual:?}"
                )
            }
            Self::UnexpectedTemplates { expected, actual } => {
                write!(
                    formatter,
                    "managed templates differ: expected {expected:?}, found {actual:?}"
                )
            }
        }
    }
}

impl std::error::Error for ManagedTemplateError {}

pub fn japanese_vocab_spec() -> ManagedModelSpec {
    ManagedModelSpec {
        model_name: "Linguist Japanese Vocabulary".into(),
        fields: ["Expression", "Picture", "Meaning", "Kanji", "Audio"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        templates: vec![
            template(
                "Comprehension",
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/front.html"
                ),
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/back.html"
                ),
            ),
            template(
                "Spelling",
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/spelling_front.html"
                ),
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/spelling_back.html"
                ),
            ),
            template(
                "Production",
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/production_front.html"
                ),
                include_str!(
                    "../../../src/linguist_anki_bridge/templates/japanese_vocab/back.html"
                ),
            ),
        ],
        css: include_str!("../../../src/linguist_anki_bridge/templates/japanese_vocab/style.css")
            .trim()
            .into(),
    }
}

pub fn plan_japanese_vocab_template(
    observed: Option<&ObservedModel>,
) -> Result<ManagedTemplatePlan, ManagedTemplateError> {
    let spec = japanese_vocab_spec();
    let Some(observed) = observed else {
        return Ok(ManagedTemplatePlan::Create { spec });
    };
    if observed.model_name != spec.model_name {
        return Err(ManagedTemplateError::ForeignModel {
            expected: spec.model_name,
            actual: observed.model_name.clone(),
        });
    }
    if observed.fields != spec.fields {
        return Err(ManagedTemplateError::UnsafeFieldOrder {
            expected: spec.fields,
            actual: observed.fields.clone(),
        });
    }
    let actual_names = observed
        .templates
        .iter()
        .map(|template| template.name.clone())
        .collect::<Vec<_>>();
    let expected_names = spec
        .templates
        .iter()
        .map(|template| template.name.clone())
        .collect::<Vec<_>>();
    if actual_names == ["Japanese Recognition"] {
        return Ok(ManagedTemplatePlan::UpgradeLegacy {
            rename: ("Japanese Recognition".into(), "Comprehension".into()),
            add: spec.templates[1..].to_vec(),
            refresh_templates: true,
            refresh_css: observed.css != spec.css,
        });
    }
    if actual_names != expected_names {
        return Err(ManagedTemplateError::UnexpectedTemplates {
            expected: expected_names,
            actual: actual_names,
        });
    }
    let templates = observed.templates != spec.templates;
    let css = observed.css != spec.css;
    Ok(if templates || css {
        ManagedTemplatePlan::Refresh { templates, css }
    } else {
        ManagedTemplatePlan::NoChange
    })
}

fn template(name: &str, front: &str, back: &str) -> ModelTemplate {
    ModelTemplate {
        name: name.into(),
        front: front.trim().into(),
        back: back.trim().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> ObservedModel {
        let spec = japanese_vocab_spec();
        ObservedModel {
            model_name: spec.model_name,
            fields: spec.fields,
            templates: spec.templates,
            css: spec.css,
        }
    }

    #[test]
    fn current_model_needs_no_write_plan() {
        assert_eq!(
            plan_japanese_vocab_template(Some(&current())).unwrap(),
            ManagedTemplatePlan::NoChange
        );
    }

    #[test]
    fn legacy_one_card_model_has_safe_upgrade_plan() {
        let mut legacy = current();
        legacy.templates = vec![ModelTemplate {
            name: "Japanese Recognition".into(),
            front: "old front".into(),
            back: "old back".into(),
        }];
        assert!(matches!(
            plan_japanese_vocab_template(Some(&legacy)).unwrap(),
            ManagedTemplatePlan::UpgradeLegacy { rename, add, .. }
                if rename == ("Japanese Recognition".into(), "Comprehension".into()) && add.len() == 2
        ));
    }

    #[test]
    fn foreign_and_reordered_models_are_rejected_before_write() {
        let mut foreign = current();
        foreign.model_name = "Foreign".into();
        assert!(matches!(
            plan_japanese_vocab_template(Some(&foreign)),
            Err(ManagedTemplateError::ForeignModel { .. })
        ));

        let mut reordered = current();
        reordered.fields.swap(0, 1);
        assert!(matches!(
            plan_japanese_vocab_template(Some(&reordered)),
            Err(ManagedTemplateError::UnsafeFieldOrder { .. })
        ));
    }

    #[test]
    fn changed_template_produces_refresh_only_plan() {
        let mut changed = current();
        changed.templates[1].front.push_str(" changed");
        assert_eq!(
            plan_japanese_vocab_template(Some(&changed)).unwrap(),
            ManagedTemplatePlan::Refresh {
                templates: true,
                css: false
            }
        );
    }
}
