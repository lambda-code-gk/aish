mod clap_cli;
mod cli;

pub use clap_cli::{AishCli, AishCommand, CompleteShell, OutputFormatArg};
pub use cli::{strip_common_options, CommonOptions, CommonOptionsError};
