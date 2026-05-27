//! ユーザー向け表示 outbound port。

use aibe_protocol::ClientResponse;

use crate::domain::ToolsStartupLine;

/// 応答の表示。
pub trait Presenter {
    fn show_tools_startup(&self, line: &ToolsStartupLine);
    fn show_response(&self, response: &ClientResponse, verbose_tools: bool);
    fn show_error(&self, message: &str);
}
