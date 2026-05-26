# 気になる点

[← 索引](README.md)

## 1. ドキュメントと実装のズレ（P0 候補）

README では provider として `openai` が列挙されているが、実装の `parse_provider_kind` は `mock`、`openai_compatible` / `openai-compatible`、`gemini` のみ。

**対応案**: `openai` を実装するか、README から消して「OpenAI 公式 API も `openai_compatible` として扱う」と明記。

→ **実装済み**: [0013](../../done/0013_provider-docs-alignment-spec.md)。当初タスク: [p0-stabilization.md](p0-stabilization.md) §2

## 2. `command_start` のログ漏洩（P0 候補）

`stdout` / `stderr` は `sanitize_log_text` を通すが、`command_start` は `LogEvent::command_start(&command)` をそのまま append。`CommandStart` に `command` と `args` が生で入るため、`curl -H "Authorization: Bearer ..."` や `cmd --api-key ...` がログに残りうる。

`docs/security.md` の方針と整合させるなら **P0 修正**。

**対応案**: `CommandStart { command, args }` にも `sanitize_log_text` 相当を適用。

→ **実装済み**: [0012](../../done/0012_command-start-log-sanitize-spec.md)。当初タスク: [p0-stabilization.md](p0-stabilization.md)

## 3. `ai` → `aibe` クレート直依存（P1 候補）

設計上は許容だが、長期的には `aibe-protocol` または `aibe-client` 分離がよい。別言語クライアント、GUI、TUI、VSCode 拡張、MCP フロントを作るなら wire protocol crate を独立させる。

→ タスク化: [p1-protocol-split.md](p1-protocol-split.md)

## 4. `shell_exec` は許可制だけでは不足（P2 候補）

`git` / `cargo` / `python` / `bash` を許可すると実質任意コード実行に近い。日常利用へ進めるなら承認ステップ、dry-run 表示、危険度分類、監査ログが必要。

→ タスク化: [p2-safe-tools.md](p2-safe-tools.md)、[sprints.md](sprints.md) Sprint 3
