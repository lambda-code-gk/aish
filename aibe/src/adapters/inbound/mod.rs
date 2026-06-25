//! 駆動アダプタ（Unix NDJSON リスナ、接続承認、socket I/O）。

pub mod client_tool_gate;
pub mod connection_approval;
pub mod control_plane;
pub mod unix_socket_server;
