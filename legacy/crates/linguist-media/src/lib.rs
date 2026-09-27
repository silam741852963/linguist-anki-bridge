//! Safe image candidate validation and reproducible RGB JPEG normalization.

use image::{
    DynamicImage, ImageEncoder, Rgba, RgbaImage, codecs::jpeg::JpegEncoder, imageops::overlay,
};
use serde::Deserialize;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php";
const JAPANESE_WIKIPEDIA_API: &str = "https://ja.wikipedia.org/w/api.php";
const ENGLISH_WIKIPEDIA_API: &str = "https://en.wikipedia.org/w/api.php";
const COMMONS_MEDIA_HOST: &str = "upload.wikimedia.org";
const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 24_000_000;

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

/// Search Japanese and English article images before Commons media files.
#[derive(Clone)]
pub struct WikipediaAndCommons {
    client: reqwest::Client,
    japanese_endpoint: reqwest::Url,
    english_endpoint: reqwest::Url,
    commons: WikimediaCommons,
}

impl WikipediaAndCommons {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent("linguist-anki-bridge/0.1 image-discovery")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            japanese_endpoint: reqwest::Url::parse(JAPANESE_WIKIPEDIA_API)
                .map_err(|error| error.to_string())?,
            english_endpoint: reqwest::Url::parse(ENGLISH_WIKIPEDIA_API)
                .map_err(|error| error.to_string())?,
            commons: WikimediaCommons::new()?,
        })
    }

    async fn search_articles(
        &self,
        endpoint: &reqwest::Url,
        query: &str,
        provider: &str,
    ) -> Result<Vec<ImageCandidate>, String> {
        let response = self
            .client
            .get(endpoint.clone())
            .query(&[
                ("action", "query"),
                ("format", "json"),
                ("formatversion", "2"),
                ("generator", "search"),
                ("gsrsearch", query),
                ("gsrlimit", "6"),
                ("prop", "pageimages"),
                ("piprop", "thumbnail|original"),
                ("pithumbsize", "640"),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let payload: WikipediaResponse =
            response.json().await.map_err(|error| error.to_string())?;
        Ok(parse_wikipedia(payload, provider))
    }
}

impl ImageSearchPort for WikipediaAndCommons {
    fn search<'a>(&'a self, query: &'a str) -> SearchFuture<'a> {
        Box::pin(async move {
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }
            let (japanese, english, commons) = tokio::join!(
                self.search_articles(&self.japanese_endpoint, query, "wikipedia-ja"),
                self.search_articles(&self.english_endpoint, query, "wikipedia-en"),
                self.commons.search_commons(query),
            );
            combine_search_results([japanese, english, commons])
        })
    }
}

fn combine_search_results(
    results: [Result<Vec<ImageCandidate>, String>; 3],
) -> Result<Vec<ImageCandidate>, String> {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok(found) => candidates.extend(found),
            Err(error) => errors.push(error),
        }
    }
    if candidates.is_empty() && errors.len() == 3 {
        Err(errors.join("; "))
    } else {
        Ok(candidates)
    }
}

#[derive(Default, Deserialize)]
struct WikipediaResponse {
    #[serde(default)]
    query: WikipediaQuery,
}
#[derive(Default, Deserialize)]
struct WikipediaQuery {
    #[serde(default)]
    pages: Vec<WikipediaPage>,
}
#[derive(Deserialize)]
struct WikipediaPage {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    thumbnail: Option<WikipediaImage>,
    #[serde(default)]
    original: Option<WikipediaImage>,
}
#[derive(Deserialize)]
struct WikipediaImage {
    source: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

fn parse_wikipedia(payload: WikipediaResponse, provider: &str) -> Vec<ImageCandidate> {
    let mut pages = payload.query.pages;
    pages.sort_by_key(|page| page.index.unwrap_or(u32::MAX));
    pages
        .into_iter()
        .filter_map(|page| {
            let image = page.thumbnail.or(page.original)?;
            let url = reqwest::Url::parse(&image.source).ok()?;
            if url.scheme() != "https" || url.host_str() != Some(COMMONS_MEDIA_HOST) {
                return None;
            }
            let mime = mime_from_path(url.path())?;
            Some(ImageCandidate {
                url: image.source,
                provider: provider.into(),
                mime: mime.into(),
                width: image.width,
                height: image.height,
            })
        })
        .collect()
}

fn mime_from_path(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// Wikimedia Commons bitmap search and bounded media fetcher.
#[derive(Clone)]
pub struct WikimediaCommons {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    media_host: String,
    max_bytes: usize,
    search_limit: usize,
}

impl WikimediaCommons {
    pub fn new() -> Result<Self, String> {
        Self::with_endpoint(COMMONS_API, COMMONS_MEDIA_HOST)
    }

    fn with_endpoint(endpoint: &str, media_host: &str) -> Result<Self, String> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|error| error.to_string())?;
        if endpoint.scheme() != "https" || endpoint.host_str().is_none() {
            return Err("Wikimedia endpoint must be an HTTPS URL".into());
        }
        if media_host.trim().is_empty() {
            return Err("Wikimedia media host must not be empty".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent("linguist-anki-bridge/0.1 image-discovery")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            endpoint,
            media_host: media_host.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            search_limit: 10,
        })
    }

    fn trusted_media_url(&self, value: &str) -> Result<reqwest::Url, String> {
        let url = reqwest::Url::parse(value).map_err(|error| error.to_string())?;
        if url.scheme() != "https" || url.host_str() != Some(self.media_host.as_str()) {
            return Err("untrusted Wikimedia media URL".into());
        }
        Ok(url)
    }

    async fn search_commons(&self, query: &str) -> Result<Vec<ImageCandidate>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .client
            .get(self.endpoint.clone())
            .query(&[
                ("action", "query".to_owned()),
                ("format", "json".to_owned()),
                ("formatversion", "2".to_owned()),
                ("generator", "search".to_owned()),
                ("gsrsearch", format!("{} filetype:bitmap", query.trim())),
                ("gsrnamespace", "6".to_owned()),
                ("gsrlimit", self.search_limit.to_string()),
                ("prop", "imageinfo".to_owned()),
                ("iiprop", "url|mime|size".to_owned()),
                ("iiurlwidth", "640".to_owned()),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let payload: CommonsResponse = response.json().await.map_err(|error| error.to_string())?;
        Ok(parse_commons(payload))
    }

    async fn fetch_media(&self, candidate: &ImageCandidate) -> Result<Vec<u8>, String> {
        let url = self.trusted_media_url(&candidate.url)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if response.status().is_redirection() {
            return Err("Wikimedia media redirect rejected".into());
        }
        let mut response = response
            .error_for_status()
            .map_err(|error| error.to_string())?;
        self.trusted_media_url(response.url().as_str())?;
        if response
            .content_length()
            .is_some_and(|length| length > self.max_bytes as u64)
        {
            return Err(format!("image exceeds {} byte limit", self.max_bytes));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
            if bytes.len().saturating_add(chunk.len()) > self.max_bytes {
                return Err(format!("image exceeds {} byte limit", self.max_bytes));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

impl ImageSearchPort for WikimediaCommons {
    fn search<'a>(&'a self, query: &'a str) -> SearchFuture<'a> {
        Box::pin(async move { self.search_commons(query).await })
    }
}

impl ImageFetchPort for WikimediaCommons {
    fn fetch<'a>(&'a self, candidate: &'a ImageCandidate) -> FetchFuture<'a> {
        Box::pin(async move { self.fetch_media(candidate).await })
    }
}

/// Retains normalized candidates until semantic adjudication is available.
pub struct ConservativeClassifier;
impl ImageClassifierPort for ConservativeClassifier {
    fn classify<'a>(
        &'a self,
        _: &'a str,
        _: &'a ImageCandidate,
        _: &'a [u8],
    ) -> ClassifyFuture<'a> {
        Box::pin(async { Ok(ImageClass::Uncertain) })
    }
}

#[derive(Deserialize)]
struct CommonsResponse {
    #[serde(default)]
    query: CommonsQuery,
}
#[derive(Default, Deserialize)]
struct CommonsQuery {
    #[serde(default)]
    pages: Vec<CommonsPage>,
}
#[derive(Deserialize)]
struct CommonsPage {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    imageinfo: Vec<CommonsImageInfo>,
}
#[derive(Deserialize)]
struct CommonsImageInfo {
    url: String,
    #[serde(default)]
    thumburl: Option<String>,
    mime: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    thumbwidth: Option<u32>,
    #[serde(default)]
    thumbheight: Option<u32>,
}

fn parse_commons(payload: CommonsResponse) -> Vec<ImageCandidate> {
    let mut pages = payload.query.pages;
    pages.sort_by_key(|page| page.index.unwrap_or(u32::MAX));
    pages
        .into_iter()
        .filter_map(|page| page.imageinfo.into_iter().next())
        .map(|info| ImageCandidate {
            url: info.thumburl.unwrap_or(info.url),
            provider: "wikimedia-commons".into(),
            mime: info.mime,
            width: info.thumbwidth.or(info.width),
            height: info.thumbheight.or(info.height),
        })
        .collect()
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
    let dimensions = image::io::Reader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageError::Decode(e.to_string()))?
        .into_dimensions()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_IMAGE_PIXELS {
        return Err(ImageError::Decode(
            "image dimensions exceed 24 megapixels".into(),
        ));
    }
    let image = image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
    let sample = image.thumbnail(256, 256).to_rgba8();
    let mut sample_background =
        RgbaImage::from_pixel(sample.width(), sample.height(), Rgba([255, 255, 255, 255]));
    overlay(&mut sample_background, &sample, 0, 0);
    let quality = image_quality(&DynamicImage::ImageRgba8(sample_background));
    if quality.mean_light < 28.0 || quality.dark_ratio > 0.82 || quality.contrast < 4.0 {
        return Err(ImageError::LowQuality);
    }
    let image = image.to_rgba8();
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

    #[test]
    fn wikipedia_candidates_keep_search_order_and_only_trusted_bitmaps() {
        let payload: WikipediaResponse = serde_json::from_str(
            r#"{"query":{"pages":[
                {"index":2,"thumbnail":{"source":"https://upload.wikimedia.org/b.jpg","width":640,"height":480}},
                {"index":1,"thumbnail":{"source":"https://upload.wikimedia.org/a.png","width":320,"height":200}},
                {"index":3,"original":{"source":"https://evil.test/c.jpg"}},
                {"index":4,"original":{"source":"https://upload.wikimedia.org/d.svg"}}
            ]}}"#,
        )
        .unwrap();
        let found = parse_wikipedia(payload, "wikipedia-ja");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].url, "https://upload.wikimedia.org/a.png");
        assert_eq!(found[0].mime, "image/png");
        assert_eq!(found[1].url, "https://upload.wikimedia.org/b.jpg");
    }

    #[test]
    fn image_search_uses_healthy_fallback_when_other_sources_fail() {
        let fallback = ImageCandidate {
            url: "https://upload.wikimedia.org/fallback.jpg".into(),
            provider: "wikimedia-commons".into(),
            mime: "image/jpeg".into(),
            width: Some(640),
            height: Some(480),
        };
        assert_eq!(
            combine_search_results([
                Err("ja unavailable".into()),
                Ok(Vec::new()),
                Ok(vec![fallback.clone()]),
            ])
            .unwrap(),
            vec![fallback]
        );
        assert!(
            combine_search_results([
                Err("ja unavailable".into()),
                Err("en unavailable".into()),
                Err("commons unavailable".into()),
            ])
            .unwrap_err()
            .contains("commons unavailable")
        );
    }

    #[test]
    fn parses_commons_imageinfo_and_prefers_thumbnail() {
        let payload: CommonsResponse = serde_json::from_str(
            r#"{
              "query": {"pages": [
                {"index": 2, "imageinfo": [{
                  "url": "https://upload.wikimedia.org/original.png",
                  "thumburl": "https://upload.wikimedia.org/thumb.jpg",
                  "mime": "image/png",
                  "width": 2400,
                  "height": 1600,
                  "thumbwidth": 1200,
                  "thumbheight": 800
                }]},
                {"index": 1, "imageinfo": [{"url": "https://upload.wikimedia.org/first.jpg", "mime": "image/jpeg"}]},
                {"index": 3, "imageinfo": []}
              ]}
            }"#,
        )
        .unwrap();
        assert_eq!(
            parse_commons(payload),
            vec![
                ImageCandidate {
                    url: "https://upload.wikimedia.org/first.jpg".into(),
                    provider: "wikimedia-commons".into(),
                    mime: "image/jpeg".into(),
                    width: None,
                    height: None,
                },
                ImageCandidate {
                    url: "https://upload.wikimedia.org/thumb.jpg".into(),
                    provider: "wikimedia-commons".into(),
                    mime: "image/png".into(),
                    width: Some(1200),
                    height: Some(800),
                },
            ]
        );
    }

    #[test]
    fn rejects_insecure_endpoint_and_untrusted_media() {
        assert!(
            WikimediaCommons::with_endpoint("http://example.test/api", "example.test").is_err()
        );
        let commons = WikimediaCommons::new().unwrap();
        assert!(
            commons
                .trusted_media_url("http://upload.wikimedia.org/a.jpg")
                .is_err()
        );
        assert!(
            commons
                .trusted_media_url("https://upload.wikimedia.org.evil.test/a.jpg")
                .is_err()
        );
        assert!(
            commons
                .trusted_media_url("https://upload.wikimedia.org/a.jpg")
                .is_ok()
        );
    }

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
        let mut image = RgbaImage::from_pixel(320, 200, Rgba([255, 255, 255, 255]));
        for y in 0..20 {
            for x in 0..20 {
                image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&image, 320, 200, image::ColorType::Rgba8)
            .unwrap();
        let (jpeg, _) = normalize_jpeg(&png, "image/png").unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 200));
        let filename = indexed_filename("wiki", "食べる!");
        assert!(filename.starts_with("wiki-media-"));
        assert!(filename.ends_with(".jpg"));
    }
    #[test]
    fn evaluates_transparency_against_white_background() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 0]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&image, 20, 20, image::ColorType::Rgba8)
            .unwrap();
        let (jpeg, quality) = normalize_jpeg(&png, "image/png").unwrap();
        assert!(quality.mean_light > 250.0);
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
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
