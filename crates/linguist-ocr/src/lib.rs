//! Cancellable Tesseract process boundary; image preprocessing stays caller-owned.
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};
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
    pub fn cancel(child: &mut Child) -> Result<(), OcrError> {
        child.kill().map_err(|e| OcrError::Spawn(e.to_string()))?;
        let _ = child.wait();
        Err(OcrError::Cancelled)
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
    #[test]
    fn parses_tesseract_language_listing() {
        assert_eq!(
            parse_languages("List of available languages in x (2):\neng\njpn\n"),
            ["eng", "jpn"]
        );
    }
}
