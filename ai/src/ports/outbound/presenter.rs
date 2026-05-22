//! ユーザー向け表示 outbound port。

use aibe::protocol::ClientResponse;

/// 応答の表示。
pub trait Presenter {
    fn show_response(&self, response: &ClientResponse);
    fn show_error(&self, message: &str);
}
