//! Contextual Memory へ child goal を書き込む adapter。

use aibe_protocol::{
    ClientResponse, MemoryContext, MemoryOperationAdd, MemoryOperationDto, MemoryScopeDto,
    MemoryStatusDto,
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
        meta: &ChildGoalMeta,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> Result<(), CollaborativeChildGoalError> {
        let text = format!(
            "[collaborative child goal {id}]\n\
Parent goal: {parent_goal}\n\
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
                    kind: "now".into(),
                    scope: Some(MemoryScopeDto::Session),
                    inject: None,
                    status: Some(MemoryStatusDto::Active),
                    text,
                    make_active: Some(true),
                }),
            )
            .map_err(|e| CollaborativeChildGoalError::Create(e.to_string()))?;
        match response {
            ClientResponse::MemoryApplyResult { .. } => Ok(()),
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
        let text = format!(
            "[collaborative child goal {id} closed: {reason:?}]\nHandoff ID: {handoff_id}",
            id = meta.id,
            handoff_id = meta.handoff_id,
        );
        let response = self
            .client
            .memory_apply(
                &self.session_id,
                &self.memory_context(),
                MemoryOperationDto::Add(MemoryOperationAdd {
                    kind: "decision".into(),
                    scope: Some(MemoryScopeDto::Project),
                    inject: None,
                    status: Some(MemoryStatusDto::Active),
                    text,
                    make_active: None,
                }),
            )
            .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?;
        match response {
            ClientResponse::MemoryApplyResult { .. } => Ok(()),
            ClientResponse::Error { message, .. } => {
                Err(CollaborativeChildGoalError::Close(message))
            }
            _ => Err(CollaborativeChildGoalError::Close(
                "unexpected memory response".into(),
            )),
        }
    }
}
