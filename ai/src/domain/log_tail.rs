//! `--log-tail` の解決ロジック。

use aibe_protocol::SHELL_LOG_TAIL_MAX_BYTES;
use thiserror::Error;

pub const DEFAULT_LOG_TAIL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LogTailResolveError {
    #[error("--log-tail must not exceed {max} bytes (got {got})")]
    ExceedsProtocolCeiling { got: usize, max: usize },
}

pub fn resolve_log_tail_bytes(
    cli: Option<usize>,
    preset: Option<usize>,
    config: Option<usize>,
) -> Result<usize, LogTailResolveError> {
    let value = cli.or(preset).or(config).unwrap_or(DEFAULT_LOG_TAIL_BYTES);
    if value > SHELL_LOG_TAIL_MAX_BYTES {
        return Err(LogTailResolveError::ExceedsProtocolCeiling {
            got: value,
            max: SHELL_LOG_TAIL_MAX_BYTES,
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_preset_and_config() {
        assert_eq!(
            resolve_log_tail_bytes(Some(1), Some(2), Some(3)).expect("ok"),
            1
        );
    }

    #[test]
    fn falls_back_to_default() {
        assert_eq!(
            resolve_log_tail_bytes(None, None, None).expect("ok"),
            DEFAULT_LOG_TAIL_BYTES
        );
    }

    #[test]
    fn rejects_above_protocol_ceiling() {
        let err =
            resolve_log_tail_bytes(Some(SHELL_LOG_TAIL_MAX_BYTES + 1), None, None).unwrap_err();
        assert!(matches!(
            err,
            LogTailResolveError::ExceedsProtocolCeiling { .. }
        ));
    }
}
