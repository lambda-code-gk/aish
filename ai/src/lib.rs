//! aibe クライアント（Hexagonal）。

#![cfg(unix)]

pub mod adapters;
pub mod application;
pub mod clap_cli;
pub mod domain;
pub mod ports;

#[cfg(feature = "memory")]
pub mod plugin_memory;
