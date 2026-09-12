//! Stable domain contracts shared by every Linguist Anki Bridge interface.
//!
//! This crate deliberately knows nothing about Qt, HTTP, SQLite, OCR engines,
//! or AnkiConnect. That keeps card semantics and recovery rules testable while
//! each Python integration is replaced independently.

mod card;
mod jobs;

pub use card::{
    CardDocument, CardMode, FieldMapping, LogicalFields, MappingError, MediaAsset, Provenance,
    SourceKind,
};
pub use jobs::{BatchItemState, BatchJobState, ClaimStage};

/// Version written into persisted interchange documents during the migration.
pub const CONTRACT_VERSION: u16 = 1;
