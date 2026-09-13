//! Additive, crash-safe native snapshot storage.
//!
//! Python's `card_snapshots.json` remains read-only history. New native v1
//! records live in one atomic file per snapshot under the same XDG config root.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use linguist_core::{Extensions, SnapshotDocument};
use serde_json::{Map, Value};

const LEGACY_FILE: &str = "card_snapshots.json";
const NATIVE_DIRECTORY: &str = "snapshots-v1";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotLoad {
    pub snapshots: Vec<SnapshotDocument>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum SnapshotRepositoryError {
    ConfigDirectoryUnavailable,
    InvalidId(String),
    DuplicateId(String),
    InvalidDocument(String),
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for SnapshotRepositoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigDirectoryUnavailable => {
                write!(formatter, "XDG config directory unavailable")
            }
            Self::InvalidId(id) => write!(formatter, "unsafe snapshot id: {id}"),
            Self::DuplicateId(id) => write!(formatter, "snapshot already exists: {id}"),
            Self::InvalidDocument(message) => write!(formatter, "invalid snapshot: {message}"),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SnapshotRepositoryError {}

impl From<io::Error> for SnapshotRepositoryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SnapshotRepositoryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug)]
pub struct SnapshotRepository {
    config_dir: PathBuf,
}

impl SnapshotRepository {
    pub fn default_location() -> Result<Self, SnapshotRepositoryError> {
        let base = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or(SnapshotRepositoryError::ConfigDirectoryUnavailable)?;
        Ok(Self::at_config_dir(base.join("linguist-anki-bridge")))
    }

    pub fn at_config_dir(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    pub fn legacy_path(&self) -> PathBuf {
        self.config_dir.join(LEGACY_FILE)
    }

    pub fn native_dir(&self) -> PathBuf {
        self.config_dir.join(NATIVE_DIRECTORY)
    }

    /// Load both sources. Corrupt legacy trailing bytes and individual native
    /// files produce warnings, never hide recoverable earlier snapshots.
    pub fn load(&self) -> Result<SnapshotLoad, SnapshotRepositoryError> {
        let mut snapshots = Vec::new();
        let mut warnings = Vec::new();
        self.load_legacy(&mut snapshots, &mut warnings)?;
        self.load_native(&mut snapshots, &mut warnings)?;
        snapshots.sort_by(|left, right| {
            right
                .snapshot
                .created_at
                .cmp(&left.snapshot.created_at)
                .then_with(|| right.snapshot.id.cmp(&left.snapshot.id))
        });
        Ok(SnapshotLoad {
            snapshots,
            warnings,
        })
    }

    /// Persist one new v1 record. Existing Python history and prior native
    /// records are never modified or deleted.
    pub fn save(&self, document: &SnapshotDocument) -> Result<(), SnapshotRepositoryError> {
        if !document.is_current_version() {
            return Err(SnapshotRepositoryError::InvalidDocument(
                "unsupported schema version".into(),
            ));
        }
        let id = &document.snapshot.id;
        validate_id(id)?;
        let directory = self.native_dir();
        fs::create_dir_all(&directory)?;
        let destination = directory.join(format!("{id}.json"));
        if destination.exists() {
            return Err(SnapshotRepositoryError::DuplicateId(id.clone()));
        }
        let temporary = directory.join(format!(".{}-{}.tmp", id, temp_suffix()));
        let write_result = (|| -> Result<(), SnapshotRepositoryError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            serde_json::to_writer_pretty(&mut file, document)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::hard_link(&temporary, &destination).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    SnapshotRepositoryError::DuplicateId(id.clone())
                } else {
                    SnapshotRepositoryError::Io(error)
                }
            })?;
            fs::remove_file(&temporary)?;
            sync_directory(&directory)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    fn load_legacy(
        &self,
        snapshots: &mut Vec<SnapshotDocument>,
        warnings: &mut Vec<String>,
    ) -> Result<(), SnapshotRepositoryError> {
        let path = self.legacy_path();
        if !path.exists() {
            return Ok(());
        }
        let bytes = fs::read(&path)?;
        let mut stream = serde_json::Deserializer::from_slice(&bytes).into_iter::<Vec<Value>>();
        match stream.next() {
            Some(Ok(records)) => {
                if !bytes[stream.byte_offset()..]
                    .iter()
                    .all(u8::is_ascii_whitespace)
                {
                    warnings.push(format!(
                        "ignored corrupt trailing data in {}",
                        path.display()
                    ));
                }
                for (index, record) in records.into_iter().enumerate() {
                    match legacy_document(record) {
                        Ok(document) => snapshots.push(document),
                        Err(error) => warnings.push(format!(
                            "ignored legacy snapshot {} record {}: {error}",
                            path.display(),
                            index + 1
                        )),
                    }
                }
            }
            Some(Err(error)) => warnings.push(format!(
                "ignored corrupt legacy snapshots {}: {error}",
                path.display()
            )),
            None => warnings.push(format!("ignored empty legacy snapshots {}", path.display())),
        }
        Ok(())
    }

    fn load_native(
        &self,
        snapshots: &mut Vec<SnapshotDocument>,
        warnings: &mut Vec<String>,
    ) -> Result<(), SnapshotRepositoryError> {
        let directory = self.native_dir();
        if !directory.exists() {
            return Ok(());
        }
        let mut paths = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            match fs::read(&path)
                .map_err(SnapshotRepositoryError::Io)
                .and_then(|bytes| {
                    serde_json::from_slice::<SnapshotDocument>(&bytes).map_err(Into::into)
                }) {
                Ok(document) if document.is_current_version() => snapshots.push(document),
                Ok(_) => warnings.push(format!("ignored unsupported snapshot {}", path.display())),
                Err(error) => warnings.push(format!(
                    "ignored corrupt snapshot {}: {error}",
                    path.display()
                )),
            }
        }
        Ok(())
    }
}

fn legacy_document(value: Value) -> Result<SnapshotDocument, SnapshotRepositoryError> {
    let mut raw = value.as_object().cloned().ok_or_else(|| {
        SnapshotRepositoryError::InvalidDocument("legacy record is not an object".into())
    })?;
    let original_note = raw
        .remove("original_note")
        .map(convert_original_note)
        .transpose()?;
    let extensions = take_extensions(&mut raw, SNAPSHOT_FIELDS)?;
    let mut snapshot = Map::new();
    for field in SNAPSHOT_FIELDS {
        if let Some(value) = raw.remove(*field) {
            snapshot.insert((*field).into(), value);
        }
    }
    snapshot.insert("original_note".into(), original_note.unwrap_or(Value::Null));
    snapshot.insert("extensions".into(), serde_json::to_value(extensions)?);
    let envelope = Value::Object(Map::from_iter([
        (
            "schema_version".into(),
            Value::from(linguist_core::CONTRACT_VERSION),
        ),
        ("snapshot".into(), Value::Object(snapshot)),
    ]));
    serde_json::from_value(envelope).map_err(Into::into)
}

fn convert_original_note(value: Value) -> Result<Value, SnapshotRepositoryError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let mut raw = value.as_object().cloned().ok_or_else(|| {
        SnapshotRepositoryError::InvalidDocument("original_note is not an object".into())
    })?;
    let extensions = take_extensions(&mut raw, ORIGINAL_NOTE_FIELDS)?;
    raw.insert("extensions".into(), serde_json::to_value(extensions)?);
    Ok(Value::Object(raw))
}

fn take_extensions(
    raw: &mut Map<String, Value>,
    known: &[&str],
) -> Result<Extensions, SnapshotRepositoryError> {
    let supplied = raw
        .remove("extensions")
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut extensions: Extensions = serde_json::from_value(supplied).map_err(|_| {
        SnapshotRepositoryError::InvalidDocument("extensions must be an object".into())
    })?;
    let extras = std::mem::take(raw);
    for (name, value) in extras {
        if !known.contains(&name.as_str()) {
            extensions.insert(name, value);
        } else {
            raw.insert(name, value);
        }
    }
    if known.iter().any(|name| extensions.contains_key(*name)) {
        return Err(SnapshotRepositoryError::InvalidDocument(
            "extensions replace known fields".into(),
        ));
    }
    Ok(extensions)
}

const SNAPSHOT_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "word",
    "mode",
    "deck_key",
    "dry_run",
    "status",
    "processed",
    "media_before",
    "result_note_id",
    "created_note_ids",
    "error",
    "reverted_at",
];
const ORIGINAL_NOTE_FIELDS: &[&str] = &["note_id", "model_name", "deck_name", "tags", "fields"];

fn validate_id(id: &str) -> Result<(), SnapshotRepositoryError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SnapshotRepositoryError::InvalidId(id.into()));
    }
    Ok(())
}

fn temp_suffix() -> String {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}-{sequence}", std::process::id())
}

fn sync_directory(directory: &Path) -> Result<(), SnapshotRepositoryError> {
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use linguist_core::{SnapshotContract, SnapshotOriginalNote};
    use serde_json::json;

    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("linguist-snapshots-{name}-{}", temp_suffix()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn document(id: &str) -> SnapshotDocument {
        SnapshotDocument {
            schema_version: linguist_core::CONTRACT_VERSION,
            snapshot: SnapshotContract {
                id: id.into(),
                created_at: "2026-09-13T10:00:00+00:00".into(),
                word: "俳優".into(),
                mode: "modernize".into(),
                deck_key: "japanese_vocab".into(),
                dry_run: false,
                status: "committed".into(),
                original_note: Some(SnapshotOriginalNote {
                    note_id: Some(42),
                    model_name: "Legacy".into(),
                    deck_name: "Japanese".into(),
                    tags: vec!["old".into()],
                    fields: BTreeMap::from([("Word".into(), "俳優".into())]),
                    extensions: Extensions::new(),
                }),
                processed: json!({"word": "俳優"}),
                media_before: BTreeMap::from([("image.jpg".into(), Some("old-b64".into()))]),
                result_note_id: Some(42),
                created_note_ids: vec![],
                error: String::new(),
                reverted_at: None,
                extensions: Extensions::new(),
            },
        }
    }

    #[test]
    fn loads_python_history_without_rewriting_it_and_saves_native_v1() {
        let directory = temporary_directory("legacy");
        let legacy = json!([{
            "id": "20260913-legacy", "created_at": "2026-09-12T10:00:00+00:00",
            "word": "俳優", "mode": "modernize", "deck_key": "japanese_vocab",
            "dry_run": false, "status": "committed",
            "original_note": {"note_id": 42, "model_name": "Legacy", "deck_name": "Japanese", "tags": ["old"], "fields": {"Word": "俳優"}, "legacy_guid": "guid-42"},
            "processed": {"word": "俳優"}, "media_before": {"old.jpg": "old-b64"},
            "result_note_id": 42, "created_note_ids": [], "error": "", "legacy_marker": "keep"
        }]);
        let path = directory.join(LEGACY_FILE);
        let bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(&path, &bytes).unwrap();
        let repository = SnapshotRepository::at_config_dir(&directory);
        let loaded = repository.load().unwrap();
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(
            loaded.snapshots[0].snapshot.extensions["legacy_marker"],
            "keep"
        );
        assert_eq!(
            loaded.snapshots[0]
                .snapshot
                .original_note
                .as_ref()
                .unwrap()
                .extensions["legacy_guid"],
            "guid-42"
        );
        repository.save(&document("20260913-native")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(
            repository
                .native_dir()
                .join("20260913-native.json")
                .exists()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovers_valid_legacy_records_before_corrupt_trailing_bytes() {
        let directory = temporary_directory("trailing");
        let path = directory.join(LEGACY_FILE);
        fs::write(
            &path,
            r#"[{"id":"one","word":"一","original_note":null}] corrupt-tail"#,
        )
        .unwrap();
        let loaded = SnapshotRepository::at_config_dir(&directory)
            .load()
            .unwrap();
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(loaded.snapshots[0].snapshot.id, "one");
        assert_eq!(loaded.warnings.len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ignores_interrupted_temp_files_and_preserves_media_and_injected_snapshots() {
        let directory = temporary_directory("interrupted");
        let repository = SnapshotRepository::at_config_dir(&directory);
        fs::create_dir_all(repository.native_dir()).unwrap();
        fs::write(repository.native_dir().join(".interrupted.tmp"), b"partial").unwrap();
        let mut injected = document("20260913-injected");
        injected.snapshot.original_note = None;
        injected.snapshot.result_note_id = Some(88);
        injected
            .snapshot
            .media_before
            .insert("new.mp3".into(), None);
        repository.save(&injected).unwrap();
        let loaded = repository.load().unwrap();
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(loaded.snapshots[0].snapshot.result_note_id, Some(88));
        assert_eq!(
            loaded.snapshots[0].snapshot.media_before["image.jpg"],
            Some("old-b64".into())
        );
        assert_eq!(loaded.snapshots[0].snapshot.media_before["new.mp3"], None);
        assert!(loaded.warnings.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unsafe_or_duplicate_ids() {
        let directory = temporary_directory("ids");
        let repository = SnapshotRepository::at_config_dir(&directory);
        assert!(matches!(
            repository.save(&document("../escape")),
            Err(SnapshotRepositoryError::InvalidId(_))
        ));
        repository.save(&document("one")).unwrap();
        assert!(matches!(
            repository.save(&document("one")),
            Err(SnapshotRepositoryError::DuplicateId(_))
        ));
        assert!(Path::new(&repository.native_dir().join("one.json")).exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
