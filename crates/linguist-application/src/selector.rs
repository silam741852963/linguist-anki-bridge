//! Immutable batch selectors and bounded metadata previews.

use crate::{NoteSummary, PortFuture};
use serde::{Deserialize, Serialize};

pub const MAX_SELECTOR_PREVIEW: usize = 200;
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchSelector {
    pub deck: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub model: Option<String>,
    pub template: Option<String>,
    pub query: String,
    pub tags: Vec<String>,
    pub image: ImageFilter,
    pub completion: CompletionFilter,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFilter {
    #[default]
    Any,
    HasImage,
    NoImage,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionFilter {
    #[default]
    Any,
    Incomplete,
    Complete,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorPreview {
    pub total: u64,
    pub notes: Vec<NoteSummary>,
    pub limited: bool,
}
pub trait SelectorMetadataPort: Send + Sync {
    fn count<'a>(&'a self, query: &'a str) -> PortFuture<'a, u64>;
    fn notes<'a>(&'a self, query: &'a str, limit: usize) -> PortFuture<'a, Vec<NoteSummary>>;
}
impl BatchSelector {
    pub fn query(&self) -> String {
        let mut terms = Vec::new();
        push(&mut terms, "deck", self.deck.as_deref());
        push(&mut terms, "note", self.model.as_deref());
        push(&mut terms, "card", self.template.as_deref());
        for tag in &self.tags {
            push(&mut terms, "tag", Some(tag));
        }
        if let Some(date) = &self.created_after {
            terms.push(format!("added:{}", date.trim()));
        }
        if let Some(date) = &self.created_before {
            terms.push(format!("-added:{}", date.trim()));
        }
        match self.image {
            ImageFilter::Any => {}
            ImageFilter::HasImage => terms.push("has:picture".into()),
            ImageFilter::NoImage => terms.push("-has:picture".into()),
        }
        match self.completion {
            CompletionFilter::Any => {}
            CompletionFilter::Incomplete => terms.push("is:new".into()),
            CompletionFilter::Complete => terms.push("-is:new".into()),
        }
        if !self.query.trim().is_empty() {
            terms.push(format!("({})", self.query.trim()))
        }
        terms.join(" ")
    }
}
pub async fn selector_preview<P: SelectorMetadataPort + ?Sized>(
    port: &P,
    selector: &BatchSelector,
) -> Result<SelectorPreview, crate::PortError> {
    let query = selector.query();
    let total = port.count(&query).await?;
    let notes = port.notes(&query, MAX_SELECTOR_PREVIEW).await?;
    Ok(SelectorPreview {
        limited: total > notes.len() as u64,
        total,
        notes,
    })
}
fn push(terms: &mut Vec<String>, prefix: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        terms.push(format!("{prefix}:\"{}\"", value.replace('"', "\\\"")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fake {
        limits: Mutex<Vec<usize>>,
    }
    impl SelectorMetadataPort for Fake {
        fn count<'a>(&'a self, _: &'a str) -> PortFuture<'a, u64> {
            Box::pin(async { Ok(201) })
        }
        fn notes<'a>(&'a self, _: &'a str, limit: usize) -> PortFuture<'a, Vec<NoteSummary>> {
            self.limits.lock().unwrap().push(limit);
            Box::pin(async {
                Ok(vec![NoteSummary {
                    note_id: 1,
                    expression: "word".into(),
                    deck_key: "deck".into(),
                    model_name: "model".into(),
                }])
            })
        }
    }
    #[test]
    fn selector_query_keeps_all_filters_immutable() {
        let selector = BatchSelector {
            deck: Some("Japanese \"Core\"".into()),
            model: Some("Vocab".into()),
            tags: vec!["review".into()],
            image: ImageFilter::HasImage,
            completion: CompletionFilter::Incomplete,
            query: "field:value".into(),
            ..Default::default()
        };
        let query = selector.query();
        assert!(query.contains("deck:\"Japanese \\\"Core\\\"\""));
        assert!(query.contains("note:\"Vocab\""));
        assert!(query.contains("tag:\"review\""));
        assert!(query.contains("has:picture"));
        assert!(query.contains("is:new"));
    }
    #[tokio::test]
    async fn preview_caps_metadata_fetch() {
        let port = Fake {
            limits: Mutex::new(Vec::new()),
        };
        let preview = selector_preview(&port, &BatchSelector::default())
            .await
            .unwrap();
        assert!(preview.limited);
        assert_eq!(*port.limits.lock().unwrap(), vec![MAX_SELECTOR_PREVIEW]);
    }
}
