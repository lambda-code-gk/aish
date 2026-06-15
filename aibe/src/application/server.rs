//! aibe サーバの composition root（依存の組み立てと socket server 起動）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::adapters::inbound::unix_socket_server;
use crate::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use crate::adapters::outbound::tools::build_registry;
use crate::adapters::outbound::{
    ConversationStore as FilesystemConversationStore, FilesystemContextualMemoryStore,
    FilesystemMemorySpaceResolver, InProcessMemorySubscriptionBroker, StaticCapabilityPolicy,
};
use crate::application::request_service::RequestService;
use crate::application::{basic_pack_arc, contextual_pack_arc};
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    ConversationStore, ExternalCommandConfig, MemoryConfig, ProfileRegistry, ToolsConfig,
};

pub async fn run(
    socket_path: PathBuf,
    profile_registry: ProfileRegistry,
    tools_config: ToolsConfig,
    external_commands: Vec<ExternalCommandConfig>,
    router_profile: String,
    conversation_store_root: PathBuf,
    memory_config: MemoryConfig,
) -> anyhow::Result<()> {
    let tool_registry = build_registry(&tools_config, &external_commands);
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        tools_config.termination_strategy,
    ));
    let active_turns = Arc::new(Mutex::new(HashMap::new()));
    let conversation_store: Arc<dyn ConversationStore> = Arc::new(
        FilesystemConversationStore::new(conversation_store_root.clone()),
    );
    let memory_space_resolver = Arc::new(FilesystemMemorySpaceResolver);
    let memory_broker: Arc<dyn crate::ports::outbound::MemorySubscriptionBroker> =
        Arc::new(InProcessMemorySubscriptionBroker::new());
    let capability_policy = StaticCapabilityPolicy::local_full();

    let (rpc_extension, turn_hook) = if memory_config.enabled {
        let memory_store_impl = FilesystemContextualMemoryStore::with_conversation_root(
            conversation_store_root.clone(),
        );
        let loader = memory_store_impl.registry_loader();
        contextual_pack_arc(
            Arc::new(memory_store_impl) as Arc<dyn crate::ports::outbound::ContextualMemoryStore>,
            memory_space_resolver,
            loader,
            memory_broker,
            Arc::clone(&capability_policy),
            profile_registry.clone(),
        )
    } else {
        basic_pack_arc()
    };

    let handler: Arc<dyn ClientRequestHandler> =
        Arc::new(RequestService::new_with_turns_and_packs(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::clone(&active_turns),
            router_profile,
            conversation_store,
            Arc::clone(&capability_policy),
            rpc_extension,
            turn_hook,
        ));

    unix_socket_server::run(socket_path, handler).await
}
