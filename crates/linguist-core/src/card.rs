use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The two workflows intentionally differ only in write policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardMode {
    Modernize,
    Inject,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaAsset {
    pub filename: String,
    pub data_base64: String,
}

/// Purpose-based values before they are mapped to a concrete Anki note type.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalFields {
    pub meaning_image: Option<String>,
    pub meaning_text: Option<String>,
    pub kanji_construction: Option<String>,
    pub audio: Option<String>,
}

/// A source marker survives generation so the GUI can explain every value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Original,
    Dictionary,
    Ocr,
    Generated,
    User,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: SourceKind,
    pub label: String,
    pub confidence_percent: Option<u8>,
}

/// Canonical boundary between enrichment, review, and Anki writes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CardDocument {
    pub schema_version: u16,
    pub expression: String,
    pub values: LogicalFields,
    #[serde(default)]
    pub media: Vec<MediaAsset>,
    #[serde(default)]
    pub obsolete_media: Vec<String>,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub provenance: BTreeMap<String, Vec<Provenance>>,
}

impl CardDocument {
    pub fn ready(&self) -> bool {
        self.schema_version == crate::CONTRACT_VERSION
            && !self.expression.trim().is_empty()
            && self.issues.is_empty()
    }

    /// Map logical purposes onto a note schema. Values sharing one physical
    /// field retain the Python implementation's deterministic `<br/>` join.
    pub fn map_fields(
        &self,
        mapping: &FieldMapping,
    ) -> Result<BTreeMap<String, String>, MappingError> {
        let expression_field = required(&mapping.expression, "expression")?;
        let meaning_field = required(&mapping.meaning_text, "meaning_text")?;

        let mut result = BTreeMap::new();
        append(&mut result, Some(expression_field), Some(&self.expression));
        append(
            &mut result,
            mapping.meaning_image.as_deref(),
            self.values.meaning_image.as_deref(),
        );
        append(
            &mut result,
            Some(meaning_field),
            self.values.meaning_text.as_deref(),
        );
        append(
            &mut result,
            mapping.kanji_construction.as_deref(),
            self.values.kanji_construction.as_deref(),
        );
        append(
            &mut result,
            mapping.audio.as_deref(),
            self.values.audio.as_deref(),
        );
        Ok(result)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FieldMapping {
    pub expression: Option<String>,
    pub meaning_image: Option<String>,
    pub meaning_text: Option<String>,
    pub kanji_construction: Option<String>,
    pub audio: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappingError {
    pub missing_purpose: &'static str,
}

impl std::fmt::Display for MappingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "missing required {} field mapping",
            self.missing_purpose
        )
    }
}

impl std::error::Error for MappingError {}

fn required<'a>(value: &'a Option<String>, purpose: &'static str) -> Result<&'a str, MappingError> {
    value
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(MappingError {
            missing_purpose: purpose,
        })
}

fn append(result: &mut BTreeMap<String, String>, field: Option<&str>, value: Option<&str>) {
    let (Some(field), Some(value)) = (field.filter(|value| !value.is_empty()), value) else {
        return;
    };
    result
        .entry(field.to_owned())
        .and_modify(|current| {
            if !current.is_empty() && !value.is_empty() {
                current.push_str("<br/>");
            }
            if !value.is_empty() {
                current.push_str(value);
            }
        })
        .or_insert_with(|| value.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> CardDocument {
        CardDocument {
            schema_version: crate::CONTRACT_VERSION,
            expression: "食べる".into(),
            values: LogicalFields {
                meaning_text: Some("to eat".into()),
                kanji_construction: Some("食 · eat".into()),
                ..LogicalFields::default()
            },
            media: vec![],
            obsolete_media: vec![],
            issues: vec![],
            tags: vec![],
            provenance: BTreeMap::new(),
        }
    }

    #[test]
    fn readiness_requires_current_schema_expression_and_no_issues() {
        let mut card = document();
        assert!(card.ready());
        card.issues.push("OCR needs review".into());
        assert!(!card.ready());
    }

    #[test]
    fn shared_fields_keep_the_existing_join_semantics() {
        let fields = document()
            .map_fields(&FieldMapping {
                expression: Some("Expression".into()),
                meaning_text: Some("Meaning".into()),
                kanji_construction: Some("Meaning".into()),
                ..FieldMapping::default()
            })
            .unwrap();
        assert_eq!(fields["Expression"], "食べる");
        assert_eq!(fields["Meaning"], "to eat<br/>食 · eat");
    }

    #[test]
    fn fixture_is_a_versioned_card_contract() {
        let fixture = include_str!("../../../contracts/fixtures/card-document.v1.json");
        let card: CardDocument = serde_json::from_str(fixture).unwrap();
        assert_eq!(card.schema_version, crate::CONTRACT_VERSION);
        assert_eq!(card.expression, "食べる");
        assert!(card.ready());
    }
}
