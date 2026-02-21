//! Outbound ポート: アプリが外界（シェル起動等）を使うための trait

pub mod memory_repository;
pub mod shell_runner;

pub use memory_repository::MemoryRepository;
pub use shell_runner::ShellRunner;
