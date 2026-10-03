//! Read-only format inspection for Anki's current collection package, optional
//! source-scope checks and a disposable decode restoration test. A valid
//! container is not a verified restore checkpoint or write authority.
use prost::Message;
use serde::{Deserialize, Serialize};
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

fn sqlite_integrity(
    path: &Path,
    deadline: Instant,
    scope: Option<&ScopeManifest>,
) -> Result<(CollectionContents, Option<ScopeReport>), String> {
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
    let report = scope
        .map(|scope| verify_scope(&connection, scope, contents.schema_version))
        .transpose()?;
    if Instant::now() >= deadline {
        return Err("CHECKPOINT_INSPECTION_TIMEOUT".into());
    }
    Ok((contents, report))
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
    inspect_inner(path, &limits, None, None).map(|(inspection, _, _)| inspection)
}

/// Inspects the package and requires every scope note, card, review count,
/// note type and media file to be present with matching bytes.
pub fn inspect_colpkg_scope(
    path: &Path,
    limits: PackageLimits,
    scope: &ScopeManifest,
) -> Result<(PackageInspection, ScopeReport), String> {
    scope.validate()?;
    let (inspection, report, _) = inspect_inner(path, &limits, Some(scope), None)?;
    Ok((inspection, report.ok_or("CHECKPOINT_SCOPE_UNVERIFIED")?))
}

/// Decodes the package into a fresh private directory below the disposable
/// `target`, reopens the restored collection and media independently, checks
/// the scope again and removes the restored files. The user's collection and
/// Anki's importer are never involved.
pub fn restore_test(
    path: &Path,
    limits: PackageLimits,
    scope: &ScopeManifest,
    target: &Path,
) -> Result<RestorationEvidence, String> {
    scope.validate()?;
    if !fs::symlink_metadata(target)
        .map_err(|_| "CHECKPOINT_RESTORE_TARGET_UNAVAILABLE")?
        .file_type()
        .is_dir()
    {
        return Err("CHECKPOINT_RESTORE_TARGET_INVALID".into());
    }
    let dir = target.join(format!("lab-restore-{}", uuid::Uuid::new_v4()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&dir)
        .map_err(|_| "CHECKPOINT_RESTORE_TARGET_CREATE_FAILED")?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let (inspection, _, restored) = inspect_inner(path, &limits, Some(scope), Some(&dir))?;
    let restored = restored.ok_or("CHECKPOINT_RESTORE_INCOMPLETE")?;
    if restored.collection_sha256 != inspection.collection_sha256 {
        return Err("CHECKPOINT_RESTORE_COLLECTION_MISMATCH".into());
    }
    Ok(RestorationEvidence {
        method: "lab-colpkg-decode-restore-v1",
        package_sha256: inspection.package_sha256,
        scope_digest: scope.digest()?,
        restored_collection_sha256: restored.collection_sha256,
        restored_note_count: restored.contents.notes,
        restored_card_count: restored.contents.cards,
        restored_review_count: restored.contents.reviews,
        restored_media_files: restored.media_files,
        restored_media_verified: true,
        scope: restored.scope,
        anki_importer_used: false,
        passed: true,
    })
}

struct Restored {
    collection_sha256: String,
    contents: CollectionContents,
    media_files: usize,
    scope: ScopeReport,
}

fn private_create(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "CHECKPOINT_RESTORE_WRITE_FAILED".into())
}

fn inspect_inner(
    path: &Path,
    limits: &PackageLimits,
    scope: Option<&ScopeManifest>,
    restore: Option<&Path>,
) -> Result<(PackageInspection, Option<ScopeReport>, Option<Restored>), String> {
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
    let mut restored_collection = restore
        .map(|dir| private_create(&dir.join("collection.anki2")))
        .transpose()?;
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
                    Some(restored_collection.as_mut().unwrap_or(&mut scratch.file)),
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
                let mut sink = restore
                    .map(|dir| private_create(&dir.join(format!(".media-{number}"))))
                    .transpose()?;
                let scanned = compressed(entry, remaining, false, deadline, sink.as_mut())?;
                if let Some(file) = sink {
                    file.sync_all()
                        .map_err(|_| "CHECKPOINT_RESTORE_WRITE_FAILED")?;
                }
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
            || declared.name == "."
            || declared.name == ".."
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
    let mut restored = None;
    let (contents, report) = if let Some(dir) = restore {
        let file = restored_collection
            .take()
            .ok_or("CHECKPOINT_RESTORE_INCOMPLETE")?;
        file.sync_all()
            .map_err(|_| "CHECKPOINT_RESTORE_WRITE_FAILED")?;
        drop(file);
        let media_dir = dir.join("collection.media");
        fs::create_dir(&media_dir).map_err(|_| "CHECKPOINT_RESTORE_WRITE_FAILED")?;
        for (index, declared) in declarations.entries.iter().enumerate() {
            fs::rename(
                dir.join(format!(".media-{index}")),
                media_dir.join(&declared.name),
            )
            .map_err(|_| "CHECKPOINT_RESTORE_WRITE_FAILED")?;
        }
        // Reopen what was written, independently of the bytes hashed while decoding.
        let restored_path = dir.join("collection.anki2");
        let reread = scan(
            File::open(&restored_path).map_err(|_| "CHECKPOINT_RESTORE_READ_FAILED")?,
            limits.max_collection_bytes,
            false,
            deadline,
        )?;
        for declared in &declarations.entries {
            let actual = scan(
                File::open(media_dir.join(&declared.name))
                    .map_err(|_| "CHECKPOINT_RESTORE_READ_FAILED")?,
                limits.max_media_bytes,
                false,
                deadline,
            )?;
            if actual.bytes != declared.size as u64 || actual.sha1 != declared.sha1 {
                return Err("CHECKPOINT_RESTORE_MEDIA_MISMATCH".into());
            }
        }
        if fs::read_dir(dir)
            .map_err(|_| "CHECKPOINT_RESTORE_READ_FAILED")?
            .count()
            != 2
        {
            return Err("CHECKPOINT_RESTORE_INCOMPLETE".into());
        }
        let scope = scope.ok_or("CHECKPOINT_SCOPE_UNVERIFIED")?;
        let (contents, report) = sqlite_integrity(&restored_path, deadline, Some(scope))?;
        let report = report.ok_or("CHECKPOINT_SCOPE_UNVERIFIED")?;
        let media_report = verify_scope_media(scope, &declarations.entries)?;
        let report = ScopeReport {
            media_verified: media_report,
            ..report
        };
        restored = Some(Restored {
            collection_sha256: reread.sha256,
            contents: CollectionContents {
                schema_version: contents.schema_version,
                notes: contents.notes,
                cards: contents.cards,
                reviews: contents.reviews,
            },
            media_files: declarations.entries.len(),
            scope: report.clone(),
        });
        (contents, Some(report))
    } else {
        sqlite_integrity(&scratch.path, deadline, scope)?
    };
    let report = match (scope, report) {
        (Some(scope), Some(report)) => Some(ScopeReport {
            media_verified: verify_scope_media(scope, &declarations.entries)?,
            ..report
        }),
        _ => None,
    };
    let inspection = PackageInspection {
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
        collection_scope_verified: report.is_some(),
        restoration_tested: false,
        checkpoint_eligible: false,
    };
    Ok((inspection, report, restored))
}

/// What one checkpoint must contain. Collection packages always carry schema and
/// whole-collection scheduling tables; the listed entries are verified explicitly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageRequirement {
    pub scheduling: bool,
    pub media: bool,
    pub schema: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCard {
    pub card_id: i64,
    pub note_id: i64,
    /// Repetitions observed before export; the package may contain more.
    pub reps: u32,
    /// Review-log rows observed before export; the package may contain more.
    pub review_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeMedia {
    pub name: String,
    /// Lowercase SHA-1 hex, the digest Anki's media map records.
    pub sha1: String,
}

/// Exact pre-state the checkpoint must cover, captured under the writer lease.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeManifest {
    pub schema_version: u8,
    pub requirement: CoverageRequirement,
    pub note_ids: Vec<i64>,
    pub cards: Vec<ScopeCard>,
    pub model_ids: Vec<i64>,
    pub media: Vec<ScopeMedia>,
}

const MAX_SCOPE_ENTRIES: usize = 100_000;

fn wire_id(id: i64) -> bool {
    (1..=9_007_199_254_740_991).contains(&id)
}

fn strictly_sorted<T: Ord>(values: impl Iterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|last| last >= &value) {
            return false;
        }
        previous = Some(value);
    }
    true
}

impl ScopeManifest {
    pub fn validate(&self) -> Result<(), String> {
        let invalid = || "CHECKPOINT_SCOPE_INVALID".to_owned();
        if self.schema_version != 1
            || self.note_ids.len() > MAX_SCOPE_ENTRIES
            || self.cards.len() > MAX_SCOPE_ENTRIES
            || self.model_ids.len() > MAX_SCOPE_ENTRIES
            || self.media.len() > MAX_SCOPE_ENTRIES
            || !self.note_ids.iter().copied().all(wire_id)
            || !self.model_ids.iter().copied().all(wire_id)
            || !strictly_sorted(self.note_ids.iter())
            || !strictly_sorted(self.model_ids.iter())
            || !strictly_sorted(self.cards.iter().map(|card| card.card_id))
            || !strictly_sorted(self.media.iter().map(|media| &media.name))
            || self.cards.iter().any(|card| {
                !wire_id(card.card_id)
                    || self.note_ids.binary_search(&card.note_id).is_err()
                    || card.review_count > card.reps.saturating_add(1_000_000)
            })
            || self.media.iter().any(|media| {
                media.name.is_empty()
                    || media.name == "."
                    || media.name == ".."
                    || media.name.len() > 255
                    || !is_nfc(&media.name)
                    || media.name.contains(['/', '\\', '\0'])
                    || media.sha1.len() != 40
                    || !media
                        .sha1
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || (self.requirement.media && self.media.is_empty() && !self.requirement.schema)
            || (self.requirement.scheduling && self.cards.is_empty() && !self.requirement.schema)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        linguist_core::canonical::digest("lab-checkpoint-scope-v1", self).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeReport {
    pub notes_verified: usize,
    pub cards_verified: usize,
    pub reviews_verified: u64,
    pub models_verified: usize,
    pub media_verified: usize,
    pub schema_included: bool,
    pub scheduling_included: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RestorationEvidence {
    pub method: &'static str,
    pub package_sha256: String,
    pub scope_digest: String,
    pub restored_collection_sha256: String,
    pub restored_note_count: u64,
    pub restored_card_count: u64,
    pub restored_review_count: u64,
    pub restored_media_files: usize,
    pub restored_media_verified: bool,
    pub scope: ScopeReport,
    pub anki_importer_used: bool,
    pub passed: bool,
}

fn table_exists(connection: &rusqlite::Connection, table: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count == 1)
        .map_err(|_| "CHECKPOINT_ANKI_SCHEMA_INVALID".into())
}

fn verify_scope(
    connection: &rusqlite::Connection,
    scope: &ScopeManifest,
    schema_version: u8,
) -> Result<ScopeReport, String> {
    let fail = |code: &str| -> String { code.to_owned() };
    for note in &scope.note_ids {
        let found: i64 = connection
            .query_row("SELECT count(*) FROM notes WHERE id=?1", [note], |r| {
                r.get(0)
            })
            .map_err(|_| fail("CHECKPOINT_ANKI_SCHEMA_INVALID"))?;
        if found != 1 {
            return Err(fail("CHECKPOINT_SCOPE_NOTE_MISSING"));
        }
    }
    let mut reviews = 0u64;
    for card in &scope.cards {
        let row: Option<(i64, i64)> = connection
            .query_row(
                "SELECT nid,reps FROM cards WHERE id=?1",
                [card.card_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => fail("CHECKPOINT_SCOPE_CARD_MISSING"),
                _ => fail("CHECKPOINT_ANKI_SCHEMA_INVALID"),
            })
            .map(Some)?;
        let (note, reps) = row.ok_or_else(|| fail("CHECKPOINT_SCOPE_CARD_MISSING"))?;
        if note != card.note_id {
            return Err(fail("CHECKPOINT_SCOPE_CARD_MISSING"));
        }
        let logged: i64 = connection
            .query_row(
                "SELECT count(*) FROM revlog WHERE cid=?1",
                [card.card_id],
                |r| r.get(0),
            )
            .map_err(|_| fail("CHECKPOINT_ANKI_SCHEMA_INVALID"))?;
        if reps < card.reps as i64 || logged < card.review_count as i64 {
            return Err(fail("CHECKPOINT_SCOPE_SCHEDULING_MISSING"));
        }
        reviews += logged as u64;
    }
    // Schema 15+ stores note types in a table; schema 11 keeps them in col.models JSON.
    let notetypes = table_exists(connection, "notetypes")?;
    let legacy_models: Option<serde_json::Value> = if notetypes || schema_version >= 15 {
        None
    } else {
        let text: String = connection
            .query_row("SELECT models FROM col WHERE id=1", [], |r| r.get(0))
            .map_err(|_| fail("CHECKPOINT_SCOPE_MODEL_MISSING"))?;
        Some(serde_json::from_str(&text).map_err(|_| fail("CHECKPOINT_SCOPE_MODEL_MISSING"))?)
    };
    for model in &scope.model_ids {
        let present = if notetypes {
            connection
                .query_row("SELECT count(*) FROM notetypes WHERE id=?1", [model], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(|_| fail("CHECKPOINT_ANKI_SCHEMA_INVALID"))?
                == 1
        } else {
            legacy_models
                .as_ref()
                .and_then(|models| models.get(model.to_string()))
                .is_some()
        };
        if !present {
            return Err(fail("CHECKPOINT_SCOPE_MODEL_MISSING"));
        }
    }
    let schema_included = notetypes || legacy_models.is_some();
    if scope.requirement.schema && !schema_included {
        return Err(fail("CHECKPOINT_SCOPE_SCHEMA_MISSING"));
    }
    Ok(ScopeReport {
        notes_verified: scope.note_ids.len(),
        cards_verified: scope.cards.len(),
        reviews_verified: reviews,
        models_verified: scope.model_ids.len(),
        media_verified: 0,
        schema_included,
        scheduling_included: true,
    })
}

fn verify_scope_media(scope: &ScopeManifest, declared: &[MediaEntry]) -> Result<usize, String> {
    for wanted in &scope.media {
        let found = declared.iter().find(|entry| entry.name == wanted.name);
        let hex = found.map(|entry| {
            entry
                .sha1
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        });
        if hex.as_deref() != Some(wanted.sha1.as_str()) {
            return Err("CHECKPOINT_SCOPE_MEDIA_MISSING".into());
        }
    }
    Ok(scope.media.len())
}
