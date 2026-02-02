//! Outbound ポート: アプリが外界（FS・時刻・プロセス・LLM・ツール・Sink 等）を使うための trait

pub mod clock;
pub mod fs;
pub mod process;
pub mod sink;
pub mod tool;

#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod signal;

pub mod id_generator;
pub mod llm_provider;

pub use clock::Clock;
pub use fs::{FileMetadata, FileSystem};
pub use process::Process;
pub use sink::{AgentEvent, EventSink};
pub use tool::Tool;
pub use id_generator::IdGenerator;
pub use llm_provider::LlmProvider;

#[cfg(unix)]
pub use pty::{Pty, PtyProcessStatus, PtySpawn, Winsize};
#[cfg(unix)]
pub use signal::Signal;
