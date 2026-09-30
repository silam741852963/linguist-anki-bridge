//! Read-only format inspection for Anki's current collection package.
//! A valid container is not a verified restore checkpoint or write authority.
use prost::Message;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use unicode_normalization::is_nfc;

#[derive(Clone)]
pub struct PackageLimits {
    pub max_package_bytes: u64,
    pub max_collection_bytes: u64,
    pub max_media_bytes: u64,
    pub max_media_map_bytes: u64,
    pub max_entries: usize,
    pub timeout: Duration,
    pub scratch_dir: PathBuf,
}

struct ScratchCollection {
    path: PathBuf,
    file: File,
}

impl ScratchCollection {
    fn create(dir: &Path) -> Result<Self, String> {
        if !fs::symlink_metadata(dir)
            .map_err(|_| "CHECKPOINT_SCRATCH_DIR_UNAVAILABLE")?
            .file_type()
            .is_dir()
        {
            return Err("CHECKPOINT_SCRATCH_DIR_INVALID".into());
        }
        for _ in 0..8 {
            let path = dir.join(format!(".lab-colpkg-{}", uuid::Uuid::new_v4()));
            let mut options = OpenOptions::new();
            options.write(true).read(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("CHECKPOINT_SCRATCH_CREATE_FAILED".into()),
            }
        }
        Err("CHECKPOINT_SCRATCH_CREATE_FAILED".into())
    }
}

impl Drop for ScratchCollection {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Serialize)]
pub struct PackageInspection {
    pub format: &'static str,
    pub package_sha256: String,
    pub package_size_bytes: u64,
    pub collection_sha256: String,
    pub collection_size_bytes: u64,
    pub collection_schema_version: u8,
    pub collection_note_count: u64,
    pub collection_card_count: u64,
    pub collection_review_count: u64,
    pub declared_media_files: usize,
    pub declared_media_bytes: u64,
    pub container_and_declared_media_verified: bool,
    pub sqlite_integrity_verified: bool,
    pub anki_core_schema_verified: bool,
    pub collection_scope_verified: bool,
    pub restoration_tested: bool,
    pub checkpoint_eligible: bool,
}

struct CollectionContents {
    schema_version: u8,
    notes: u64,
    cards: u64,
    reviews: u64,
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
    scan_with_sink(&mut input, limit, capture, deadline, None)
}

fn scan_with_sink(
    mut input: impl Read,
    limit: u64,
    capture: bool,
    deadline: Instant,
    mut sink: Option<&mut File>,
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
        if let Some(file) = sink.as_mut() {
            file.write_all(&chunk[..count])
                .map_err(|_| "CHECKPOINT_SCRATCH_WRITE_FAILED")?;
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
    sink: Option<&mut File>,
) -> Result<Scanned, String> {
    let mut decoder =
        zstd::stream::read::Decoder::new(input).map_err(|_| "CHECKPOINT_ZSTD_INVALID")?;
    let result = scan_with_sink(&mut decoder, limit, capture, deadline, sink)?;
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

fn sqlite_integrity(path: &Path, deadline: Instant) -> Result<CollectionContents, String> {
    if Instant::now() >= deadline {
        return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
    }
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "CHECKPOINT_SQLITE_INVALID")?;
    // Anki indexes use this collation. Its version is pinned by Anki because
    // a comparator change can make an otherwise sound index look corrupt.
    connection
        .create_collation("unicase", |left, right| {
            unicase::UniCase::new(left).cmp(&unicase::UniCase::new(right))
        })
        .map_err(|_| "CHECKPOINT_SQLITE_INVALID")?;
    connection
        .progress_handler(1000, Some(move || Instant::now() >= deadline))
        .map_err(|_| "CHECKPOINT_SQLITE_INVALID")?;
    let check = (|| -> rusqlite::Result<bool> {
        let mut statement = connection.prepare("PRAGMA integrity_check")?;
        let mut rows = statement.query([])?;
        let first = rows
            .next()?
            .map(|row| row.get::<_, String>(0))
            .transpose()?;
        Ok(first.as_deref() == Some("ok") && rows.next()?.is_none())
    })();
    if Instant::now() >= deadline {
        return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
    }
    if !matches!(check, Ok(true)) {
        return Err("CHECKPOINT_SQLITE_INTEGRITY_FAILED".into());
    }
    // The v11 core persists in later Anki schemas. Check names and key columns
    // before reporting a package as an Anki collection; do not infer full
    // semantic correctness or source coverage from these checks.
    for (table, required) in [
        ("col", &["id", "ver"][..]),
        ("notes", &["id", "mid", "flds"][..]),
        (
            "cards",
            &[
                "id", "nid", "did", "ord", "queue", "due", "ivl", "factor", "reps", "lapses",
            ][..],
        ),
        ("revlog", &["id", "cid", "ease", "ivl", "lastIvl"][..]),
        ("graves", &["usn", "oid", "type"][..]),
    ] {
        let mut columns = std::collections::BTreeSet::new();
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info(?1)")
            .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID")?;
        let rows = statement
            .query_map([table], |row| row.get::<_, String>(0))
            .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID")?;
        for column in rows {
            columns.insert(column.map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID")?);
        }
        if required.iter().any(|name| !columns.contains(*name)) {
            return Err("CHECKPOINT_ANKI_SCHEMA_UNRECOGNIZED".into());
        }
    }
    let version: i64 = connection
        .query_row("SELECT ver FROM col WHERE id=1", [], |row| row.get(0))
        .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_UNRECOGNIZED")?;
    let col_count: i64 = connection
        .query_row("SELECT count(*) FROM col", [], |row| row.get(0))
        .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID")?;
    // 12/13 are intermediate schemas that Anki itself refuses to reopen.
    if col_count != 1 || !(11..=18).contains(&version) || matches!(version, 12 | 13) {
        return Err("CHECKPOINT_ANKI_SCHEMA_UNRECOGNIZED".into());
    }
    let count = |table: &str| -> Result<u64, String> {
        let sql = format!("SELECT count(*) FROM {table}");
        let value: i64 = connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID")?;
        u64::try_from(value).map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID".into())
    };
    let contents = CollectionContents {
        schema_version: version as u8,
        notes: count("notes")?,
        cards: count("cards")?,
        reviews: count("revlog")?,
    };
    if Instant::now() >= deadline {
        return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
    }
    Ok(contents)
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

/// Checks the latest `.colpkg` container, decoded SQLite integrity and every
/// declared media byte without extracting paths. Scope and restoration require
/// separate evidence before a checkpoint can be used.
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
    let mut scratch = ScratchCollection::create(&limits.scratch_dir)?;
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
                    Some(&mut scratch.file),
                )?)
            }
            "collection.anki2" => dummy = Some(scan(entry, 16 * 1024 * 1024, false, deadline)?),
            "media" => {
                media_map = Some(compressed(
                    entry,
                    limits.max_media_map_bytes,
                    true,
                    deadline,
                    None,
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
                let scanned = compressed(entry, remaining, false, deadline, None)?;
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
    scratch
        .file
        .flush()
        .map_err(|_| "CHECKPOINT_SCRATCH_WRITE_FAILED")?;
    let contents = sqlite_integrity(&scratch.path, deadline)?;
    Ok(PackageInspection {
        format: "anki-colpkg-latest-v3",
        package_sha256: initial.sha256,
        package_size_bytes: initial.bytes,
        collection_sha256: collection.sha256,
        collection_size_bytes: collection.bytes,
        collection_schema_version: contents.schema_version,
        collection_note_count: contents.notes,
        collection_card_count: contents.cards,
        collection_review_count: contents.reviews,
        declared_media_files: media.len(),
        declared_media_bytes: media_bytes,
        container_and_declared_media_verified: true,
        sqlite_integrity_verified: true,
        anki_core_schema_verified: true,
        collection_scope_verified: false,
        restoration_tested: false,
        checkpoint_eligible: false,
    })
}
