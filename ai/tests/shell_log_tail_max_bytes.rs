//! ai が aibe の `ShellLogTail::MAX_BYTES` を正本として参照すること（0005 回帰）。

use aibe::ShellLogTail;

#[test]
fn ask_uses_aibe_shell_log_tail_max_bytes_constant() {
    // 定数の正本は aibe 側 1 箇所。ai はリテラル直書き禁止。
    assert_eq!(ShellLogTail::MAX_BYTES, 16 * 1024);
}
