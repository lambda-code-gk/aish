# 仕様・指示書インデックス（000x 系列）

| 番号 | ファイル | 状態 | 概要 |
|------|----------|------|------|
| 0001 | [0001_aibe-tool-agent-loop-spec.md](0001_aibe-tool-agent-loop-spec.md) | 実装済み | aibe ツール付きエージェントループ |
| 0002 | [0002_ai-tools-client-spec.md](0002_ai-tools-client-spec.md) | 実装済み | `ai ask` の allowlist・表示契約 |
| 0003 | [0003_architecture-review-refactor-spec.md](0003_architecture-review-refactor-spec.md) | 実装済み | レビュー反映（cwd・ドメイン型・レイヤー分割） |
| 0004 | [0004_tool-name-type-adoption-spec.md](0004_tool-name-type-adoption-spec.md) | 実装済み | `ToolName` API 全面適用 |
| 0005 | [0005_request-context-domain-spec.md](0005_request-context-domain-spec.md) | 実装済み | `AgentTurnContext` ドメイン化 |
| 0006 | [0006_max-tool-rounds-terminator-spec.md](0006_max-tool-rounds-terminator-spec.md) | 実装済み | max-round 終端戦略の差し替え可能化 |
| 0007 | [0007_agent-turn-loop-modularization-spec.md](0007_agent-turn-loop-modularization-spec.md) | 実装済み | ループ 1 ラウンドの `ToolRoundExecutor` 抽出 |
| 0008 | [0008_chat-message-and-protocol-typing-spec.md](0008_chat-message-and-protocol-typing-spec.md) | 実装済み（PR 1） | `MessageRole` / `TryFrom` 化 / 注入メッセージ段階 PR（phase 2 未実装） |
| 0009 | [0009_ai-tool-category-sync-spec.md](0009_ai-tool-category-sync-spec.md) | 未実装 | カテゴリ表と `KNOWN_TOOLS` の機械同期 |

運用上の正本（要約）: [architecture.md](architecture.md)。

実装順の目安: **0004** → **0005** → **0006**（実装済み）→ **0007**（ループ分割）→ **0008** / **0009**（低優先・独立）。
