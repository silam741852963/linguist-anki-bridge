use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchJobState {
    Queued,
    Running,
    Pausing,
    Paused,
    Completed,
    Failed,
    Cancelled,
    RollingBack,
    RollbackPaused,
    RolledBack,
    RollbackPartial,
}

impl BatchJobState {
    /// Recovery mirrors the durable Python runner: work never restarts merely
    /// because the desktop application was reopened.
    pub fn after_interruption(self) -> Self {
        match self {
            Self::Running | Self::Pausing => Self::Paused,
            Self::RollingBack => Self::RollbackPaused,
            state => state,
        }
    }

    pub fn can_resume(self) -> bool {
        matches!(self, Self::Queued | Self::Paused | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemState {
    Pending,
    Processing,
    Processed,
    Committing,
    Completed,
    Failed,
    Skipped,
    Reverted,
    RollbackFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStage {
    Process,
    Commit,
}

impl BatchItemState {
    pub fn after_interruption(self) -> Self {
        match self {
            Self::Processing => Self::Pending,
            Self::Committing => Self::Processed,
            state => state,
        }
    }

    pub fn claim_stage(self) -> Option<ClaimStage> {
        match self {
            Self::Pending => Some(ClaimStage::Process),
            Self::Processed => Some(ClaimStage::Commit),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Skipped | Self::Reverted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruption_returns_work_to_safe_boundaries() {
        assert_eq!(
            BatchJobState::Running.after_interruption(),
            BatchJobState::Paused
        );
        assert_eq!(
            BatchItemState::Processing.after_interruption(),
            BatchItemState::Pending
        );
        assert_eq!(
            BatchItemState::Committing.after_interruption(),
            BatchItemState::Processed
        );
        assert_eq!(
            BatchItemState::Processed.claim_stage(),
            Some(ClaimStage::Commit)
        );
    }
}
