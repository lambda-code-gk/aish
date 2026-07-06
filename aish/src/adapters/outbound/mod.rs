mod jsonl_log;
mod process_shell;
mod pty_shell;
mod replay_log;
mod replay_picker;
mod session_info;
mod session_store;
pub mod shell_completion;
pub mod toml_config;

pub use jsonl_log::JsonlFileLog;
pub use process_shell::ProcessShell;
pub use pty_shell::{HumanReturnMarker, PtyShell};
pub use replay_log::{read_log_events, ReplayLogReadError};
pub use replay_picker::{pick_entry, require_interactive_tty, PickerEntry, ReplayPickerError};
pub use session_info::{
    read_session_info, resolve_replay_log_path, session_dir_from_env, ReplayLogResolveError,
    SessionReadError,
};
pub use session_store::{
    create_shell_session, prune_old_sessions, resolve_sessions_parent, SessionLayout,
    SessionStoreError,
};
pub use shell_completion::{
    detect_child_shell, prepare_interactive_rc, ChildShellKind, ShellRcLayout,
};
