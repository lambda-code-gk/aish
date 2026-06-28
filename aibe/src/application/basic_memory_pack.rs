//! basic プロファイル用 memory pack（no-op 注入 + RPC 拒否）。

use async_trait::async_trait;
use std::sync::Arc;

use crate::application::memory_runtime::memory_disabled_response;
use crate::domain::{AgentTurnContext, ChatMessage};
use crate::ports::outbound::{MemorySubscription, RpcExtension, TurnHook, TurnHookError};
use aibe_protocol::{
    ClientResponse, MemoryApplyRequestBody, MemoryKindListRequestBody, MemoryQueryRequestBody,
    MemoryRecipeRunRequestBody, MemorySubscribeRequestBody, WorkApplyRequestBody,
    WorkQueryRequestBody,
};

/// `[memory] enabled = false` 時に選ばれる pack。
#[derive(Debug, Default, Clone, Copy)]
pub struct BasicPack;

impl TurnHook for BasicPack {
    fn prepare_turn_messages(
        &self,
        _context: &AgentTurnContext,
        messages: Vec<ChatMessage>,
    ) -> Result<Vec<ChatMessage>, TurnHookError> {
        Ok(messages)
    }
}

#[async_trait]
impl RpcExtension for BasicPack {
    fn memory_apply(&self, body: MemoryApplyRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }

    fn memory_query(&self, body: MemoryQueryRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }

    fn memory_kind_list(&self, body: MemoryKindListRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }

    async fn memory_recipe_run(&self, body: MemoryRecipeRunRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }

    fn memory_subscribe_begin(
        &self,
        body: MemorySubscribeRequestBody,
    ) -> (ClientResponse, Option<MemorySubscription>) {
        (memory_disabled_response(body.id), None)
    }

    fn work_apply(&self, body: WorkApplyRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }

    fn work_query(&self, body: WorkQueryRequestBody) -> ClientResponse {
        memory_disabled_response(body.id)
    }
}

/// テスト・composition root 用: `BasicPack` を trait object として返す。
pub fn basic_pack_arc() -> (Arc<dyn RpcExtension>, Arc<dyn TurnHook>) {
    let pack = Arc::new(BasicPack);
    (
        Arc::clone(&pack) as Arc<dyn RpcExtension>,
        Arc::clone(&pack) as Arc<dyn TurnHook>,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AgentTurnContext;

    #[test]
    fn turn_hook_prepare_does_not_call_llm_provider() {
        let pack = BasicPack;
        let ctx = AgentTurnContext::for_text_only(None);
        let msgs = pack
            .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
            .expect("noop");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hi");
    }

    #[test]
    fn turn_hook_is_noop() {
        let pack = BasicPack;
        let ctx = AgentTurnContext::for_text_only(None);
        let msgs = pack
            .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
            .expect("noop");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hi");
    }

    #[test]
    fn rpc_extension_rejects_memory_apply() {
        let pack = BasicPack;
        let resp = pack.memory_apply(MemoryApplyRequestBody {
            id: "m1".into(),
            session_id: "01234567890123456789012345678901".into(),
            context: aibe_protocol::MemoryContext {
                cwd: None,
                memory_space_id: None,
            },
            operation: aibe_protocol::MemoryOperationDto::Add(aibe_protocol::MemoryOperationAdd {
                kind: "goal".into(),
                scope: None,
                inject: None,
                status: None,
                text: "x".into(),
                make_active: None,
            }),
        });
        match resp {
            ClientResponse::Error { message, .. } => {
                assert!(
                    message.contains(crate::application::memory_runtime::MEMORY_DISABLED_MESSAGE)
                );
            }
            other => panic!("expected error: {other:?}"),
        }
    }

    #[test]
    fn basic_pack_rejects_work_rpc_and_does_not_inject_work() {
        let pack = BasicPack;
        let context = aibe_protocol::MemoryContext {
            cwd: None,
            memory_space_id: Some("project_test".into()),
        };
        for response in [
            pack.work_query(WorkQueryRequestBody {
                id: "work-query".into(),
                session_id: "session".into(),
                context: context.clone(),
            }),
            pack.work_apply(WorkApplyRequestBody {
                id: "work-apply".into(),
                session_id: "session".into(),
                context,
                operation: aibe_protocol::WorkOperationDto::Start {
                    goal: "goal".into(),
                },
            }),
        ] {
            match response {
                ClientResponse::Error { message, .. } => {
                    assert!(message
                        .contains(crate::application::memory_runtime::MEMORY_DISABLED_MESSAGE))
                }
                other => panic!("expected disabled error: {other:?}"),
            }
        }
        let ctx = AgentTurnContext::for_text_only(None);
        let messages = pack
            .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
            .expect("turn hook");
        assert_eq!(messages, vec![ChatMessage::user("hi")]);
    }
}
