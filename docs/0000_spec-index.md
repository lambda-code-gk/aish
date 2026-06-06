# 仕様・指示書インデックス（000x / 001x 系列）

## ドキュメントの置き場所（2026-06 以降）

| 種別 | 置き場所 | 作成者（既定） | 移動タイミング |
|------|----------|----------------|----------------|
| **設計書** | [spec/](spec/) | Codex | 設計確定後も **spec に残す** |
| **実装指示書**（進行中） | [tasks/](tasks/) | Codex または Cursor | 実装完了 **コミット時**に [done/](done/) へ移動 |
| **実装済み指示書** | [done/](done/) | — | 履歴保管 |
| **検討メモ** | [todo/](todo/) | 任意 | 昇格（spec / tasks）または削除 |

**番号**: 設計書（`spec/`）と実装指示書（`tasks/` / `done/`）は **同じ 00xx** を使う。ファイル名で区別する（設計: `00xx_<topic>-spec.md`、実装: `00xx_<topic>-implementation-spec.md`）。

運用上の正本（要約）: [architecture.md](architecture.md)。手動検証: [manual/](manual/)。

## 設計書（docs/spec/）

| 番号 | ファイル | 状態 | 概要 |
|------|----------|------|------|
| 0026 | [0026_external-commands-spec.md](spec/0026_external-commands-spec.md) | 設計確定 | 外部コマンド（CLI コーディングエージェント） |
| 0027 | [0027_ai-ux-spec.md](spec/0027_ai-ux-spec.md) | 設計確定 | `ai` コマンド UX 改善 |
| 0028 | [0028_ai-ux-gap-closure-spec.md](spec/0028_ai-ux-gap-closure-spec.md) | 設計確定 | `ai` UX の残ギャップ解消 |

## 実装指示書（docs/tasks/ — 進行中）

（なし）

## 実装済み指示書（docs/done/）

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
| 0018 | [0018_safe-tools-policy-spec.md](done/0018_safe-tools-policy-spec.md) | 実装済み | `safe-tools-policy` の docs 同期（`testing.md` / `security.md`） |
| 0019 | [0019_aish-session-log-integration-spec.md](done/0019_aish-session-log-integration-spec.md) | 実装済み | `aish shell` セッション dir + `ai ask` ログ連携（P3） |
| 0020 | [0020_p4-daily-use-polish-spec.md](done/0020_p4-daily-use-polish-spec.md) | 実装済み | P4 日常利用前の仕上げ（`aish shell` / `aibe-client` / `shell_exec` 承認） |
| 0021 | [0021_tab-completion-spec.md](done/0021_tab-completion-spec.md) | 実装済み | CLI Tab 補完（`aish` / `ai` / `aibe`、bash / zsh、`clap` 移行） |
| 0022 | [0022_ai-filter-spec.md](done/0022_ai-filter-spec.md) | 実装済み | `AI_FILTER` / `[ask].filter` による assistant 本文の output filter |
| 0023 | [0023_shell-exec-approval-hardening-spec.md](done/0023_shell-exec-approval-hardening-spec.md) | 実装済み | `shell_exec` 承認 UI: 非対話 stdin fail-closed、表示 escape、`aibe-client` 往復テスト |
| 0024 | [0024_cli-subagent-provider-spec.md](done/0024_cli-subagent-provider-spec.md) | 非採用 | CLI サブエージェント（first-class 統合案）— [0026](spec/0026_external-commands-spec.md) で代替 |
| 0025 | [0025_cli-subagent-implementation-spec.md](done/0025_cli-subagent-implementation-spec.md) | 非採用 | CLI サブエージェント実装指示書 — [0026](spec/0026_external-commands-spec.md) で代替 |
| 0026 | [0026_external-commands-implementation-spec.md](done/0026_external-commands-implementation-spec.md) | 実装済み | 外部コマンド（設計: [0026](spec/0026_external-commands-spec.md)） |
| 0027 | [0027_ai-ux-implementation-spec.md](done/0027_ai-ux-implementation-spec.md) | 実装済み | `ai` コマンド UX 改善（設計: [0027](spec/0027_ai-ux-spec.md)） |
| 0028 | [0028_ai-ux-gap-closure-implementation-spec.md](done/0028_ai-ux-gap-closure-implementation-spec.md) | 実装済み | `ai` UX 残ギャップ解消（設計: [0028](spec/0028_ai-ux-gap-closure-spec.md)） |

実装順の目安（完了）: **0004** → **0005** → **0006** → **0007** → **0008** / **0009** → **0010** → **0011** → **0012** → **0013** → **0014** → **0015** → **0016**。
