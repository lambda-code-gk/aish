//! ユーザー向け表示 outbound port。

use aibe_protocol::ClientResponse;

use crate::domain::ToolsStartupLine;

/// 応答の表示。
pub trait Presenter {
    fn show_tools_startup(&self, line: &ToolsStartupLine);
    fn show_external_commands(&self, names: &[String]);
    fn show_progress(&self, phase: &str, message: Option<&str>);
    fn show_stream_chunk(&self, chunk: &str);
    fn show_response(&self, response: &ClientResponse, verbose_tools: bool, streamed: bool);
    fn show_error(&self, message: &str);
}
