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
    pub preview_ready: bool,
    pub fields: Vec<PlannedChange>,
    pub media: Vec<String>,
    pub model_changed: bool,
    pub snapshots: Vec<SnapshotHistoryItem>,
}

pub trait CommitExecutor<Request> {
    fn preview(&mut self, request: &Request) -> Result<CommitPreview, String>;
    fn apply(&mut self, request: &Request) -> Result<SnapshotHistoryItem, String>;
    fn restore(&mut self, snapshot_id: &str) -> Result<(), String>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitPreview {
    pub before: BTreeMap<String, String>,
    pub after: BTreeMap<String, String>,
    pub media: Vec<String>,
    pub model_changed: bool,
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
        self.preview_ready = true;
    }

    pub fn invalidate_preview(&mut self) {
        self.dry_run = true;
        self.preview_ready = false;
        self.fields.clear();
        self.media.clear();
        self.model_changed = false;
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

    pub fn preview_with<Request, Executor: CommitExecutor<Request>>(
        &mut self,
        executor: &mut Executor,
        request: &Request,
    ) -> Result<(), String> {
        self.invalidate_preview();
        let preview = executor.preview(request)?;
        self.preview(
            &preview.before,
            &preview.after,
            preview.media,
            preview.model_changed,
        );
        self.dry_run = true;
        Ok(())
    }

    pub fn apply_with<Request, Executor: CommitExecutor<Request>>(
        &mut self,
        executor: &mut Executor,
        request: &Request,
    ) -> Result<(), String> {
        if !self.preview_ready {
            return Err("Preview changes before applying them".into());
        }
        let snapshot = executor.apply(request)?;
        self.dry_run = false;
        self.preview_ready = false;
        self.snapshots.insert(0, snapshot);
        Ok(())
    }

    pub fn restore_with<Request, Executor: CommitExecutor<Request>>(
        &mut self,
        executor: &mut Executor,
        snapshot_id: &str,
    ) -> Result<(), String> {
        if !self
            .snapshots
            .iter()
            .any(|snapshot| snapshot.snapshot_id == snapshot_id)
        {
            return Err("Snapshot is not available in this history".into());
        }
        executor.restore(snapshot_id)
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

    #[derive(Default)]
    struct FakeExecutor {
        calls: Vec<String>,
    }
    impl CommitExecutor<()> for FakeExecutor {
        fn preview(&mut self, _: &()) -> Result<CommitPreview, String> {
            self.calls.push("preview".into());
            Ok(CommitPreview {
                before: BTreeMap::new(),
                after: BTreeMap::from([("Meaning".into(), "new".into())]),
                media: vec![],
                model_changed: false,
            })
        }
        fn apply(&mut self, _: &()) -> Result<SnapshotHistoryItem, String> {
            self.calls.push("apply".into());
            Ok(SnapshotHistoryItem {
                snapshot_id: "snapshot-1".into(),
                note_id: 42,
            })
        }
        fn restore(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("restore:{id}"));
            Ok(())
        }
    }

    #[test]
    fn command_controller_keeps_preview_dry_until_apply_and_restores_history() {
        let mut view = CommitViewState::new();
        let mut executor = FakeExecutor::default();
        view.preview_with(&mut executor, &()).unwrap();
        assert!(view.dry_run);
        view.apply_with(&mut executor, &()).unwrap();
        view.restore_with(&mut executor, "snapshot-1").unwrap();
        assert_eq!(executor.calls, ["preview", "apply", "restore:snapshot-1"]);
    }

    #[test]
    fn apply_requires_a_successful_preview() {
        let mut view = CommitViewState::new();
        let mut executor = FakeExecutor::default();
        assert_eq!(
            view.apply_with(&mut executor, &()).unwrap_err(),
            "Preview changes before applying them"
        );
        assert!(executor.calls.is_empty());
    }
}
