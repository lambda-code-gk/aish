# 外部コマンド（CLI coding agent）手動検証

設計: [0026_external-commands-spec.md](../spec/0026_external-commands-spec.md)

## 前提

- `~/.config/aibe/config.toml` に `[[external_commands]]` と `tools.shell_exec.allowed_commands` を設定済み
- 対象 CLI（Codex / Claude Code 等）が PATH にあり、ログイン済み（実 CLI を使う場合）
- mock 検証のみなら `echo` を外部コマンドとして登録してよい

## 設定例（mock）

```toml
[tools.shell_exec]
enabled = true
allowed_commands = ["echo"]
shell_exec_approval = "always"

[[external_commands]]
name = "fixture-echo"
description = "mock external command"
command = "echo"
args = ["{prompt}"]
timeout_secs = 30
```

## チェックリスト

1. `@exec` で `shell_exec` が有効になること
   ```bash
   ai ask --tools @exec "hello" 2>&1 | head -5
   ```
   - 1 行目: `warning: ai: tools enabled: shell_exec (@exec)`
   - 2 行目（設定あり時）: `warning: ai: external commands registered: fixture-echo`

2. 外部コマンドが allowlist を通って実行されること（aibe 常駐 + mock LLM または統合テスト）
   - 自動: `cargo test -p aibe --test external_commands`

3. `tool_calls` に `approval_source=shell_exec_approval=...;external_command=...` が残ること
   - `ai ask --tools @exec --verbose-tools "..."` で stderr の tool 行を確認

4. allowlist 外コマンドが拒否されること
   - `allowed_commands` から `echo` を外した設定で aibe 再起動後、同様の turn が `command_not_allowed` になること

5. AISH が CLI thread を保存しないこと
   - `AISH_SESSION_DIR` 配下に `cli-thread.json` が作成されないこと（`ai ask` 成功後）

## 既知の制限

- 外部コマンドの stdout は raw text。JSON 出力の構造化パースは AISH の契約外
- `@full` は `shell_exec` を暗黙有効化しない
