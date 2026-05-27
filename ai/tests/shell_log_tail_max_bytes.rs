//! ai が `aibe_protocol::SHELL_LOG_TAIL_MAX_BYTES` を正本として参照すること（0005 回帰）。

use aibe_protocol::SHELL_LOG_TAIL_MAX_BYTES;

#[test]
fn ask_uses_protocol_shell_log_tail_max_bytes_constant() {
    assert_eq!(SHELL_LOG_TAIL_MAX_BYTES, 16 * 1024);
}
