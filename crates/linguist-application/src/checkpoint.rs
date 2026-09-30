//! Read-only format inspection for Anki's current collection package.
//! A valid container is not a verified restore checkpoint or write authority.
use prost::Message;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};
use unicode_normalization::is_nfc;

#[derive(Clone, Copy)]
pub struct PackageLimits {
    pub max_package_bytes: u64,
    pub max_collection_bytes: u64,
    pub max_media_bytes: u64,
    pub max_media_map_bytes: u64,
    pub max_entries: usize,
    pub timeout: Duration,
}

#[derive(Debug, Serialize)]
pub struct PackageInspection {
    pub format: &'static str,
    pub package_sha256: String,
    pub package_size_bytes: u64,
    pub collection_sha256: String,
    pub collection_size_bytes: u64,
    pub declared_media_files: usize,
    pub declared_media_bytes: u64,
    pub container_and_declared_media_verified: bool,
    pub sqlite_integrity_verified: bool,
    pub collection_scope_verified: bool,
    pub restoration_tested: bool,
    pub checkpoint_eligible: bool,
}

#[derive(Clone, PartialEq, Message)]
struct MediaEntries {
    #[prost(message, repeated, tag = "1")]
    entries: Vec<MediaEntry>,
}
#[derive(Clone, PartialEq, Message)]
struct MediaEntry {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    size: u32,
    #[prost(bytes, tag = "3")]
    sha1: Vec<u8>,
}

struct Scanned {
    bytes: u64,
    prefix: Vec<u8>,
    sha256: String,
    sha1: Vec<u8>,
}

fn scan(
    mut input: impl Read,
    limit: u64,
    capture: bool,
    deadline: Instant,
) -> Result<Scanned, String> {
    let mut sha256 = <sha2::Sha256 as sha2::Digest>::new();
    let mut sha1 = <sha1::Sha1 as sha1::Digest>::new();
    let mut bytes = 0u64;
    let mut prefix = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
        }
        let count = input
            .read(&mut chunk)
            .map_err(|_| "CHECKPOINT_PACKAGE_READ_FAILED")?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or("CHECKPOINT_CONTENT_LIMIT")?;
        if bytes > limit {
            return Err("CHECKPOINT_CONTENT_LIMIT".into());
        }
        sha2::Digest::update(&mut sha256, &chunk[..count]);
        sha1::Digest::update(&mut sha1, &chunk[..count]);
        if capture {
            prefix.extend_from_slice(&chunk[..count]);
        } else if prefix.len() < 100 {
            prefix.extend_from_slice(&chunk[..count.min(100 - prefix.len())]);
        }
    }
    Ok(Scanned {
        bytes,
        prefix,
        sha256: format!("{:x}", sha2::Digest::finalize(sha256)),
        sha1: sha1::Digest::finalize(sha1).to_vec(),
    })
}

fn compressed(
    input: impl Read,
    limit: u64,
    capture: bool,
    deadline: Instant,
) -> Result<Scanned, String> {
    let mut decoder =
        zstd::stream::read::Decoder::new(input).map_err(|_| "CHECKPOINT_ZSTD_INVALID")?;
    let result = scan(&mut decoder, limit, capture, deadline)?;
    decoder
        .finish_frame()
        .map_err(|_| "CHECKPOINT_ZSTD_INVALID")?;
    let mut raw = decoder.finish();
    let mut trailing = [0u8; 1];
    if raw
        .read(&mut trailing)
        .map_err(|_| "CHECKPOINT_ZIP_ENTRY_INVALID")?
        != 0
    {
        return Err("CHECKPOINT_ZSTD_TRAILING_DATA".into());
    }
    Ok(result)
}

fn sqlite_header(scan: &Scanned) -> bool {
    scan.bytes >= 100 && scan.prefix.starts_with(b"SQLite format 3\0")
}

fn count_media_records(mut bytes: &[u8], max: usize) -> Result<usize, String> {
    let mut count = 0usize;
    while !bytes.is_empty() {
        if bytes[0] != 0x0a {
            return Err("CHECKPOINT_MEDIA_MAP_INVALID".into());
        }
        bytes = &bytes[1..];
        let mut length = 0usize;
        let mut complete = false;
        for shift in (0..=28).step_by(7) {
            let (&byte, rest) = bytes.split_first().ok_or("CHECKPOINT_MEDIA_MAP_INVALID")?;
            bytes = rest;
            length |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                complete = true;
                break;
            }
        }
        if !complete || length > bytes.len() {
            return Err("CHECKPOINT_MEDIA_MAP_INVALID".into());
        }
        bytes = &bytes[length..];
        count += 1;
        if count > max {
            return Err("CHECKPOINT_MEDIA_MAP_LIMIT".into());
        }
    }
    Ok(count)
}

/// Checks the latest `.colpkg` container, decoded collection header and every
/// declared media byte without extracting paths. Scope, SQLite integrity and
/// restoration require separate evidence before a checkpoint can be used.
pub fn inspect_colpkg(path: &Path, limits: PackageLimits) -> Result<PackageInspection, String> {
    if limits.max_package_bytes == 0
        || limits.max_collection_bytes == 0
        || limits.max_media_bytes == 0
        || limits.max_media_map_bytes == 0
        || !(4..=1_000_000).contains(&limits.max_entries)
        || limits.timeout.is_zero()
        || limits.timeout > Duration::from_secs(3600)
    {
        return Err("CHECKPOINT_LIMITS_INVALID".into());
    }
    let deadline = Instant::now()
        .checked_add(limits.timeout)
        .ok_or("CHECKPOINT_LIMITS_INVALID")?;
    let metadata = fs::symlink_metadata(path).map_err(|_| "CHECKPOINT_FILE_UNAVAILABLE")?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > limits.max_package_bytes
    {
        return Err("CHECKPOINT_FILE_INVALID".into());
    }
    let mut file = File::open(path).map_err(|_| "CHECKPOINT_FILE_UNAVAILABLE")?;
    if file
        .metadata()
        .map_err(|_| "CHECKPOINT_FILE_UNAVAILABLE")?
        .len()
        != metadata.len()
    {
        return Err("CHECKPOINT_FILE_CHANGED".into());
    }
    let initial = scan(&mut file, limits.max_package_bytes, false, deadline)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "CHECKPOINT_FILE_READ_FAILED")?;
    let mut archive = zip::ZipArchive::new(&mut file).map_err(|_| "CHECKPOINT_ZIP_INVALID")?;
    if archive.len() < 4 || archive.len() > limits.max_entries {
        return Err("CHECKPOINT_ENTRY_COUNT_INVALID".into());
    }
    let mut seen = BTreeSet::new();
    let mut meta = None;
    let mut collection = None;
    let mut dummy = None;
    let mut media_map = None;
    let mut media = BTreeMap::new();
    let mut media_bytes = 0u64;
    for index in 0..archive.len() {
        if Instant::now() >= deadline {
            return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
        }
        let entry = archive
            .by_index(index)
            .map_err(|_| "CHECKPOINT_ZIP_ENTRY_INVALID")?;
        let name = entry.name().to_owned();
        if !seen.insert(name.clone())
            || entry.is_dir()
            || entry.encrypted()
            || entry.compression() != zip::CompressionMethod::Stored
            || entry.size() > limits.max_package_bytes
        {
            return Err("CHECKPOINT_ZIP_ENTRY_INVALID".into());
        }
        match name.as_str() {
            "meta" => meta = Some(scan(entry, 32, true, deadline)?),
            "collection.anki21b" => {
                collection = Some(compressed(
                    entry,
                    limits.max_collection_bytes,
                    false,
                    deadline,
                )?)
            }
            "collection.anki2" => dummy = Some(scan(entry, 16 * 1024 * 1024, false, deadline)?),
            "media" => {
                media_map = Some(compressed(
                    entry,
                    limits.max_media_map_bytes,
                    true,
                    deadline,
                )?)
            }
            _ => {
                if name.is_empty()
                    || name.len() > 7
                    || (name != "0" && name.starts_with('0'))
                    || !name.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err("CHECKPOINT_ZIP_ENTRY_NAME_INVALID".into());
                }
                let number = name
                    .parse::<usize>()
                    .map_err(|_| "CHECKPOINT_ZIP_ENTRY_NAME_INVALID")?;
                let remaining = limits
                    .max_media_bytes
                    .checked_sub(media_bytes)
                    .ok_or("CHECKPOINT_MEDIA_LIMIT")?;
                let scanned = compressed(entry, remaining, false, deadline)?;
                media_bytes = media_bytes
                    .checked_add(scanned.bytes)
                    .ok_or("CHECKPOINT_MEDIA_LIMIT")?;
                media.insert(number, scanned);
            }
        }
    }
    if meta.as_ref().map(|entry| entry.prefix.as_slice()) != Some(&[8, 3]) {
        return Err("CHECKPOINT_FORMAT_UNSUPPORTED".into());
    }
    let collection = collection.ok_or("CHECKPOINT_COLLECTION_MISSING")?;
    if !sqlite_header(&collection) || !dummy.as_ref().is_some_and(sqlite_header) {
        return Err("CHECKPOINT_COLLECTION_HEADER_INVALID".into());
    }
    let map = media_map.ok_or("CHECKPOINT_MEDIA_MAP_MISSING")?;
    let record_count = count_media_records(&map.prefix, limits.max_entries - 4)?;
    let declarations =
        MediaEntries::decode(map.prefix.as_slice()).map_err(|_| "CHECKPOINT_MEDIA_MAP_INVALID")?;
    if declarations.encode_to_vec() != map.prefix
        || declarations.entries.len() != record_count
        || declarations.entries.len() != media.len()
        || declarations.entries.len() + 4 != seen.len()
    {
        return Err("CHECKPOINT_MEDIA_MAP_INVALID".into());
    }
    let mut names = BTreeSet::new();
    let mut declared_bytes = 0u64;
    for (index, declared) in declarations.entries.iter().enumerate() {
        if declared.name.is_empty()
            || declared.name.len() > 255
            || !is_nfc(&declared.name)
            || declared.name.contains(['/', '\\', '\0'])
            || !names.insert(&declared.name)
            || declared.sha1.len() != 20
        {
            return Err("CHECKPOINT_MEDIA_MAP_INVALID".into());
        }
        let actual = media.get(&index).ok_or("CHECKPOINT_MEDIA_MISSING")?;
        if actual.bytes != declared.size as u64 || actual.sha1 != declared.sha1 {
            return Err("CHECKPOINT_MEDIA_CONTENT_INVALID".into());
        }
        declared_bytes = declared_bytes
            .checked_add(actual.bytes)
            .ok_or("CHECKPOINT_MEDIA_LIMIT")?;
    }
    if declared_bytes != media_bytes {
        return Err("CHECKPOINT_MEDIA_CONTENT_INVALID".into());
    }
    drop(archive);
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "CHECKPOINT_FILE_READ_FAILED")?;
    let final_hash = scan(&mut file, limits.max_package_bytes, false, deadline)?;
    if initial.bytes != metadata.len()
        || final_hash.bytes != initial.bytes
        || final_hash.sha256 != initial.sha256
    {
        return Err("CHECKPOINT_FILE_CHANGED".into());
    }
    Ok(PackageInspection {
        format: "anki-colpkg-latest-v3",
        package_sha256: initial.sha256,
        package_size_bytes: initial.bytes,
        collection_sha256: collection.sha256,
        collection_size_bytes: collection.bytes,
        declared_media_files: media.len(),
        declared_media_bytes: media_bytes,
        container_and_declared_media_verified: true,
        sqlite_integrity_verified: false,
        collection_scope_verified: false,
        restoration_tested: false,
        checkpoint_eligible: false,
    })
}
