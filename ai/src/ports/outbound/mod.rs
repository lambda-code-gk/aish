mod agent_client;
mod collab_outcome;
mod history_store;
mod human_handoff;
mod memory_client;
mod presenter;
mod shell_log;
mod suggested_command_recall_store;
mod work_client;

pub use agent_client::{AgentClient, AgentError};
pub use collab_outcome::{CollabOutcomeCollectionError, CollabOutcomeCollector};
pub use history_store::{HistoryStore, HistoryStoreError};
pub use human_handoff::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, ShellTranscriptReader,
};
pub use memory_client::MemoryClient;
pub use presenter::Presenter;
pub use shell_log::{LogReadError, ShellLogSource};
pub use suggested_command_recall_store::{
    SuggestedCommandRecallStore, SuggestedCommandRecallStoreError,
};
pub use work_client::WorkClient;
