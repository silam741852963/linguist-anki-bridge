//! Archive-only boundary: v1 has no task, sense, language, or collection identity.
//! A v1 file is never silently promoted to an apply-ready v2 document.
use crate::canonical::{self, ContractError};
use serde_json::Value;
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyArchive {
    original: Value,
    raw_json: Vec<u8>,
    /// Canonical structural digest for comparing equivalent v1 documents.
    pub digest: String,
    /// Exact input-byte digest for recovering the original file representation.
    pub raw_digest: String,
}
impl LegacyArchive {
    pub fn from_json(bytes: &[u8]) -> Result<Self, ContractError> {
        let original: Value = canonical::parse(bytes)?;
        if original.get("schema_version").and_then(Value::as_u64) != Some(1) {
            return Err(ContractError("UNSUPPORTED_LEGACY_VERSION".into()));
        }
        let digest = canonical::digest("legacy-v1-archive", &original)?;
        Ok(Self {
            original,
            raw_json: bytes.to_vec(),
            digest,
            raw_digest: canonical::asset_digest(bytes),
        })
    }
    pub fn original(&self) -> &Value {
        &self.original
    }
    pub fn to_v1_json(&self) -> Result<Vec<u8>, ContractError> {
        Ok(self.raw_json.clone())
    }
}
pub fn export_v1(_: &crate::LearningDocument) -> Result<Vec<u8>, ContractError> {
    Err(ContractError(
        "V1_UNREPRESENTABLE: v2 identity, tasks, evidence and field intents require v2".into(),
    ))
}
