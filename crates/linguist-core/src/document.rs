use crate::canonical::ContractError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct Language(String);
impl TryFrom<String> for Language {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        language_tags::LanguageTag::parse(&value)
            .map_err(|e| e.to_string())?
            .validate()
            .map_err(|e| e.to_string())?;
        Ok(Self(value.to_ascii_lowercase()))
    }
}
impl From<Language> for String {
    fn from(v: Language) -> String {
        v.0
    }
}
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Language {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_target_supported(&self) -> bool {
        matches!(self.0.split('-').next(), Some("en" | "ja"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct AnkiId(String);
impl TryFrom<String> for AnkiId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        if s.is_empty()
            || s.starts_with('0')
            || !s.bytes().all(|b| b.is_ascii_digit())
            || s.parse::<i64>().is_err()
        {
            Err("Anki ID must be a positive canonical decimal string".into())
        } else {
            Ok(Self(s))
        }
    }
}
impl From<AnkiId> for String {
    fn from(v: AnkiId) -> String {
        v.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    tag = "intent",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FieldIntent<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}
impl<T: Clone> FieldIntent<T> {
    pub fn resolve(&self, source: Option<&T>) -> Result<Option<T>, ContractError> {
        match self {
            Self::Keep => source
                .cloned()
                .map(Some)
                .ok_or_else(|| ContractError("KEEP_WITHOUT_SOURCE".into())),
            Self::Set(v) => Ok(Some(v.clone())),
            Self::Clear => Ok(None),
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Comprehension,
    Production,
    Spelling,
    Recognition,
    Application,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Source,
    Dictionary,
    Ocr,
    Generated,
    User,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub sentence: String,
    pub translation: String,
    pub provenance: Provenance,
    #[serde(default)]
    pub evidence_ids: Vec<Uuid>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sense {
    pub key: String,
    pub definitions: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub examples: Vec<Example>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DictionaryEntry {
    pub provider: String,
    pub source_url: String,
    pub language: Language,
    pub forms: Vec<String>,
    pub readings: Vec<String>,
    pub senses: Vec<Sense>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub related_entries: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub expression: String,
    pub meaning: String,
    pub sense_key: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub pronunciation: String,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub examples: Vec<Example>,
    #[serde(default)]
    pub dictionary: Vec<DictionaryEntry>,
    #[serde(default)]
    pub kanji: String,
    #[serde(default)]
    pub production_prompt: String,
    #[serde(default)]
    pub spelling_prompt: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grammar {
    pub pattern: String,
    pub use_key: String,
    pub meaning: String,
    pub formation: String,
    pub recognition_prompt: String,
    pub examples: Vec<Example>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub exercise_prompt: String,
    #[serde(default)]
    pub exercise_answer: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LearningContent {
    Vocabulary(Vocabulary),
    Grammar(Grammar),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LearningDocument {
    #[schemars(range(min = 2, max = 2))]
    pub schema_version: u16,
    pub id: Uuid,
    pub target_language: Language,
    pub explanation_language: Language,
    pub content: LearningContent,
    pub requested_tasks: Vec<Task>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub personal_notes: String,
    #[serde(default)]
    pub source_summary: String,
    #[serde(default)]
    pub sources: Vec<crate::records::SourceRecord>,
    #[serde(default)]
    pub archives: Vec<crate::records::SourceArchive>,
    #[serde(default)]
    pub regions: Vec<crate::records::SourceRegion>,
    #[serde(default)]
    pub evidence: Vec<crate::records::Evidence>,
    #[serde(default)]
    pub media: Vec<crate::records::MediaAsset>,
    #[serde(default)]
    pub edits: BTreeMap<String, FieldIntent<String>>,
    #[serde(default)]
    pub reviews: Vec<crate::records::ReviewDecision>,
    #[serde(default)]
    pub issues: Vec<crate::validation::Issue>,
}
impl LearningDocument {
    pub fn from_json(data: &[u8]) -> Result<Self, ContractError> {
        let doc: Self = crate::canonical::parse(data)?;
        if doc.schema_version != 2 {
            return Err(ContractError("UNSUPPORTED_DOCUMENT_VERSION".into()));
        }
        Ok(doc)
    }
    pub fn semantic_digest(&self) -> Result<String, ContractError> {
        let mut value = serde_json::to_value(self)?;
        value.as_object_mut().unwrap().remove("reviews");
        value.as_object_mut().unwrap().remove("issues");
        if let Some(sources) = value.get_mut("sources").and_then(|v| v.as_array_mut()) {
            for source in sources {
                if let Some(cards) = source.get_mut("cards").and_then(|v| v.as_array_mut()) {
                    for card in cards {
                        if let Some(card) = card.as_object_mut() {
                            card.remove("scheduler");
                            card.remove("history_digest");
                        }
                    }
                }
            }
        }
        crate::canonical::digest("learning-document", &value)
    }
}
