//! ALG-OCR applied to captured source documents before any generation.
//!
//! Runs only for `images.existing_policy=inspect`. Original image bytes stay in
//! the source archive; OCR adds regions, evidence and review issues, and its
//! raw TSV and report are returned as assets to archive with the revision.
use crate::ocr::{Engine, OcrError, OcrResult};
use linguist_config::Effective;
use linguist_core::{
    LearningContent, LearningDocument, Provenance, canonical,
    records::{Evidence, EvidenceTarget, MediaOwner, SourceRegion},
    validation::{self, Issue, Severity},
};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

const SETTINGS: [&str; 7] = [
    "images.existing_policy",
    "classification.decision_threshold",
    "classification.confirmation_margin",
    "classification.vision_adjudication",
    "classification.vision_accept_confidence",
    "cache.policy",
    "cache.ttl_hours",
];

/// Heuristic image classes. Scores are uncalibrated layout evidence, not truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageClass {
    Illustration,
    Dictionary,
    Grammar,
    Mixed,
    Uncertain,
}

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub class: ImageClass,
    pub text_score: f64,
    pub threshold: f64,
    pub margin: f64,
    pub coverage: f64,
    pub lines: usize,
    pub characters: usize,
    pub method: &'static str,
    /// The dictionary marker found in the OCR text, when it decided the class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<&'static str>,
    pub needs_review: bool,
}

/// Substrings only dictionary sites print (Kotobank's heading, Jisho's
/// badges, Mazii's comment and photo links, also as OCR reads them without
/// Vietnamese diacritics, Cambridge's word-list links).
const DICTIONARY_MARKERS: [(&str, &str); 10] = [
    ("smart vocabulary", "cambridge-links"),
    ("thesaurus: synonyms", "cambridge-links"),
    ("の解説", "kotobank-heading"),
    ("wanikani", "jisho-badges"),
    ("đóng góp", "mazii"),
    ("dong gop", "mazii"),
    ("xem thêm ảnh", "mazii"),
    ("xem them anh", "mazii"),
    ("động từ nhóm", "mazii"),
    ("dong tir nhom", "mazii"),
];

/// Jisho's part-of-speech line heads ("Noun", "Godan verb with su ending,
/// Transitive verb").
const JISHO_LABELS: [&str; 20] = [
    "Noun",
    "Na-adjective",
    "I-adjective",
    "No-adjective",
    "Taru-adjective",
    "Pre-noun adjectival",
    "Godan verb",
    "Ichidan verb",
    "Suru verb",
    "Kuru verb",
    "Transitive verb",
    "Intransitive verb",
    "Adverb",
    "Adverbial noun",
    "Expressions",
    "Pronoun",
    "Conjunction",
    "Interjection",
    "Counter",
    "Wikipedia definition",
];

/// Cambridge's part-of-speech line ("noun [C]", "verb | UK").
const ENGLISH_LABELS: [&str; 6] = [
    "noun",
    "verb",
    "adjective",
    "adverb",
    "phrasal verb",
    "idiom",
];

/// What makes OCR text a dictionary screenshot whatever its layout: a dark
/// Jisho or Kotobank capture holds one or two short lines, so the layout
/// score calls it an illustration (WP-24: 39 kept as pictures).
fn dictionary_marker(result: &OcrResult) -> Option<&'static str> {
    let lines: Vec<&str> = result
        .regions
        .iter()
        .map(|r| r.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let characters: usize = lines.iter().map(|l| l.chars().count()).sum();
    // A strip of text cut from a page (one definition line); no picture
    // in the collection is four times wider than tall and holds text.
    if characters >= 4
        && result.source_height > 0
        && result.source_width >= 4 * result.source_height
    {
        return Some("text-strip");
    }
    for line in lines {
        let lower = line.to_lowercase();
        if let Some((_, name)) = DICTIONARY_MARKERS.iter().find(|(m, _)| lower.contains(m)) {
            return Some(name);
        }
        let short = line.chars().count() <= 80;
        if short
            && JISHO_LABELS.iter().any(|label| {
                line.strip_prefix(label).is_some_and(|rest| {
                    rest.is_empty() || [",", " (", " with"].iter().any(|s| rest.starts_with(s))
                })
            })
        {
            return Some("jisho-part-of-speech");
        }
        if line.chars().count() <= 40
            && (line.contains('[') || line.contains('|'))
            && ENGLISH_LABELS.iter().any(|label| {
                line.strip_prefix(label)
                    .is_some_and(|rest| rest.starts_with([' ', '|', '[']))
            })
        {
            return Some("english-part-of-speech");
        }
        // The same line as OCR often reads it: "nouni[C]", "nounic]", or
        // with a register label, "adjective = formal".
        if line.chars().count() <= 25
            && ENGLISH_LABELS.iter().any(|label| {
                line.strip_prefix(label).is_some_and(|rest| {
                    (rest.chars().count() <= 6 && rest.contains(']'))
                        || rest
                            .trim_start()
                            .starts_with(['=', '•', '·', '[', '|', '('])
                })
            })
        {
            return Some("english-part-of-speech");
        }
        // Cambridge's pronunciation line: "us /ˈswɪft/ uk /swɪft/" (OCR may
        // read "us" as "use"): a UK/US label and two transcriptions.
        let tokens: Vec<String> = lower
            .split(|c: char| !c.is_ascii_alphabetic())
            .map(str::to_owned)
            .collect();
        let uk = tokens.iter().any(|t| t == "uk");
        let us = tokens.iter().any(|t| t == "us" || t == "use");
        // A label followed by the UK/US audio buttons: "adjective us) uk §)".
        if line.chars().count() <= 30
            && (uk || us)
            && ENGLISH_LABELS.iter().any(|label| lower.starts_with(label))
        {
            return Some("english-part-of-speech");
        }
        let slashes = line.matches('/').count();
        if line.chars().count() <= 60
            && ((uk && us && slashes >= 2) || ((uk || us) && slashes >= 4))
        {
            return Some("pronunciation-line");
        }
        // Cambridge's headword line ("trap noun (MUSIC)").
        let words: Vec<&str> = line.split_whitespace().collect();
        if line.chars().count() <= 40
            && words.len() >= 2
            && ENGLISH_LABELS.contains(&words[1])
            && (words.len() == 2 || words[2].starts_with('('))
        {
            return Some("english-headword");
        }
        if grammar_code(line) {
            return Some("grammar-code");
        }
        if numbered_sense(line) {
            return Some("numbered-sense");
        }
        if line.contains('」') && headword_dash(line) {
            return Some("headword-dash-example");
        }
    }
    None
}

/// A Cambridge grammar code on its own line, with an optional CEFR level:
/// "[C]", "B2 [S]", "[C] slang" (OCR may read "[U]" as "[Ul]").
fn grammar_code(line: &str) -> bool {
    let rest = match line.split_once(' ') {
        Some((level, rest))
            if level.len() == 2
                && matches!(level.as_bytes()[0], b'A' | b'B' | b'C')
                && matches!(level.as_bytes()[1], b'1' | b'2') =>
        {
            rest.trim_start()
        }
        _ => line,
    };
    let Some(inner) = rest
        .strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .map(|(code, after)| (code, after.trim()))
    else {
        return false;
    };
    let (code, after) = inner;
    !code.is_empty()
        && code.len() <= 6
        && code
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == ' ' || c == ',')
        && code
            .chars()
            .next()
            .is_some_and(|c| "CUTISLcutisl".contains(c))
        && after.chars().count() <= 10
}

/// "1. to till; to plow" (OCR may read the digit as a circled one).
fn numbered_sense(line: &str) -> bool {
    let mut chars = line.chars();
    let digit = chars
        .next()
        .is_some_and(|c| c.is_ascii_digit() || ('①'..='⑨').contains(&c));
    let dot = chars.next().is_some_and(|c| c == '.' || c == '．');
    let rest = chars.as_str().trim_start();
    digit
        && dot
        && rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '(')
        && (rest.contains(';') || line.chars().count() > 15)
}

/// A Kotobank example names its headword with a dash: 「―込めて作り上げる」.
fn headword_dash(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        if !matches!(c, '「' | '`' | '‘') {
            return false;
        }
        let mut next = chars[i + 1..].iter().skip_while(|c| c.is_whitespace());
        next.next().is_some_and(|c| matches!(c, '一' | '―' | 'ー'))
            && next.next().is_some_and(|c| !c.is_whitespace())
    })
}

/// Text-likeness from OCR coverage, line count and confidence. Near-threshold
/// scores, mixed layouts and uncertain results always require review.
pub fn classify(
    result: &OcrResult,
    document: &LearningDocument,
    threshold: f64,
    margin: f64,
) -> Classification {
    let area = f64::from(result.source_width) * f64::from(result.source_height);
    let covered: f64 = result
        .regions
        .iter()
        .map(|r| f64::from(r.bounds.width) * f64::from(r.bounds.height))
        .sum();
    let coverage = if area > 0.0 {
        (covered / area).min(1.0)
    } else {
        0.0
    };
    let lines = result.regions.len();
    let characters = result.regions.iter().map(|r| r.text.chars().count()).sum();
    let confidence = result.confidence.unwrap_or(0.0);
    let text_score =
        ((coverage / 0.25).min(1.0) * 0.6 + (lines as f64 / 4.0).min(1.0) * 0.4) * confidence;
    let text_class = match document.content {
        LearningContent::Vocabulary(_) => ImageClass::Dictionary,
        LearningContent::Grammar(_) => ImageClass::Grammar,
    };
    // On a vocabulary note a dictionary marker decides: the screenshot is
    // archived whatever its layout score.
    let marker = match document.content {
        LearningContent::Vocabulary(_) => dictionary_marker(result).or_else(|| {
            // An English card's capture of a Cambridge picture carries its
            // photo credit: the screenshot goes, and the dictionary's own
            // picture (staged first among candidates) takes its place.
            let english = document.target_language.as_str().split('-').next() == Some("en");
            (english
                && result
                    .regions
                    .iter()
                    .any(|r| r.text.to_lowercase().contains("getty")))
            .then_some("dictionary-photo-credit")
        }),
        LearningContent::Grammar(_) => None,
    };
    let class = if marker.is_some() {
        text_class
    } else if text_score >= threshold + margin {
        if coverage < 0.15 && lines < 4 {
            ImageClass::Mixed
        } else {
            text_class
        }
    } else if text_score <= threshold - margin {
        ImageClass::Illustration
    } else {
        ImageClass::Uncertain
    };
    Classification {
        class,
        text_score,
        threshold,
        margin,
        coverage,
        lines,
        characters,
        method: if marker.is_some() {
            "ocr-text-markers-v1"
        } else {
            "ocr-layout-heuristic-v1"
        },
        marker,
        needs_review: matches!(class, ImageClass::Mixed | ImageClass::Uncertain),
    }
}

/// Persistent OCR result cache keyed by [`Engine::fingerprint`].
struct Cache {
    root: std::path::PathBuf,
    policy: String,
    ttl: u64,
}
impl Cache {
    fn new(settings: &Effective, environment: &BTreeMap<String, String>) -> Result<Self, String> {
        let root = linguist_config::expand_path(
            settings.values["storage.cache_dir"]
                .as_str()
                .ok_or("OCR_CACHE_PATH_MISSING")?,
            environment,
        )?;
        if !root.is_absolute() {
            return Err("OCR_CACHE_PATH_RELATIVE".into());
        }
        Ok(Self {
            root: root.join("ocr-v1"),
            policy: settings.values["cache.policy"].as_str().unwrap().to_owned(),
            ttl: settings.values["cache.ttl_hours"].as_u64().unwrap() * 3600,
        })
    }
    fn load(&self, key: &str) -> Option<OcrResult> {
        let report = std::fs::read(self.root.join(format!("{key}.json"))).ok()?;
        let raw = std::fs::read(self.root.join(format!("{key}.tsv"))).ok()?;
        let (stored_at, mut result): (u64, OcrResult) = serde_json::from_slice(&report).ok()?;
        let now = linguist_provider::unix_now();
        if result.cache_fingerprint != key
            || stored_at > now
            || now - stored_at >= self.ttl
            || linguist_provider::sha256_hex(&raw) != result.raw_output_sha256
        {
            return None;
        }
        result.raw_output = raw;
        Some(result)
    }
    fn store(&self, result: &OcrResult) {
        if self.ttl == 0 {
            return;
        }
        let write = || -> std::io::Result<()> {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&self.root)?;
            let key = &result.cache_fingerprint;
            let body = serde_json::to_vec(&(linguist_provider::unix_now(), result))?;
            for (name, bytes) in [
                (format!("{key}.tsv"), &result.raw_output),
                (format!("{key}.json"), &body),
            ] {
                let temporary = self
                    .root
                    .join(format!("{name}.tmp-{}", uuid::Uuid::new_v4()));
                linguist_provider::process::write_private(&temporary, bytes)?;
                std::fs::rename(&temporary, self.root.join(name))?;
            }
            Ok(())
        };
        // The cache is disposable; a failed write never fails recognition.
        let _ = write();
    }
}

/// Reject unavailable OCR configuration before any state is created.
pub fn preflight(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<(), String> {
    let registry = linguist_config::Registry::builtin();
    for key in SETTINGS {
        registry.validate_value(key, settings.values.get(key).ok_or("OCR_SETTING_MISSING")?)?;
    }
    if settings.values["images.existing_policy"] != "inspect" {
        return Ok(());
    }
    if settings.values["classification.vision_adjudication"] == true {
        return Err("CAPABILITY_UNAVAILABLE: classification.vision_adjudication requires a vision adapter that is not available in this build; set it to false".into());
    }
    Engine::probe(settings, environment)
        .map(|_| ())
        .map_err(engine_error)
}

fn engine_error(error: OcrError) -> String {
    format!("{}: {}", error, error.guidance())
}

/// Inspect every source-owned image in reading order and return the OCR
/// assets (raw TSV and report) that must be archived before the revision.
pub fn inspect_documents(
    documents: &mut [LearningDocument],
    assets: &BTreeMap<String, Vec<u8>>,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    preflight(settings, environment)?;
    let mut archived = BTreeMap::new();
    if settings.values["images.existing_policy"] != "inspect" {
        return Ok(archived);
    }
    let threshold = settings.values["classification.decision_threshold"]
        .as_f64()
        .unwrap();
    let margin = settings.values["classification.confirmation_margin"]
        .as_f64()
        .unwrap();
    let cache = Cache::new(settings, environment)?;
    let mut engine = None;
    for document in documents.iter_mut() {
        let images: Vec<_> = document
            .media
            .iter()
            .filter(|m| m.owner == MediaOwner::Source && m.mime.starts_with("image/"))
            .filter_map(|m| m.source_id.map(|source| (m.digest.clone(), source)))
            .collect();
        let mut stored: Vec<Issue> = document
            .issues
            .iter()
            .filter(|i| i.stage != "validation")
            .cloned()
            .collect();
        for (reading_index, (digest, source_id)) in images.into_iter().enumerate() {
            let bytes = assets.get(&digest).ok_or("OCR_SOURCE_ASSET_MISSING")?;
            let engine = match &mut engine {
                Some(engine) => engine,
                None => engine.insert(Engine::probe(settings, environment).map_err(engine_error)?),
            };
            let field = format!("ocr:{digest}");
            let key = engine.fingerprint(bytes);
            let cached = match cache.policy.as_str() {
                "prefer_cache" | "cache_only" => cache.load(&key),
                _ => None,
            };
            let outcome = match cached {
                Some(hit) => Ok((hit, true)),
                None if cache.policy == "cache_only" => {
                    return Err(format!(
                        "OCR_CACHE_MISS: cache.policy=cache_only has no OCR result for source image {digest}"
                    ));
                }
                None => engine.recognize(bytes).map(|result| {
                    cache.store(&result);
                    (result, false)
                }),
            };
            let mut review = |code: &str, message: String| {
                let mut issue = Issue::new(code, Severity::Review, Some(&field), message);
                issue.id = format!("{code}:{source_id}:{digest}");
                issue.stage = "ocr".into();
                issue.source_refs = vec![source_id.to_string()];
                stored.push(issue);
            };
            let (result, from_cache) = match outcome {
                Ok(found) => found,
                Err(failure) => {
                    // One image failing is an item review, not a process failure.
                    document.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4(),
                        field: field.clone(),
                        provenance: Provenance::Ocr,
                        source_id: Some(source_id),
                        region_id: None,
                        target: Some(EvidenceTarget::MediaAsset {
                            digest: digest.clone(),
                        }),
                        source_span: None,
                        language: document.target_language.clone(),
                        claim: serde_json::to_string(&json!({
                            "asset_digest": digest,
                            "failure": failure,
                            "scope": "OCR failed; original image retained; no text evidence",
                        }))
                        .map_err(|e| e.to_string())?,
                        source_url: None,
                        ambiguous: true,
                    });
                    review(
                        "OCR_FAILED_REVIEW",
                        format!(
                            "{failure}. {} Inspect the image before generation.",
                            failure.guidance()
                        ),
                    );
                    continue;
                }
            };
            let classification = classify(&result, document, threshold, margin);
            let raw_digest = canonical::asset_digest(&result.raw_output);
            let report = serde_json::to_vec(&json!({
                "schema": "linguist-ocr-report-v1",
                "result": result,
                "classification": classification,
                "from_cache": from_cache,
            }))
            .map_err(|e| e.to_string())?;
            let report_digest = canonical::asset_digest(&report);
            archived.insert(raw_digest.clone(), result.raw_output.clone());
            archived.insert(report_digest.clone(), report);
            let engine_name = format!(
                "tesseract/{}",
                result.engine_version.trim_start_matches("tesseract ")
            );
            for (order, region) in result.regions.iter().enumerate() {
                let region_id = uuid::Uuid::new_v4();
                document.regions.push(SourceRegion {
                    id: region_id,
                    source_id,
                    image_digest: digest.clone(),
                    bounds: [
                        region.bounds.left,
                        region.bounds.top,
                        region.bounds.width.max(1),
                        region.bounds.height.max(1),
                    ],
                    reading_order: u32::try_from(reading_index * 100_000 + order)
                        .map_err(|_| "OCR_REGION_ORDER_LIMIT")?,
                    engine: engine_name.clone(),
                    settings_digest: result.cache_fingerprint.clone(),
                    text: region.text.clone(),
                    confidence: Some(region.confidence.clamp(0.0, 1.0)),
                    language: document.target_language.clone(),
                });
                document.evidence.push(Evidence {
                    id: uuid::Uuid::new_v4(),
                    field: field.clone(),
                    provenance: Provenance::Ocr,
                    source_id: Some(source_id),
                    region_id: Some(region_id),
                    target: Some(EvidenceTarget::MediaAsset {
                        digest: digest.clone(),
                    }),
                    source_span: None,
                    language: document.target_language.clone(),
                    claim: region.text.clone(),
                    source_url: None,
                    ambiguous: region.confidence
                        < settings.values["ocr.minimum_confidence"].as_f64().unwrap(),
                });
            }
            document.evidence.push(Evidence {
                id: uuid::Uuid::new_v4(),
                field: field.clone(),
                provenance: Provenance::Ocr,
                source_id: Some(source_id),
                region_id: None,
                target: Some(EvidenceTarget::MediaAsset {
                    digest: digest.clone(),
                }),
                source_span: None,
                language: document.target_language.clone(),
                claim: serde_json::to_string(&json!({
                    "asset_digest": digest,
                    "engine": engine_name,
                    "cache_fingerprint": result.cache_fingerprint,
                    "raw_output_digest": raw_digest,
                    "report_digest": report_digest,
                    "confidence": result.confidence,
                    "confidence_semantics": result.confidence_semantics,
                    "classification": classification,
                    "from_cache": from_cache,
                }))
                .map_err(|e| e.to_string())?,
                source_url: None,
                ambiguous: result.needs_review || classification.needs_review,
            });
            if result.needs_review {
                review(
                    "OCR_TEXT_REVIEW",
                    format!(
                        "OCR needs review ({}). Compare every region with the image; the picture is preserved.",
                        result.review_reasons.join(", ")
                    ),
                );
            }
            if classification.needs_review {
                review(
                    "IMAGE_CLASSIFICATION_REVIEW",
                    format!(
                        "Image classified as {:?} with uncalibrated score {:.2}; confirm how this image is used.",
                        classification.class, classification.text_score
                    ),
                );
            }
        }
        if let Some(issue) = segmentation_issue(document) {
            stored.push(issue);
        }
        document.issues = stored;
        document.issues = validation::validate(document);
    }
    Ok(archived)
}

/// Pattern markers that identify a grammar heading line in OCR text.
const PATTERN_MARKERS: [char; 3] = ['〜', '～', '~'];

/// ALG-GRAMMAR step 2–3: every OCR line that looks like a pattern is a
/// candidate unit; without markers every line is a candidate. A reviewer
/// chooses one unit or several (which then requires a split).
fn segmentation_issue(document: &LearningDocument) -> Option<Issue> {
    if !matches!(document.content, LearningContent::Grammar(_)) {
        return None;
    }
    let mut regions: Vec<_> = document
        .regions
        .iter()
        .filter(|r| !r.text.trim().is_empty())
        .collect();
    regions.sort_by_key(|r| r.reading_order);
    let marked: Vec<_> = regions
        .iter()
        .copied()
        .filter(|r| r.text.trim_start().starts_with(PATTERN_MARKERS))
        .collect();
    let candidates = if marked.is_empty() { regions } else { marked };
    if candidates.is_empty() {
        return None;
    }
    let mut issue = Issue::new(
        "GRAMMAR_SEGMENTATION_REVIEW",
        Severity::Review,
        Some("pattern"),
        format!(
            "OCR found {} candidate grammar unit(s). Choose the one this note teaches, or several to split into separate notes.",
            candidates.len()
        ),
    );
    issue.id = format!("GRAMMAR_SEGMENTATION_REVIEW:{}", document.id);
    issue.stage = "segmentation".into();
    issue.source_refs = candidates.iter().map(|r| r.id.to_string()).collect();
    Some(issue)
}

/// Generation gate: required OCR must exist and its reviews must be resolved.
pub fn generation_ready(document: &LearningDocument, settings: &Effective) -> Result<(), String> {
    // Reviewed segmentation must finish before any unit is generated.
    if validation::validate(document).iter().any(|i| {
        i.code == "GRAMMAR_SPLIT_PENDING"
            || i.stage == "segmentation" && i.severity == Severity::Review
    }) {
        return Err("GENERATION_SEGMENTATION_REQUIRED: resolve grammar segmentation (and split) before generation".into());
    }
    if settings
        .values
        .get("images.existing_policy")
        .and_then(|v| v.as_str())
        != Some("inspect")
    {
        return Ok(());
    }
    for media in document
        .media
        .iter()
        .filter(|m| m.owner == MediaOwner::Source && m.mime.starts_with("image/"))
    {
        let field = format!("ocr:{}", media.digest);
        if !document
            .evidence
            .iter()
            .any(|e| e.provenance == Provenance::Ocr && e.field == field)
        {
            return Err(
                "GENERATION_OCR_REQUIRED: run OCR on source images before generation".into(),
            );
        }
    }
    if validation::validate(document)
        .iter()
        .any(|i| i.stage == "ocr" && i.severity == Severity::Review)
    {
        return Err(
            "GENERATION_OCR_REVIEW_REQUIRED: resolve OCR review issues before generation".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{Bounds, OcrRegion, Preprocessing};

    fn result(lines: u32, width: u32, confidence: f64) -> OcrResult {
        let regions = (0..lines)
            .map(|line| OcrRegion {
                block: 1,
                paragraph: 1,
                line,
                text: "text".into(),
                confidence,
                bounds: Bounds {
                    left: 0,
                    top: line * 10,
                    width,
                    height: 10,
                },
                words: vec![],
            })
            .collect();
        OcrResult {
            engine: "tesseract".into(),
            engine_version: "tesseract 5".into(),
            executable_sha256: String::new(),
            languages: vec![],
            page_segmentation_mode: 3,
            engine_mode: 1,
            preprocessing: Preprocessing {
                enabled: true,
                grayscale: true,
                inverted: false,
                scale: 1,
            },
            source_sha256: String::new(),
            source_width: 100,
            source_height: 100,
            derivative_sha256: String::new(),
            regions,
            text: String::new(),
            confidence: (lines > 0).then_some(confidence),
            confidence_semantics: String::new(),
            needs_review: false,
            review_reasons: vec![],
            raw_output_sha256: String::new(),
            cache_fingerprint: String::new(),
            raw_output: vec![],
        }
    }

    fn document(grammar: bool) -> LearningDocument {
        let content = if grammar {
            json!({"kind":"grammar","body":{"pattern":"p","use_key":"u","meaning":"","formation":"","recognition_prompt":"","examples":[],"usage":"","exercise_prompt":"","exercise_answer":""}})
        } else {
            json!({"kind":"vocabulary","body":{"expression":"e","meaning":"","sense_key":"","reading":"","pronunciation":"","usage":"","examples":[],"dictionary":[],"kanji":"","production_prompt":"","spelling_prompt":""}})
        };
        serde_json::from_value(json!({
            "schema_version": 2, "id": uuid::Uuid::nil(), "target_language": "en",
            "explanation_language": "en", "content": content, "requested_tasks": []
        }))
        .unwrap()
    }

    #[test]
    fn classes_follow_configured_threshold_and_margin() {
        let vocab = document(false);
        let dense = result(8, 90, 0.95);
        assert_eq!(
            classify(&dense, &vocab, 0.5, 0.12).class,
            ImageClass::Dictionary
        );
        assert_eq!(
            classify(&dense, &document(true), 0.5, 0.12).class,
            ImageClass::Grammar
        );
        assert_eq!(
            classify(&result(0, 0, 0.0), &vocab, 0.5, 0.12).class,
            ImageClass::Illustration
        );
        // A short caption on a picture is mixed and needs review.
        let caption = classify(&result(1, 90, 0.95), &vocab, 0.2, 0.05);
        assert_eq!(caption.class, ImageClass::Mixed);
        assert!(caption.needs_review);
        // The same evidence is uncertain under a nondefault threshold/margin.
        let near = classify(&dense, &vocab, 0.9, 0.1);
        assert_eq!(near.class, ImageClass::Uncertain);
        assert!(near.needs_review);
        assert!(!classify(&dense, &vocab, 0.3, 0.0).needs_review);
    }

    fn lines(texts: &[&str], width: u32, height: u32) -> OcrResult {
        let mut found = result(texts.len() as u32, 10, 0.9);
        for (region, text) in found.regions.iter_mut().zip(texts) {
            region.text = (*text).into();
        }
        (found.source_width, found.source_height) = (width, height);
        found
    }

    #[test]
    fn dictionary_text_is_a_dictionary_screenshot_whatever_its_layout() {
        // OCR text of screenshots WP-24 kept as pictures (layout score low).
        for (texts, size, marker) in [
            (
                &[
                    "Godan verb with su ending, Transitive verb",
                    "1. to till; to plow",
                ][..],
                (589, 293),
                "jisho-part-of-speech",
            ),
            (
                &["Noun", "①. celebrity; famous person"][..],
                (400, 175),
                "jisho-part-of-speech",
            ),
            (
                &["jlptn3 wanikani level 46", "1. cotton"][..],
                (423, 175),
                "jisho-badges",
            ),
            (
                &["ざつ - こく【雛袴】の解説", "米・麦以外の穀類の総称。"][..],
                (700, 300),
                "kotobank-heading",
            ),
            (
                &["-_ wy 2 ニム", "たましい。精神。 ` 一込めて作り上げる」"][..],
                (790, 69 * 3),
                "headword-dash-example",
            ),
            (&["[ たいまつ」に同じ。"][..], (768, 69), "text-strip"),
            (&["C6 10 y kién dong gop", "JinZz"][..], (584, 300), "mazii"),
            (
                &["noun |[C]", "a small container with medicine"][..],
                (600, 300),
                "english-part-of-speech",
            ),
            (
                &["trap noun (MUSIC)", "[Ul]", "short for trap music"][..],
                (400, 200),
                "english-headword",
            ),
            (
                &["certain of your beliefs:", "B2 [S]"][..],
                (430, 130),
                "grammar-code",
            ),
            (
                &["a device", "+ SMART Vocabulary: related words"][..],
                (400, 300),
                "cambridge-links",
            ),
            (
                &["nouni[C]", "a shish kebab"][..],
                (300, 400),
                "english-part-of-speech",
            ),
            (
                &["us) /swit/ uk) /swift/", "a small bird"][..],
                (520, 240),
                "pronunciation-line",
            ),
            (
                &[
                    "unable to be removed:",
                    "uk¥) /rneriianab?l/ use) /rneLlia.na.b?/",
                ][..],
                (480, 260),
                "pronunciation-line",
            ),
            (
                &["adjective = formal", "unable to be removed:"][..],
                (480, 260),
                "english-part-of-speech",
            ),
            (
                &["crescent", "adjective us) uk §)", "* the crescent moon"][..],
                (480, 160),
                "english-part-of-speech",
            ),
        ] {
            let found = classify(&lines(texts, size.0, size.1), &document(false), 0.46, 0.03);
            assert_eq!(found.class, ImageClass::Dictionary, "{texts:?}");
            assert_eq!(found.marker, Some(marker), "{texts:?}");
            assert!(!found.needs_review);
        }
        // Text in real pictures is no marker.
        for (texts, size) in [
            (&["Placenta", "Incision through the skin"][..], (600, 400)),
            (&["FRENCH HORN", "SNARE DRUM"][..], (500, 500)),
            (
                &["| お行儀の悪い人 | と言われないために一一"][..],
                (400, 600),
            ),
            (
                &["Download from 50884824", "Dreamstime.com"][..],
                (800, 400),
            ),
        ] {
            let found = classify(&lines(texts, size.0, size.1), &document(false), 0.46, 0.03);
            assert_eq!(found.marker, None, "{texts:?}");
        }
        // A grammar page keeps the layout heuristic.
        let page = lines(&["Noun", "1. to till; to plow"], 600, 300);
        assert_eq!(classify(&page, &document(true), 0.46, 0.03).marker, None);
        // A Cambridge picture captured with its credit, on an English card
        // only (a Japanese card's stock photo keeps the layout heuristic).
        let shoe = lines(&["a running shoe", "xxmmx/E+/GettyImages"], 370, 140);
        let english = classify(&shoe, &document(false), 0.46, 0.03);
        assert_eq!(english.class, ImageClass::Dictionary);
        assert_eq!(english.marker, Some("dictionary-photo-credit"));
        let mut japanese = document(false);
        japanese.target_language = "ja".to_owned().try_into().unwrap();
        assert_eq!(classify(&shoe, &japanese, 0.46, 0.03).marker, None);
    }
}
