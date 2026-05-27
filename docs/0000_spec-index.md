# 仕様・指示書インデックス（000x / 001x 系列）

| 番号 | ファイル | 状態 | 概要 |
|------|----------|------|------|
| 0001 | [0001_aibe-tool-agent-loop-spec.md](done/0001_aibe-tool-agent-loop-spec.md) | 実装済み | aibe ツール付きエージェントループ |
| 0002 | [0002_ai-tools-client-spec.md](done/0002_ai-tools-client-spec.md) | 実装済み | `ai ask` の allowlist・表示契約 |
| 0003 | [0003_architecture-review-refactor-spec.md](done/0003_architecture-review-refactor-spec.md) | 実装済み | レビュー反映（cwd・ドメイン型・レイヤー分割） |
| 0004 | [0004_tool-name-type-adoption-spec.md](done/0004_tool-name-type-adoption-spec.md) | 実装済み | `ToolName` API 全面適用 |
| 0005 | [0005_request-context-domain-spec.md](done/0005_request-context-domain-spec.md) | 実装済み | `AgentTurnContext` ドメイン化 |
| 0006 | [0006_max-tool-rounds-terminator-spec.md](done/0006_max-tool-rounds-terminator-spec.md) | 実装済み | max-round 終端戦略の差し替え可能化 |
| 0007 | [0007_agent-turn-loop-modularization-spec.md](done/0007_agent-turn-loop-modularization-spec.md) | 実装済み | ループ 1 ラウンドの `ToolRoundExecutor` 抽出 |
| 0008 | [0008_chat-message-and-protocol-typing-spec.md](done/0008_chat-message-and-protocol-typing-spec.md) | 実装済み（PR 1） | `MessageRole` / `TryFrom` 化（phase 2 未実装） |
| 0009 | [0009_ai-tool-category-sync-spec.md](done/0009_ai-tool-category-sync-spec.md) | 実装済み | カテゴリ表と `KNOWN_TOOLS` の機械同期 |
| 0010 | [0010_gemini-provider-spec.md](done/0010_gemini-provider-spec.md) | 実装済み | Gemini プロバイダ（Google AI Studio） |
| 0011 | [0011_llm-profiles-spec.md](done/0011_llm-profiles-spec.md) | 実装済み | LLM 接続定義 + プロファイル（2 段設定） |
| 0012 | [0012_command-start-log-sanitize-spec.md](done/0012_command-start-log-sanitize-spec.md) | 実装済み | `command_start` の `command` / `args` サニタイズ |
| 0013 | [0013_provider-docs-alignment-spec.md](done/0013_provider-docs-alignment-spec.md) | 実装済み | docs / provider 表記と OpenAI 公式 API の整合 |
| 0014 | [0014_ci-smoke-stabilization-spec.md](done/0014_ci-smoke-stabilization-spec.md) | 実装済み | CI + スモーク（mock aibe 導通の自動化） |
| 0015 | [0015_shell-exec-timeout-kill-spec.md](done/0015_shell-exec-timeout-kill-spec.md) | 実装済み | `shell_exec` タイムアウト時の子プロセス kill / reap |
| 0016 | [0016_aish-shell-stdin-thread-spec.md](done/0016_aish-shell-stdin-thread-spec.md) | 実装済み | `aish shell` stdin 中継の FD 分離と終了時ハング解消 |
| 0017 | [0017_aibe-protocol-client-split-spec.md](done/0017_aibe-protocol-client-split-spec.md) | 実装済み | `aibe-protocol` / `aibe-client` 分離 |

**進行中・未着手**の指示書は `docs/00xx_*-spec.md`（ルート直下）。検討メモは [todo/](todo/)。

運用上の正本（要約）: [architecture.md](architecture.md)。実装済み一式: [done/](done/)。

実装順の目安（完了）: **0004** → **0005** → **0006** → **0007** → **0008** / **0009** → **0010** → **0011** → **0012** → **0013** → **0014** → **0015** → **0016**。
