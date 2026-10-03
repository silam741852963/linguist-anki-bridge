//! ALG-OCR extraction through a bounded local Tesseract child process.
//!
//! Source bytes are never modified: the engine reads a private temporary PNG
//! derivative that is removed when recognition finishes. Recognized text is
//! untrusted data and is never interpreted as instructions.
use image::{DynamicImage, ImageReader, Limits, imageops::FilterType};
use linguist_config::Effective;
use linguist_provider::process::{
    ProcessError, ProcessLimits, TempDir, command, hash_file, hex, run, write_private,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::PathBuf,
    time::{Duration, Instant},
};

const SETTINGS: [&str; 14] = [
    "ocr.engine",
    "ocr.languages",
    "ocr.preprocess",
    "ocr.timeout_seconds",
    "ocr.page_segmentation_mode",
    "ocr.engine_mode",
    "ocr.minimum_confidence",
    "ocr.max_pixels",
    "ocr.max_regions",
    "ocr.executable",
    "ocr.resource_path",
    "helpers.memory_limit_mb",
    "helpers.max_output_mb",
    "storage.temp_dir",
];
const LANGUAGE_PACK_LIMIT: u64 = 1024 * 1024 * 1024;
const MAX_UPSCALE: u32 = 3;
/// Mean luminance below this is treated as light text on a dark background.
const INVERT_BELOW: f64 = 120.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OcrError {
    InvalidSettings {
        message: String,
    },
    /// The selected engine has no implemented adapter in this build.
    OcrEngineUnavailable {
        engine: String,
    },
    OcrExecutableUnavailable {
        status: String,
    },
    OcrResourcePathUnavailable {
        status: String,
    },
    OcrLanguagePackMissing {
        missing: Vec<String>,
    },
    OcrLanguagePackUnreadable {
        language: String,
    },
    OcrImageRejected {
        image_code: String,
    },
    OcrPixelLimit {
        pixels: u64,
        max_pixels: u64,
    },
    OcrDerivativeFailed,
    OcrProcessSpawnFailed,
    OcrProcessTimeout,
    OcrProcessFailed {
        exit_code: Option<i32>,
    },
    OcrOutputLimit,
    OcrOutputMalformed {
        reason: String,
    },
    OcrRegionLimit {
        regions: usize,
        max_regions: u64,
    },
}
impl OcrError {
    pub fn guidance(&self) -> &'static str {
        match self {
            Self::InvalidSettings { .. } => "Correct the OCR configuration before preparing again.",
            Self::OcrEngineUnavailable { .. } => {
                "Select ocr.engine=tesseract, or provide the item's text manually. Other OCR engines are not available in this build."
            }
            Self::OcrExecutableUnavailable { .. } => {
                "Install Tesseract or set ocr.executable to an absolute regular executable file. Run `doctor --local` to confirm."
            }
            Self::OcrResourcePathUnavailable { .. } => {
                "Set ocr.resource_path to an existing absolute tessdata directory, or unset it to use the engine default."
            }
            Self::OcrLanguagePackMissing { .. } => {
                "Install the missing Tesseract language packs, or narrow ocr.languages for this purpose. Packs are never downloaded implicitly."
            }
            Self::OcrLanguagePackUnreadable { .. } => {
                "Check that the language pack is a readable regular file within the size limit."
            }
            Self::OcrImageRejected { .. } | Self::OcrPixelLimit { .. } => {
                "Inspect the original image. Provide a smaller supported replacement or transcribe it manually; the source asset is preserved."
            }
            Self::OcrDerivativeFailed => {
                "Check temporary-directory space and permissions, then retry."
            }
            Self::OcrProcessSpawnFailed | Self::OcrProcessFailed { .. } => {
                "Run the OCR executable manually on a sample image. The selected ocr.engine_mode needs matching installed training data."
            }
            Self::OcrProcessTimeout => {
                "Retry with a smaller image, or deliberately raise ocr.timeout_seconds for this purpose."
            }
            Self::OcrOutputLimit => {
                "The engine produced more output than helpers.max_output_mb permits. Nothing was truncated; review the image or raise the limit."
            }
            Self::OcrOutputMalformed { .. } => {
                "The engine output was not valid Tesseract TSV. Verify the executable is a supported Tesseract build."
            }
            Self::OcrRegionLimit { .. } => {
                "The image has more text lines than ocr.max_regions permits. Split or transcribe it manually, or raise the limit."
            }
        }
    }
    pub fn code(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}
impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSettings { message } => write!(f, "OCR_INVALID_SETTINGS:{message}"),
            _ => f.write_str(&self.code()),
        }
    }
}
impl std::error::Error for OcrError {}
impl From<ProcessError> for OcrError {
    fn from(error: ProcessError) -> Self {
        match error {
            ProcessError::SpawnFailed => Self::OcrProcessSpawnFailed,
            ProcessError::Timeout => Self::OcrProcessTimeout,
            ProcessError::Failed { exit_code } => Self::OcrProcessFailed { exit_code },
            ProcessError::OutputLimit => Self::OcrOutputLimit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagePack {
    pub code: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preprocessing {
    pub enabled: bool,
    pub grayscale: bool,
    pub inverted: bool,
    /// Integer upscale factor; derivative coordinates divide exactly back to source.
    pub scale: u32,
}

/// Coordinates are in source-image pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrWord {
    pub text: String,
    pub confidence: f64,
    pub bounds: Bounds,
}

/// One recognized text line in engine reading order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrRegion {
    pub block: u32,
    pub paragraph: u32,
    pub line: u32,
    pub text: String,
    pub confidence: f64,
    pub bounds: Bounds,
    pub words: Vec<OcrWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub engine: String,
    pub engine_version: String,
    pub executable_sha256: String,
    pub languages: Vec<LanguagePack>,
    pub page_segmentation_mode: u64,
    pub engine_mode: u64,
    pub preprocessing: Preprocessing,
    pub source_sha256: String,
    pub source_width: u32,
    pub source_height: u32,
    pub derivative_sha256: String,
    pub regions: Vec<OcrRegion>,
    pub text: String,
    /// Character-weighted mean of normalized word confidences.
    pub confidence: Option<f64>,
    /// Engine-specific score; it is not a calibrated probability of correctness.
    pub confidence_semantics: String,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
    pub raw_output_sha256: String,
    /// Cache key: source bytes plus engine, resources and recognition settings.
    pub cache_fingerprint: String,
    /// Exact engine TSV for archival; excluded from serialized summaries.
    #[serde(skip)]
    pub raw_output: Vec<u8>,
}

pub fn validate_settings(settings: &Effective) -> Result<(), String> {
    let registry = linguist_config::Registry::builtin();
    for key in SETTINGS {
        registry.validate_value(key, settings.values.get(key).ok_or("OCR_SETTING_MISSING")?)?;
    }
    Ok(())
}

/// A probed engine: executable, version and installed language packs are
/// resolved and hashed once, so a cache key exists before recognition runs.
pub struct Engine {
    executable: PathBuf,
    tessdata: Option<PathBuf>,
    temp_root: PathBuf,
    engine_version: String,
    executable_sha256: String,
    languages: Vec<LanguagePack>,
    psm: u64,
    oem: u64,
    preprocess: bool,
    minimum: f64,
    max_pixels: u64,
    max_regions: u64,
    timeout: Duration,
    output: usize,
    memory: u64,
    settings: Effective,
}

impl Engine {
    /// Engine-wide failures (unavailable engine, executable or packs) are
    /// reported here, before any source image is processed.
    pub fn probe(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, OcrError> {
        validate_settings(settings).map_err(|message| OcrError::InvalidSettings { message })?;
        let value = |key: &str| &settings.values[key];
        let engine = value("ocr.engine").as_str().unwrap();
        if engine != "tesseract" {
            return Err(OcrError::OcrEngineUnavailable {
                engine: engine.into(),
            });
        }
        let codes: Vec<String> = value("ocr.languages")
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        if let Some(bad) = codes.iter().find(|l| !valid_language_code(l)) {
            return Err(OcrError::InvalidSettings {
                message: format!("ocr.languages:{bad}"),
            });
        }
        let timeout = Duration::from_secs(value("ocr.timeout_seconds").as_u64().unwrap());
        let output = value("helpers.max_output_mb").as_u64().unwrap() as usize * 1024 * 1024;
        let memory = value("helpers.memory_limit_mb").as_u64().unwrap() * 1024 * 1024;
        let limits = ProcessLimits {
            deadline: Instant::now() + timeout,
            output,
            memory,
        };
        let executable = linguist_config::resources::resolve_executable(
            value("ocr.executable").as_str().unwrap(),
            environment,
        )
        .map_err(|status| OcrError::OcrExecutableUnavailable {
            status: status.into(),
        })?;
        let tessdata = match value("ocr.resource_path").as_str() {
            None => None,
            Some(reference) => Some(resource_directory(reference, environment)?),
        };
        let temp_root =
            linguist_config::expand_path(value("storage.temp_dir").as_str().unwrap(), environment)
                .ok()
                .filter(|p| p.is_absolute())
                .ok_or(OcrError::OcrDerivativeFailed)?;
        let executable_sha256 = hash_file(&executable, u64::MAX)
            .map_err(|_| OcrError::OcrExecutableUnavailable {
                status: "unreadable".into(),
            })?
            .0;
        let mut version = command(&executable);
        version.arg("--version");
        let engine_version = String::from_utf8_lossy(&run(version, &limits)?)
            .lines()
            .next()
            .map(str::trim)
            .filter(|l| l.starts_with("tesseract "))
            .ok_or_else(|| OcrError::OcrOutputMalformed {
                reason: "version".into(),
            })?
            .to_owned();
        let mut list = command(&executable);
        if let Some(dir) = &tessdata {
            list.arg("--tessdata-dir").arg(dir);
        }
        list.arg("--list-langs");
        let (pack_dir, installed) = parse_language_list(&run(list, &limits)?)?;
        let missing: Vec<String> = codes
            .iter()
            .filter(|l| !installed.contains(*l))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(OcrError::OcrLanguagePackMissing { missing });
        }
        let pack_dir = tessdata.clone().unwrap_or(pack_dir);
        let mut languages = Vec::new();
        for code in &codes {
            let path = pack_dir.join(format!("{code}.traineddata"));
            let (sha256, bytes) = hash_file(&path, LANGUAGE_PACK_LIMIT).map_err(|_| {
                OcrError::OcrLanguagePackUnreadable {
                    language: code.clone(),
                }
            })?;
            languages.push(LanguagePack {
                code: code.clone(),
                sha256,
                bytes,
            });
        }
        Ok(Self {
            executable,
            tessdata,
            temp_root,
            engine_version,
            executable_sha256,
            languages,
            psm: value("ocr.page_segmentation_mode").as_u64().unwrap(),
            oem: value("ocr.engine_mode").as_u64().unwrap(),
            preprocess: value("ocr.preprocess").as_bool().unwrap(),
            minimum: value("ocr.minimum_confidence").as_f64().unwrap(),
            max_pixels: value("ocr.max_pixels").as_u64().unwrap(),
            max_regions: value("ocr.max_regions").as_u64().unwrap(),
            timeout,
            output,
            memory,
            settings: settings.clone(),
        })
    }

    /// Probe result for diagnostics: engine identity and hashed language packs.
    pub fn identity(&self) -> serde_json::Value {
        serde_json::json!({
            "engine": "tesseract",
            "engine_version": self.engine_version,
            "executable_sha256": self.executable_sha256,
            "languages": self.languages,
        })
    }

    /// Cache key for one source image under this engine, resources and settings.
    pub fn fingerprint(&self, source: &[u8]) -> String {
        let material = serde_json::json!({
            "schema": "linguist-ocr-cache-v1",
            "source_sha256": hex(&Sha256::digest(source)),
            "engine": "tesseract",
            "engine_version": self.engine_version,
            "executable_sha256": self.executable_sha256,
            "languages": self.languages,
            "page_segmentation_mode": self.psm,
            "engine_mode": self.oem,
            "preprocess": self.preprocess,
            "minimum_confidence": self.minimum,
            "max_pixels": self.max_pixels,
            "max_regions": self.max_regions,
        });
        hex(&Sha256::digest(
            serde_jcs::to_vec(&material).expect("JSON values serialize"),
        ))
    }

    pub fn recognize(&self, source: &[u8]) -> Result<OcrResult, OcrError> {
        // Decode safely before any process is started.
        let inspection = crate::media::inspect_image(source, &self.settings).map_err(|e| {
            OcrError::OcrImageRejected {
                image_code: e.to_string(),
            }
        })?;
        let pixels = u64::from(inspection.width) * u64::from(inspection.height);
        if pixels > self.max_pixels {
            return Err(OcrError::OcrPixelLimit {
                pixels,
                max_pixels: self.max_pixels,
            });
        }
        let (derivative, preprocessing) =
            derivative(source, &self.settings, self.preprocess, self.max_pixels)?;
        let derivative_sha256 = hex(&Sha256::digest(&derivative));
        let workspace =
            TempDir::create_in(&self.temp_root).map_err(|_| OcrError::OcrDerivativeFailed)?;
        let input = workspace.0.join("derivative.png");
        write_private(&input, &derivative).map_err(|_| OcrError::OcrDerivativeFailed)?;
        let mut recognize = command(&self.executable);
        recognize.arg(&input).arg("stdout");
        if let Some(dir) = &self.tessdata {
            recognize.arg("--tessdata-dir").arg(dir);
        }
        recognize
            .arg("-l")
            .arg(
                self.languages
                    .iter()
                    .map(|l| l.code.as_str())
                    .collect::<Vec<_>>()
                    .join("+"),
            )
            .arg("--psm")
            .arg(self.psm.to_string())
            .arg("--oem")
            .arg(self.oem.to_string())
            .arg("tsv");
        let limits = ProcessLimits {
            deadline: Instant::now() + self.timeout,
            output: self.output,
            memory: self.memory,
        };
        let raw_output = run(recognize, &limits)?;
        drop(workspace);

        let regions = parse_tsv(&raw_output, preprocessing.scale)?;
        if regions.len() as u64 > self.max_regions {
            return Err(OcrError::OcrRegionLimit {
                regions: regions.len(),
                max_regions: self.max_regions,
            });
        }
        let text = regions
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let (weighted, characters) =
            regions
                .iter()
                .flat_map(|r| &r.words)
                .fold((0.0, 0usize), |(sum, count), word| {
                    let n = word.text.chars().count();
                    (sum + word.confidence * n as f64, count + n)
                });
        let confidence = (characters > 0).then(|| weighted / characters as f64);
        let mut review_reasons = Vec::new();
        match confidence {
            None => review_reasons.push("no_text_recognized".to_owned()),
            Some(c) if c < self.minimum => {
                review_reasons.push("confidence_below_minimum".to_owned())
            }
            _ => {}
        }
        Ok(OcrResult {
            engine: "tesseract".into(),
            engine_version: self.engine_version.clone(),
            executable_sha256: self.executable_sha256.clone(),
            languages: self.languages.clone(),
            page_segmentation_mode: self.psm,
            engine_mode: self.oem,
            preprocessing,
            source_sha256: hex(&Sha256::digest(source)),
            source_width: inspection.width,
            source_height: inspection.height,
            derivative_sha256,
            text,
            confidence,
            confidence_semantics: "tesseract_word_confidence_normalized_uncalibrated".into(),
            needs_review: !review_reasons.is_empty(),
            review_reasons,
            raw_output_sha256: hex(&Sha256::digest(&raw_output)),
            regions,
            cache_fingerprint: self.fingerprint(source),
            raw_output,
        })
    }
}

/// Probe the engine and recognize one source image.
pub fn recognize(
    source: &[u8],
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<OcrResult, OcrError> {
    Engine::probe(settings, environment)?.recognize(source)
}

fn valid_language_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn resource_directory(
    reference: &str,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, OcrError> {
    let unavailable = |status: &str| OcrError::OcrResourcePathUnavailable {
        status: status.into(),
    };
    let path = linguist_config::expand_path(reference, environment)
        .map_err(|_| unavailable("invalid_path_expansion"))?;
    if !path.is_absolute() {
        return Err(unavailable("relative_path"));
    }
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => Err(unavailable("symlink")),
        Ok(m) if m.is_dir() => Ok(path),
        Ok(_) => Err(unavailable("wrong_file_type")),
        Err(_) => Err(unavailable("missing")),
    }
}

/// Build a derivative from the first decoded frame. Preprocessing converts to
/// grayscale, inverts dark backgrounds and upscales by an integer factor within
/// the pixel budget so region coordinates map exactly back to the source.
fn derivative(
    source: &[u8],
    settings: &Effective,
    preprocess: bool,
    max_pixels: u64,
) -> Result<(Vec<u8>, Preprocessing), OcrError> {
    let cap = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
    let mut limits = Limits::default();
    limits.max_alloc = Some(cap);
    let mut reader = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|_| OcrError::OcrDerivativeFailed)?;
    reader.limits(limits);
    let image = reader.decode().map_err(|_| OcrError::OcrDerivativeFailed)?;
    let mut info = Preprocessing {
        enabled: preprocess,
        grayscale: false,
        inverted: false,
        scale: 1,
    };
    let image = if preprocess {
        let mut gray = image.to_luma8();
        let sum: u64 = gray.as_raw().iter().map(|&p| u64::from(p)).sum();
        let mean = sum as f64 / gray.as_raw().len().max(1) as f64;
        if mean < INVERT_BELOW {
            image::imageops::invert(&mut gray);
            info.inverted = true;
        }
        info.grayscale = true;
        let pixels = u64::from(gray.width()) * u64::from(gray.height());
        let mut scale = MAX_UPSCALE;
        while scale > 1 && pixels * u64::from(scale * scale) > max_pixels {
            scale -= 1;
        }
        info.scale = scale;
        let gray = if scale > 1 {
            image::imageops::resize(
                &gray,
                gray.width() * scale,
                gray.height() * scale,
                FilterType::Lanczos3,
            )
        } else {
            gray
        };
        DynamicImage::ImageLuma8(gray)
    } else {
        image
    };
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| OcrError::OcrDerivativeFailed)?;
    Ok((out.into_inner(), info))
}

fn parse_language_list(output: &[u8]) -> Result<(PathBuf, Vec<String>), OcrError> {
    let malformed = || OcrError::OcrOutputMalformed {
        reason: "language_list".into(),
    };
    let text = std::str::from_utf8(output).map_err(|_| malformed())?;
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(malformed)?;
    let start = header.find('"').ok_or_else(malformed)? + 1;
    let end = start + header[start..].find('"').ok_or_else(malformed)?;
    let directory = PathBuf::from(&header[start..end]);
    if !header.starts_with("List of available languages") || !directory.is_absolute() {
        return Err(malformed());
    }
    Ok((
        directory,
        lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    ))
}

const TSV_HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

/// Parse Tesseract TSV word rows (level 5) into lines in engine order.
fn parse_tsv(output: &[u8], scale: u32) -> Result<Vec<OcrRegion>, OcrError> {
    let malformed = |reason: &str| OcrError::OcrOutputMalformed {
        reason: reason.into(),
    };
    let text = std::str::from_utf8(output).map_err(|_| malformed("utf8"))?;
    let mut lines = text.lines();
    if lines.next() != Some(TSV_HEADER) {
        return Err(malformed("header"));
    }
    let mut regions: Vec<OcrRegion> = Vec::new();
    let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    for row in lines {
        if row.is_empty() {
            continue;
        }
        let columns: Vec<&str> = row.splitn(12, '\t').collect();
        if columns.len() != 12 {
            return Err(malformed("columns"));
        }
        let number = |i: usize| columns[i].parse::<u32>().map_err(|_| malformed("number"));
        let level = number(0)?;
        if !(1..=5).contains(&level) {
            return Err(malformed("level"));
        }
        let confidence: f64 = columns[10].parse().map_err(|_| malformed("confidence"))?;
        if !confidence.is_finite() || !(-1.0..=100.0).contains(&confidence) {
            return Err(malformed("confidence"));
        }
        let word = columns[11];
        if level != 5 || confidence < 0.0 || word.trim().is_empty() {
            continue;
        }
        let (left, top, width, height) = (number(6)?, number(7)?, number(8)?, number(9)?);
        let right = left.checked_add(width).ok_or_else(|| malformed("bounds"))?;
        let bottom = top.checked_add(height).ok_or_else(|| malformed("bounds"))?;
        let key = (number(2)?, number(3)?, number(4)?);
        let word = OcrWord {
            text: word.to_owned(),
            confidence: confidence / 100.0,
            bounds: to_source(left, top, right, bottom, scale),
        };
        match regions.last_mut() {
            Some(r) if (r.block, r.paragraph, r.line) == key => {
                let b = boxes.last_mut().unwrap();
                *b = (b.0.min(left), b.1.min(top), b.2.max(right), b.3.max(bottom));
                r.words.push(word);
            }
            _ => {
                if regions
                    .iter()
                    .any(|r| (r.block, r.paragraph, r.line) == key)
                {
                    return Err(malformed("line_order"));
                }
                boxes.push((left, top, right, bottom));
                regions.push(OcrRegion {
                    block: key.0,
                    paragraph: key.1,
                    line: key.2,
                    text: String::new(),
                    confidence: 0.0,
                    bounds: Bounds {
                        left: 0,
                        top: 0,
                        width: 0,
                        height: 0,
                    },
                    words: vec![word],
                });
            }
        }
    }
    for (region, (left, top, right, bottom)) in regions.iter_mut().zip(boxes) {
        region.bounds = to_source(left, top, right, bottom, scale);
        region.text = join_words(&region.words);
        region.confidence =
            region.words.iter().map(|w| w.confidence).sum::<f64>() / region.words.len() as f64;
    }
    Ok(regions)
}

fn to_source(left: u32, top: u32, right: u32, bottom: u32, scale: u32) -> Bounds {
    let (left, top) = (left / scale, top / scale);
    Bounds {
        left,
        top,
        width: right.div_ceil(scale) - left,
        height: bottom.div_ceil(scale) - top,
    }
}

/// CJK engines emit one word per character; only join non-CJK neighbours with a space.
fn join_words(words: &[OcrWord]) -> String {
    let mut text = String::new();
    for word in words {
        let separate = matches!(
            (text.chars().last(), word.text.chars().next()),
            (Some(a), Some(b)) if !(is_cjk(a) && is_cjk(b))
        );
        if separate {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    text
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_lines_map_back_to_source_coordinates() {
        let tsv = format!(
            "{TSV_HEADER}\n1\t1\t0\t0\t0\t0\t0\t0\t90\t30\t-1\t\n\
             5\t1\t1\t1\t1\t1\t3\t6\t9\t9\t90\t日\n\
             5\t1\t1\t1\t1\t2\t12\t6\t9\t10\t70\t本\n\
             5\t1\t1\t1\t1\t3\t24\t6\t12\t9\t80\tOK\n\
             5\t1\t1\t1\t2\t1\t3\t20\t7\t5\t50\tnext\n"
        );
        let regions = parse_tsv(tsv.as_bytes(), 3).unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].text, "日本 OK");
        assert_eq!(
            regions[0].bounds,
            Bounds {
                left: 1,
                top: 2,
                width: 11,
                height: 4
            }
        );
        assert!((regions[0].confidence - 0.8).abs() < 1e-9);
        assert_eq!(
            regions[1].words[0].bounds,
            Bounds {
                left: 1,
                top: 6,
                width: 3,
                height: 3
            }
        );
    }

    #[test]
    fn malformed_tsv_is_rejected_not_truncated() {
        for bad in [
            "nope\n".to_owned(),
            format!("{TSV_HEADER}\n5\t1\t1\t1\t1\t1\t0\t0\t1\t1\tx\tw\n"),
            format!("{TSV_HEADER}\n5\t1\t1\t1\t1\n"),
            format!("{TSV_HEADER}\n9\t1\t1\t1\t1\t1\t0\t0\t1\t1\t5\tw\n"),
            format!(
                "{TSV_HEADER}\n5\t1\t1\t1\t1\t1\t0\t0\t1\t1\t5\ta\n\
                 5\t1\t1\t1\t2\t1\t0\t0\t1\t1\t5\tb\n5\t1\t1\t1\t1\t2\t0\t0\t1\t1\t5\tc\n"
            ),
        ] {
            assert!(matches!(
                parse_tsv(bad.as_bytes(), 1),
                Err(OcrError::OcrOutputMalformed { .. })
            ));
        }
    }

    #[test]
    fn language_list_requires_absolute_directory() {
        let (dir, langs) = parse_language_list(
            b"List of available languages in \"/x/tessdata/\" (2):\neng\njpn\n",
        )
        .unwrap();
        assert_eq!(
            (dir, langs),
            (
                PathBuf::from("/x/tessdata/"),
                vec!["eng".into(), "jpn".into()]
            )
        );
        assert!(parse_language_list(b"List of available languages in \"rel\" (0):\n").is_err());
    }
}
