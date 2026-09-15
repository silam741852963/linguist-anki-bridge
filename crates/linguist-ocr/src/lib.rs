//! Cancellable Tesseract process boundary; image preprocessing stays caller-owned.
use std::{
    io::Cursor,
    path::Path,
    process::{Child, Command, Stdio},
};

use image::{DynamicImage, ImageFormat};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrEvidence {
    pub text: String,
    pub lines: Vec<String>,
    pub languages: Vec<String>,
    pub preprocessed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OcrFailureKind {
    Unavailable,
    InvalidInput,
    Retryable,
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct Tesseract {
    binary: String,
}
#[derive(Debug)]
pub enum OcrError {
    Spawn(String),
    Failed(String),
    Cancelled,
}
impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(x) | Self::Failed(x) => f.write_str(x),
            Self::Cancelled => f.write_str("OCR cancelled"),
        }
    }
}
impl std::error::Error for OcrError {}
impl Tesseract {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
    pub fn languages(&self) -> Result<Vec<String>, OcrError> {
        let out = Command::new(&self.binary)
            .arg("--list-langs")
            .output()
            .map_err(|e| OcrError::Spawn(e.to_string()))?;
        if !out.status.success() {
            return Err(OcrError::Failed(
                String::from_utf8_lossy(&out.stderr).into(),
            ));
        }
        Ok(parse_languages(&String::from_utf8_lossy(&out.stdout)))
    }
    pub fn start(&self, image: &Path, languages: &str) -> Result<Child, OcrError> {
        if languages.trim().is_empty() {
            return Err(OcrError::Failed("OCR language selection is empty".into()));
        }
        Command::new(&self.binary)
            .arg(image)
            .arg("stdout")
            .arg("-l")
            .arg(languages)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Spawn(e.to_string()))
    }

    pub fn finish(
        child: Child,
        languages: &str,
        preprocessed: bool,
    ) -> Result<OcrEvidence, OcrError> {
        let output = child
            .wait_with_output()
            .map_err(|error| OcrError::Failed(error.to_string()))?;
        if !output.status.success() {
            return Err(OcrError::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        let text = normalize_text(&String::from_utf8_lossy(&output.stdout));
        let lines = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(OcrEvidence {
            text,
            lines,
            languages: languages
                .split('+')
                .map(str::trim)
                .filter(|language| !language.is_empty())
                .map(str::to_owned)
                .collect(),
            preprocessed,
        })
    }
    pub fn cancel(child: &mut Child) -> Result<(), OcrError> {
        child.kill().map_err(|e| OcrError::Spawn(e.to_string()))?;
        let _ = child.wait();
        Err(OcrError::Cancelled)
    }
}

pub fn preprocess(bytes: &[u8]) -> Result<Vec<u8>, OcrError> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| OcrError::Failed(format!("invalid OCR image: {error}")))?;
    let grayscale = image.grayscale().adjust_contrast(24.0);
    encode_png(grayscale)
}

fn encode_png(image: DynamicImage) -> Result<Vec<u8>, OcrError> {
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| OcrError::Failed(format!("OCR preprocessing failed: {error}")))?;
    Ok(output.into_inner())
}

pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub fn cache_key(bytes: &[u8], languages: &str, preprocessed: bool) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes
        .iter()
        .copied()
        .chain([0])
        .chain(languages.bytes())
        .chain([u8::from(preprocessed)])
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("tesseract-v1-{hash:016x}")
}

pub fn classify_error(error: &OcrError) -> OcrFailureKind {
    match error {
        OcrError::Cancelled => OcrFailureKind::Cancelled,
        OcrError::Spawn(_) => OcrFailureKind::Unavailable,
        OcrError::Failed(message)
            if message.contains("invalid OCR image")
                || message.contains("language selection is empty") =>
        {
            OcrFailureKind::InvalidInput
        }
        OcrError::Failed(_) => OcrFailureKind::Retryable,
    }
}
pub fn parse_languages(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|line| line.contains("available languages"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};
    #[test]
    fn parses_tesseract_language_listing() {
        assert_eq!(
            parse_languages("List of available languages in x (2):\neng\njpn\n"),
            ["eng", "jpn"]
        );
    }

    #[test]
    fn preprocesses_to_stable_grayscale_png_and_keys_configuration() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_pixel(2, 2, Luma([120])));
        let mut source = Cursor::new(Vec::new());
        image.write_to(&mut source, ImageFormat::Png).unwrap();
        let output = preprocess(source.get_ref()).unwrap();
        let decoded = image::load_from_memory(&output).unwrap();
        assert_eq!(decoded.color(), image::ColorType::L8);
        assert_eq!(
            cache_key(&output, "jpn+eng", true),
            cache_key(&output, "jpn+eng", true)
        );
        assert_ne!(
            cache_key(&output, "jpn", true),
            cache_key(&output, "eng", true)
        );
    }

    #[test]
    fn normalizes_evidence_and_classifies_failures() {
        assert_eq!(
            normalize_text("  食べる  \r\n meaning \r\n"),
            "食べる\nmeaning"
        );
        assert_eq!(
            classify_error(&OcrError::Cancelled),
            OcrFailureKind::Cancelled
        );
        assert_eq!(
            classify_error(&OcrError::Spawn("missing".into())),
            OcrFailureKind::Unavailable
        );
        assert_eq!(
            classify_error(&OcrError::Failed("invalid OCR image: bad".into())),
            OcrFailureKind::InvalidInput
        );
    }
}
