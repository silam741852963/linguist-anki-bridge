//! Immutable batch selectors and bounded metadata previews.

use crate::{NoteSummary, PortFuture};
use chrono::{Local, NaiveDate};
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
    #[serde(default)]
    pub excluded_tags: Vec<String>,
    pub image: ImageFilter,
    pub completion: CompletionFilter,
    #[serde(default)]
    pub limit: usize,
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
    fn preview<'a>(
        &'a self,
        selector: &'a BatchSelector,
        limit: usize,
    ) -> PortFuture<'a, SelectorPreview>;
}
impl BatchSelector {
    pub fn query(&self) -> Result<String, String> {
        self.query_on(Local::now().date_naive())
    }

    pub fn query_on(&self, today: NaiveDate) -> Result<String, String> {
        let mut terms = Vec::new();
        push(&mut terms, "deck", self.deck.as_deref());
        push(&mut terms, "note", self.model.as_deref());
        push(&mut terms, "card", self.template.as_deref());
        for tag in &self.tags {
            push(&mut terms, "tag", Some(tag));
        }
        for tag in &self.excluded_tags {
            push(&mut terms, "-tag", Some(tag));
        }
        append_date_range(&mut terms, self, today)?;
        match self.completion {
            CompletionFilter::Any => {}
            CompletionFilter::Incomplete => terms.push("is:new".into()),
            CompletionFilter::Complete => terms.push("-is:new".into()),
        }
        if !self.query.trim().is_empty() {
            terms.push(format!("({})", self.query.trim()))
        }
        Ok(terms.join(" "))
    }
}
pub async fn selector_preview<P: SelectorMetadataPort + ?Sized>(
    port: &P,
    selector: &BatchSelector,
) -> Result<SelectorPreview, crate::PortError> {
    port.preview(selector, MAX_SELECTOR_PREVIEW).await
}
fn push(terms: &mut Vec<String>, prefix: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        terms.push(format!("{prefix}:\"{}\"", value.replace('"', "\\\"")));
    }
}

fn append_date_range(
    terms: &mut Vec<String>,
    selector: &BatchSelector,
    today: NaiveDate,
) -> Result<(), String> {
    let from = parse_optional_date(selector.created_after.as_deref(), "From")?;
    let to = parse_optional_date(selector.created_before.as_deref(), "To")?;
    if from.is_some() != to.is_some() {
        return Err("Both From and To dates are required".into());
    }
    let (Some(from), Some(to)) = (from, to) else {
        return Ok(());
    };
    if from > to {
        return Err("From date must not be later than To date".into());
    }
    if to > today {
        return Err("To date cannot be in the future".into());
    }
    terms.push(format!("added:{}", (today - from).num_days() + 1));
    if to < today {
        terms.push(format!("-added:{}", (today - to).num_days()));
    }
    Ok(())
}

fn parse_optional_date(value: Option<&str>, label: &str) -> Result<Option<NaiveDate>, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| format!("{label} date must use YYYY-MM-DD"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fake {
        limits: Mutex<Vec<usize>>,
    }
    impl SelectorMetadataPort for Fake {
        fn preview<'a>(
            &'a self,
            _: &'a BatchSelector,
            limit: usize,
        ) -> PortFuture<'a, SelectorPreview> {
            self.limits.lock().unwrap().push(limit);
            Box::pin(async {
                Ok(SelectorPreview {
                    total: 201,
                    limited: true,
                    notes: vec![NoteSummary {
                        note_id: 1,
                        expression: "word".into(),
                        deck_key: "deck".into(),
                        model_name: "model".into(),
                    }],
                })
            })
        }
    }
    #[test]
    fn selector_query_keeps_all_filters_immutable() {
        let selector = BatchSelector {
            deck: Some("Japanese \"Core\"".into()),
            model: Some("Vocab".into()),
            tags: vec!["review".into()],
            excluded_tags: vec!["blocked".into()],
            image: ImageFilter::HasImage,
            completion: CompletionFilter::Incomplete,
            query: "field:value".into(),
            ..Default::default()
        };
        let query = selector
            .query_on(NaiveDate::from_ymd_opt(2026, 9, 21).unwrap())
            .unwrap();
        assert!(query.contains("deck:\"Japanese \\\"Core\\\"\""));
        assert!(query.contains("note:\"Vocab\""));
        assert!(query.contains("tag:\"review\""));
        assert!(query.contains("-tag:\"blocked\""));
        assert!(!query.contains("picture"));
        assert!(query.contains("is:new"));
    }
    #[test]
    fn dates_become_exact_inclusive_anki_added_range() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();
        let selector = BatchSelector {
            created_after: Some("2026-09-01".into()),
            created_before: Some("2026-09-14".into()),
            ..Default::default()
        };
        assert_eq!(selector.query_on(today).unwrap(), "added:21 -added:7");
        assert!(
            BatchSelector {
                created_after: Some("2026-09-22".into()),
                created_before: Some("2026-09-22".into()),
                ..Default::default()
            }
            .query_on(today)
            .unwrap_err()
            .contains("future")
        );
        assert!(
            BatchSelector {
                created_after: Some("2026/09/01".into()),
                created_before: Some("2026-09-02".into()),
                ..Default::default()
            }
            .query_on(today)
            .unwrap_err()
            .contains("YYYY-MM-DD")
        );
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
