# 0054 — Safe File Write Tools Phase 8 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 実装済み（Phase 8）  
> **前提**: [Phase 7](0054_safe-file-write-tools-phase7-implementation-spec.md) 完了

## 0. 目的

設計書 §14.4、§15 の **`ai` 側 write tool 承認 UX** と **`aibe-client` callback** を実装する。AIBE が生成した diff preview を **そのまま表示** し、TTY 上で yes/no を受け付ける。参照: [0023](../done/0023_shell-exec-approval-hardening-spec.md) の non-TTY fail-closed パターン。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `file_write_approval_ui.rs` | §15 |
| stderr への prompt / path / summary / diff | §15.1 |
| 制御文字 escape（`\xNN`） | §15.1 |
| `preview_truncated` 明示 | §15.1 |
| 入力 `y` / `n` / Enter=no | §15.2 |
| non-TTY → `approval_unavailable` | §15.3 |
| `aibe-client` `ToolApprovalPrompt` callback | §14.4 |
| `AgentTurnCallbacks` または同等の callback 集約 | §14.4 |
| 既存呼び出し側 compatibility wrapper | §14.4 |
| `--verbose-tools` で `change_id` 表示 | §19.5 |
| `aibe-client` socket 統合テスト | §27.10 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| session cache（`[a]lways-this-session` 相当） | 非目標 §2, §13.2 |
| AIBE 側 diff 再計算 | `ai` は表示のみ §25.4 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `approval_ui_stderr_only` | 承認 UI が stderr のみ | `file_write_approval_ui_writes_stderr_only` | false |
| `approval_ui_escapes_control_chars` | diff 内制御文字を escape | `file_write_approval_ui_escapes_control_chars` | false |
| `approval_ui_truncation_notice` | truncate 時に明示メッセージ | `file_write_approval_ui_shows_truncation_notice` | false |
| `approval_ui_non_tty` | non-TTY → deny | `file_write_approval_ui_rejects_non_tty` | false |
| `approval_ui_yes_executes` | `y` で write 実行 | `file_write_approval_ui_yes_executes_write` | false |
| `approval_ui_no_continues_turn` | `n` で turn 継続・変更なし | `file_write_approval_ui_no_continues_turn` | false |
| `aibe_client_tool_approval_roundtrip` | client が prompt→approval 往復 | `aibe_client_tool_approval_roundtrip` | false |
| `verbose_tools_change_id` | verbose 時 change_id 表示 | `verbose_tools_shows_change_id` | false |
| `shell_and_write_approval_mixed` | 同一 turn で shell/write 承認混在 | `mixed_shell_and_write_approval_in_one_turn` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| ai | `ai/src/adapters/outbound/file_write_approval_ui.rs`（新規） |
| ai | `ai/src/adapters/outbound/aibe_client.rs` |
| ai | `ai/src/main.rs` |
| client | `aibe-client/src/lib.rs`, `transport.rs` |
| tests | `ai/tests/file_write_approval_ui.rs` |
| tests | `aibe-client/tests/tool_approval.rs` |
| tests | `aibe/tests/file_write_approval_socket.rs` |

## 4. 実装手順

### 4.1 表示（§15.1）

```text
ai: file write approval required:
  tool: apply_patch
  path: src/main.rs
  change: +8 -3
  preview:
--- a/src/main.rs
+++ b/src/main.rs
...
Apply this change? [y/N]
```

- stdout に出さない
- path の制御文字 escape
- raw ANSI を端末へ送らない

### 4.2 non-TTY（§15.3）

`stdin.is_terminal() == false` なら **read 前に** deny。pipe への `y` は承認にならない（0023 同様）。

非対話実行は config で `approval = "always"` を明示。

### 4.3 aibe-client（§14.4）

`ShellExecApprovalPrompt` に加え `ToolApprovalPrompt` callback を追加。API が肥大化する場合は `AgentTurnCallbacks` に集約し、旧 API は薄い wrapper で維持。

### 4.4 `ai` の責務境界（§25.4）

AIBE の preview を **再計算・改変しない**。表示と yes/no のみ。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package aibe-client
./scripts/verify-targeted.sh --package ai
cargo test -p aibe-client tool_approval -j 1
cargo test -p ai file_write_approval -j 1
./scripts/verify.sh
```
