//! Stable domain contracts shared by every Linguist Anki Bridge interface.
//!
//! This crate deliberately knows nothing about Qt, HTTP, SQLite, OCR engines,
//! or AnkiConnect. That keeps card semantics and recovery rules testable while
//! each Python integration is replaced independently.

mod card;
mod expression;
mod jobs;
mod managed_template;
mod persistence;

pub use card::{
    AudioAsset, CardBuildInput, CardDocument, CardMode, DictionaryData, ExamplePair, FieldMapping,
    LlmResponse, LogicalFields, MappingError, MediaAsset, ProcessedCardData, Provenance,
    RenamedImage, SourceKind, build_card_document,
};
pub use expression::normalize_expression;
pub use jobs::{BatchItemState, BatchJobState, ClaimStage};
pub use managed_template::{
    ManagedModelSpec, ManagedTemplateError, ManagedTemplatePlan, ModelTemplate, ObservedModel,
    japanese_vocab_spec, plan_japanese_vocab_template,
};
pub use persistence::{
    BatchArtifactReference, BatchItemContract, BatchJobContract, BatchJobDocument, Extensions,
    SnapshotContract, SnapshotDocument, SnapshotOriginalNote,
};

/// Version written into persisted interchange documents during the migration.
pub const CONTRACT_VERSION: u16 = 1;
