//! Use-case boundaries for the native rewrite.
//!
//! Adapters for AnkiConnect, dictionaries, OCR, Ollama, media, and SQLite
//! implement these ports. The Qt layer consumes application events and does
//! not call providers directly.

use std::{future::Future, pin::Pin};

use linguist_core::CardDocument;

pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, PortError>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortError {
    pub operation: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for PortError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for PortError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSummary {
    pub note_id: i64,
    pub expression: String,
    pub deck_key: String,
    pub model_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceNote {
    pub summary: NoteSummary,
    pub fields: Vec<(String, String)>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReceipt {
    pub note_id: i64,
    pub snapshot_id: String,
}

pub trait AnkiPort: Send + Sync {
    fn note<'a>(&'a self, note_id: i64) -> PortFuture<'a, SourceNote>;
    fn find_exact<'a>(
        &'a self,
        deck_key: &'a str,
        expression: &'a str,
    ) -> PortFuture<'a, Option<SourceNote>>;
    fn commit<'a>(
        &'a self,
        source: Option<&'a SourceNote>,
        document: &'a CardDocument,
    ) -> PortFuture<'a, CommitReceipt>;
    fn restore<'a>(&'a self, snapshot_id: &'a str) -> PortFuture<'a, ()>;
}

pub trait EnrichmentPort: Send + Sync {
    fn modernize<'a>(&'a self, note: &'a SourceNote) -> PortFuture<'a, CardDocument>;
    fn inject<'a>(
        &'a self,
        deck_key: &'a str,
        expression: &'a str,
        context: &'a str,
    ) -> PortFuture<'a, CardDocument>;
}

pub trait JobPort: Send + Sync {
    fn pause<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
    fn resume<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
    fn retry_failures<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, u64>;
    fn rollback<'a>(&'a self, job_id: &'a str) -> PortFuture<'a, ()>;
}
