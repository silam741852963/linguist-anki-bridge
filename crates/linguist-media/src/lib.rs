//! Safe image candidate validation and reproducible RGB JPEG normalization.

use image::{
    DynamicImage, ImageEncoder, Rgba, RgbaImage, codecs::jpeg::JpegEncoder, imageops::overlay,
};
use std::{future::Future, pin::Pin};

pub type SearchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ImageCandidate>, String>> + Send + 'a>>;
/// Provider discovery is injected. The media core never embeds provider keys,
/// browser automation, or a network client.
pub trait ImageSearchPort: Send + Sync {
    fn search<'a>(&'a self, query: &'a str) -> SearchFuture<'a>;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCandidate {
    pub url: String,
    pub provider: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageClass {
    Dictionary,
    VisualRecall,
    Mixed,
    Uncertain,
    NoImage,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ImageQuality {
    pub mean_light: f32,
    pub dark_ratio: f32,
    pub contrast: f32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageError {
    UnsupportedMime,
    Decode(String),
    LowQuality,
}
impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedMime => f.write_str("unsupported image type"),
            Self::Decode(e) => f.write_str(e),
            Self::LowQuality => f.write_str("low quality image"),
        }
    }
}
impl std::error::Error for ImageError {}

pub fn supported_mime(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    )
}
pub fn usable_candidates(
    candidates: impl IntoIterator<Item = ImageCandidate>,
) -> Vec<ImageCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| {
            candidate.url.starts_with("https://")
                && supported_mime(&candidate.mime)
                && candidate.width.unwrap_or(1) > 0
                && candidate.height.unwrap_or(1) > 0
        })
        .collect()
}
pub fn visual_recall_retained(classification: ImageClass, replacement_available: bool) -> bool {
    matches!(
        classification,
        ImageClass::VisualRecall | ImageClass::Mixed | ImageClass::Uncertain
    ) || !replacement_available
}
pub fn indexed_filename(provider: &str, query: &str) -> String {
    format!("{}-{}.jpg", safe_stem(provider), safe_stem(query))
}
pub fn normalize_jpeg(bytes: &[u8], mime: &str) -> Result<(Vec<u8>, ImageQuality), ImageError> {
    if !supported_mime(mime) {
        return Err(ImageError::UnsupportedMime);
    }
    let image = image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
    let quality = image_quality(&image);
    if quality.mean_light < 28.0 || quality.dark_ratio > 0.82 || quality.contrast < 4.0 {
        return Err(ImageError::LowQuality);
    }
    let image = image.thumbnail(256, 256).to_rgba8();
    let (width, height) = image.dimensions();
    let mut background = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    overlay(&mut background, &image, 0, 0);
    let rgb = DynamicImage::ImageRgba8(background).to_rgb8();
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 90)
        .write_image(&rgb, width, height, image::ColorType::Rgb8)
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    Ok((output, quality))
}
pub fn image_quality(image: &DynamicImage) -> ImageQuality {
    let luma = image.to_luma8();
    let pixels = luma.as_raw();
    let count = pixels.len().max(1) as f32;
    let mean = pixels.iter().map(|pixel| f32::from(*pixel)).sum::<f32>() / count;
    let variance = pixels
        .iter()
        .map(|pixel| {
            let diff = f32::from(*pixel) - mean;
            diff * diff
        })
        .sum::<f32>()
        / count;
    ImageQuality {
        mean_light: mean,
        dark_ratio: pixels.iter().filter(|pixel| **pixel < 18).count() as f32 / count,
        contrast: variance.sqrt(),
    }
}
fn safe_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        "media".into()
    } else {
        stem.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_image(color: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(20, 20, Rgba(color));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&image, 20, 20, image::ColorType::Rgba8)
            .unwrap();
        bytes
    }
    #[test]
    fn normalizes_supported_image_to_jpeg() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&image, 20, 20, image::ColorType::Rgba8)
            .unwrap();
        let (jpeg, _) = normalize_jpeg(&png, "image/png").unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        assert_eq!(indexed_filename("wiki", "食べる!"), "wiki-media.jpg");
    }
    #[test]
    fn rejects_unusable_and_keeps_visual_recall() {
        assert_eq!(
            normalize_jpeg(&test_image([0, 0, 0, 255]), "image/png"),
            Err(ImageError::LowQuality)
        );
        assert!(visual_recall_retained(ImageClass::VisualRecall, true));
        assert!(!visual_recall_retained(ImageClass::Dictionary, true));
    }
    #[test]
    fn filters_non_image_and_untrusted_candidates() {
        let candidates = usable_candidates(vec![
            ImageCandidate {
                url: "https://img.example/a.jpg".into(),
                provider: "wiki".into(),
                mime: "image/jpeg".into(),
                width: Some(20),
                height: Some(20),
            },
            ImageCandidate {
                url: "https://img.example/a.svg".into(),
                provider: "wiki".into(),
                mime: "image/svg+xml".into(),
                width: Some(20),
                height: Some(20),
            },
            ImageCandidate {
                url: "http://img.example/a.png".into(),
                provider: "wiki".into(),
                mime: "image/png".into(),
                width: Some(20),
                height: Some(20),
            },
        ]);
        assert_eq!(candidates.len(), 1);
    }
}
