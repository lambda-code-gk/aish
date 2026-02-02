//! Outbound ポート: アプリが外界（シェル起動等）を使うための trait

pub mod shell_runner;

pub use shell_runner::ShellRunner;
