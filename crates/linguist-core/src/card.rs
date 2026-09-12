use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The two workflows intentionally differ only in write policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardMode {
    #[default]
    Modernize,
    Inject,
}

#[derive(Clone, Copy)]
struct CardModePolicy {
    create_note: bool,
    include_context: bool,
    remove_replaced_media: bool,
}

fn mode_policy(mode: CardMode) -> CardModePolicy {
    match mode {
        CardMode::Modernize => CardModePolicy {
            create_note: false,
            include_context: false,
            remove_replaced_media: true,
        },
        CardMode::Inject => CardModePolicy {
            create_note: true,
            include_context: true,
            remove_replaced_media: false,
        },
    }
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

/// Deterministic enrichment output accepted by the native card builder.
///
/// Provider adapters populate this type; it deliberately excludes HTTP,
/// filesystem, and GUI concerns. Its serde shape also accepts the fixture
/// sources generated from Python during the staged rewrite.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CardBuildInput {
    pub mode: CardMode,
    pub language_key: String,
    #[serde(default)]
    pub processed_data: ProcessedCardData,
    #[serde(default)]
    pub provenance: BTreeMap<String, Vec<Provenance>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessedCardData {
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub scraped: DictionaryData,
    #[serde(default)]
    pub llm_response: LlmResponse,
    #[serde(default)]
    pub meaning_override: Option<String>,
    #[serde(default)]
    pub new_image_filename: String,
    #[serde(default)]
    pub new_image_b64: Option<String>,
    #[serde(default)]
    pub orig_filenames: Vec<String>,
    #[serde(default)]
    pub renamed_images: Vec<RenamedImage>,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub classification: String,
    #[serde(default)]
    pub audio_assets: Vec<AudioAsset>,
    #[serde(default)]
    pub audio_filename: String,
    #[serde(default)]
    pub audio_b64: Option<String>,
    #[serde(default)]
    pub kanji_construction: String,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub type_tag: String,
    #[serde(default)]
    pub source_note: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DictionaryData {
    #[serde(default)]
    pub found: bool,
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub definition: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    #[serde(default)]
    pub nuances: String,
    #[serde(default)]
    pub examples: Vec<ExamplePair>,
    #[serde(default)]
    pub grammar_point: Option<String>,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub rules: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExamplePair {
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub translation: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AudioAsset {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub b64: Option<String>,
    #[serde(default)]
    pub reading: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenamedImage {
    #[serde(default)]
    pub original_name: String,
    #[serde(default)]
    pub new_name: String,
    #[serde(default)]
    pub b64: Option<String>,
    #[serde(default)]
    pub classification: String,
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

/// Build a versioned document using the same deterministic policy as Python.
pub fn build_card_document(input: CardBuildInput) -> CardDocument {
    let policy = mode_policy(input.mode);
    let source = input.processed_data;
    let original_word = source.word.trim();
    let expression = if policy.create_note && !source.suggestion.trim().is_empty() {
        source.suggestion.trim()
    } else {
        original_word
    }
    .to_owned();

    let mut meaning = source.meaning_override.unwrap_or_else(|| {
        if input.language_key.ends_with("grammar") {
            format_grammar_html(
                source
                    .llm_response
                    .grammar_point
                    .as_deref()
                    .unwrap_or(&expression),
                &source.llm_response.meaning,
                &source.llm_response.rules,
                &source.llm_response.examples,
            )
        } else {
            format_dictionary_html(
                &source.scraped,
                &expression,
                &source.llm_response.nuances,
                &source.llm_response.examples,
            )
        }
    });
    if policy.include_context {
        meaning.push_str(&format_injection_context(
            &source.type_tag,
            &source.source_note,
        ));
    }

    let mut media = Vec::new();
    let mut obsolete_media = Vec::new();
    let mut images = Vec::new();
    if !source.new_image_filename.is_empty() && has_data(source.new_image_b64.as_deref()) {
        media.push(MediaAsset {
            filename: source.new_image_filename.clone(),
            data_base64: source.new_image_b64.clone().unwrap_or_default(),
        });
        images.push(image_html(&source.new_image_filename));
        if policy.remove_replaced_media {
            obsolete_media.extend(source.orig_filenames.iter().cloned());
        }
    } else if policy.remove_replaced_media {
        let has_retained_image = source
            .renamed_images
            .iter()
            .any(|image| image.classification != "dictionary");
        for image in &source.renamed_images {
            if image.classification == "dictionary" && has_retained_image {
                if !image.original_name.is_empty() {
                    obsolete_media.push(image.original_name.clone());
                }
                continue;
            }
            if !image.new_name.is_empty() && has_data(image.b64.as_deref()) {
                media.push(MediaAsset {
                    filename: image.new_name.clone(),
                    data_base64: image.b64.clone().unwrap_or_default(),
                });
                images.push(image_html(&image.new_name));
                if !image.original_name.is_empty() && image.original_name != image.new_name {
                    obsolete_media.push(image.original_name.clone());
                }
            }
        }
        if images.is_empty() {
            if !source.filename.is_empty() && source.classification != "dictionary" {
                images.push(image_html(&source.filename));
            } else if source.classification == "dictionary" {
                if has_data(source.new_image_b64.as_deref()) {
                    obsolete_media.extend(source.orig_filenames.iter().cloned());
                } else if !source.filename.is_empty() {
                    images.push(image_html(&source.filename));
                }
            }
        }
    }

    let mut audio = if policy.create_note {
        Some(String::new())
    } else {
        None
    };
    let mut audio_assets = source.audio_assets;
    if audio_assets.is_empty() && !source.audio_filename.is_empty() {
        audio_assets.push(AudioAsset {
            filename: source.audio_filename,
            b64: source.audio_b64,
            reading: source.scraped.reading.clone(),
        });
    }
    let mut audio_rows = Vec::new();
    for asset in audio_assets {
        if asset.filename.is_empty() {
            continue;
        }
        if let Some(data_base64) = asset.b64.filter(|data| !data.is_empty()) {
            media.push(MediaAsset {
                filename: asset.filename.clone(),
                data_base64,
            });
        }
        let sound = format!("[sound:{}]", asset.filename);
        if input.language_key.starts_with("japanese") && !asset.reading.trim().is_empty() {
            audio_rows.push(format!("{} {sound}", escape_html(asset.reading.trim())));
        } else {
            audio_rows.push(sound);
        }
    }
    if !audio_rows.is_empty() {
        audio = Some(audio_rows.join("<br/>"));
    }

    let mut issues = source.issues;
    if expression.is_empty() {
        issues.push("Expression is empty".into());
    }
    let mut unique_issues = Vec::new();
    for issue in issues {
        if !unique_issues.contains(&issue) {
            unique_issues.push(issue);
        }
    }

    let mut tags = Vec::new();
    if policy.create_note {
        tags.push("linguist-injected".into());
    }
    let type_tag = normalize_tag(&source.type_tag);
    if !type_tag.is_empty() {
        tags.push(format!("linguist::{type_tag}"));
    }

    CardDocument {
        schema_version: crate::CONTRACT_VERSION,
        expression,
        values: LogicalFields {
            meaning_image: Some(images.concat()),
            meaning_text: Some(meaning),
            kanji_construction: Some(source.kanji_construction),
            audio,
        },
        media,
        obsolete_media,
        issues: unique_issues,
        tags,
        provenance: input.provenance,
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

fn format_dictionary_html(
    scraped: &DictionaryData,
    expression: &str,
    nuances: &str,
    examples: &[ExamplePair],
) -> String {
    if !scraped.found {
        return "<div>Not found in standard dictionary.</div>".into();
    }
    let word = if scraped.word.is_empty() {
        expression
    } else {
        &scraped.word
    };
    let heading = [scraped.reading.as_str(), word]
        .into_iter()
        .filter(|value| !value.is_empty())
        .map(escape_html)
        .collect::<Vec<_>>()
        .join(" / ");
    let definition = escape_html(&scraped.definition).replace('\n', "<br/>");
    format!(
        "<section><div><b>{heading}</b></div><div style='margin-top:7px'>{definition}</div>{}</section>",
        format_llm_annotations(nuances, examples)
    )
}

fn format_grammar_html(
    grammar_point: &str,
    meaning: &str,
    rules: &str,
    examples: &[ExamplePair],
) -> String {
    let mut output = format!(
        "<div><b>Grammar Point:</b> <span style='font-size:1.2em;color:#e68e0d'>{}</span></div>",
        escape_html(grammar_point)
    );
    output.push_str(&format!(
        "<div style='margin-top:5px'><b>Meaning:</b> {}</div>",
        escape_html(meaning)
    ));
    if !rules.is_empty() {
        output.push_str(&format!(
            "<div style='margin-top:5px'><b>Structure/Rules:</b> <pre>{}</pre></div>",
            escape_html(rules)
        ));
    }
    if !examples.is_empty() {
        output.push_str(&format_llm_annotations("", examples));
    }
    output
}

fn format_llm_annotations(nuances: &str, examples: &[ExamplePair]) -> String {
    let mut output = String::new();
    if !nuances.is_empty() {
        output.push_str(&format!(
            "<div data-source='llm' style='margin-top:6px;font-style:italic;color:#888'><b>Nuance:</b> {}</div>",
            escape_html(nuances)
        ));
    }
    let valid_examples = examples
        .iter()
        .map(|example| {
            (
                sanitize_generated_text(&example.sentence),
                sanitize_generated_text(&example.translation),
            )
        })
        .filter(|(sentence, translation)| !sentence.is_empty() || !translation.is_empty())
        .collect::<Vec<_>>();
    if valid_examples.is_empty() {
        return output;
    }
    output.push_str("<div data-source='llm' style='margin-top:10px'><b>Examples:</b><div style='margin-top:5px'>");
    for (index, (sentence, translation)) in valid_examples.iter().enumerate() {
        let separator = if !sentence.is_empty() && !translation.is_empty() {
            " — "
        } else {
            ""
        };
        output.push_str(&format!(
            "<div style='margin-bottom:4px'><b>{}. {}</b>{separator}<span style='color:#666;font-size:.9em'>{}</span></div>",
            index + 1,
            escape_html(sentence),
            escape_html(translation)
        ));
    }
    output.push_str("</div></div>");
    output
}

fn format_injection_context(type_tag: &str, source_note: &str) -> String {
    let mut rows = String::new();
    if !type_tag.is_empty() {
        rows.push_str(&format!(
            "<div><b>Learning focus:</b> {}</div>",
            escape_html(type_tag)
        ));
    }
    if !source_note.is_empty() {
        rows.push_str(&format!(
            "<div><b>Personal context:</b> {}</div>",
            escape_html(source_note)
        ));
    }
    if rows.is_empty() {
        String::new()
    } else {
        format!(
            "<aside data-source='user' style='margin-top:12px;border-top:1px dashed #888;padding-top:8px'>{rows}</aside>"
        )
    }
}

fn image_html(filename: &str) -> String {
    format!("<img src='{}'/>", escape_html(filename))
}

fn has_data(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn sanitize_generated_text(value: &str) -> String {
    value
        .replace('\u{3000}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    ':' | '：'
                        | ';'
                        | '；'
                        | '|'
                        | '/'
                        | '\\'
                        | '>'
                        | '*'
                        | '#'
                        | '•'
                        | '·'
                        | '-'
                        | '–'
                        | '—'
                        | '"'
                        | '\''
                )
        })
        .to_owned()
}

fn normalize_tag(value: &str) -> String {
    let mut output = String::new();
    let mut previous_was_separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            output.push(character);
            previous_was_separator = false;
        } else if !previous_was_separator {
            output.push('_');
            previous_was_separator = true;
        }
    }
    output.trim_matches('_').chars().take(80).collect()
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

    #[test]
    fn generated_python_fixtures_deserialize_as_contract_v1() {
        let fixtures = [
            (
                "modernization",
                include_str!("../../../contracts/fixtures/modernization-card.v1.json"),
                true,
            ),
            (
                "injection",
                include_str!("../../../contracts/fixtures/injection-card.v1.json"),
                true,
            ),
            (
                "shared fields",
                include_str!("../../../contracts/fixtures/shared-fields-card.v1.json"),
                true,
            ),
            (
                "media replacement",
                include_str!("../../../contracts/fixtures/media-replacement-card.v1.json"),
                false,
            ),
            (
                "validation issues",
                include_str!("../../../contracts/fixtures/validation-issues-card.v1.json"),
                false,
            ),
            (
                "grammar",
                include_str!("../../../contracts/fixtures/grammar-card.v1.json"),
                true,
            ),
            (
                "dictionary preserve",
                include_str!("../../../contracts/fixtures/dictionary-preserve-card.v1.json"),
                true,
            ),
        ];

        for (name, fixture, ready) in fixtures {
            let card: CardDocument = serde_json::from_str(fixture)
                .unwrap_or_else(|error| panic!("{name} fixture failed to deserialize: {error}"));
            assert_eq!(card.schema_version, crate::CONTRACT_VERSION, "{name}");
            assert_eq!(card.ready(), ready, "{name}");
        }
    }

    #[test]
    fn rust_builder_matches_every_python_golden_fixture() {
        let fixtures = [
            (
                "modernization",
                include_str!("../../../contracts/fixture-sources/modernization.json"),
                include_str!("../../../contracts/fixtures/modernization-card.v1.json"),
            ),
            (
                "injection",
                include_str!("../../../contracts/fixture-sources/injection.json"),
                include_str!("../../../contracts/fixtures/injection-card.v1.json"),
            ),
            (
                "shared fields",
                include_str!("../../../contracts/fixture-sources/shared-fields.json"),
                include_str!("../../../contracts/fixtures/shared-fields-card.v1.json"),
            ),
            (
                "media replacement",
                include_str!("../../../contracts/fixture-sources/media-replacement.json"),
                include_str!("../../../contracts/fixtures/media-replacement-card.v1.json"),
            ),
            (
                "validation issues",
                include_str!("../../../contracts/fixture-sources/validation-issues.json"),
                include_str!("../../../contracts/fixtures/validation-issues-card.v1.json"),
            ),
            (
                "grammar",
                include_str!("../../../contracts/fixture-sources/grammar.json"),
                include_str!("../../../contracts/fixtures/grammar-card.v1.json"),
            ),
            (
                "dictionary preserve",
                include_str!("../../../contracts/fixture-sources/dictionary-preserve.json"),
                include_str!("../../../contracts/fixtures/dictionary-preserve-card.v1.json"),
            ),
        ];

        for (name, source, expected) in fixtures {
            let source: FixtureSource = serde_json::from_str(source)
                .unwrap_or_else(|error| panic!("{name} source failed to deserialize: {error}"));
            let expected: CardDocument = serde_json::from_str(expected)
                .unwrap_or_else(|error| panic!("{name} fixture failed to deserialize: {error}"));
            let actual = build_card_document(source.input);
            assert_eq!(actual, expected, "{name}");
            if let (Some(mapping), Some(mapped_fields)) =
                (source.field_mapping, source.expected_mapped_fields)
            {
                assert_eq!(
                    actual.map_fields(&mapping).unwrap(),
                    mapped_fields,
                    "{name}"
                );
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct FixtureSource {
        #[serde(flatten)]
        input: CardBuildInput,
        field_mapping: Option<FieldMapping>,
        expected_mapped_fields: Option<BTreeMap<String, String>>,
    }
}
