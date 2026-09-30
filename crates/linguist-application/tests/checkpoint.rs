use linguist_application::checkpoint::{PackageLimits, inspect_colpkg};
use prost::Message;
use sha1::Digest as _;
use std::{io::Write, time::Duration};

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

fn sqlite_header() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-sqlite-test-{}", uuid::Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .create_collation("unicase", |left, right| {
            unicase::UniCase::new(left).cmp(&unicase::UniCase::new(right))
        })
        .unwrap();
    connection.execute_batch("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT); CREATE INDEX test_unicase ON test(value COLLATE unicase); INSERT INTO test (value) VALUES ('test');").unwrap();
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

fn package(meta: &[u8], declared: &[u8], media: Option<&[u8]>, extra: Option<&str>) -> Vec<u8> {
    package_with_collection(meta, declared, media, extra, &sqlite_header())
}

fn package_with_collection(
    meta: &[u8],
    declared: &[u8],
    media: Option<&[u8]>,
    extra: Option<&str>,
    collection: &[u8],
) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entry = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    entry("meta", meta);
    entry(
        "collection.anki21b",
        &zstd::encode_all(collection, 0).unwrap(),
    );
    entry("collection.anki2", &sqlite_header());
    entry("media", &zstd::encode_all(declared, 0).unwrap());
    if let Some(data) = media {
        entry("0", &zstd::encode_all(data, 0).unwrap());
    }
    if let Some(name) = extra {
        entry(name, b"unexpected");
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn sqlite_corruption_fails_and_private_scratch_is_removed() {
    let dir = std::env::temp_dir().join(format!("lab-colpkg-scratch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let mut limits = limits();
    limits.scratch_dir = dir.clone();
    let good = sqlite_header();
    assert!(good.len() > 4096);
    let mut corrupt = good.clone();
    corrupt[4096] = 0xff;
    let result = inspect(
        &package_with_collection(&[8, 3], &[], None, None, &corrupt),
        limits.clone(),
    );
    assert_eq!(result.unwrap_err(), "CHECKPOINT_SQLITE_INTEGRITY_FAILED");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    let result = inspect(
        &package_with_collection(&[8, 3], &[], None, None, &good),
        limits,
    )
    .unwrap();
    assert_eq!(result["sqlite_integrity_verified"], true);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(dir).unwrap();
}

fn limits() -> PackageLimits {
    PackageLimits {
        max_package_bytes: 1024 * 1024,
        max_collection_bytes: 1024 * 1024,
        max_media_bytes: 1024 * 1024,
        max_media_map_bytes: 1024 * 1024,
        max_entries: 100,
        timeout: Duration::from_secs(5),
        scratch_dir: std::env::temp_dir(),
    }
}

fn inspect(bytes: &[u8], limits: PackageLimits) -> Result<serde_json::Value, String> {
    let path = std::env::temp_dir().join(format!("lab-colpkg-{}.colpkg", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).unwrap();
    let result = inspect_colpkg(&path, limits)
        .and_then(|report| serde_json::to_value(report).map_err(|e| e.to_string()));
    std::fs::remove_file(path).unwrap();
    result
}

#[test]
fn latest_container_checks_every_declared_media_byte_but_not_restore_eligibility() {
    let media = b"sound bytes";
    let map = MediaEntries {
        entries: vec![MediaEntry {
            name: "voice.ogg".into(),
            size: media.len() as u32,
            sha1: sha1::Sha1::digest(media).to_vec(),
        }],
    }
    .encode_to_vec();
    let result = inspect(&package(&[8, 3], &map, Some(media), None), limits()).unwrap();
    assert_eq!(result["format"], "anki-colpkg-latest-v3");
    assert_eq!(result["declared_media_files"], 1);
    assert_eq!(result["declared_media_bytes"], media.len());
    assert_eq!(result["container_and_declared_media_verified"], true);
    assert_eq!(result["sqlite_integrity_verified"], true);
    for flag in [
        "collection_scope_verified",
        "restoration_tested",
        "checkpoint_eligible",
    ] {
        assert_eq!(result[flag], false);
    }
}

#[test]
fn invalid_scratch_directory_fails_without_persistent_file() {
    let mut limits = limits();
    limits.scratch_dir = std::env::temp_dir().join(format!("lab-missing-{}", uuid::Uuid::new_v4()));
    let result = inspect(&package(&[8, 3], &[], None, None), limits);
    assert_eq!(result.unwrap_err(), "CHECKPOINT_SCRATCH_DIR_UNAVAILABLE");
}

#[test]
fn missing_changed_or_unsafe_media_and_corrupt_packages_fail() {
    let media = b"sound bytes";
    let correct = MediaEntries {
        entries: vec![MediaEntry {
            name: "voice.ogg".into(),
            size: media.len() as u32,
            sha1: sha1::Sha1::digest(media).to_vec(),
        }],
    };
    let map = correct.encode_to_vec();
    assert!(inspect(&package(&[8, 3], &map, None, None), limits()).is_err());
    let mut map_bound = limits();
    map_bound.max_entries = 4;
    assert_eq!(
        inspect(&package(&[8, 3], &map, None, None), map_bound).unwrap_err(),
        "CHECKPOINT_MEDIA_MAP_LIMIT"
    );
    let mut changed = correct.clone();
    changed.entries[0].sha1[0] ^= 1;
    assert!(
        inspect(
            &package(&[8, 3], &changed.encode_to_vec(), Some(media), None),
            limits()
        )
        .is_err()
    );
    changed = correct.clone();
    changed.entries[0].name = "../escape.ogg".into();
    assert!(
        inspect(
            &package(&[8, 3], &changed.encode_to_vec(), Some(media), None),
            limits()
        )
        .is_err()
    );
    assert!(inspect(&package(&[8, 2], &map, Some(media), None), limits()).is_err());
    assert!(
        inspect(
            &package(&[8, 3], &map, Some(media), Some("../escape")),
            limits()
        )
        .is_err()
    );
    let mut truncated = package(&[8, 3], &map, Some(media), None);
    truncated.truncate(truncated.len() - 24);
    assert!(inspect(&truncated, limits()).is_err());
    let mut small = limits();
    small.max_media_bytes = media.len() as u64 - 1;
    assert!(inspect(&package(&[8, 3], &map, Some(media), None), small).is_err());
}
