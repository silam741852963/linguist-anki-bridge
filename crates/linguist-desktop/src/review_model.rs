#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueState {
    Loading,
    Empty,
    Error(String),
    Ready,
}

impl QueueState {
    pub fn label(&self) -> String {
        match self {
            Self::Loading => "Loading".into(),
            Self::Empty => "Empty".into(),
            Self::Error(_) => "Error".into(),
            Self::Ready => "Ready".into(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Loading => "Loading review queue…".into(),
            Self::Empty => "No cards match this view.".into(),
            Self::Error(message) => message.clone(),
            Self::Ready => String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Constructed by the Anki queue adapter, added after this UI boundary.
pub enum ReviewState {
    Ready,
    NeedsReview,
    Failed,
}

impl ReviewState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NeedsReview => "Needs review",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRow {
    pub note_id: i64,
    pub expression: String,
    pub detail: String,
    pub state: ReviewState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewQueueData {
    pub decks: Vec<String>,
    pub rows: Vec<ReviewRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewQueueModel {
    state: QueueState,
    decks: Vec<String>,
    rows: Vec<ReviewRow>,
    selected_note_id: Option<i64>,
    selected_deck: Option<String>,
}

impl Default for ReviewQueueModel {
    fn default() -> Self {
        Self {
            state: QueueState::Loading,
            decks: Vec::new(),
            rows: Vec::new(),
            selected_note_id: None,
            selected_deck: None,
        }
    }
}

impl ReviewQueueModel {
    pub fn state(&self) -> &QueueState {
        &self.state
    }

    pub fn decks(&self) -> &[String] {
        &self.decks
    }

    pub fn rows(&self) -> &[ReviewRow] {
        &self.rows
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_note_id
            .and_then(|note_id| self.rows.iter().position(|row| row.note_id == note_id))
    }

    pub fn selected_deck_index(&self) -> Option<usize> {
        self.selected_deck
            .as_ref()
            .and_then(|deck| self.decks.iter().position(|candidate| candidate == deck))
    }

    pub fn begin_loading(&mut self) {
        self.state = QueueState::Loading;
    }

    pub fn replace(&mut self, data: ReviewQueueData) {
        let old_selection = self.selected_note_id;
        self.decks = data.decks;
        self.rows = data.rows;
        self.state = if self.rows.is_empty() {
            QueueState::Empty
        } else {
            QueueState::Ready
        };
        self.selected_note_id = old_selection
            .filter(|note_id| self.rows.iter().any(|row| row.note_id == *note_id))
            .or_else(|| self.rows.first().map(|row| row.note_id));
        self.selected_deck = self
            .selected_deck
            .take()
            .filter(|deck| self.decks.iter().any(|candidate| candidate == deck))
            .or_else(|| self.decks.first().cloned());
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.rows.clear();
        self.state = QueueState::Error(message.into());
        self.selected_note_id = None;
    }

    pub fn select_index(&mut self, index: usize) -> Option<&ReviewRow> {
        let row = self.rows.get(index)?;
        self.selected_note_id = Some(row.note_id);
        self.rows.get(index)
    }

    pub fn select_deck_index(&mut self, index: usize) -> Option<&str> {
        let deck = self.decks.get(index)?;
        self.selected_deck = Some(deck.clone());
        self.selected_deck.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(note_id: i64, state: ReviewState) -> ReviewRow {
        ReviewRow {
            note_id,
            expression: format!("card {note_id}"),
            detail: "Japanese · vocabulary".into(),
            state,
        }
    }

    #[test]
    fn refresh_keeps_selection_when_note_still_exists() {
        let mut model = ReviewQueueModel::default();
        model.replace(ReviewQueueData {
            decks: vec!["Japanese".into()],
            rows: vec![row(1, ReviewState::Ready), row(2, ReviewState::NeedsReview)],
        });
        model.select_index(1);
        model.replace(ReviewQueueData {
            decks: vec!["Japanese".into(), "English".into()],
            rows: vec![
                row(2, ReviewState::NeedsReview),
                row(3, ReviewState::Failed),
            ],
        });
        assert_eq!(model.selected_index(), Some(0));
        assert_eq!(model.rows()[0].note_id, 2);
    }

    #[test]
    fn exposes_loading_empty_error_ready_and_row_states() {
        let mut model = ReviewQueueModel::default();
        assert_eq!(model.state(), &QueueState::Loading);
        model.replace(ReviewQueueData {
            decks: vec![],
            rows: vec![],
        });
        assert_eq!(model.state(), &QueueState::Empty);
        model.fail("Anki offline");
        assert_eq!(model.state().message(), "Anki offline");
        model.replace(ReviewQueueData {
            decks: vec!["Japanese".into()],
            rows: vec![
                row(1, ReviewState::Ready),
                row(2, ReviewState::NeedsReview),
                row(3, ReviewState::Failed),
            ],
        });
        assert_eq!(model.state(), &QueueState::Ready);
        assert_eq!(model.rows()[1].state.label(), "Needs review");
        assert_eq!(model.rows()[2].state.label(), "Failed");
    }

    #[test]
    fn fifty_thousand_rows_stay_index_addressable_without_qml_data_copy() {
        let mut model = ReviewQueueModel::default();
        model.replace(ReviewQueueData {
            decks: vec!["Japanese".into()],
            rows: (0..50_000)
                .map(|note_id| row(note_id, ReviewState::NeedsReview))
                .collect(),
        });
        assert_eq!(model.rows().len(), 50_000);
        assert_eq!(model.rows()[49_999].note_id, 49_999);
    }
}
