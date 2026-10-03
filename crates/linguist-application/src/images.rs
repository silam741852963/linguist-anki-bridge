//! Image candidates from Wikimedia Commons (`images.provider=wikimedia`).
//!
//! Candidates are evidence for review: rights and relevance are never accepted
//! automatically. Each downloaded thumbnail is decoded under the media limits and
//! kept with its title, page, license and attribution metadata.
use linguist_config::Effective;
use linguist_provider::{ReadError, Reader, Service, sha256_hex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const API: &str = "https://commons.wikimedia.org/w/api.php";
pub const HOSTS: [&str; 3] = [
    "commons.wikimedia.org",
    "upload.wikimedia.org",
    "thumb.wikimedia.org",
];
const THUMB_WIDTH: u32 = 640;
const SETTINGS: [&str; 5] = [
    "images.provider",
    "images.search_when_missing",
    "images.query_suffix",
    "images.candidate_limit",
    "images.custom_endpoint",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImageSearchError {
    InvalidSettings {
        message: String,
    },
    /// The selected provider has no adapter in this build.
    ImageProviderUnavailable {
        provider: String,
    },
    ImageSearchDisabled,
    ImageQueryInvalid,
    ImageSearchRead {
        error: String,
    },
    ImageSearchSchema,
}
impl std::fmt::Display for ImageSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = serde_json::to_value(self)
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned));
        f.write_str(&code.unwrap_or_default())
    }
}
impl std::error::Error for ImageSearchError {}

#[derive(Debug, Clone, Serialize)]
pub struct ImageCandidate {
    pub provider: &'static str,
    pub title: String,
    pub page_url: String,
    pub original_url: String,
    pub source_url: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub size_bytes: u64,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub artist: Option<String>,
    pub credit: Option<String>,
    pub attribution_required: Option<bool>,
    pub usage_terms: Option<String>,
    pub description: Option<String>,
    pub fetched_at: u64,
    pub from_cache: bool,
    /// Rights and relevance always need a reviewer decision.
    pub review_required: bool,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RejectedCandidate {
    pub title: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct ImageSearch {
    pub query: String,
    pub request_url: String,
    pub response_sha256: String,
    pub candidates: Vec<ImageCandidate>,
    pub rejected: Vec<RejectedCandidate>,
    /// Exact search response bytes for archival.
    #[serde(skip)]
    pub response: Vec<u8>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    query: Option<Query>,
}
#[derive(Deserialize)]
struct Query {
    pages: Vec<Page>,
}
#[derive(Deserialize)]
struct Page {
    title: String,
    index: u32,
    #[serde(default)]
    imageinfo: Vec<Info>,
}
#[derive(Deserialize)]
struct Info {
    url: String,
    descriptionurl: String,
    #[serde(default)]
    thumburl: Option<String>,
    #[serde(default)]
    extmetadata: BTreeMap<String, Meta>,
}
#[derive(Deserialize)]
struct Meta {
    value: serde_json::Value,
}

/// Visible text from provider HTML metadata; markup and scripts are dropped.
fn plain(html: &str) -> String {
    let fragment = scraper::Html::parse_fragment(html);
    let mut parts = Vec::new();
    for node in fragment.root_element().descendants() {
        if let Some(text) = node.value().as_text() {
            let hidden = node.ancestors().any(|a| {
                a.value()
                    .as_element()
                    .is_some_and(|e| matches!(e.name(), "script" | "style"))
            });
            if !hidden {
                parts.push(&**text);
            }
        }
    }
    parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct ImageSearchClient {
    reader: Reader,
    limit: u64,
    suffix: String,
    settings: Effective,
}

impl ImageSearchClient {
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, ImageSearchError> {
        let registry = linguist_config::Registry::builtin();
        for key in SETTINGS {
            registry
                .validate_value(
                    key,
                    settings
                        .values
                        .get(key)
                        .ok_or(ImageSearchError::ImageSearchSchema)?,
                )
                .map_err(|message| ImageSearchError::InvalidSettings { message })?;
        }
        match settings.values["images.provider"].as_str() {
            Some("wikimedia") => {}
            Some("disabled") => return Err(ImageSearchError::ImageSearchDisabled),
            other => {
                return Err(ImageSearchError::ImageProviderUnavailable {
                    provider: other.unwrap_or_default().into(),
                });
            }
        }
        let reader = Reader::from_settings(settings, environment, Service::Image, &HOSTS, &[])
            .map_err(|e| ImageSearchError::ImageSearchRead {
                error: e.to_string(),
            })?;
        Ok(Self::with_reader(reader, settings))
    }

    pub fn with_reader(reader: Reader, settings: &Effective) -> Self {
        Self {
            reader,
            limit: settings.values["images.candidate_limit"]
                .as_u64()
                .unwrap_or(5),
            suffix: settings.values["images.query_suffix"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            settings: settings.clone(),
        }
    }

    pub fn search_url(&self, base: &url::Url, query: &str) -> url::Url {
        let mut url = base.clone();
        url.query_pairs_mut()
            .append_pair("action", "query")
            .append_pair("format", "json")
            .append_pair("formatversion", "2")
            .append_pair("generator", "search")
            .append_pair("gsrsearch", &format!("{query} filetype:bitmap"))
            .append_pair("gsrnamespace", "6")
            .append_pair("gsrlimit", &self.limit.to_string())
            .append_pair("prop", "imageinfo")
            .append_pair("iiprop", "url|mime|extmetadata")
            .append_pair("iiurlwidth", &THUMB_WIDTH.to_string())
            .append_pair(
                "iiextmetadatafilter",
                "LicenseShortName|LicenseUrl|Artist|Credit|AttributionRequired|UsageTerms|ImageDescription",
            );
        url
    }

    /// Search and download candidates in provider order.
    pub fn search(&self, expression: &str) -> Result<ImageSearch, ImageSearchError> {
        self.search_at(&url::Url::parse(API).expect("static URL"), expression)
    }

    pub fn search_at(
        &self,
        base: &url::Url,
        expression: &str,
    ) -> Result<ImageSearch, ImageSearchError> {
        let expression = expression.trim();
        if expression.is_empty()
            || expression.chars().count() > 200
            || expression.chars().any(char::is_control)
        {
            return Err(ImageSearchError::ImageQueryInvalid);
        }
        let query = if self.suffix.trim().is_empty() {
            expression.to_owned()
        } else {
            format!("{expression} {}", self.suffix.trim())
        };
        let url = self.search_url(base, &query);
        let read = |e: ReadError| ImageSearchError::ImageSearchRead {
            error: e.to_string(),
        };
        let fetched = self.reader.get(&url, &["application/json"]).map_err(read)?;
        let response: Response = serde_json::from_slice(&fetched.bytes)
            .map_err(|_| ImageSearchError::ImageSearchSchema)?;
        let mut pages = response.query.map(|q| q.pages).unwrap_or_default();
        if pages.len() as u64 > self.limit {
            return Err(ImageSearchError::ImageSearchSchema);
        }
        pages.sort_by_key(|p| p.index);
        let allowed: Vec<&str> = self.settings.values["media.allowed_image_types"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let mut result = ImageSearch {
            query,
            request_url: url.to_string(),
            response_sha256: sha256_hex(&fetched.bytes),
            candidates: Vec::new(),
            rejected: Vec::new(),
            response: fetched.bytes,
        };
        for page in pages {
            let mut reject = |code: String| {
                result.rejected.push(RejectedCandidate {
                    title: page.title.clone(),
                    code,
                })
            };
            if !page.title.starts_with("File:") {
                return Err(ImageSearchError::ImageSearchSchema);
            }
            let Some(info) = page.imageinfo.first() else {
                reject("IMAGE_CANDIDATE_METADATA_MISSING".into());
                continue;
            };
            let source = info.thumburl.as_deref().unwrap_or(&info.url);
            let Ok(source_url) = url::Url::parse(source) else {
                reject("IMAGE_CANDIDATE_URL_INVALID".into());
                continue;
            };
            // Downloads use the same allowlist and private-address policy.
            let download = match self.reader.get(&source_url, &allowed) {
                Ok(download) => download,
                Err(error) => {
                    reject(error.to_string());
                    continue;
                }
            };
            let inspection = match crate::media::inspect_image(&download.bytes, &self.settings) {
                Ok(inspection) => inspection,
                Err(error) => {
                    reject(error.to_string());
                    continue;
                }
            };
            let meta = |key: &str| -> Option<String> {
                info.extmetadata
                    .get(key)
                    .and_then(|m| match &m.value {
                        serde_json::Value::String(s) => Some(plain(s)),
                        serde_json::Value::Bool(b) => Some(b.to_string()),
                        serde_json::Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .filter(|s| !s.is_empty())
            };
            result.candidates.push(ImageCandidate {
                provider: "wikimedia_commons",
                title: page.title.clone(),
                page_url: info.descriptionurl.clone(),
                original_url: info.url.clone(),
                source_url: download.final_url.to_string(),
                mime: inspection.mime,
                width: inspection.width,
                height: inspection.height,
                sha256: sha256_hex(&download.bytes),
                size_bytes: download.bytes.len() as u64,
                license: meta("LicenseShortName"),
                license_url: meta("LicenseUrl"),
                artist: meta("Artist"),
                credit: meta("Credit"),
                attribution_required: meta("AttributionRequired").map(|v| v == "true"),
                usage_terms: meta("UsageTerms"),
                description: meta("ImageDescription"),
                fetched_at: download.fetched_at,
                from_cache: download.from_cache,
                review_required: true,
                bytes: download.bytes,
            });
        }
        Ok(result)
    }
}
