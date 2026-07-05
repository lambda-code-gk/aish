//! Work stack 連携用 child goal port（0055）。

use std::path::Path;

use crate::domain::{ChildGoalCloseReason, ChildGoalMeta};

#[derive(Debug, thiserror::Error)]
pub enum CollaborativeChildGoalError {
    #[error("failed to create child goal: {0}")]
    Create(String),
    #[error("failed to close child goal: {0}")]
    Close(String),
}

pub trait CollaborativeChildGoalService: Send + Sync {
    fn create_child_goal(
        &self,
        meta: &mut ChildGoalMeta,
        cwd: &Path,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> Result<(), CollaborativeChildGoalError>;

    fn close_child_goal(
        &self,
        meta: &ChildGoalMeta,
        cwd: &Path,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError>;
}

#[derive(Debug, Default)]
pub struct NoopCollaborativeChildGoalService;

impl CollaborativeChildGoalService for NoopCollaborativeChildGoalService {
    fn create_child_goal(
        &self,
        _meta: &mut ChildGoalMeta,
        _cwd: &Path,
        _parent_goal: &str,
        _handoff_reason: &str,
        _requested_command: &str,
        _human_request: &str,
    ) -> Result<(), CollaborativeChildGoalError> {
        Ok(())
    }

    fn close_child_goal(
        &self,
        _meta: &ChildGoalMeta,
        _cwd: &Path,
        _reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        Ok(())
    }
}
