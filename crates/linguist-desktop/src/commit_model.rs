use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotHistoryItem {
    pub snapshot_id: String,
    pub note_id: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitViewState {
    pub dry_run: bool,
    pub fields: Vec<PlannedChange>,
    pub media: Vec<String>,
    pub model_changed: bool,
    pub snapshots: Vec<SnapshotHistoryItem>,
}

impl CommitViewState {
    pub fn new() -> Self {
        Self {
            dry_run: true,
            ..Self::default()
        }
    }

    pub fn preview(
        &mut self,
        before: &BTreeMap<String, String>,
        after: &BTreeMap<String, String>,
        media: Vec<String>,
        model_changed: bool,
    ) {
        self.fields = after
            .iter()
            .filter_map(|(field, after)| {
                let before = before.get(field).cloned().unwrap_or_default();
                (before != *after).then(|| PlannedChange {
                    field: field.clone(),
                    before,
                    after: after.clone(),
                })
            })
            .collect();
        self.media = media;
        self.model_changed = model_changed;
    }

    pub fn record_snapshot(&mut self, snapshot_id: impl Into<String>, note_id: i64) {
        self.snapshots.insert(
            0,
            SnapshotHistoryItem {
                snapshot_id: snapshot_id.into(),
                note_id,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_dry_run_by_default_and_keeps_only_real_changes() {
        let mut view = CommitViewState::new();
        view.preview(
            &BTreeMap::from([("Meaning".into(), "old".into())]),
            &BTreeMap::from([
                ("Meaning".into(), "new".into()),
                ("Kanji".into(), "".into()),
            ]),
            vec!["new.jpg".into()],
            true,
        );
        assert!(view.dry_run);
        assert_eq!(view.fields.len(), 1);
        assert_eq!(view.fields[0].field, "Meaning");
        assert_eq!(view.media, ["new.jpg"]);
    }

    #[test]
    fn commit_receipt_becomes_restorable_history() {
        let mut view = CommitViewState::new();
        view.record_snapshot("snapshot-1", 42);
        assert_eq!(view.snapshots[0].note_id, 42);
    }
}
