//! セッション情報の整形（表示内容の組み立て）。

use crate::domain::{OutputFormat, SessionInfo};

pub fn format_session(info: &SessionInfo, format: OutputFormat) -> String {
    info.render(format)
}
