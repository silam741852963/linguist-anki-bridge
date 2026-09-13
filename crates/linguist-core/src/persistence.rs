//! Versioned persistence contracts shared with the Python migration adapter.
//!
//! These types describe data only. SQLite, file I/O, and migration policy stay
//! in adapters so opening the desktop application cannot alter legacy data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Extensions = BTreeMap<String, Value>;

/// A batch export has its own envelope so future versions can migrate jobs
/// independently of card documents and snapshots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchJobDocument {
    pub schema_version: u16,
    pub job: BatchJobContract,
    #[serde(default)]
    pub items: Vec<BatchItemContract>,
}

impl BatchJobDocument {
    pub fn is_current_version(&self) -> bool {
        self.schema_version == crate::CONTRACT_VERSION
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchJobContract {
    pub id: String,
    pub deck_key: String,
    #[serde(default)]
    pub deck_name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub last_error: String,
    /// Unknown Python database columns are deliberately retained here until a
    /// later native migration understands them.
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchItemContract {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub ordinal: Option<i64>,
    pub note_id: i64,
    pub word: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub next_attempt_at: Option<String>,
    #[serde(default)]
    pub artifact: Option<BatchArtifactReference>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub result_note_id: Option<i64>,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
}

/// The processed artifact stays external to the database export. Its stable
/// reference makes interrupted commits reusable without embedding media blobs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchArtifactReference {
    pub reference: String,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotDocument {
    pub schema_version: u16,
    pub snapshot: SnapshotContract,
}

impl SnapshotDocument {
    pub fn is_current_version(&self) -> bool {
        self.schema_version == crate::CONTRACT_VERSION
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotContract {
    pub id: String,
    #[serde(default)]
    pub created_at: String,
    pub word: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub deck_key: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub original_note: Option<SnapshotOriginalNote>,
    /// The legacy processed result is opaque until deterministic processing is
    /// ported, so it is preserved byte-for-byte as JSON data.
    #[serde(default)]
    pub processed: Value,
    #[serde(default)]
    pub media_before: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub result_note_id: Option<i64>,
    #[serde(default)]
    pub created_note_ids: Vec<i64>,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub reverted_at: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotOriginalNote {
    #[serde(default)]
    pub note_id: Option<i64>,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub deck_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub extensions: Extensions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_batch_fixture_preserves_extensions_and_artifact_reference() {
        let document: BatchJobDocument = serde_json::from_str(include_str!(
            "../../../contracts/fixtures/batch-job.v1.json"
        ))
        .unwrap();
        assert!(document.is_current_version());
        assert_eq!(document.job.extensions["legacy_priority"], "overnight");
        assert_eq!(
            document.items[0].artifact.as_ref().unwrap().reference,
            "jobs_artifacts/batch-1/1.json"
        );
        assert_eq!(document.items[0].extensions["legacy_stage"], "ocr-complete");
    }

    #[test]
    fn python_snapshot_fixture_retains_legacy_note_data() {
        let document: SnapshotDocument =
            serde_json::from_str(include_str!("../../../contracts/fixtures/snapshot.v1.json"))
                .unwrap();
        assert!(document.is_current_version());
        assert_eq!(document.snapshot.extensions["legacy_sync_marker"], "v0");
        assert_eq!(
            document.snapshot.original_note.unwrap().extensions["legacy_guid"],
            "note-guid-42"
        );
    }
}
