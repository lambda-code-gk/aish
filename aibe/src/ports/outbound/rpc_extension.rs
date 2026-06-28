//! memory 系 RPC の拡張ポイント（memory pack 等）。

use async_trait::async_trait;

use crate::ports::outbound::MemorySubscription;
use aibe_protocol::{
    ClientResponse, MemoryApplyRequestBody, MemoryKindListRequestBody, MemoryQueryRequestBody,
    MemoryRecipeRunRequestBody, MemorySubscribeRequestBody, WorkApplyRequestBody,
    WorkQueryRequestBody,
};

/// memory 系 RPC を束ねる trait。
#[async_trait]
pub trait RpcExtension: Send + Sync {
    fn memory_apply(&self, body: MemoryApplyRequestBody) -> ClientResponse;

    fn memory_query(&self, body: MemoryQueryRequestBody) -> ClientResponse;

    fn memory_kind_list(&self, body: MemoryKindListRequestBody) -> ClientResponse;

    async fn memory_recipe_run(&self, body: MemoryRecipeRunRequestBody) -> ClientResponse;

    /// subscribe 専用接続の初回応答と subscription 実体を返す。push loop は transport 層が担当。
    fn memory_subscribe_begin(
        &self,
        body: MemorySubscribeRequestBody,
    ) -> (ClientResponse, Option<MemorySubscription>);

    fn work_apply(&self, body: WorkApplyRequestBody) -> ClientResponse;

    fn work_query(&self, body: WorkQueryRequestBody) -> ClientResponse;
}
