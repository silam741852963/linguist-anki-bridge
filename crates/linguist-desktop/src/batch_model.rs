use linguist_core::BatchJobState;
use linguist_jobs::{
    BatchRollbackPort, ItemPage, JobRepository, JobSummary, NewJob, RollbackReport,
};

pub const PAGE_SIZE: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchAction {
    Cancel,
    Rollback,
    Delete,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BatchViewState {
    pub jobs: Vec<JobSummary>,
    pub selected_job_id: Option<String>,
    pub page: Option<ItemPage>,
    pub pending_confirmation: Option<BatchAction>,
    pub error: String,
}

pub trait BatchManagementPort {
    fn job_summaries(&self) -> Result<Vec<JobSummary>, String>;
    fn item_page(&self, job_id: &str, limit: usize, offset: usize) -> Result<ItemPage, String>;
    fn create(&mut self, job: NewJob) -> Result<String, String>;
    fn pause(&mut self, job_id: &str) -> Result<(), String>;
    fn resume(&mut self, job_id: &str) -> Result<(), String>;
    fn retry(&mut self, job_id: &str) -> Result<(), String>;
    fn cancel(&mut self, job_id: &str) -> Result<(), String>;
    fn rollback(&mut self, job_id: &str) -> Result<RollbackReport, String>;
    fn delete(&mut self, job_id: &str) -> Result<(), String>;
}

pub struct RepositoryBatchPort<'a, Restore> {
    pub repository: &'a JobRepository,
    pub restore: &'a Restore,
}

impl<Restore: BatchRollbackPort> BatchManagementPort for RepositoryBatchPort<'_, Restore> {
    fn job_summaries(&self) -> Result<Vec<JobSummary>, String> {
        self.repository
            .job_summaries()
            .map_err(|error| error.to_string())
    }

    fn item_page(&self, job_id: &str, limit: usize, offset: usize) -> Result<ItemPage, String> {
        self.repository
            .item_page(job_id, limit, offset)
            .map_err(|error| error.to_string())
    }

    fn create(&mut self, job: NewJob) -> Result<String, String> {
        self.repository
            .create_job(job)
            .map_err(|error| error.to_string())
    }

    fn pause(&mut self, job_id: &str) -> Result<(), String> {
        self.repository
            .set_job_state(job_id, BatchJobState::Paused, "")
            .map_err(|error| error.to_string())
    }

    fn resume(&mut self, job_id: &str) -> Result<(), String> {
        self.repository
            .set_job_state(job_id, BatchJobState::Running, "")
            .map_err(|error| error.to_string())
    }

    fn retry(&mut self, job_id: &str) -> Result<(), String> {
        self.repository
            .set_job_state(job_id, BatchJobState::Running, "")
            .map_err(|error| error.to_string())
    }

    fn cancel(&mut self, job_id: &str) -> Result<(), String> {
        self.repository
            .cancel(job_id)
            .map_err(|error| error.to_string())
    }

    fn rollback(&mut self, job_id: &str) -> Result<RollbackReport, String> {
        self.repository
            .rollback(job_id, self.restore)
            .map_err(|error| error.to_string())
    }

    fn delete(&mut self, job_id: &str) -> Result<(), String> {
        self.repository
            .delete_job(job_id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

impl BatchViewState {
    pub fn create<P: BatchManagementPort>(&mut self, port: &mut P, job: NewJob) {
        match port.create(job) {
            Ok(job_id) => {
                self.refresh(port);
                self.select(port, &job_id, 0);
            }
            Err(error) => self.error = error,
        }
    }

    pub fn pause<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.run(port, BatchManagementPort::pause);
    }

    pub fn resume<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.run(port, BatchManagementPort::resume);
    }

    pub fn retry<P: BatchManagementPort>(&mut self, port: &mut P) {
        self.run(port, BatchManagementPort::retry);
    }
    pub fn refresh<P: BatchManagementPort>(&mut self, port: &P) {
        match port.job_summaries() {
            Ok(jobs) => {
                self.jobs = jobs;
                if self
                    .selected_job_id
                    .as_ref()
                    .is_some_and(|id| !self.jobs.iter().any(|job| &job.job.id == id))
                {
                    self.selected_job_id = None;
                    self.page = None;
                }
                self.error.clear();
            }
            Err(error) => self.error = error,
        }
    }

    pub fn select<P: BatchManagementPort>(&mut self, port: &P, job_id: &str, offset: usize) {
        match port.item_page(job_id, PAGE_SIZE, offset) {
            Ok(page) => {
                self.selected_job_id = Some(job_id.into());
                self.page = Some(page);
                self.error.clear();
            }
            Err(error) => self.error = error,
        }
    }

    pub fn request_confirmation(&mut self, action: BatchAction) {
        self.pending_confirmation = Some(action);
    }

    pub fn confirm<P: BatchManagementPort>(&mut self, port: &mut P) {
        let Some(action) = self.pending_confirmation.take() else {
            return;
        };
        let Some(job_id) = self.selected_job_id.clone() else {
            self.error = "Select a batch job first".into();
            return;
        };
        let result = match action {
            BatchAction::Cancel => port.cancel(&job_id).map(|_| ()),
            BatchAction::Rollback => port.rollback(&job_id).map(|_| ()),
            BatchAction::Delete => port.delete(&job_id),
        };
        if let Err(error) = result {
            self.error = error;
            return;
        }
        self.refresh(port);
        if self.selected_job_id.as_deref() == Some(&job_id) {
            self.select(port, &job_id, 0);
        }
    }

    fn run<P: BatchManagementPort>(
        &mut self,
        port: &mut P,
        command: impl FnOnce(&mut P, &str) -> Result<(), String>,
    ) {
        let Some(job_id) = self.selected_job_id.clone() else {
            self.error = "Select a batch job first".into();
            return;
        };
        if let Err(error) = command(port, &job_id) {
            self.error = error;
            return;
        }
        self.refresh(port);
        self.select(port, &job_id, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use linguist_core::{BatchItemContract, BatchJobContract};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakePort {
        calls: Vec<String>,
    }
    impl BatchManagementPort for FakePort {
        fn job_summaries(&self) -> Result<Vec<JobSummary>, String> {
            Ok(vec![JobSummary {
                job: BatchJobContract {
                    id: "batch-1".into(),
                    deck_key: "jp".into(),
                    deck_name: "Japanese".into(),
                    status: "paused".into(),
                    dry_run: true,
                    settings: BTreeMap::new(),
                    created_at: Some(String::new()),
                    updated_at: Some(String::new()),
                    started_at: None,
                    finished_at: None,
                    last_error: String::new(),
                    extensions: BTreeMap::new(),
                },
                total: 500,
                counts: BTreeMap::new(),
            }])
        }
        fn item_page(&self, _: &str, limit: usize, offset: usize) -> Result<ItemPage, String> {
            Ok(ItemPage {
                items: Vec::<BatchItemContract>::new(),
                total: 500,
                offset,
                limit,
            })
        }
        fn create(&mut self, _: NewJob) -> Result<String, String> {
            Ok("batch-1".into())
        }
        fn pause(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("pause:{id}"));
            Ok(())
        }
        fn resume(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("resume:{id}"));
            Ok(())
        }
        fn retry(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("retry:{id}"));
            Ok(())
        }
        fn cancel(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("cancel:{id}"));
            Ok(())
        }
        fn rollback(&mut self, id: &str) -> Result<RollbackReport, String> {
            self.calls.push(format!("rollback:{id}"));
            Ok(RollbackReport::default())
        }
        fn delete(&mut self, id: &str) -> Result<(), String> {
            self.calls.push(format!("delete:{id}"));
            Ok(())
        }
    }

    #[test]
    fn jobs_page_without_materializing_all_items_and_confirm_destructive_actions() {
        let mut view = BatchViewState::default();
        let mut port = FakePort::default();
        view.refresh(&port);
        view.select(&port, "batch-1", 200);
        assert_eq!(view.page.as_ref().unwrap().limit, PAGE_SIZE);
        assert_eq!(view.page.as_ref().unwrap().offset, 200);
        view.pause(&mut port);
        view.resume(&mut port);
        view.retry(&mut port);
        view.request_confirmation(BatchAction::Cancel);
        view.confirm(&mut port);
        view.request_confirmation(BatchAction::Rollback);
        view.confirm(&mut port);
        view.request_confirmation(BatchAction::Delete);
        view.confirm(&mut port);
        assert_eq!(
            port.calls,
            [
                "pause:batch-1",
                "resume:batch-1",
                "retry:batch-1",
                "cancel:batch-1",
                "rollback:batch-1",
                "delete:batch-1"
            ]
        );
    }

    struct NoRestore;
    impl BatchRollbackPort for NoRestore {
        fn restore_snapshot(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn repository_adapter_reopens_paused_job_and_pages_without_all_rows() {
        let path = PathBuf::from(format!(
            "/tmp/linguist-desktop-batch-{}",
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let repository = JobRepository::open(&path).unwrap();
        let mut adapter = RepositoryBatchPort {
            repository: &repository,
            restore: &NoRestore,
        };
        let id = adapter
            .create(NewJob {
                deck_key: "jp".into(),
                deck_name: "Japanese".into(),
                dry_run: true,
                settings: BTreeMap::new(),
                items: (0..500)
                    .map(|note_id| linguist_jobs::BatchItemSeed {
                        note_id,
                        word: format!("word-{note_id}"),
                    })
                    .collect(),
            })
            .unwrap();
        adapter.resume(&id).unwrap();
        repository.recover_interrupted().unwrap();
        assert_eq!(repository.job(&id).unwrap().unwrap().status, "paused");
        let page = adapter.item_page(&id, PAGE_SIZE, 200).unwrap();
        assert_eq!(page.items.len(), PAGE_SIZE);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(repository.artifact_root());
    }
}
