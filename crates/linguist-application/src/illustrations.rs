//! Japanese illustration candidates from いらすとや (`images.illustrations=irasutoya`).
//!
//! The site's Blogger feed is searched with the Japanese expression; posts whose
//! title contains it come first. Each candidate is the post's picture at 400px,
//! decoded under the media limits and kept with its post URL and the site's terms.
//! Rights and relevance still need a reviewer decision, as for Commons.
use crate::images::{ImageCandidate, ImageSearch, ImageSearchError, RejectedCandidate};
use linguist_config::Effective;
use linguist_provider::{ReadError, Reader, Service, sha256_hex};
use serde::Deserialize;
use std::collections::BTreeMap;

const FEED: &str = "https://www.irasutoya.com/feeds/posts/summary";
pub const HOSTS: [&str; 2] = ["www.irasutoya.com", "blogger.googleusercontent.com"];
pub const TERMS_URL: &str = "https://www.irasutoya.com/p/terms.html";
const TERMS: &str = "Free for personal and commercial use within いらすとや's terms; copyright is not waived; redistributing the material itself is not allowed; a commercial design with 21 or more items needs a paid licence.";
/// Feed results read before ranking; the candidate limit applies after it.
const FEED_RESULTS: u64 = 20;

#[derive(Deserialize)]
struct Feed {
    feed: Body,
}
#[derive(Deserialize)]
struct Body {
    #[serde(default)]
    entry: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    title: Text,
    #[serde(default)]
    summary: Option<Text>,
    #[serde(default)]
    link: Vec<Link>,
    #[serde(rename = "media$thumbnail", default)]
    thumbnail: Option<Thumbnail>,
}
#[derive(Deserialize)]
struct Text {
    #[serde(rename = "$t")]
    text: String,
}
#[derive(Deserialize)]
struct Link {
    rel: String,
    href: String,
}
#[derive(Deserialize)]
struct Thumbnail {
    url: String,
}

pub struct IllustrationClient {
    reader: Reader,
    limit: u64,
    settings: Effective,
}

impl IllustrationClient {
    /// `Ok(None)` when `images.illustrations=disabled`.
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Option<Self>, ImageSearchError> {
        let registry = linguist_config::Registry::builtin();
        let value = settings
            .values
            .get("images.illustrations")
            .ok_or(ImageSearchError::ImageSearchSchema)?;
        registry
            .validate_value("images.illustrations", value)
            .map_err(|message| ImageSearchError::InvalidSettings { message })?;
        if *value != "irasutoya" {
            return Ok(None);
        }
        let reader = Reader::from_settings(settings, environment, Service::Image, &HOSTS, &[])
            .map_err(|e| ImageSearchError::ImageSearchRead {
                error: e.to_string(),
            })?;
        Ok(Some(Self::with_reader(reader, settings)))
    }

    pub fn with_reader(reader: Reader, settings: &Effective) -> Self {
        Self {
            reader,
            limit: settings.values["images.candidate_limit"]
                .as_u64()
                .unwrap_or(5),
            settings: settings.clone(),
        }
    }

    pub fn search(&self, expression: &str) -> Result<ImageSearch, ImageSearchError> {
        self.search_at(&url::Url::parse(FEED).expect("static URL"), expression)
    }

    pub fn search_at(
        &self,
        base: &url::Url,
        expression: &str,
    ) -> Result<ImageSearch, ImageSearchError> {
        let query = expression.trim();
        if query.is_empty() || query.chars().count() > 200 || query.chars().any(char::is_control) {
            return Err(ImageSearchError::ImageQueryInvalid);
        }
        let mut url = base.clone();
        url.query_pairs_mut()
            .append_pair("alt", "json")
            .append_pair("max-results", &FEED_RESULTS.to_string())
            .append_pair("q", query);
        let read = |e: ReadError| ImageSearchError::ImageSearchRead {
            error: e.to_string(),
        };
        let fetched = self.reader.get(&url, &["application/json"]).map_err(read)?;
        let feed: Feed = serde_json::from_slice(&fetched.bytes)
            .map_err(|_| ImageSearchError::ImageSearchSchema)?;
        if feed.feed.entry.len() as u64 > FEED_RESULTS {
            return Err(ImageSearchError::ImageSearchSchema);
        }
        // The feed also matches post text; a title naming the word ranks first.
        let mut entries = feed.feed.entry;
        entries.sort_by_key(|e| !e.title.text.contains(query));
        let allowed: Vec<&str> = self.settings.values["media.allowed_image_types"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let mut result = ImageSearch {
            query: query.to_owned(),
            request_url: url.to_string(),
            response_sha256: sha256_hex(&fetched.bytes),
            candidates: Vec::new(),
            rejected: Vec::new(),
            response: fetched.bytes,
        };
        for entry in entries {
            if result.candidates.len() as u64 >= self.limit {
                break;
            }
            let title = entry.title.text.trim().to_owned();
            let mut reject = |code: &str| {
                result.rejected.push(RejectedCandidate {
                    title: title.clone(),
                    code: code.into(),
                })
            };
            let page = entry.link.iter().find(|l| l.rel == "alternate");
            let (Some(page), Some(thumbnail)) = (page, entry.thumbnail.as_ref()) else {
                reject("IMAGE_CANDIDATE_METADATA_MISSING");
                continue;
            };
            // Blogger thumbnails are `…/s72-c/name.png`; `s400` is the same picture at 400px.
            let Some(source_url) = thumbnail
                .url
                .contains("/s72-c/")
                .then(|| thumbnail.url.replacen("/s72-c/", "/s400/", 1))
                .and_then(|u| url::Url::parse(&u).ok())
            else {
                reject("IMAGE_CANDIDATE_URL_INVALID");
                continue;
            };
            let download = match self.reader.get(&source_url, &allowed) {
                Ok(download) => download,
                Err(error) => {
                    reject(&error.to_string());
                    continue;
                }
            };
            let inspection = match crate::media::inspect_image(&download.bytes, &self.settings) {
                Ok(inspection) => inspection,
                Err(error) => {
                    reject(&error.to_string());
                    continue;
                }
            };
            result.candidates.push(ImageCandidate {
                provider: "irasutoya",
                title,
                page_url: page.href.clone(),
                original_url: source_url.to_string(),
                source_url: download.final_url.to_string(),
                mime: inspection.mime,
                width: inspection.width,
                height: inspection.height,
                sha256: sha256_hex(&download.bytes),
                size_bytes: download.bytes.len() as u64,
                license: Some("いらすとや terms of use".into()),
                license_url: Some(TERMS_URL.into()),
                artist: Some("みふねたかし (いらすとや)".into()),
                credit: None,
                attribution_required: Some(false),
                usage_terms: Some(TERMS.into()),
                description: entry
                    .summary
                    .map(|s| s.text.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|s| !s.is_empty()),
                fetched_at: download.fetched_at,
                from_cache: download.from_cache,
                review_required: true,
                bytes: download.bytes,
            });
        }
        Ok(result)
    }
}
