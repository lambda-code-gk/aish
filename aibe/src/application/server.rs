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
    FilesystemMemorySpaceResolver,
};
use crate::application::request_service::RequestService;
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    ConversationStore, ExternalCommandConfig, ProfileRegistry, ToolsConfig,
};

pub async fn run(
    socket_path: PathBuf,
    profile_registry: ProfileRegistry,
    tools_config: ToolsConfig,
    external_commands: Vec<ExternalCommandConfig>,
    router_profile: String,
    conversation_store_root: PathBuf,
) -> anyhow::Result<()> {
    let tool_registry = build_registry(&tools_config, &external_commands);
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        tools_config.termination_strategy,
    ));
    let active_turns = Arc::new(Mutex::new(HashMap::new()));
    let conversation_store: Arc<dyn ConversationStore> = Arc::new(
        FilesystemConversationStore::new(conversation_store_root.clone()),
    );
    let memory_store: Arc<dyn crate::ports::outbound::ContextualMemoryStore> = Arc::new(
        FilesystemContextualMemoryStore::with_conversation_root(conversation_store_root),
    );
    let memory_space_resolver: Arc<dyn crate::ports::outbound::MemorySpaceResolver> =
        Arc::new(FilesystemMemorySpaceResolver);
    let handler: Arc<dyn ClientRequestHandler> = Arc::new(RequestService::new_with_turns(
        profile_registry,
        tool_registry,
        tools_config,
        terminator,
        Arc::clone(&active_turns),
        router_profile,
        conversation_store,
        memory_store,
        memory_space_resolver,
    ));

    unix_socket_server::run(socket_path, handler).await
}
