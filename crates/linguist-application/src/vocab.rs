//! ALG-VOCAB step 5–6: optional enrichment after OCR and dictionary lookup.
//!
//! Kanji facts fill an empty Kanji field with dictionary provenance. Image and
//! speech results are staged as archive-only candidates that a reviewer must
//! select before they render. A provider failure for this optional material is
//! a warning; configuration that selects an unavailable adapter is an error.
use crate::images::{ImageSearch, ImageSearchClient, ImageSearchError};
use crate::speech::{SpeechError, Synthesis};
use linguist_config::Effective;
use linguist_core::{
    Language, LearningContent, LearningDocument, Provenance, canonical,
    records::{
        Evidence, EvidenceTarget, MediaAsset, MediaOwner, MediaRole, PlanRevision, SourceArchive,
        SourceRecord,
    },
    validation::{self, Issue, Severity},
};
use linguist_dictionary::kanji::{KanjiClient, KanjiEntry};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub trait KanjiPort {
    fn lookup(&self, character: char) -> Result<Option<KanjiEntry>, String>;
    /// Stroke-order GIF bytes; `Ok(None)` when disabled or not published.
    fn stroke_order(&self, _character: char) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
}
pub trait ImagePort {
    fn search(&self, expression: &str) -> Result<ImageSearch, String>;
}
/// Native-speaker recordings (`audio.provider=dictionary`); `Ok(None)` is "none published".
pub trait RecordingPort {
    fn japanese(
        &self,
        expression: &str,
        kana: &str,
    ) -> Result<Option<linguist_dictionary::recording::Recording>, String>;
}
impl RecordingPort for linguist_dictionary::recording::RecordingClient {
    fn japanese(
        &self,
        expression: &str,
        kana: &str,
    ) -> Result<Option<linguist_dictionary::recording::Recording>, String> {
        linguist_dictionary::recording::RecordingClient::japanese(self, expression, kana)
            .map_err(|e| e.to_string())
    }
}
pub trait SpeechPort {
    fn synthesize(&self, text: &str, target: &Language) -> Result<Synthesis, String>;
}

impl KanjiPort for KanjiClient {
    fn lookup(&self, character: char) -> Result<Option<KanjiEntry>, String> {
        KanjiClient::lookup(self, character).map_err(|e| e.to_string())
    }
    fn stroke_order(&self, character: char) -> Result<Option<Vec<u8>>, String> {
        KanjiClient::stroke_order(self, character).map_err(|e| e.to_string())
    }
}

/// Injected provider ports; `None` uses the configured live adapter.
#[derive(Default, Clone, Copy)]
pub struct Providers<'a> {
    pub dictionary: Option<&'a dyn crate::DictionaryPort>,
    pub kanji: Option<&'a dyn KanjiPort>,
    pub images: Option<&'a dyn ImagePort>,
    pub speech: Option<&'a dyn SpeechPort>,
    pub recordings: Option<&'a dyn RecordingPort>,
}

const SETTINGS: [&str; 5] = [
    "kanji.enabled",
    "images.search_when_missing",
    "images.provider",
    "audio.provider",
    "storage.cache_dir",
];

fn kanji_requested(settings: &Effective, document: &LearningDocument) -> bool {
    matches!(&document.content, LearningContent::Vocabulary(v)
        if document.target_language.as_str().split('-').next() == Some("ja")
            && settings.values["kanji.enabled"] == true
            && !linguist_dictionary::kanji::characters(&v.expression).is_empty()
            && v.kanji_details.is_empty()
            && !document.edits.contains_key("Kanji"))
}
fn images_requested(settings: &Effective, document: &LearningDocument) -> bool {
    matches!(document.content, LearningContent::Vocabulary(_))
        && settings.values["images.search_when_missing"] == true
        && settings.values["images.provider"] != "disabled"
        // Kanji stroke animations are not pictures.
        && !document
            .media
            .iter()
            .any(|m| m.mime.starts_with("image/") && m.role != MediaRole::KanjiStroke)
}
fn audio_requested(settings: &Effective, document: &LearningDocument) -> bool {
    matches!(document.content, LearningContent::Vocabulary(_))
        && matches!(
            settings.values["audio.provider"].as_str(),
            Some("piper" | "dictionary")
        )
        && !document.media.iter().any(|m| m.mime.starts_with("audio/"))
}

/// Reject selected adapters this build cannot run, before any state exists.
pub fn preflight(settings: &Effective) -> Result<(), String> {
    let registry = linguist_config::Registry::builtin();
    for key in SETTINGS {
        registry.validate_value(
            key,
            settings.values.get(key).ok_or("VOCAB_SETTING_MISSING")?,
        )?;
    }
    if settings.values["images.search_when_missing"] == true
        && !matches!(
            settings.values["images.provider"].as_str(),
            Some("wikimedia" | "disabled")
        )
    {
        return Err("CAPABILITY_UNAVAILABLE: images.provider=custom has no adapter in this build; select wikimedia or disabled".into());
    }
    if !matches!(
        settings.values["audio.provider"].as_str(),
        Some("preserve" | "disabled" | "piper" | "dictionary")
    ) {
        return Err("CAPABILITY_UNAVAILABLE: audio.provider=custom has no adapter in this build; select dictionary, piper, preserve or disabled".into());
    }
    if settings.values["kanji.enabled"] == true
        && settings.values.get("kanji.schema") != Some(&json!(linguist_dictionary::kanji::SCHEMA))
    {
        return Err(
            "CAPABILITY_UNAVAILABLE: only kanji.schema=builtin:jisho-kanji-v2 is available".into(),
        );
    }
    Ok(())
}

fn warning(document: &mut LearningDocument, code: &str, field: &str, message: String) {
    let mut issue = Issue::new(code, Severity::Warning, Some(field), message);
    issue.stage = "enrichment".into();
    document.issues.push(issue);
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        _ => "wav",
    }
}

/// Enrich one document. Returned bytes must be archived before the revision.
pub fn enrich_document(
    document: &LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    providers: Providers<'_>,
) -> Result<(LearningDocument, Vec<Vec<u8>>), String> {
    preflight(settings)?;
    let mut document = document.clone();
    let mut assets = Vec::new();
    // Keep stored stage observations; validation-stage issues are recomputed.
    document.issues.retain(|i| i.stage != "validation");
    if kanji_requested(settings, &document) {
        enrich_kanji(
            &mut document,
            settings,
            environment,
            providers.kanji,
            &mut assets,
        )?;
    }
    if images_requested(settings, &document) {
        stage_images(
            &mut document,
            settings,
            environment,
            providers.images,
            &mut assets,
        )?;
    }
    if audio_requested(settings, &document) && settings.values["audio.provider"] == "dictionary" {
        stage_recording(
            &mut document,
            settings,
            environment,
            providers.recordings,
            &mut assets,
        )?;
    } else if audio_requested(settings, &document) {
        stage_audio(
            &mut document,
            settings,
            environment,
            providers.speech,
            &mut assets,
        )?;
    }
    document.issues = validation::validate(&document);
    Ok((document, assets))
}

fn enrich_kanji(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn KanjiPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    let characters = linguist_dictionary::kanji::characters(&vocab.expression);
    if characters.len() > linguist_dictionary::kanji::MAX_CHARACTERS {
        warning(
            document,
            "KANJI_ENRICHMENT_FAILED",
            "kanji",
            "The expression has more kanji than one lookup may read.".into(),
        );
        return Ok(());
    }
    let live;
    let port: &dyn KanjiPort = match port {
        Some(port) => port,
        None => {
            live = match KanjiClient::from_settings(settings, environment) {
                Ok(client) => client,
                Err(linguist_provider::ReadError::Unavailable) => {
                    return Err(
                        "CAPABILITY_UNAVAILABLE: the selected kanji settings have no adapter"
                            .into(),
                    );
                }
                Err(error) => {
                    warning(
                        document,
                        "KANJI_ENRICHMENT_FAILED",
                        "kanji",
                        format!("{error}; the Kanji field stays empty."),
                    );
                    return Ok(());
                }
            };
            &live
        }
    };
    let mut found = Vec::new();
    for character in characters {
        match port.lookup(character) {
            Ok(Some(entry)) => found.push(entry),
            Ok(None) => warning(
                document,
                "KANJI_NOT_FOUND",
                "kanji",
                format!("No kanji details were found for {character}."),
            ),
            Err(error) => {
                // All-or-nothing: partial kanji data could misrepresent the word.
                warning(
                    document,
                    "KANJI_ENRICHMENT_FAILED",
                    "kanji",
                    format!("{error}; the Kanji field stays empty."),
                );
                return Ok(());
            }
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    let source_id = uuid::Uuid::new_v4();
    let mut fields = BTreeMap::new();
    let mut digests = Vec::new();
    for entry in &found {
        let page = String::from_utf8(entry.raw_bytes.clone()).map_err(|_| "KANJI_PAGE_ENCODING")?;
        fields.insert(entry.character.clone(), page);
        digests.push(canonical::asset_digest(&entry.raw_bytes));
        assets.push(entry.raw_bytes.clone());
    }
    let manifest = canonical::digest("kanji-pages", &digests).map_err(|e| e.to_string())?;
    document.sources.push(SourceRecord {
        id: source_id,
        kind: "jisho_kanji_pages_v2".into(),
        location: found
            .iter()
            .map(|e| e.source_url.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        digest: manifest.clone(),
        text: None,
        fields: fields.clone(),
        model_manifest: linguist_dictionary::kanji::SCHEMA.into(),
        template_manifest: None,
        captured_at_unix_seconds: found.iter().map(|e| e.fetched_at).max(),
        tags: vec![],
        cards: vec![],
        media_refs: vec![],
    });
    document.archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id,
        digest: manifest,
        original_text: None,
        original_fields: fields,
        asset_digests: digests,
    });
    let explanation = document.explanation_language.clone();
    for entry in &found {
        document.evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field: "kanji".into(),
            provenance: Provenance::Dictionary,
            source_id: Some(source_id),
            region_id: None,
            target: None,
            source_span: None,
            language: explanation.clone(),
            claim: serde_json::to_string(entry).map_err(|e| e.to_string())?,
            source_url: Some(entry.source_url.clone()),
            ambiguous: false,
        });
    }
    // v3: structured facts plus an optional stroke-order animation per
    // character. A missing animation only drops the picture, never the facts.
    let mut details = Vec::new();
    for entry in &found {
        let character = entry
            .character
            .chars()
            .next()
            .ok_or("KANJI_CHARACTER_EMPTY")?;
        let stroke_digest = match port.stroke_order(character) {
            Ok(Some(bytes)) if linguist_dictionary::kanji::is_gif(&bytes) => {
                let digest = canonical::asset_digest(&bytes);
                if !document.media.iter().any(|m| m.digest == digest) {
                    document.media.push(MediaAsset {
                        digest: digest.clone(),
                        filename: format!("lab_{digest}.gif"),
                        original_filename: Some(format!("{:x}.gif", character as u32)),
                        size_bytes: bytes.len() as u64,
                        mime: "image/gif".into(),
                        owner: MediaOwner::App,
                        role: MediaRole::KanjiStroke,
                        source_id: None,
                        attribution: linguist_dictionary::kanji::STROKE_ORDER_ATTRIBUTION.into(),
                        license: Some(linguist_dictionary::kanji::STROKE_ORDER_LICENSE.into()),
                    });
                    assets.push(bytes);
                }
                Some(digest)
            }
            Ok(Some(_)) => {
                warning(
                    document,
                    "KANJI_STROKE_ORDER_FAILED",
                    "kanji",
                    format!("The stroke-order image for {character} is not a GIF."),
                );
                None
            }
            Ok(None) => None,
            Err(error) => {
                warning(
                    document,
                    "KANJI_STROKE_ORDER_FAILED",
                    "kanji",
                    format!("{error}; {character} is shown without its stroke order."),
                );
                None
            }
        };
        details.push(linguist_core::document::KanjiDetail {
            character: entry.character.clone(),
            meanings: entry.meanings.clone(),
            on_readings: entry.on_readings.clone(),
            kun_readings: entry.kun_readings.clone(),
            strokes: entry.strokes,
            radical: entry.radical.clone(),
            parts: entry.parts.clone(),
            jlpt: entry.jlpt.clone(),
            stroke_digest,
        });
    }
    if let LearningContent::Vocabulary(vocab) = &mut document.content {
        vocab.kanji_details = details;
    }
    Ok(())
}

fn stage_images(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn ImagePort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    let expression = vocab.expression.clone();
    let result = match port {
        Some(port) => port.search(&expression),
        None => match ImageSearchClient::from_settings(settings, environment) {
            Ok(client) => client.search(&expression).map_err(|e| e.to_string()),
            Err(ImageSearchError::ImageProviderUnavailable { provider }) => {
                return Err(format!(
                    "CAPABILITY_UNAVAILABLE: images.provider={provider} has no adapter"
                ));
            }
            Err(error) => Err(error.to_string()),
        },
    };
    let search = match result {
        Ok(search) => search,
        Err(error) => {
            warning(
                document,
                "IMAGE_SEARCH_FAILED",
                "picture",
                format!("{error}; no picture candidate was staged. The picture is optional."),
            );
            return Ok(());
        }
    };
    assets.push(search.response.clone());
    let response_digest = canonical::asset_digest(&search.response);
    let mut digests = Vec::new();
    for candidate in search.candidates {
        let digest = canonical::asset_digest(&candidate.bytes);
        if digests.contains(&digest) {
            continue;
        }
        document.media.push(MediaAsset {
            digest: digest.clone(),
            filename: format!("candidate_{digest}.{}", extension(&candidate.mime)),
            original_filename: Some(candidate.title.clone()),
            size_bytes: candidate.size_bytes,
            mime: candidate.mime.clone(),
            owner: MediaOwner::External,
            role: MediaRole::Archive,
            source_id: None,
            attribution: format!(
                "{} — {} ({})",
                candidate.title,
                candidate
                    .artist
                    .clone()
                    .unwrap_or_else(|| "unknown author".into()),
                candidate.page_url
            ),
            license: candidate.license.clone(),
        });
        document.evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field: "picture".into(),
            provenance: Provenance::Provider,
            source_id: None,
            region_id: None,
            target: Some(EvidenceTarget::MediaAsset { digest: digest.clone() }),
            source_span: None,
            language: document.explanation_language.clone(),
            claim: serde_json::to_string(&json!({
                "candidate": candidate,
                "inspection": {"mime": candidate.mime, "width": candidate.width, "height": candidate.height},
                "query": search.query,
                "request_url": search.request_url,
                "response_digest": response_digest,
                "scope": "search candidate; rights and relevance unreviewed",
            }))
            .map_err(|e| e.to_string())?,
            source_url: Some(candidate.page_url.clone()),
            ambiguous: true,
        });
        assets.push(candidate.bytes);
        digests.push(digest);
    }
    for rejected in &search.rejected {
        warning(
            document,
            "IMAGE_CANDIDATE_REJECTED",
            "picture",
            format!("{} was not staged: {}", rejected.title, rejected.code),
        );
    }
    if digests.is_empty() {
        warning(
            document,
            "IMAGE_CANDIDATES_NOT_FOUND",
            "picture",
            "Image search returned no usable candidate. The picture is optional.".into(),
        );
        return Ok(());
    }
    let mut issue = Issue::new(
        "IMAGE_CANDIDATE_REVIEW",
        Severity::Review,
        Some("picture"),
        "Choose one picture candidate after checking relevance, license and attribution, or decline all.",
    );
    issue.id = format!("IMAGE_CANDIDATE_REVIEW:{}", document.id);
    issue.stage = "enrichment".into();
    issue.source_refs = digests;
    document.issues.push(issue);
    Ok(())
}

fn stage_audio(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn SpeechPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    let japanese = document.target_language.as_str().split('-').next() == Some("ja");
    // Speak the kana when known: Piper may misread kanji.
    let spoken: String = vocab.pronunciation.split_whitespace().collect();
    let text = if japanese && !vocab.reading.trim().is_empty() {
        vocab.reading.clone()
    } else if japanese && !spoken.is_empty() {
        spoken
    } else {
        vocab.expression.clone()
    };
    let target = document.target_language.clone();
    let result = match port {
        Some(port) => port.synthesize(&text, &target),
        None => match crate::speech::synthesize(&text, &target, settings, environment) {
            Ok(synthesis) => Ok(synthesis),
            // Configuration and resource gaps are not optional-media outages.
            Err(
                error @ (SpeechError::InvalidSettings { .. }
                | SpeechError::SpeechProviderUnavailable { .. }
                | SpeechError::SpeechExecutableUnavailable { .. }
                | SpeechError::SpeechVoiceUnavailable { .. }
                | SpeechError::SpeechVoiceConfigInvalid
                | SpeechError::SpeechVoiceLanguageMismatch { .. }
                | SpeechError::SpeechSpeakerInvalid),
            ) => {
                return Err(format!(
                    "{error}: correct the audio.* settings or select audio.provider=preserve"
                ));
            }
            Err(error) => Err(error.to_string()),
        },
    };
    let synthesis = match result {
        Ok(synthesis) => synthesis,
        Err(error) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                format!("{error}; no audio candidate was staged. Audio is optional."),
            );
            return Ok(());
        }
    };
    let digest = canonical::asset_digest(&synthesis.bytes);
    document.media.push(MediaAsset {
        digest: digest.clone(),
        filename: format!("candidate_{digest}.{}", extension(&synthesis.mime)),
        original_filename: None,
        size_bytes: synthesis.size_bytes,
        mime: synthesis.mime.clone(),
        owner: MediaOwner::App,
        role: MediaRole::Archive,
        source_id: None,
        attribution: format!(
            "Synthesized locally with Piper ({}, voice {})",
            synthesis.engine_version,
            synthesis
                .voice_dataset
                .clone()
                .unwrap_or_else(|| synthesis.voice_language.clone())
        ),
        license: None,
    });
    document.evidence.push(Evidence {
        id: uuid::Uuid::new_v4(),
        field: "audio".into(),
        provenance: Provenance::Provider,
        source_id: None,
        region_id: None,
        target: Some(EvidenceTarget::MediaAsset {
            digest: digest.clone(),
        }),
        source_span: None,
        language: document.target_language.clone(),
        claim: serde_json::to_string(&json!({
            "synthesis": synthesis,
            "text": text,
            "inspection": {"mime": synthesis.mime, "sample_rate": synthesis.sample_rate},
            "scope": "synthesized pronunciation candidate; correctness unreviewed",
        }))
        .map_err(|e| e.to_string())?,
        source_url: None,
        ambiguous: true,
    });
    assets.push(synthesis.bytes);
    let mut issue = Issue::new(
        "AUDIO_CANDIDATE_REVIEW",
        Severity::Review,
        Some("audio"),
        "Listen to the synthesized pronunciation; select it or decline it.",
    );
    issue.id = format!("AUDIO_CANDIDATE_REVIEW:{}", document.id);
    issue.stage = "enrichment".into();
    issue.source_refs = vec![digest];
    document.issues.push(issue);
    Ok(())
}

fn kana_only(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            matches!(c as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F) || c == 'ー'
        })
}

/// Stage one native-speaker recording as an audio candidate that needs review.
fn stage_recording(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn RecordingPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    if document.target_language.as_str().split('-').next() != Some("ja") {
        warning(
            document,
            "AUDIO_NOT_FOUND",
            "audio",
            "Dictionary recordings are only available for Japanese.".into(),
        );
        return Ok(());
    }
    let expression = vocab.expression.trim().to_owned();
    let spoken: String = vocab.pronunciation.split_whitespace().collect();
    // Before the sense decision, exact dictionary entries that all agree on
    // one reading identify the spoken form; several readings stay ambiguous.
    let exact: BTreeSet<&str> = vocab
        .dictionary
        .iter()
        .filter(|entry| entry.forms.contains(&expression))
        .flat_map(|entry| entry.readings.iter().map(String::as_str))
        .filter(|reading| kana_only(reading))
        .collect();
    let agreed = (exact.len() == 1).then(|| exact.iter().next().unwrap().to_string());
    let kana = [vocab.reading.trim(), spoken.as_str(), expression.as_str()]
        .into_iter()
        .find(|text| kana_only(text))
        .map(str::to_owned)
        .or(agreed);
    let Some(kana) = kana else {
        warning(
            document,
            "AUDIO_NOT_FOUND",
            "audio",
            "A single kana reading is needed to look up a dictionary recording; choose the sense, then run plans regenerate --stage enrichment.".into(),
        );
        return Ok(());
    };
    let live;
    let port: &dyn RecordingPort = match port {
        Some(port) => port,
        None => {
            live = linguist_dictionary::recording::RecordingClient::from_settings(
                settings,
                environment,
            )
            .map_err(|e| format!("CAPABILITY_UNAVAILABLE: audio.provider=dictionary: {e}"))?;
            &live
        }
    };
    let recording = match port.japanese(&expression, &kana) {
        Ok(Some(recording)) => recording,
        Ok(None) => {
            warning(
                document,
                "AUDIO_NOT_FOUND",
                "audio",
                format!("The dictionary has no recording for {expression} ({kana})."),
            );
            return Ok(());
        }
        Err(error) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                format!("{error}; no audio candidate was staged. Audio is optional."),
            );
            return Ok(());
        }
    };
    // Decoded inspection is what lets a reviewer select the candidate.
    let inspection = match crate::media::inspect_source_media(&recording.bytes, settings) {
        Ok(crate::media::SourceMediaInspection::Audio(inspection)) => inspection,
        Ok(_) | Err(_) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                "The dictionary recording did not decode as audio; nothing was staged.".into(),
            );
            return Ok(());
        }
    };
    let digest = canonical::asset_digest(&recording.bytes);
    document.media.push(MediaAsset {
        digest: digest.clone(),
        filename: format!("candidate_{digest}.mp3"),
        original_filename: None,
        size_bytes: recording.bytes.len() as u64,
        mime: "audio/mpeg".into(),
        owner: MediaOwner::App,
        role: MediaRole::Archive,
        source_id: None,
        attribution: linguist_dictionary::recording::ATTRIBUTION.into(),
        license: None,
    });
    document.evidence.push(Evidence {
        id: uuid::Uuid::new_v4(),
        field: "audio".into(),
        provenance: Provenance::Provider,
        source_id: None,
        region_id: None,
        target: Some(EvidenceTarget::MediaAsset {
            digest: digest.clone(),
        }),
        source_span: None,
        language: document.target_language.clone(),
        claim: serde_json::to_string(&json!({
            "recording": recording,
            "text": {"expression": expression, "kana": kana},
            "inspection": inspection,
            "scope": "native speaker dictionary recording; word match unreviewed",
        }))
        .map_err(|e| e.to_string())?,
        source_url: Some(recording.source_url.clone()),
        ambiguous: true,
    });
    assets.push(recording.bytes);
    let mut issue = Issue::new(
        "AUDIO_CANDIDATE_REVIEW",
        Severity::Review,
        Some("audio"),
        "Listen to the dictionary recording; select it or decline it.",
    );
    issue.id = format!("AUDIO_CANDIDATE_REVIEW:{}", document.id);
    issue.stage = "enrichment".into();
    issue.source_refs = vec![digest];
    document.issues.push(issue);
    Ok(())
}

/// True when any document still needs optional vocabulary enrichment.
pub fn requested(settings: &Effective, plan: &PlanRevision) -> bool {
    plan.documents.iter().any(|d| {
        kanji_requested(settings, d)
            || images_requested(settings, d)
            || audio_requested(settings, d)
    }) && !plan
        .documents
        .iter()
        .any(|d| d.issues.iter().any(|i| i.stage == "enrichment"))
}

/// Publish an enrichment child of the latest revision using its frozen settings.
pub fn enrich_revision(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    environment: &BTreeMap<String, String>,
    providers: Providers<'_>,
) -> Result<PlanRevision, String> {
    if store.latest_revision(base.id)? != base.revision
        || store.revision(base.id, base.revision)? != *base
    {
        return Err("ENRICHMENT_BASE_CONFLICT".into());
    }
    let settings = Effective {
        version: base.settings.version,
        values: base.settings.values.clone(),
        provenance: base.settings.provenance.clone(),
        fingerprint: base.settings.fingerprint.clone(),
        semantic_fingerprint: base.settings.semantic_fingerprint.clone(),
        execution_fingerprint: base.settings.execution_fingerprint.clone(),
    };
    let cap = settings.values["network.max_response_mb"]
        .as_u64()
        .unwrap_or(20)
        * 1024
        * 1024;
    let mut child = base.clone();
    let mut assets = Vec::new();
    for document in &mut child.documents {
        let (enriched, bytes) = enrich_document(document, &settings, environment, providers)?;
        *document = enriched;
        assets.extend(bytes);
    }
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("ENRICHMENT_REVISION_LIMIT")?;
    child.parent_digest = Some(base.approval_digest().map_err(|e| e.to_string())?);
    child.binding = None;
    child.rendered.clear();
    let sources: Vec<_> = child.documents.iter().flat_map(|d| &d.sources).collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    for bytes in assets {
        store.publish_asset(&bytes, cap)?;
    }
    store.publish_revision(&child)?;
    Ok(child)
}
