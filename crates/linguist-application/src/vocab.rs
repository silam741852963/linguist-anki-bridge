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
impl ImagePort for crate::illustrations::IllustrationClient {
    fn search(&self, expression: &str) -> Result<ImageSearch, String> {
        self.search(expression).map_err(|e| e.to_string())
    }
}
/// Native-speaker recordings (`audio.provider=dictionary`); `Ok(None)` is "none published".
pub trait RecordingPort {
    fn japanese(
        &self,
        expression: &str,
        kana: &str,
    ) -> Result<Option<linguist_dictionary::recording::Recording>, String>;
    /// Wiktionary IPA and pronunciation recording for an English word.
    fn english(
        &self,
        _word: &str,
    ) -> Result<Option<linguist_dictionary::recording::EnglishLookup>, String> {
        Ok(None)
    }
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
    fn english(
        &self,
        word: &str,
    ) -> Result<Option<linguist_dictionary::recording::EnglishLookup>, String> {
        linguist_dictionary::recording::RecordingClient::english(self, word)
            .map_err(|e| e.to_string())
    }
}
/// Media a dictionary page names (Cambridge illustrations and US audio).
pub trait DictionaryMediaPort {
    fn fetch(&self, url: &str, accept: &[&str]) -> Result<Vec<u8>, String>;
}
impl DictionaryMediaPort for linguist_dictionary::cambridge::MediaClient {
    fn fetch(&self, url: &str, accept: &[&str]) -> Result<Vec<u8>, String> {
        linguist_dictionary::cambridge::MediaClient::fetch(self, url, accept)
            .map_err(|e| e.to_string())
    }
}
fn dictionary_media<'a>(
    port: Option<&'a dyn DictionaryMediaPort>,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    live: &'a mut Option<linguist_dictionary::cambridge::MediaClient>,
) -> Result<&'a dyn DictionaryMediaPort, String> {
    if let Some(port) = port {
        return Ok(port);
    }
    let client = linguist_dictionary::cambridge::MediaClient::from_settings(settings, environment)
        .map_err(|e| e.to_string())?;
    Ok(live.insert(client))
}
/// Exact dictionary entries of this vocabulary item.
fn exact_entries(vocab: &linguist_core::Vocabulary) -> Vec<&linguist_core::DictionaryEntry> {
    vocab
        .dictionary
        .iter()
        .filter(|entry| entry.forms.contains(&vocab.expression))
        .collect()
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
    /// Japanese illustrations (いらすとや). With `images` injected and this
    /// left `None`, no live illustration search runs.
    pub illustrations: Option<&'a dyn ImagePort>,
    pub speech: Option<&'a dyn SpeechPort>,
    pub recordings: Option<&'a dyn RecordingPort>,
    pub dictionary_media: Option<&'a dyn DictionaryMediaPort>,
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
        // Only a chosen picture counts: a captured source image waits for
        // review (it may be a dictionary screenshot to replace), and kanji
        // stroke animations are not pictures.
        && !document.media.iter().any(|m| m.role == MediaRole::Picture)
        && !document.issues.iter().any(|i| i.code == "IMAGE_CANDIDATE_REVIEW")
}
fn audio_requested(settings: &Effective, document: &LearningDocument) -> bool {
    matches!(document.content, LearningContent::Vocabulary(_))
        && matches!(
            settings.values["audio.provider"].as_str(),
            Some("piper" | "dictionary")
        )
        // Only a chosen audio counts: a captured source recording waits for
        // review and may be replaced by the dictionary's.
        && !document.media.iter().any(|m| m.role == MediaRole::Audio)
        && !document.issues.iter().any(|i| i.code == "AUDIO_CANDIDATE_REVIEW")
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
            providers,
            &mut assets,
        )?;
    }
    if audio_requested(settings, &document) && settings.values["audio.provider"] == "dictionary" {
        stage_recording(
            &mut document,
            settings,
            environment,
            providers.recordings,
            providers.dictionary_media,
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
                // Dedupe only against stroke assets: a captured copy of the
                // same bytes (an older Kanji field) stays an archive.
                if !document
                    .media
                    .iter()
                    .any(|m| m.digest == digest && m.role == MediaRole::KanjiStroke)
                {
                    document.media.push(MediaAsset {
                        digest: digest.clone(),
                        filename: format!("lab_stroke_{digest}.gif"),
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
    providers: Providers<'_>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let (port, media_port) = (providers.images, providers.dictionary_media);
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    // Japanese titles rarely match Commons files; the dictionary's English
    // gloss of the selected sense (else the first exact entry's first sense)
    // finds illustrations.
    let japanese = document.target_language.as_str().split('-').next() == Some("ja");
    let senses: Vec<_> = exact_entries(vocab)
        .into_iter()
        .flat_map(|entry| entry.senses.iter())
        .collect();
    let expression = senses
        .iter()
        .find(|sense| !vocab.sense_key.is_empty() && sense.key == vocab.sense_key)
        .or(senses.first())
        .and_then(|sense| sense.definitions.first())
        .filter(|_| japanese)
        .map(|gloss| gloss.split(';').next().unwrap_or(gloss).trim().to_owned())
        .filter(|gloss| !gloss.is_empty())
        .unwrap_or_else(|| vocab.expression.clone());
    // Dictionary illustrations (Cambridge) come first.
    let illustrations: Vec<String> = exact_entries(vocab)
        .iter()
        .flat_map(|entry| entry.metadata.get("images").into_iter().flatten())
        .take(2)
        .cloned()
        .collect();
    let japanese_expression = vocab.expression.clone();
    let collocation_nouns = collocation_nouns(vocab);
    let mut digests = Vec::new();
    if !illustrations.is_empty() {
        let mut live = None;
        match dictionary_media(media_port, settings, environment, &mut live) {
            Ok(media) => {
                for url in illustrations {
                    match stage_illustration(document, settings, media, &url, assets) {
                        Ok(Some(digest)) => digests.push(digest),
                        Ok(None) => {}
                        Err(error) => warning(
                            document,
                            "IMAGE_SEARCH_FAILED",
                            "picture",
                            format!("{error}; the dictionary illustration was not staged."),
                        ),
                    }
                }
            }
            Err(error) => warning(
                document,
                "IMAGE_SEARCH_FAILED",
                "picture",
                format!("{error}; dictionary illustrations were not staged."),
            ),
        }
    }
    // いらすとや illustrations are searched by the Japanese word itself and
    // come before Commons photographs. When no post title names the word, the
    // nouns of its generated collocations are tried (費用を賄う: 費用).
    if japanese {
        let live;
        let client: Option<&dyn ImagePort> = match (providers.illustrations, port) {
            (Some(port), _) => Some(port),
            (None, Some(_)) => None,
            (None, None) => {
                match crate::illustrations::IllustrationClient::from_settings(settings, environment)
                {
                    Ok(Some(client)) => {
                        live = client;
                        Some(&live)
                    }
                    Ok(None) => None,
                    Err(error) => {
                        warning(
                            document,
                            "IMAGE_SEARCH_FAILED",
                            "picture",
                            format!("{error}; no illustration candidate was staged."),
                        );
                        None
                    }
                }
            }
        };
        let queries = std::iter::once(japanese_expression).chain(collocation_nouns);
        if let Some(client) = client {
            for query in queries {
                match client.search(&query) {
                    Ok(search) => {
                        let named = search.candidates.iter().any(|c| c.title.contains(&query));
                        stage_search(document, search, &mut digests, assets)?;
                        if named {
                            break;
                        }
                    }
                    Err(error) => {
                        warning(
                            document,
                            "IMAGE_SEARCH_FAILED",
                            "picture",
                            format!("{error}; no illustration candidate was staged."),
                        );
                        break;
                    }
                }
            }
        }
    }
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
                format!("{error}; no search candidate was staged. The picture is optional."),
            );
            return candidate_review(document, digests);
        }
    };
    stage_search(document, search, &mut digests, assets)?;
    candidate_review(document, digests)
}

/// The noun before the first particle of each collocation that uses the
/// word (`生活費を賄う` gives `生活費`), at most two.
fn collocation_nouns(vocab: &linguist_core::Vocabulary) -> Vec<String> {
    let mut nouns: Vec<String> = Vec::new();
    for collocation in &vocab.collocations {
        let phrase = collocation.phrase.trim();
        let Some(at) = phrase.find(['を', 'が', 'に', 'で', 'と', 'の', 'へ', 'も']) else {
            continue;
        };
        let noun = &phrase[..at];
        if !phrase.contains(&vocab.expression)
            || noun.is_empty()
            || noun.chars().count() > 8
            || noun.contains(&vocab.expression)
            || nouns.iter().any(|n| n == noun)
        {
            continue;
        }
        nouns.push(noun.to_owned());
        if nouns.len() == 2 {
            break;
        }
    }
    nouns
}

/// Stage every candidate of one search as a reviewable archive asset.
fn stage_search(
    document: &mut LearningDocument,
    search: ImageSearch,
    digests: &mut Vec<String>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    assets.push(search.response.clone());
    let response_digest = canonical::asset_digest(&search.response);
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
    Ok(())
}

/// One review issue over every staged picture candidate.
fn candidate_review(document: &mut LearningDocument, digests: Vec<String>) -> Result<(), String> {
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

/// English with a Cambridge entry: its American IPA fills an empty
/// Pronunciation and its US recording becomes the audio candidate. Returns
/// false when the page has no US pronunciation, so Wiktionary is tried.
fn stage_cambridge_pronunciation(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    media_port: Option<&dyn DictionaryMediaPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<bool, String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(false);
    };
    let entries: Vec<_> = exact_entries(vocab)
        .into_iter()
        .filter(|e| e.provider == linguist_dictionary::cambridge::PROVIDER)
        .collect();
    // The selected sense's entry, else the American pronunciation all exact
    // entries agree on (parts of speech can differ, as for "record").
    let selected = entries
        .iter()
        .find(|e| e.senses.iter().any(|s| s.key == vocab.sense_key));
    let pick = |key: &str| -> Option<String> {
        if let Some(entry) = selected {
            return entry.metadata.get(key).and_then(|v| v.first().cloned());
        }
        let values: BTreeSet<_> = entries
            .iter()
            .filter_map(|e| e.metadata.get(key).and_then(|v| v.first()))
            .collect();
        (values.len() == 1).then(|| values.into_iter().next().unwrap().clone())
    };
    let (Some(ipa), Some(audio)) = (pick("ipa_us"), pick("audio_us")) else {
        return Ok(false);
    };
    let source_url = entries
        .first()
        .map(|e| e.source_url.clone())
        .unwrap_or_default();
    let target = document.target_language.clone();
    if let LearningContent::Vocabulary(vocab) = &mut document.content
        && vocab.pronunciation.trim().is_empty()
    {
        vocab.pronunciation = format!("/{ipa}/");
        document.evidence.push(Evidence {
            id: uuid::Uuid::new_v4(),
            field: "pronunciation".into(),
            provenance: Provenance::Dictionary,
            source_id: None,
            region_id: None,
            target: None,
            source_span: None,
            language: target.clone(),
            claim: serde_json::to_string(&json!({ "ipa_us": ipa })).map_err(|e| e.to_string())?,
            source_url: Some(source_url),
            ambiguous: false,
        });
    }
    let mut live = None;
    let bytes = match dictionary_media(media_port, settings, environment, &mut live)
        .and_then(|media| media.fetch(&audio, &["audio/mpeg"]))
    {
        Ok(bytes) if linguist_dictionary::recording::is_mp3(&bytes) => bytes,
        Ok(_) | Err(_) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                "The Cambridge US recording could not be read; no audio was staged.".into(),
            );
            return Ok(true);
        }
    };
    let Ok(crate::media::SourceMediaInspection::Audio(inspection)) =
        crate::media::inspect_source_media(&bytes, settings)
    else {
        warning(
            document,
            "AUDIO_SYNTHESIS_FAILED",
            "audio",
            "The Cambridge US recording did not decode as audio.".into(),
        );
        return Ok(true);
    };
    let digest = canonical::asset_digest(&bytes);
    // The note may already carry this exact recording.
    if document.media.iter().any(|m| m.digest == digest) {
        return Ok(true);
    }
    document.media.push(MediaAsset {
        digest: digest.clone(),
        filename: format!("candidate_{digest}.mp3"),
        original_filename: audio.rsplit('/').next().map(str::to_owned),
        size_bytes: bytes.len() as u64,
        mime: "audio/mpeg".into(),
        owner: MediaOwner::App,
        role: MediaRole::Archive,
        source_id: None,
        attribution: "American English pronunciation from the Cambridge Dictionary".into(),
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
        language: target,
        claim: serde_json::to_string(&json!({
            "recording": audio,
            "inspection": inspection,
            "scope": "Cambridge US pronunciation; word match unreviewed",
        }))
        .map_err(|e| e.to_string())?,
        source_url: Some(audio.clone()),
        ambiguous: true,
    });
    assets.push(bytes);
    let mut issue = Issue::new(
        "AUDIO_CANDIDATE_REVIEW",
        Severity::Review,
        Some("audio"),
        "Listen to the Cambridge US recording; select it or decline it.",
    );
    issue.id = format!("AUDIO_CANDIDATE_REVIEW:{}", document.id);
    issue.stage = "enrichment".into();
    issue.source_refs = vec![digest];
    document.issues.push(issue);
    Ok(true)
}

/// English: Wiktionary IPA fills an empty Pronunciation (dictionary fact) and
/// its pronunciation recording becomes a reviewed audio candidate.
fn stage_english_recording(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn RecordingPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    let word = vocab.expression.trim().to_owned();
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
    let lookup = match port.english(&word) {
        Ok(Some(lookup)) => lookup,
        Ok(None) => {
            warning(
                document,
                "AUDIO_NOT_FOUND",
                "audio",
                format!("Wiktionary has no page for {word}."),
            );
            return Ok(());
        }
        Err(error) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                format!("{error}; no pronunciation was staged. Audio is optional."),
            );
            return Ok(());
        }
    };
    let pronunciation = &lookup.pronunciation;
    if let Some(ipa) = &pronunciation.ipa {
        let target = document.target_language.clone();
        if let LearningContent::Vocabulary(vocab) = &mut document.content
            && vocab.pronunciation.trim().is_empty()
        {
            vocab.pronunciation = ipa.clone();
            document.evidence.push(Evidence {
                id: uuid::Uuid::new_v4(),
                field: "pronunciation".into(),
                provenance: Provenance::Dictionary,
                source_id: None,
                region_id: None,
                target: None,
                source_span: None,
                language: target,
                claim: serde_json::to_string(&json!({
                    "ipa": ipa, "ipa_lines": pronunciation.ipa_lines,
                    "page_sha256": lookup.page_sha256,
                }))
                .map_err(|e| e.to_string())?,
                source_url: Some(lookup.page_url.clone()),
                ambiguous: false,
            });
        }
    } else if pronunciation.sections > 1 {
        warning(
            document,
            "PRONUNCIATION_AMBIGUOUS",
            "pronunciation",
            format!(
                "Wiktionary lists {} pronunciations for {word}; set the one for this sense with plans edit.",
                pronunciation.sections
            ),
        );
    }
    let Some(recording) = lookup.recording.clone() else {
        warning(
            document,
            "AUDIO_NOT_FOUND",
            "audio",
            format!("Wiktionary has no single pronunciation recording for {word}."),
        );
        return Ok(());
    };
    let inspection = match crate::media::inspect_source_media(&recording.bytes, settings) {
        Ok(crate::media::SourceMediaInspection::Audio(inspection)) => inspection,
        Ok(_) | Err(_) => {
            warning(
                document,
                "AUDIO_SYNTHESIS_FAILED",
                "audio",
                "The Wiktionary recording did not decode as audio; nothing was staged.".into(),
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
        attribution: linguist_dictionary::recording::english::ATTRIBUTION.into(),
        license: Some("CC-BY-SA".into()),
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
            "text": {"expression": word},
            "inspection": inspection,
            "scope": "Wiktionary pronunciation recording; accent and word match unreviewed",
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
        "Listen to the Wiktionary recording; select it or decline it.",
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

/// Stage one dictionary illustration as a picture candidate. Decoded
/// inspection is required, as for search candidates.
fn stage_illustration(
    document: &mut LearningDocument,
    settings: &Effective,
    media: &dyn DictionaryMediaPort,
    url: &str,
    assets: &mut Vec<Vec<u8>>,
) -> Result<Option<String>, String> {
    let bytes = media.fetch(url, &["image/jpeg", "image/png", "image/gif", "image/webp"])?;
    let inspection = match crate::media::inspect_source_media(&bytes, settings) {
        Ok(crate::media::SourceMediaInspection::Image(inspection)) => inspection,
        _ => return Ok(None),
    };
    let digest = canonical::asset_digest(&bytes);
    if document.media.iter().any(|m| m.digest == digest) {
        return Ok(None);
    }
    document.media.push(MediaAsset {
        digest: digest.clone(),
        filename: format!("candidate_{digest}.{}", extension(&inspection.mime)),
        original_filename: url.rsplit('/').next().map(str::to_owned),
        size_bytes: bytes.len() as u64,
        mime: inspection.mime.clone(),
        owner: MediaOwner::External,
        role: MediaRole::Archive,
        source_id: None,
        attribution: format!("Illustration from the Cambridge Dictionary entry ({url})"),
        license: None,
    });
    document.evidence.push(Evidence {
        id: uuid::Uuid::new_v4(),
        field: "picture".into(),
        provenance: Provenance::Dictionary,
        source_id: None,
        region_id: None,
        target: Some(EvidenceTarget::MediaAsset {
            digest: digest.clone(),
        }),
        source_span: None,
        language: document.explanation_language.clone(),
        claim: serde_json::to_string(&json!({
            "illustration": url,
            "inspection": inspection,
            "scope": "dictionary entry illustration; rights and relevance unreviewed",
        }))
        .map_err(|e| e.to_string())?,
        source_url: Some(url.to_owned()),
        ambiguous: true,
    });
    assets.push(bytes);
    Ok(Some(digest))
}

/// Stage one native-speaker recording as an audio candidate that needs review.
fn stage_recording(
    document: &mut LearningDocument,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    port: Option<&dyn RecordingPort>,
    media_port: Option<&dyn DictionaryMediaPort>,
    assets: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return Ok(());
    };
    match document.target_language.as_str().split('-').next() {
        Some("ja") => {}
        Some("en") => {
            if stage_cambridge_pronunciation(document, settings, environment, media_port, assets)? {
                return Ok(());
            }
            return stage_english_recording(document, settings, environment, port, assets);
        }
        _ => {
            warning(
                document,
                "AUDIO_NOT_FOUND",
                "audio",
                "Dictionary recordings are only available for Japanese and English.".into(),
            );
            return Ok(());
        }
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
