# 手動検証

実ターミナルや実 API が必要な確認手順を置く。自動テストで代替できないときだけ追加する。

## ファイル命名

`docs/manual/<topic>.md`（例: `aish-shell-log.md`, `aibe-openai-compatible.md`）

| ファイル | 内容 |
|----------|------|
| [aibe-openai-compatible.md](aibe-openai-compatible.md) | 実 LLM（OpenAI 互換）1 ターン |
| [ai-ask-tools.md](ai-ask-tools.md) | `ai ask` の `--tools` / 表示契約（mock + 任意で実 LLM） |
| [aish-shell-log.md](aish-shell-log.md) | `aish shell` と JSONL ログ |
| [codex-linux-sandbox.md](codex-linux-sandbox.md) | Codex MCP / bwrap（Landlock 回避） |

テンプレは [testing.md](../testing.md) の「手動検証ドキュメント」を参照。
