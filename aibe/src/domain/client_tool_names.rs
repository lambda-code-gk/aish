//! client tool の logical name と LLM provider 向け safe name の対応。

pub const AISH_REPLAY_SHOW_LOGICAL: &str = "aish.replay_show";
pub const AISH_REPLAY_SHOW_PROVIDER: &str = "aish_replay_show";
pub const AISH_REQUEST_HUMAN_ACTION_LOGICAL: &str = "aish.request_human_action";
pub const AISH_REQUEST_HUMAN_ACTION_PROVIDER: &str = "aish_request_human_action";

/// wire / audit / client gate 向け logical name から provider API 向け name へ。
pub fn provider_tool_name(logical: &str) -> Option<&'static str> {
    match logical {
        AISH_REPLAY_SHOW_LOGICAL => Some(AISH_REPLAY_SHOW_PROVIDER),
        AISH_REQUEST_HUMAN_ACTION_LOGICAL => Some(AISH_REQUEST_HUMAN_ACTION_PROVIDER),
        _ => None,
    }
}

/// LLM provider 返却名から logical name へ。
pub fn logical_tool_name(provider: &str) -> Option<&'static str> {
    match provider {
        AISH_REPLAY_SHOW_PROVIDER | AISH_REPLAY_SHOW_LOGICAL => Some(AISH_REPLAY_SHOW_LOGICAL),
        AISH_REQUEST_HUMAN_ACTION_PROVIDER | AISH_REQUEST_HUMAN_ACTION_LOGICAL => {
            Some(AISH_REQUEST_HUMAN_ACTION_LOGICAL)
        }
        _ => None,
    }
}

/// 会話履歴を LLM provider へ再送するときの tool 名。
pub fn tool_name_for_provider(name: &str) -> String {
    provider_tool_name(name).unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_tool_name_maps_aish_replay_show_bidirectionally() {
        assert_eq!(
            provider_tool_name(AISH_REPLAY_SHOW_LOGICAL),
            Some(AISH_REPLAY_SHOW_PROVIDER)
        );
        assert_eq!(
            logical_tool_name(AISH_REPLAY_SHOW_PROVIDER),
            Some(AISH_REPLAY_SHOW_LOGICAL)
        );
        assert_eq!(
            logical_tool_name(AISH_REPLAY_SHOW_LOGICAL),
            Some(AISH_REPLAY_SHOW_LOGICAL)
        );
        assert!(provider_tool_name("read_file").is_none());
        assert!(logical_tool_name("unknown_tool").is_none());
    }
}
