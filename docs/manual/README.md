# 手動検証

実ターミナルや実 API が必要な確認手順を置く。自動テストで代替できないときだけ追加する。

## ファイル命名

`docs/manual/<topic>.md`（例: `aish-shell-log.md`, `aibe-openai-compatible.md`）

| ファイル | 内容 |
|----------|------|
| [aibe-openai-compatible.md](aibe-openai-compatible.md) | 実 LLM（`openai_compatible`: 公式 OpenAI API / ローカル互換）1 ターン |
| [gemini-provider.md](gemini-provider.md) | Gemini プロバイダ（Google AI Studio / `generateContent`） |
| [llm-profiles.md](llm-profiles.md) | LLM 接続 + プロファイル（`--profile` / 2 段 config） |
| [ai-ask-tools.md](ai-ask-tools.md) | `ai ask` の `--tools` / 表示契約（mock + 任意で実 LLM） |
| [ai-ux.md](ai-ux.md) | `ai chat` / progress / timeout / `--yes-exec` の手動確認 |
| [ai-smart-observation-report.md](ai-smart-observation-report.md) | ai smart stats/recent/report の read-only 観測確認 |
| [ai-smart-entry.md](ai-smart-entry.md) | `ai '...'` の smart entry / `--new` / `AI_SESSION_ID` |
| [aish-shell-log.md](aish-shell-log.md) | `aish shell` と JSONL ログ |
| [aish-command-output-replay.md](aish-command-output-replay.md) | `aish replay list/show/pick`（過去出力の再表示） |
| [tab-completion.md](tab-completion.md) | CLI Tab 補完（bash / zsh / PATH / cargo run / aish shell） |
| [codex-linux-sandbox.md](codex-linux-sandbox.md) | Codex MCP / bwrap（Landlock 回避） |
| [codex-spec-impl-review-loop.md](codex-spec-impl-review-loop.md) | Codex 指示書→実装レビューの 7 ステップ運用手順 |
| [contextual-memory.md](contextual-memory.md) | contextual memory CLI 手動検証（goal / now / idea / mem / context） |
| [contextual-memory-kinds-toml.md](contextual-memory-kinds-toml.md) | `kinds.toml` サンプル（registry override / custom kind） |
| [contextual-memory-multi-client.md](contextual-memory-multi-client.md) | multi-client readiness（memory space 共有・capability・subscribe 制限） |
| [aibe-graceful-restart.md](aibe-graceful-restart.md) | `aibe stop` / `restart` / `status`（mock 可） |

テンプレは [testing.md](../testing.md) の「手動検証ドキュメント」を参照。
