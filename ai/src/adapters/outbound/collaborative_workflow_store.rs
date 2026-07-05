//! `workflow.json` を正本とする collaborative workflow store。

use crate::domain::CollaborativeWorkflow;
use crate::ports::outbound::{CollaborativeWorkflowRepository, HandoffStoreError};

use super::FilesystemHandoffStore;

impl CollaborativeWorkflowRepository for FilesystemHandoffStore {
    fn create_workflow(&self, workflow: &CollaborativeWorkflow) -> Result<(), HandoffStoreError> {
        workflow.validate()?;
        self.with_handoff_lock(workflow.id(), || {
            let path = self.workflow_path(workflow.id())?;
            if path.exists() {
                return Err(HandoffStoreError::RevisionConflict {
                    expected: 0,
                    actual: self.read_json::<CollaborativeWorkflow>(&path)?.revision,
                });
            }
            self.write_json_atomic(&path, workflow)
        })?;
        self.with_index_lock(|| self.update_index(&workflow.handoff))
    }

    fn load_workflow(&self, handoff_id: &str) -> Result<CollaborativeWorkflow, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.workflow_path(handoff_id)?;
            if path.is_file() {
                let workflow: CollaborativeWorkflow = self.read_json(&path)?;
                workflow.validate()?;
                return Ok(workflow);
            }

            // 旧形式は読み取り移行だけを許可する。両方が揃わない部分状態は
            // unified reconciler の対象外として明示エラーにする。
            self.read_workflow_locked(handoff_id)
        })
    }

    fn list_workflows(&self) -> Result<Vec<CollaborativeWorkflow>, HandoffStoreError> {
        let handoffs = <Self as crate::ports::outbound::HandoffRepository>::list_handoffs(self)?;
        let mut workflows = Vec::new();
        for handoff in handoffs {
            match self.load_workflow(&handoff.id) {
                Ok(workflow) => workflows.push(workflow),
                Err(HandoffStoreError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        workflows.sort_by_key(|workflow| std::cmp::Reverse(workflow.handoff.updated_at_ms));
        Ok(workflows)
    }

    fn compare_and_swap_workflow(
        &self,
        expected_revision: u64,
        workflow: &CollaborativeWorkflow,
    ) -> Result<(), HandoffStoreError> {
        workflow.validate()?;
        if workflow.revision != expected_revision.saturating_add(1) {
            return Err(HandoffStoreError::RevisionConflict {
                expected: expected_revision.saturating_add(1),
                actual: workflow.revision,
            });
        }
        self.with_handoff_lock(workflow.id(), || {
            let path = self.workflow_path(workflow.id())?;
            let current: CollaborativeWorkflow = self.read_json(&path)?;
            if current.revision != expected_revision {
                return Err(HandoffStoreError::RevisionConflict {
                    expected: expected_revision,
                    actual: current.revision,
                });
            }
            self.write_json_atomic(&path, workflow)
        })?;
        self.with_index_lock(|| self.update_index(&workflow.handoff))
    }
}

#[cfg(test)]
fn _workflow_file_is_private(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777 == 0o600)
        .unwrap_or(false)
}
