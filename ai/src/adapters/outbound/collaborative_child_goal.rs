//! Contextual Memory へ child goal を書き込む adapter。

use aibe_protocol::{
    ClientResponse, MemoryContext, MemoryOperationAdd, MemoryOperationArchive, MemoryOperationDto,
    MemoryScopeDto, MemoryStatusDto,
};

use crate::domain::{ChildGoalCloseReason, ChildGoalMeta};
use crate::ports::outbound::{
    CollaborativeChildGoalError, CollaborativeChildGoalService, MemoryClient,
};

pub struct AibeCollaborativeChildGoalService<C: MemoryClient> {
    client: C,
    session_id: String,
    memory_space_id: String,
}

impl<C: MemoryClient> AibeCollaborativeChildGoalService<C> {
    pub fn new(client: C, session_id: String, memory_space_id: String) -> Self {
        Self {
            client,
            session_id,
            memory_space_id,
        }
    }

    fn memory_context(&self) -> MemoryContext {
        MemoryContext {
            cwd: None,
            memory_space_id: Some(self.memory_space_id.clone()),
        }
    }
}

impl<C: MemoryClient> CollaborativeChildGoalService for AibeCollaborativeChildGoalService<C> {
    fn create_child_goal(
        &self,
        meta: &mut ChildGoalMeta,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> Result<(), CollaborativeChildGoalError> {
        let parent_goal_ref = meta
            .parent_goal_id
            .as_deref()
            .map(|id| format!("parent goal entry: {id}"))
            .unwrap_or_else(|| format!("parent goal: {parent_goal}"));
        let text = format!(
            "[collaborative child goal {id}]\n\
{parent_goal_ref}\n\
Handoff reason: {handoff_reason}\n\
Pending command: {requested_command}\n\
Human request: {human_request}\n\
Handoff ID: {handoff_id}",
            id = meta.id,
            handoff_id = meta.handoff_id,
        );
        let response = self
            .client
            .memory_apply(
                &self.session_id,
                &self.memory_context(),
                MemoryOperationDto::Add(MemoryOperationAdd {
                    kind: "goal".into(),
                    scope: Some(MemoryScopeDto::Project),
                    inject: None,
                    status: Some(MemoryStatusDto::Active),
                    text,
                    make_active: Some(true),
                }),
            )
            .map_err(|e| CollaborativeChildGoalError::Create(e.to_string()))?;
        match response {
            ClientResponse::MemoryApplyResult { entries, .. } => {
                meta.memory_entry_id = entries.first().map(|entry| entry.id.clone());
                Ok(())
            }
            ClientResponse::Error { message, .. } => {
                Err(CollaborativeChildGoalError::Create(message))
            }
            _ => Err(CollaborativeChildGoalError::Create(
                "unexpected memory response".into(),
            )),
        }
    }

    fn close_child_goal(
        &self,
        meta: &ChildGoalMeta,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        let Some(entry_id) = meta.memory_entry_id.as_deref() else {
            return Ok(());
        };
        let response = self
            .client
            .memory_apply(
                &self.session_id,
                &self.memory_context(),
                MemoryOperationDto::Archive(MemoryOperationArchive {
                    id: entry_id.to_string(),
                    expected_version: None,
                }),
            )
            .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?;
        match response {
            ClientResponse::MemoryApplyResult { .. } => Ok(()),
            ClientResponse::Error { message, .. } => Err(CollaborativeChildGoalError::Close(
                format!("{message} (reason: {reason:?})"),
            )),
            _ => Err(CollaborativeChildGoalError::Close(
                "unexpected memory response".into(),
            )),
        }
    }
}
