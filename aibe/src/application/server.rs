//! aibe サーバの composition root（依存の組み立てと socket server 起動）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::adapters::inbound::unix_socket_server;
use crate::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use crate::adapters::outbound::tools::build_registry;
use crate::adapters::outbound::{
    ConversationStore as FilesystemConversationStore, FilesystemFeatureRegistryLoader,
    StaticCapabilityPolicy,
};
#[cfg(feature = "memory")]
use crate::adapters::outbound::{
    FilesystemContextualMemoryStore, FilesystemMemoryRecipeRegistryLoader,
    FilesystemMemorySpaceResolver, InProcessMemorySubscriptionBroker,
};
use crate::application::basic_pack_arc;
#[cfg(feature = "memory")]
use crate::application::contextual_pack_arc;
use crate::application::request_service::RequestService;
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    ConversationStore, ExternalCommandConfig, FeatureRegistryLoader, MemoryConfig, ProfileRegistry,
    ToolsConfig,
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
    let capability_policy = StaticCapabilityPolicy::local_full();

    let (rpc_extension, turn_hook) = if memory_config.enabled {
        #[cfg(feature = "memory")]
        {
            let memory_space_resolver = Arc::new(FilesystemMemorySpaceResolver);
            let memory_broker: Arc<dyn crate::ports::outbound::MemorySubscriptionBroker> =
                Arc::new(InProcessMemorySubscriptionBroker::new());
            let memory_store_impl =
                FilesystemContextualMemoryStore::with_conversation_root_and_config(
                    conversation_store_root.clone(),
                    memory_config.clone(),
                );
            let loader = memory_store_impl.registry_loader();
            let recipe_loader = Arc::new(FilesystemMemoryRecipeRegistryLoader::new(
                memory_config.clone(),
            ));
            contextual_pack_arc(
                Arc::new(memory_store_impl)
                    as Arc<dyn crate::ports::outbound::ContextualMemoryStore>,
                memory_space_resolver,
                loader,
                recipe_loader,
                memory_broker,
                Arc::clone(&capability_policy),
                profile_registry.clone(),
            )
        }
        #[cfg(not(feature = "memory"))]
        {
            basic_pack_arc()
        }
    } else {
        basic_pack_arc()
    };

    let feature_registry = FilesystemFeatureRegistryLoader::new(memory_config.clone())
        .load()
        .map_err(|e| anyhow::anyhow!("feature registry: {e}"))?;

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
            feature_registry,
        ));

    unix_socket_server::run(socket_path, handler).await
}
