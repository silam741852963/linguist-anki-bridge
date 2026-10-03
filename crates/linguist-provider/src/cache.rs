//! Disposable provider response cache under `storage.cache_dir`.
//!
//! Entries are keyed by service, URL, accepted types and client identity, and
//! carry provenance (URL, final URL, MIME, fetch time, SHA-256). Corrupt or
//! expired entries are misses. Accepted plan assets are archived separately and
//! never depend on this cache.
use crate::{Fetched, Service, sha256_hex, unix_now};
use linguist_config::Effective;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

pub const SETTINGS: [&str; 2] = ["cache.policy", "cache.ttl_hours"];
const ENTRY_LIMIT: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Fetch; serve an unexpired entry only after a transient read failure.
    PreferFresh,
    /// Serve an unexpired entry; fetch on miss.
    PreferCache,
    /// Never contact the service; a miss is an error.
    CacheOnly,
}

#[derive(Debug, Clone)]
pub struct Cache {
    pub root: PathBuf,
    pub policy: Policy,
    pub ttl_seconds: u64,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    schema: String,
    url: String,
    final_url: String,
    mime: String,
    fetched_at: u64,
    sha256: String,
    bytes: u64,
}

/// Request fingerprint used as the cache key.
pub fn request_key(service: Service, url: &url::Url, accept: &[&str], user_agent: &str) -> String {
    let mut accept = accept.to_vec();
    accept.sort_unstable();
    let material = serde_json::json!({
        "schema": "linguist-provider-cache-v1",
        "service": service.name(),
        "url": url.as_str(),
        "accept": accept,
        "user_agent": user_agent,
    });
    sha256_hex(material.to_string().as_bytes())
}

impl Cache {
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
        service: Service,
    ) -> Result<Self, crate::ReadError> {
        let v = &settings.values;
        let policy = match v["cache.policy"].as_str() {
            Some("prefer_fresh") => Policy::PreferFresh,
            Some("prefer_cache") => Policy::PreferCache,
            Some("cache_only") => Policy::CacheOnly,
            _ => return Err(crate::ReadError::Policy),
        };
        let base = linguist_config::expand_path(
            v["storage.cache_dir"]
                .as_str()
                .ok_or(crate::ReadError::Policy)?,
            environment,
        )
        .map_err(|_| crate::ReadError::Policy)?;
        if !base.is_absolute() {
            return Err(crate::ReadError::Policy);
        }
        Ok(Self {
            root: base.join("provider-v1").join(service.name()),
            policy,
            ttl_seconds: v["cache.ttl_hours"].as_u64().unwrap() * 3600,
        })
    }

    fn paths(&self, key: &str) -> (PathBuf, PathBuf) {
        (
            self.root.join(format!("{key}.json")),
            self.root.join(format!("{key}.body")),
        )
    }

    /// Return an unexpired, intact entry for exactly this URL.
    pub fn load(&self, key: &str, url: &url::Url) -> Option<Fetched> {
        let (meta, body) = self.paths(key);
        let entry: Entry = serde_json::from_slice(&read_bounded(&meta, 64 * 1024)?).ok()?;
        let now = unix_now();
        if entry.schema != "linguist-provider-cache-v1"
            || entry.url != url.as_str()
            || entry.fetched_at > now
            || now - entry.fetched_at >= self.ttl_seconds
        {
            return None;
        }
        let bytes = read_bounded(&body, ENTRY_LIMIT)?;
        if bytes.len() as u64 != entry.bytes || sha256_hex(&bytes) != entry.sha256 {
            return None;
        }
        Some(Fetched {
            url: url.clone(),
            final_url: entry.final_url.parse().ok()?,
            mime: entry.mime,
            bytes,
            fetched_at: entry.fetched_at,
            from_cache: true,
        })
    }

    /// Publish body then metadata with private permissions and atomic renames.
    pub fn store(&self, key: &str, fetched: &Fetched) -> std::io::Result<()> {
        if self.ttl_seconds == 0 || fetched.bytes.len() as u64 > ENTRY_LIMIT {
            return Ok(());
        }
        create_private_dirs(&self.root)?;
        let (meta, body) = self.paths(key);
        write_atomic(&body, &fetched.bytes)?;
        let entry = Entry {
            schema: "linguist-provider-cache-v1".into(),
            url: fetched.url.to_string(),
            final_url: fetched.final_url.to_string(),
            mime: fetched.mime.clone(),
            fetched_at: fetched.fetched_at,
            sha256: sha256_hex(&fetched.bytes),
            bytes: fetched.bytes.len() as u64,
        };
        write_atomic(
            &meta,
            &serde_json::to_vec(&entry).map_err(std::io::Error::other)?,
        )
    }
}

fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

fn create_private_dirs(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
