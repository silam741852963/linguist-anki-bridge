//! Safe image candidate validation and reproducible RGB JPEG normalization.

use image::{
    DynamicImage, ImageEncoder, Rgba, RgbaImage, codecs::jpeg::JpegEncoder, imageops::overlay,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub type SearchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ImageCandidate>, String>> + Send + 'a>>;
/// Provider discovery is injected. The media core never embeds provider keys,
/// browser automation, or a network client.
pub trait ImageSearchPort: Send + Sync {
    fn search<'a>(&'a self, query: &'a str) -> SearchFuture<'a>;
}
pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
pub trait ImageFetchPort: Send + Sync {
    fn fetch<'a>(&'a self, candidate: &'a ImageCandidate) -> FetchFuture<'a>;
}
pub type ClassifyFuture<'a> = Pin<Box<dyn Future<Output = Result<ImageClass, String>> + Send + 'a>>;
pub trait ImageClassifierPort: Send + Sync {
    fn classify<'a>(
        &'a self,
        query: &'a str,
        candidate: &'a ImageCandidate,
        normalized_jpeg: &'a [u8],
    ) -> ClassifyFuture<'a>;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCandidate {
    pub url: String,
    pub provider: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageClass {
    Dictionary,
    VisualRecall,
    Mixed,
    Uncertain,
    NoImage,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImage {
    pub candidate: ImageCandidate,
    pub filename: String,
    pub jpeg: Vec<u8>,
    pub quality: ImageQuality,
    pub classification: ImageClass,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageDiscovery {
    pub selected: Option<PreparedImage>,
    pub issues: Vec<String>,
    pub retain_existing: bool,
    pub cancelled: bool,
}

pub async fn discover_image<S, F, C>(
    search: &S,
    fetch: &F,
    classifier: &C,
    query: &str,
    existing_class: Option<ImageClass>,
    cancelled: Arc<AtomicBool>,
    max_candidates: usize,
) -> ImageDiscovery
where
    S: ImageSearchPort + ?Sized,
    F: ImageFetchPort + ?Sized,
    C: ImageClassifierPort + ?Sized,
{
    let mut result = ImageDiscovery::default();
    if cancelled.load(Ordering::Relaxed) {
        result.cancelled = true;
        result.retain_existing = true;
        return result;
    }
    let candidates = match search.search(query).await {
        Ok(candidates) => usable_candidates(candidates),
        Err(error) => {
            result.issues.push(format!("Image search: {error}"));
            result.retain_existing = true;
            return result;
        }
    };
    for candidate in candidates.into_iter().take(max_candidates.max(1)) {
        if cancelled.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        let bytes = match fetch.fetch(&candidate).await {
            Ok(bytes) => bytes,
            Err(error) => {
                result
                    .issues
                    .push(format!("Image fetch from {}: {error}", candidate.provider));
                continue;
            }
        };
        let (jpeg, quality) = match normalize_jpeg(&bytes, &candidate.mime) {
            Ok(prepared) => prepared,
            Err(error) => {
                result.issues.push(format!(
                    "Image rejected from {}: {error}",
                    candidate.provider
                ));
                continue;
            }
        };
        let classification = match classifier.classify(query, &candidate, &jpeg).await {
            Ok(classification) => classification,
            Err(error) => {
                result.issues.push(format!("Image classification: {error}"));
                ImageClass::Uncertain
            }
        };
        if classification == ImageClass::NoImage {
            continue;
        }
        result.selected = Some(PreparedImage {
            filename: indexed_filename(&candidate.provider, query),
            candidate,
            jpeg,
            quality,
            classification,
        });
        break;
    }
    result.retain_existing = existing_class
        .is_some_and(|class| visual_recall_retained(class, result.selected.is_some()));
    result
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
    format!(
        "{}-{}-{:08x}.jpg",
        safe_stem(provider),
        safe_stem(query),
        short_hash(query)
    )
}
pub fn cache_key(provider_revision: &str, query: &str) -> String {
    format!(
        "image-v1-{:016x}",
        stable_hash(
            provider_revision
                .as_bytes()
                .iter()
                .copied()
                .chain([0])
                .chain(query.trim().as_bytes().iter().copied())
        )
    )
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
fn short_hash(value: &str) -> u32 {
    stable_hash(value.as_bytes().iter().copied()) as u32
}
fn stable_hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
        let filename = indexed_filename("wiki", "食べる!");
        assert!(filename.starts_with("wiki-media-"));
        assert!(filename.ends_with(".jpg"));
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

    struct Search;
    impl ImageSearchPort for Search {
        fn search<'a>(&'a self, _: &'a str) -> SearchFuture<'a> {
            Box::pin(async {
                Ok(vec![ImageCandidate {
                    url: "https://img.example/food.png".into(),
                    provider: "fixture".into(),
                    mime: "image/png".into(),
                    width: Some(20),
                    height: Some(20),
                }])
            })
        }
    }
    struct Fetch;
    impl ImageFetchPort for Fetch {
        fn fetch<'a>(&'a self, _: &'a ImageCandidate) -> FetchFuture<'a> {
            Box::pin(async {
                let mut image = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
                image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
                let mut bytes = Vec::new();
                image::codecs::png::PngEncoder::new(&mut bytes)
                    .write_image(&image, 20, 20, image::ColorType::Rgba8)
                    .unwrap();
                Ok(bytes)
            })
        }
    }
    struct Classifier;
    impl ImageClassifierPort for Classifier {
        fn classify<'a>(
            &'a self,
            _: &'a str,
            _: &'a ImageCandidate,
            _: &'a [u8],
        ) -> ClassifyFuture<'a> {
            Box::pin(async { Ok(ImageClass::Dictionary) })
        }
    }

    #[tokio::test]
    async fn composes_discovery_fetch_normalization_and_classification() {
        let result = discover_image(
            &Search,
            &Fetch,
            &Classifier,
            "食べる",
            Some(ImageClass::VisualRecall),
            Arc::new(AtomicBool::new(false)),
            3,
        )
        .await;
        let selected = result.selected.unwrap();
        assert!(selected.jpeg.starts_with(&[0xff, 0xd8]));
        assert_eq!(selected.classification, ImageClass::Dictionary);
        assert!(result.retain_existing);
        assert!(result.issues.is_empty());
    }

    #[tokio::test]
    async fn cancellation_never_calls_providers() {
        let result = discover_image(
            &Search,
            &Fetch,
            &Classifier,
            "term",
            None,
            Arc::new(AtomicBool::new(true)),
            3,
        )
        .await;
        assert!(result.cancelled);
        assert!(result.selected.is_none());
        assert!(result.retain_existing);
    }
}
