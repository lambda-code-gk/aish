//! aibe サーバの composition root（依存の組み立てと socket server 起動）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Mutex;

use crate::adapters::inbound::unix_socket_server;
use crate::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use crate::adapters::outbound::tools::build_registry;
use crate::adapters::outbound::{
    ConversationStore as FilesystemConversationStore, EnvLlmCallTracer,
    FilesystemFeatureRegistryLoader, StaticCapabilityPolicy, TomlConfig,
};
#[cfg(feature = "memory")]
use crate::adapters::outbound::{
    FilesystemContextualMemoryStore, FilesystemMemoryRecipeRegistryLoader,
    FilesystemMemorySpaceResolver, FilesystemWorkStore, InProcessMemorySubscriptionBroker,
};
use crate::application::basic_pack_arc;
#[cfg(feature = "memory")]
use crate::application::contextual_pack_with_work_arc;
use crate::application::request_service::RequestService;
use crate::daemon::{build_current_pid_record, default_pid_file_path, write_pid_file};
use crate::domain::{FeatureEligibilityContext, FeatureRegistry};
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::inbound::ShutdownCoordinator;
use crate::ports::outbound::{
    ConversationStore, ExternalCommandConfig, FeatureRegistryLoader, LlmCallTracer, MemoryConfig,
    ProfileRegistry, ToolsConfig,
};

#[allow(clippy::too_many_arguments)]
pub async fn run(
    socket_path: PathBuf,
    config_path: PathBuf,
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
    let llm_tracer: Arc<dyn LlmCallTracer> = Arc::new(EnvLlmCallTracer);

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
            contextual_pack_with_work_arc(
                Arc::new(memory_store_impl)
                    as Arc<dyn crate::ports::outbound::ContextualMemoryStore>,
                memory_space_resolver,
                loader,
                recipe_loader,
                memory_broker,
                Arc::clone(&capability_policy),
                profile_registry.clone(),
                Arc::clone(&llm_tracer),
                Arc::new(FilesystemWorkStore::with_conversation_root(
                    conversation_store_root.clone(),
                )) as Arc<dyn crate::ports::outbound::WorkStore>,
            )
        }
        #[cfg(not(feature = "memory"))]
        {
            basic_pack_arc()
        }
    } else {
        basic_pack_arc()
    };

    let feature_registry = if memory_config.enabled {
        FilesystemFeatureRegistryLoader::new(memory_config.resolve_feature_pack())
            .load()
            .map_err(|e| anyhow::anyhow!("feature registry: {e}"))?
    } else {
        FeatureRegistry::empty()
    };
    let feature_eligibility = FeatureEligibilityContext::from_memory_kinds_and_recipes(
        memory_config.memory_kinds_enabled(),
        memory_config.recipes_enabled(),
    );

    let request_service = Arc::new(RequestService::new_with_turns_and_packs(
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
        feature_eligibility,
        llm_tracer,
    ));
    let handler: Arc<dyn ClientRequestHandler> = request_service.clone();

    let pid_file_path = default_pid_file_path();
    let pid_record = build_current_pid_record(config_path.clone(), socket_path.clone())
        .map_err(|e| anyhow::anyhow!("pid file: {e}"))?;
    write_pid_file(&pid_file_path, &pid_record).map_err(|e| anyhow::anyhow!("pid file: {e}"))?;

    let shutdown = ShutdownCoordinator::new();
    install_signal_handlers(Arc::clone(&shutdown), Arc::clone(&request_service));

    let run_result = unix_socket_server::run(socket_path.clone(), handler, shutdown).await;

    crate::daemon::cleanup_runtime_artifacts(&pid_file_path, &socket_path);
    run_result
}

fn install_signal_handlers(
    shutdown: Arc<ShutdownCoordinator>,
    request_service: Arc<RequestService>,
) {
    tokio::spawn(async move {
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(_) => return,
        };
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
        request_service.cancel_all_active_turns().await;
        shutdown.trigger();
    });
}

/// config path を解決する（composition root 用）。
pub fn resolve_config_path() -> PathBuf {
    TomlConfig::resolve_path_for_display()
}
