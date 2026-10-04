//! Pinned local resources: `resources list` (OP-57) and `resources install`
//! (OP-58). Installation never runs downloaded content, never sets executable
//! bits, never replaces an existing resource and records a receipt with the
//! source, version, license and per-file SHA-256.
use linguist_config::Effective;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const RECEIPT_SCHEMA: u32 = 1;
const MAX_ENTRIES: usize = 10_000;
/// Public download hosts for the supported resource kinds; others need
/// `network.allowed_remote_service_hosts`.
pub const PUBLIC_HOSTS: [&str; 7] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "raw.githubusercontent.com",
    "huggingface.co",
    "cdn-lfs.hf.co",
    "cdn-lfs-us-1.hf.co",
];
/// SPDX identifiers accepted for installed resources.
pub const LICENSES: [&str; 15] = [
    "Apache-2.0",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MPL-2.0",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "Unlicense",
    "LGPL-2.1-only",
    "LGPL-3.0-only",
    "GPL-2.0-only",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// One `NAME.traineddata` file in the shared tessdata directory.
    Tesseract,
    /// A Piper voice (single file or archive) in its own versioned directory.
    Piper,
    /// A prompt, schema or other data file in its own versioned directory.
    File,
    /// An Ollama model pulled through the configured local Ollama service.
    Ollama,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Tesseract => "tesseract",
            Self::Piper => "piper",
            Self::File => "file",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceId {
    pub kind: Kind,
    pub name: String,
}

impl std::str::FromStr for ResourceId {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let (kind, name) = text
            .split_once(':')
            .ok_or("RESOURCE_ID_INVALID: use KIND:NAME (tesseract, piper, file, ollama)")?;
        let kind = match kind {
            "tesseract" => Kind::Tesseract,
            "piper" => Kind::Piper,
            "file" | "prompt" | "schema" => Kind::File,
            "ollama" => Kind::Ollama,
            _ => return Err(format!("RESOURCE_KIND_UNSUPPORTED: {kind}")),
        };
        let extra = if kind == Kind::Ollama { ":/" } else { "" };
        if name.is_empty()
            || name.len() > 128
            || name.starts_with(['.', '-'])
            || name.contains("..")
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c) || extra.contains(c))
        {
            return Err("RESOURCE_NAME_INVALID".into());
        }
        Ok(Self {
            kind,
            name: name.to_owned(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReceiptFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub schema_version: u32,
    pub resource: String,
    pub kind: Kind,
    pub name: String,
    pub version: String,
    pub license: String,
    /// Credential-free source description (URL, local path or `ollama-registry`).
    pub source: String,
    pub final_source: Option<String>,
    pub sha256: String,
    pub downloaded: bool,
    pub archive: bool,
    pub install_path: String,
    pub files: Vec<ReceiptFile>,
    pub total_bytes: u64,
    pub installed_unix_seconds: u64,
}

pub struct InstallRequest {
    pub resource: String,
    pub source: String,
    pub version: String,
    pub sha256: String,
    pub license: String,
    pub destination: Option<PathBuf>,
    pub execute: bool,
}

pub(crate) fn resource_root(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let root = linguist_config::expand_path(
        settings.values["storage.resource_dir"]
            .as_str()
            .ok_or("RESOURCE_SETTING_MISSING")?,
        environment,
    )?;
    if !root.is_absolute() {
        return Err("RESOURCE_DIR_MUST_BE_ABSOLUTE".into());
    }
    Ok(root)
}

fn private_dir(path: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| "RESOURCE_DIRECTORY_IO")?;
    let meta = std::fs::symlink_metadata(path).map_err(|_| "RESOURCE_DIRECTORY_IO")?;
    if !meta.is_dir() || meta.is_symlink() {
        return Err("RESOURCE_UNSAFE_DIRECTORY".into());
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Data only: never executable, whatever the archive or source says.
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|_| "RESOURCE_WRITE_IO".into())
}

fn sync_dir(path: &Path) -> Result<(), String> {
    std::fs::File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| "RESOURCE_DIRECTORY_SYNC".into())
}

fn sha256_file(path: &Path, limit: u64) -> Result<(u64, String), String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path).map_err(|_| "RESOURCE_READ_IO")?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|_| "RESOURCE_READ_IO")?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err("RESOURCE_SIZE_LIMIT".into());
        }
        hasher.update(&buffer[..n]);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut probe = path.to_path_buf();
        while !probe.exists() {
            probe = probe.parent()?.to_path_buf();
        }
        let c = std::ffi::CString::new(probe.as_os_str().as_bytes()).ok()?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: valid C string and writable statvfs storage.
        if unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: statvfs succeeded.
        let stat = unsafe { stat.assume_init() };
        #[allow(clippy::unnecessary_cast)] // field widths differ across platforms
        Some(stat.f_bavail as u64 * stat.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn redacted_source(source: &str) -> String {
    match url::Url::parse(source) {
        Ok(mut url) if url.scheme() != "file" => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        _ => source.to_owned(),
    }
}

/// Validate request fields shared by preview and execution.
fn validate(request: &InstallRequest) -> Result<ResourceId, String> {
    let id: ResourceId = request.resource.parse()?;
    if request.version.is_empty()
        || request.version.len() > 64
        || request.version.starts_with('.')
        || request.version.contains("..")
        || !request
            .version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
    {
        return Err("RESOURCE_VERSION_INVALID".into());
    }
    let digest = request
        .sha256
        .strip_prefix("sha256:")
        .unwrap_or(&request.sha256);
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(
            "RESOURCE_SHA256_INVALID: pin the lowercase hex SHA-256 of the artifact".into(),
        );
    }
    if !LICENSES.contains(&request.license.as_str()) {
        return Err(format!(
            "RESOURCE_LICENSE_UNRECOGNIZED: {} is not an accepted SPDX identifier ({})",
            request.license,
            LICENSES.join(", ")
        ));
    }
    Ok(id)
}

enum Source {
    Local(PathBuf),
    Remote(url::Url),
    Ollama,
}

fn parse_source(id: &ResourceId, source: &str) -> Result<Source, String> {
    if id.kind == Kind::Ollama {
        return if source == "ollama-registry" {
            Ok(Source::Ollama)
        } else {
            Err("RESOURCE_SOURCE_INVALID: Ollama models use --source ollama-registry".into())
        };
    }
    if source.starts_with('/') {
        return Ok(Source::Local(PathBuf::from(source)));
    }
    let url = url::Url::parse(source).map_err(|_| "RESOURCE_SOURCE_INVALID")?;
    match url.scheme() {
        "file" => Ok(Source::Local(
            url.to_file_path().map_err(|_| "RESOURCE_SOURCE_INVALID")?,
        )),
        "https" | "http" => {
            if !url.username().is_empty() || url.password().is_some() {
                return Err("RESOURCE_SOURCE_CREDENTIALS: credentials in URLs are rejected".into());
            }
            Ok(Source::Remote(url))
        }
        _ => Err("RESOURCE_SOURCE_INVALID: use https://, file:// or an absolute path".into()),
    }
}

fn destinations(
    settings: &Effective,
    url: &url::Url,
) -> Result<linguist_provider::Destinations, String> {
    let mut destinations = linguist_provider::Destinations::public(&PUBLIC_HOSTS);
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    let allowed = settings.values["network.allowed_remote_service_hosts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|h| h.eq_ignore_ascii_case(&host));
    if loopback || allowed {
        destinations.configured.insert(host);
    } else if url.scheme() != "https" || !PUBLIC_HOSTS.contains(&host.as_str()) {
        return Err(format!(
            "RESOURCE_HOST_NOT_ALLOWED: {host} is not a builtin resource host; add it to network.allowed_remote_service_hosts after review"
        ));
    }
    Ok(destinations)
}

fn file_name_of(source: &Source) -> Option<String> {
    let name = match source {
        Source::Local(path) => path.file_name()?.to_string_lossy().into_owned(),
        Source::Remote(url) => url.path_segments()?.next_back()?.to_owned(),
        Source::Ollama => return None,
    };
    (!name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)))
    .then_some(name)
}

/// Where a resource and its receipt live.
fn layout(
    root: &Path,
    id: &ResourceId,
    version: &str,
    destination: Option<&Path>,
) -> Result<(PathBuf, PathBuf), String> {
    let receipt = root
        .join("receipts")
        .join(id.kind.name())
        .join(id.name.replace([':', '/'], "_"))
        .join(format!("{version}.json"));
    let install = match id.kind {
        Kind::Tesseract => root.join("tessdata"),
        Kind::Ollama => root.join("ollama"),
        _ => match destination {
            Some(path) => {
                if !path.is_absolute()
                    || path.components().any(|c| matches!(c, Component::ParentDir))
                    || !path.starts_with(root)
                    || path == root
                {
                    return Err("RESOURCE_DESTINATION_OUTSIDE_RESOURCE_DIR: destinations must be new directories inside storage.resource_dir".into());
                }
                path.to_owned()
            }
            None => root.join(id.kind.name()).join(&id.name).join(version),
        },
    };
    Ok((install, receipt))
}

/// Validate every entry before extracting any byte.
fn check_archive(
    archive: &mut zip::ZipArchive<std::fs::File>,
    max_unpacked: u64,
) -> Result<Vec<(usize, PathBuf)>, String> {
    if archive.len() > MAX_ENTRIES {
        return Err("RESOURCE_ARCHIVE_TOO_MANY_ENTRIES".into());
    }
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut declared = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| "RESOURCE_ARCHIVE_INVALID")?;
        let raw = entry.name().to_owned();
        if raw.is_empty()
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains('\0')
            || raw.contains(':')
        {
            return Err(format!("RESOURCE_ARCHIVE_UNSAFE_PATH: {raw}"));
        }
        let path = PathBuf::from(raw.trim_end_matches('/'));
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(format!("RESOURCE_ARCHIVE_UNSAFE_PATH: {raw}"));
        }
        if entry.encrypted() {
            return Err("RESOURCE_ARCHIVE_ENCRYPTED".into());
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if kind == 0o120000 {
                return Err(format!("RESOURCE_ARCHIVE_SYMLINK: {raw}"));
            }
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err(format!("RESOURCE_ARCHIVE_SPECIAL_FILE: {raw}"));
            }
        }
        if !matches!(
            entry.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        ) {
            return Err("RESOURCE_ARCHIVE_METHOD_UNSUPPORTED".into());
        }
        if !seen.insert(path.clone()) {
            return Err(format!("RESOURCE_ARCHIVE_DUPLICATE_PATH: {raw}"));
        }
        if !entry.is_dir() {
            declared = declared.saturating_add(entry.size());
            if declared > max_unpacked {
                return Err(
                    "RESOURCE_UNPACKED_LIMIT: archive exceeds resources.max_unpacked_mb".into(),
                );
            }
            files.push((index, path));
        }
    }
    // A file may not also be the parent of another entry.
    for (_, path) in &files {
        let mut parent = path.parent();
        while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if files.iter().any(|(_, f)| f == p) {
                return Err(format!("RESOURCE_ARCHIVE_PATH_CONFLICT: {}", p.display()));
            }
            parent = p.parent();
        }
    }
    if files.is_empty() {
        return Err("RESOURCE_ARCHIVE_EMPTY".into());
    }
    Ok(files)
}

fn extract(staged: &Path, target: &Path, max_unpacked: u64) -> Result<Vec<ReceiptFile>, String> {
    let file = std::fs::File::open(staged).map_err(|_| "RESOURCE_READ_IO")?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "RESOURCE_ARCHIVE_INVALID")?;
    let entries = check_archive(&mut archive, max_unpacked)?;
    let mut total = 0u64;
    let mut out = Vec::new();
    for (index, relative) in entries {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "RESOURCE_ARCHIVE_INVALID")?;
        let path = target.join(&relative);
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        let mut writer = private_file(&path)?;
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut bytes = 0u64;
        loop {
            let n = entry
                .read(&mut buffer)
                .map_err(|_| "RESOURCE_ARCHIVE_INVALID")?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            total += n as u64;
            // Actual output is counted; declared sizes are not trusted.
            if total > max_unpacked {
                return Err(
                    "RESOURCE_UNPACKED_LIMIT: archive exceeds resources.max_unpacked_mb".into(),
                );
            }
            sha2::Digest::update(&mut hasher, &buffer[..n]);
            writer
                .write_all(&buffer[..n])
                .map_err(|_| "RESOURCE_WRITE_IO")?;
        }
        writer.sync_all().map_err(|_| "RESOURCE_WRITE_IO")?;
        out.push(ReceiptFile {
            path: relative.to_string_lossy().into_owned(),
            bytes,
            sha256: format!("{:x}", sha2::Digest::finalize(hasher)),
        });
    }
    Ok(out)
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Preview (default) or execute one pinned installation.
pub fn install(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    request: &InstallRequest,
) -> Result<Value, String> {
    let id = validate(request)?;
    let expected = request
        .sha256
        .strip_prefix("sha256:")
        .unwrap_or(&request.sha256)
        .to_owned();
    let source = parse_source(&id, &request.source)?;
    let root = resource_root(settings, environment)?;
    let (install, receipt_path) =
        layout(&root, &id, &request.version, request.destination.as_deref())?;
    let offline = settings.values["network.offline"] == true;
    let max_download = settings.values["resources.max_download_mb"]
        .as_u64()
        .ok_or("RESOURCE_SETTING_MISSING")?
        * 1024
        * 1024;
    let max_unpacked = settings.values["resources.max_unpacked_mb"]
        .as_u64()
        .ok_or("RESOURCE_SETTING_MISSING")?
        * 1024
        * 1024;
    let reserve = settings.values["storage.free_space_reserve_mb"]
        .as_u64()
        .ok_or("RESOURCE_SETTING_MISSING")?
        * 1024
        * 1024;
    if let Source::Remote(url) = &source {
        if offline {
            return Err("RESOURCE_OFFLINE: network.offline forbids downloads; install from a local file instead".into());
        }
        destinations(settings, url)?;
    }
    if matches!(source, Source::Ollama) && offline {
        return Err("RESOURCE_OFFLINE: pulling an Ollama model needs the network".into());
    }
    let file_name = file_name_of(&source);
    if id.kind == Kind::Tesseract
        && file_name.as_deref() != Some(&format!("{}.traineddata", id.name))
        && file_name.as_deref().is_none_or(|n| !n.ends_with(".zip"))
    {
        return Err(format!(
            "RESOURCE_TYPE_MISMATCH: tesseract:{} expects {}.traineddata",
            id.name, id.name
        ));
    }
    if id.kind != Kind::Ollama && file_name.is_none() {
        return Err(
            "RESOURCE_SOURCE_NAME_INVALID: the source must end in a plain file name".into(),
        );
    }
    // An identical receipt makes the request an idempotent no-op.
    if let Ok(bytes) = std::fs::read(&receipt_path) {
        let existing: Receipt =
            serde_json::from_slice(&bytes).map_err(|_| "RESOURCE_RECEIPT_CORRUPT")?;
        if existing.sha256 == expected && existing.license == request.license {
            return Ok(json!({
                "schema_version": 1,
                "resource": request.resource,
                "executed": false,
                "already_installed": true,
                "receipt": existing,
            }));
        }
        return Err("RESOURCE_VERSION_INSTALLED_DIFFERENTLY: a receipt for this version names another checksum or license".into());
    }
    let target_file = match id.kind {
        Kind::Tesseract => Some(install.join(format!("{}.traineddata", id.name))),
        _ => None,
    };
    let occupied = match &target_file {
        Some(file) => std::fs::symlink_metadata(file).is_ok(),
        None => id.kind != Kind::Ollama && std::fs::symlink_metadata(&install).is_ok(),
    };
    if occupied {
        return Err("RESOURCE_DESTINATION_EXISTS: existing resources are never overwritten; remove or rename them yourself first".into());
    }
    let available = free_bytes(&root);
    let mut plan = json!({
        "schema_version": 1,
        "resource": request.resource,
        "kind": id.kind,
        "version": request.version,
        "license": request.license,
        "source": redacted_source(&request.source),
        "expected_sha256": expected,
        "install_path": target_file.as_ref().unwrap_or(&install),
        "receipt_path": receipt_path,
        "network": matches!(source, Source::Remote(_) | Source::Ollama),
        "limits": {"max_download_bytes": max_download, "max_unpacked_bytes": max_unpacked, "free_space_reserve_bytes": reserve},
        "available_bytes": available,
        "executed": false,
        "runs_downloaded_content": false,
    });
    if !request.execute {
        plan["next"] = json!("repeat with --execute to download, verify and install");
        return Ok(plan);
    }
    if available.is_some_and(|a| a < reserve) {
        return Err("RESOURCE_FREE_SPACE: storage.free_space_reserve_mb is not available".into());
    }
    let receipt = if matches!(source, Source::Ollama) {
        pull_ollama(settings, environment, &id, request, &expected, &install)?
    } else {
        install_files(
            settings,
            &root,
            &id,
            request,
            &expected,
            &source,
            file_name.as_deref().unwrap(),
            &install,
            target_file.as_deref(),
            max_download,
            max_unpacked,
            reserve,
        )?
    };
    private_dir(receipt_path.parent().unwrap())?;
    let mut file = private_file(&receipt_path)?;
    let mut bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "RESOURCE_RECEIPT_IO")?;
    sync_dir(receipt_path.parent().unwrap())?;
    plan["executed"] = json!(true);
    plan["receipt"] = serde_json::to_value(&receipt).map_err(|e| e.to_string())?;
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
fn install_files(
    settings: &Effective,
    root: &Path,
    id: &ResourceId,
    request: &InstallRequest,
    expected: &str,
    source: &Source,
    file_name: &str,
    install: &Path,
    target_file: Option<&Path>,
    max_download: u64,
    max_unpacked: u64,
    reserve: u64,
) -> Result<Receipt, String> {
    private_dir(root)?;
    let staging = root.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    private_dir(&staging)?;
    let result = (|| -> Result<Receipt, String> {
        let staged = staging.join(file_name);
        let (bytes, digest, final_source) = match source {
            Source::Local(path) => {
                let meta =
                    std::fs::symlink_metadata(path).map_err(|_| "RESOURCE_SOURCE_NOT_FOUND")?;
                if !meta.is_file() {
                    return Err("RESOURCE_SOURCE_NOT_REGULAR_FILE".into());
                }
                if meta.len() > max_download {
                    return Err(
                        "RESOURCE_DOWNLOAD_LIMIT: source exceeds resources.max_download_mb".into(),
                    );
                }
                let mut input = std::fs::File::open(path).map_err(|_| "RESOURCE_READ_IO")?;
                let mut output = private_file(&staged)?;
                let copied = std::io::copy(&mut (&mut input).take(max_download + 1), &mut output)
                    .map_err(|_| "RESOURCE_WRITE_IO")?;
                if copied > max_download {
                    return Err(
                        "RESOURCE_DOWNLOAD_LIMIT: source exceeds resources.max_download_mb".into(),
                    );
                }
                output.sync_all().map_err(|_| "RESOURCE_WRITE_IO")?;
                let (bytes, digest) = sha256_file(&staged, max_download)?;
                (bytes, digest, None)
            }
            Source::Remote(url) => {
                let destinations = destinations(settings, url)?;
                let v = &settings.values;
                let timeout = v["network.request_timeout_seconds"].as_u64().unwrap();
                let limits = linguist_provider::download::Limits {
                    connect_timeout: std::time::Duration::from_secs(
                        v["network.connect_timeout_seconds"].as_u64().unwrap(),
                    ),
                    // Large packs need far longer than one provider read.
                    deadline: std::time::Duration::from_secs(timeout * 60),
                    max_bytes: max_download,
                    max_redirects: v["network.max_redirects"].as_u64().unwrap() as u32,
                };
                let mut output = private_file(&staged)?;
                let downloaded = linguist_provider::download::download(
                    url,
                    &destinations,
                    &limits,
                    v["dictionary.user_agent"]
                        .as_str()
                        .unwrap_or("LinguistAnkiBridge/CLI"),
                    &mut output,
                )
                .map_err(|e| match e {
                    linguist_provider::ReadError::ResponseLimit => {
                        "RESOURCE_DOWNLOAD_LIMIT: response exceeds resources.max_download_mb"
                            .to_owned()
                    }
                    linguist_provider::ReadError::Policy
                    | linguist_provider::ReadError::Redirect => {
                        format!("RESOURCE_HOST_NOT_ALLOWED: {e}")
                    }
                    other => format!("RESOURCE_DOWNLOAD_FAILED: {other}"),
                })?;
                output.sync_all().map_err(|_| "RESOURCE_WRITE_IO")?;
                (
                    downloaded.bytes,
                    downloaded.sha256,
                    Some(redacted_source(&downloaded.final_url)),
                )
            }
            Source::Ollama => unreachable!(),
        };
        if digest != expected {
            return Err(format!(
                "RESOURCE_CHECKSUM_MISMATCH: expected {expected}, received {digest}; nothing was installed"
            ));
        }
        let archive = file_name.ends_with(".zip");
        let content = staging.join("content");
        let files = if archive {
            private_dir(&content)?;
            extract(&staged, &content, max_unpacked)?
        } else {
            if bytes > max_unpacked {
                return Err("RESOURCE_UNPACKED_LIMIT".into());
            }
            vec![ReceiptFile {
                path: file_name.to_owned(),
                bytes,
                sha256: digest.clone(),
            }]
        };
        let total: u64 = files.iter().map(|f| f.bytes).sum();
        if free_bytes(root).is_some_and(|a| a < reserve) {
            return Err(
                "RESOURCE_FREE_SPACE: installing would cross storage.free_space_reserve_mb".into(),
            );
        }
        if id.kind == Kind::Tesseract {
            let wanted = format!("{}.traineddata", id.name);
            let (source_path, entry) = if archive {
                let entry = files
                    .iter()
                    .find(|f| f.path == wanted)
                    .ok_or_else(|| format!("RESOURCE_TYPE_MISMATCH: archive lacks {wanted}"))?;
                if files.len() != 1 {
                    return Err(
                        "RESOURCE_TYPE_MISMATCH: a tesseract archive must hold exactly one pack"
                            .into(),
                    );
                }
                (content.join(&entry.path), entry.clone())
            } else {
                (staged.clone(), files[0].clone())
            };
            let target = target_file.unwrap();
            private_dir(install)?;
            std::fs::hard_link(&source_path, target).map_err(
                |_| "RESOURCE_DESTINATION_EXISTS: existing resources are never overwritten",
            )?;
            sync_dir(install)?;
            return Ok(Receipt {
                schema_version: RECEIPT_SCHEMA,
                resource: request.resource.clone(),
                kind: id.kind,
                name: id.name.clone(),
                version: request.version.clone(),
                license: request.license.clone(),
                source: redacted_source(&request.source),
                final_source,
                sha256: digest,
                downloaded: matches!(source, Source::Remote(_)),
                archive,
                install_path: target.to_string_lossy().into_owned(),
                total_bytes: entry.bytes,
                files: vec![entry],
                installed_unix_seconds: now_seconds(),
            });
        }
        let payload = if archive {
            content
        } else {
            let single = staging.join("single");
            private_dir(&single)?;
            std::fs::rename(&staged, single.join(file_name)).map_err(|_| "RESOURCE_WRITE_IO")?;
            single
        };
        private_dir(install.parent().ok_or("RESOURCE_DESTINATION_INVALID")?)?;
        // rename(2) onto an absent path is atomic; an existing path fails.
        if std::fs::symlink_metadata(install).is_ok() {
            return Err(
                "RESOURCE_DESTINATION_EXISTS: existing resources are never overwritten".into(),
            );
        }
        std::fs::rename(&payload, install).map_err(|_| "RESOURCE_INSTALL_IO")?;
        sync_dir(install.parent().unwrap())?;
        Ok(Receipt {
            schema_version: RECEIPT_SCHEMA,
            resource: request.resource.clone(),
            kind: id.kind,
            name: id.name.clone(),
            version: request.version.clone(),
            license: request.license.clone(),
            source: redacted_source(&request.source),
            final_source,
            sha256: digest,
            downloaded: matches!(source, Source::Remote(_)),
            archive,
            install_path: install.to_string_lossy().into_owned(),
            files,
            total_bytes: total,
            installed_unix_seconds: now_seconds(),
        })
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Explicit `ollama pull` through the configured local service, then verify
/// the installed model digest. Generation never pulls models.
fn pull_ollama(
    settings: &Effective,
    _environment: &BTreeMap<String, String>,
    id: &ResourceId,
    request: &InstallRequest,
    expected: &str,
    install: &Path,
) -> Result<Receipt, String> {
    let endpoint = url::Url::parse(settings.values["llm.endpoint"].as_str().unwrap_or_default())
        .map_err(|_| "RESOURCE_OLLAMA_ENDPOINT_INVALID")?;
    let host = endpoint.host_str().unwrap_or_default().to_ascii_lowercase();
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        return Err(
            "RESOURCE_OLLAMA_ENDPOINT_NOT_LOOPBACK: model pulls use a local Ollama service only"
                .into(),
        );
    }
    let timeout = settings.values["network.request_timeout_seconds"]
        .as_u64()
        .unwrap();
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(
            settings.values["network.connect_timeout_seconds"]
                .as_u64()
                .unwrap(),
        ))
        .timeout(std::time::Duration::from_secs(timeout * 60))
        .build()
        .map_err(|_| "RESOURCE_OLLAMA_UNAVAILABLE")?;
    let base = endpoint.as_str().trim_end_matches('/');
    let response = http
        .post(format!("{base}/api/pull"))
        .json(&json!({"model": id.name, "stream": false}))
        .send()
        .map_err(|_| "RESOURCE_OLLAMA_UNAVAILABLE")?;
    if !response.status().is_success() {
        return Err(format!(
            "RESOURCE_OLLAMA_PULL_FAILED: HTTP {}",
            response.status().as_u16()
        ));
    }
    let tags: Value = http
        .get(format!("{base}/api/tags"))
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| "RESOURCE_OLLAMA_UNAVAILABLE")?
        .json()
        .map_err(|_| "RESOURCE_OLLAMA_RESPONSE_INVALID")?;
    let model = tags["models"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["name"].as_str() == Some(&id.name) || m["model"].as_str() == Some(&id.name))
        .ok_or("RESOURCE_OLLAMA_MODEL_MISSING_AFTER_PULL")?;
    let digest = model["digest"]
        .as_str()
        .map(|d| d.strip_prefix("sha256:").unwrap_or(d).to_owned())
        .ok_or("RESOURCE_OLLAMA_RESPONSE_INVALID")?;
    if digest != expected {
        return Err(format!(
            "RESOURCE_CHECKSUM_MISMATCH: Ollama reports {digest}, expected {expected}; the pulled model remains in Ollama and no receipt was written (remove it with `ollama rm {}` if unwanted)",
            id.name
        ));
    }
    let size = model["size"].as_u64().unwrap_or(0);
    Ok(Receipt {
        schema_version: RECEIPT_SCHEMA,
        resource: request.resource.clone(),
        kind: id.kind,
        name: id.name.clone(),
        version: request.version.clone(),
        license: request.license.clone(),
        source: "ollama-registry".into(),
        final_source: Some(redacted_source(base)),
        sha256: digest.clone(),
        downloaded: true,
        archive: false,
        install_path: install.to_string_lossy().into_owned(),
        files: vec![ReceiptFile {
            path: id.name.clone(),
            bytes: size,
            sha256: digest,
        }],
        total_bytes: size,
        installed_unix_seconds: now_seconds(),
    })
}

fn verify_receipt(receipt: &Receipt) -> &'static str {
    if receipt.kind == Kind::Ollama {
        return "recorded_service_model";
    }
    let base = PathBuf::from(&receipt.install_path);
    for file in &receipt.files {
        let path = if receipt.kind == Kind::Tesseract {
            base.clone()
        } else {
            base.join(&file.path)
        };
        match sha256_file(&path, file.bytes.saturating_add(1)) {
            Ok((bytes, digest)) if bytes == file.bytes && digest == file.sha256 => {}
            Ok(_) | Err(_) if !path.exists() => return "missing_files",
            _ => return "modified",
        }
    }
    "verified"
}

/// Installed receipts (re-verified), configured requirements and stray packs.
pub fn list(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    filter: Option<&str>,
    installed_only: bool,
    required_only: bool,
) -> Result<Value, String> {
    let root = resource_root(settings, environment)?;
    let mut installed = Vec::new();
    let mut receipted = BTreeSet::new();
    let receipts = root.join("receipts");
    if let Ok(kinds) = std::fs::read_dir(&receipts) {
        let mut paths = Vec::new();
        for kind in kinds.flatten() {
            for name in std::fs::read_dir(kind.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                for version in std::fs::read_dir(name.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    paths.push(version.path());
                }
            }
        }
        paths.sort();
        for path in paths {
            let entry = match std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Receipt>(&b).ok())
            {
                Some(receipt) => {
                    if filter.is_some_and(|f| !receipt.resource.starts_with(f)) {
                        continue;
                    }
                    receipted.insert(PathBuf::from(&receipt.install_path));
                    json!({
                        "resource": receipt.resource,
                        "version": receipt.version,
                        "license": receipt.license,
                        "sha256": receipt.sha256,
                        "install_path": receipt.install_path,
                        "total_bytes": receipt.total_bytes,
                        "status": verify_receipt(&receipt),
                    })
                }
                None => json!({"receipt": path, "status": "receipt_corrupt"}),
            };
            installed.push(entry);
        }
    }
    let mut unreceipted = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("tessdata")) {
        for entry in entries.flatten() {
            if !receipted.contains(&entry.path()) {
                unreceipted.push(entry.path());
            }
        }
    }
    unreceipted.sort();
    let mut required = Vec::new();
    let checks = linguist_config::resources::inspect(settings, environment);
    for check in &checks.checks {
        required.push(
            json!({"setting": check.key, "status": check.status, "required": check.required}),
        );
    }
    if settings.values["ocr.engine"] == "tesseract" {
        let tessdata = settings.values["ocr.resource_path"]
            .as_str()
            .and_then(|p| linguist_config::expand_path(p, environment).ok());
        let mut languages: BTreeSet<String> = settings.values["ocr.languages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        for (key, value) in &settings.values {
            if key.starts_with("purposes.") && key.ends_with(".ocr_languages") {
                languages.extend(
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned)),
                );
            }
        }
        for language in languages {
            let resource = format!("tesseract:{language}");
            if filter.is_some_and(|f| !resource.starts_with(f)) {
                continue;
            }
            let managed = root
                .join("tessdata")
                .join(format!("{language}.traineddata"));
            let selected = tessdata
                .as_ref()
                .map(|d| d.join(format!("{language}.traineddata")));
            required.push(json!({
                "resource": resource,
                "installed_in_resource_dir": managed.is_file(),
                "present_in_ocr_resource_path": selected.as_ref().map(|p| p.is_file()),
                "note": if tessdata.is_none() { "ocr.resource_path is unset: Tesseract uses its own tessdata; the helper probe in `doctor` lists its packs" } else { "" },
            }));
        }
    }
    if let Some(model) = settings.values["llm.model"].as_str()
        && settings.values["llm.enabled"] == true
    {
        let resource = format!("ollama:{model}");
        if filter.is_none_or(|f| resource.starts_with(f)) {
            required.push(json!({
                "resource": resource,
                "status": "not_checked_without_service",
                "note": "the local Ollama service is not contacted by resources list; use doctor or resources install --source ollama-registry",
            }));
        }
    }
    Ok(json!({
        "schema_version": 1,
        "resource_dir": root,
        "installed": if required_only { Value::Null } else { Value::Array(installed) },
        "unreceipted_tessdata": if required_only { Value::Null } else { json!(unreceipted) },
        "required": if installed_only { Value::Null } else { Value::Array(required) },
        "remote_catalogue": "unavailable: this build has no catalogue; install pinned sources explicitly",
        "downloads": false,
    }))
}
