//! CLI domain contracts. No transport, UI, Anki database or filesystem mutation.
pub mod approval;
pub mod canonical;
pub mod document;
pub mod editing;
pub mod inspection;
pub mod legacy;
pub mod model;
pub mod plan_validation;
pub mod records;
pub mod render;
pub mod review;
pub mod validation;
pub use document::*;
pub use validation::{Issue, Severity, validate};
