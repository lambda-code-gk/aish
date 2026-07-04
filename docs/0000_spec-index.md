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
| 0029 | [0029_ai-ux-polish-spec.md](spec/0029_ai-ux-polish-spec.md) | 設計確定 | `--yes-exec` 検証・history GC・streaming mock テスト |
| 0030 | [0030_ai-smart-entry-spec.md](spec/0030_ai-smart-entry-spec.md) | 設計確定 | `ai` スマート入口（`route_turn` / `AI_SESSION_ID` / aibe transcript 保持） |
| 0031 | [0031_hexagonal-effect-boundary-spec.md](spec/0031_hexagonal-effect-boundary-spec.md) | 設計確定 | Hexagonal effect boundary 検査（副作用 API 混入のルールファイル駆動チェック） |
| 0032 | [0032_ai-console-hint-toggle-spec.md](spec/0032_ai-console-hint-toggle-spec.md) | 設計確定 | `ai` コンソールヒント切り替え |
| 0033 | [0033_ai-structured-output-stream-fix-spec.md](spec/0033_ai-structured-output-stream-fix-spec.md) | 設計確定 | `ai` structured output と streaming の衝突解消 |
| 0034 | [0034_aibe-contextual-memory-spec.md](spec/0034_aibe-contextual-memory-spec.md) | 設計確定 | AIBE Contextual Memory MVP（`goal` / `now` / `idea`） |
| 0035 | [0035_aibe-memory-identity-split-spec.md](spec/0035_aibe-memory-identity-split-spec.md) | 設計確定 | AIBE Memory Identity Split（`AI_SESSION_ID` / `memory_space_id` 分離） |
| 0036 | [0036_shell-exec-approval-ux-spec.md](spec/0036_shell-exec-approval-ux-spec.md) | 設計確定 | `shell_exec` 承認 UX 拡張（`y/n/a/c`、tier、pattern auto-approve） |
| 0037 | [0037_aibe-contextual-memory-runtime-v1-spec.md](spec/0037_aibe-contextual-memory-runtime-v1-spec.md) | 設計確定 | AIBE Contextual Memory Runtime v1（registry / resolver / recipe / subscribe / capability） |
| 0038 | [0038_contextual-memory-pack-phase-a-spec.md](spec/0038_contextual-memory-pack-phase-a-spec.md) | 設計確定 | Contextual Memory Pack Phase A（`[memory] enabled` による basic 切替） |
| 0038 | [0038_contextual-memory-pack-phase-b-spec.md](spec/0038_contextual-memory-pack-phase-b-spec.md) | 設計確定 | Contextual Memory Pack Phase B（TurnHook / RpcExtension trait 化、Pack 合成） |
| 0038 | [0038_contextual-memory-pack-phase-c-spec.md](spec/0038_contextual-memory-pack-phase-c-spec.md) | 設計確定（実装済み） | Contextual Memory Pack Phase C（CLI / built-in kind の pack 移行） |
| 0038 | [0038_contextual-memory-pack-phase-d-spec.md](spec/0038_contextual-memory-pack-phase-d-spec.md) | 設計確定（実装済み） | Contextual Memory Pack Phase D（`memory` feature / basic build） |
| 0039 | [0039_aish-memory-pack-externalization-spec.md](spec/0039_aish-memory-pack-externalization-spec.md) | 設計確定（実装済み） | AISH Memory Pack Externalization（builtin kind / recipe の TOML 外出し） |
| 0040 | [0040_generic-recipe-cli-aish-name-cleanup-spec.md](spec/0040_generic-recipe-cli-aish-name-cleanup-spec.md) | 設計確定（実装済み） | Generic Recipe CLI / AISH Name Cleanup（recipe CLI 一般化、material 順序・title 正本化） |
| 0041 | [0041_ai-smart-feature-plan-spec.md](spec/0041_ai-smart-feature-plan-spec.md) | 設計確定（実装済み） | `ai` Smart Feature Plan（`route_turn` / `feature_actions` / approval gate） |
| 0042 | [0042_configurable-smart-features-spec.md](spec/0042_configurable-smart-features-spec.md) | 設計確定（実装済み） | Configurable Smart Features（`features.toml` / registry / prompt schema） |
| 0043 | [0043_feature-pack-boundary-hardening-spec.md](spec/0043_feature-pack-boundary-hardening-spec.md) | 設計確定（Phase 1–3 実装済み） | Feature Pack Boundary Hardening（memory.enabled ゲート / eligibility / read-only tools / feature pack 分離） |
| 0044 | [0044_smart-preprocessor-spec.md](spec/0044_smart-preprocessor-spec.md) | 設計確定（実装済み） | AISH Smart Preprocessor / Local Intent Router（Phase 2.9 まで: `LocalRouteDecision` / tool enablement fast path / observation metrics） |
| 0045 | [0045_pack-composition-spec.md](spec/0045_pack-composition-spec.md) | 設計確定 | パック構成（Pack Composition）— optional 機能の静的合成・脱着機構（0038 参照実装） |
| 0046 | [0046_aibe-graceful-restart-spec.md](spec/0046_aibe-graceful-restart-spec.md) | 設計確定（実装済み） | aibe graceful restart（PID file / SIGTERM / stop / restart / status） |
| 0047 | [0047_ai-interactive-prompt-input-spec.md](spec/0047_ai-interactive-prompt-input-spec.md) | 設計確定（実装済み） | `ai` 対話的プロンプト入力（bare `ai` / AI_EDITOR / reedline） |
| 0048 | [0048_ai-filter-streaming-fix-spec.md](spec/0048_ai-filter-streaming-fix-spec.md) | 設計確定（実装済み） | `ai` output filter と assistant streaming の整合化 |
| 0049 | [0049_aish-command-output-replay-spec.md](spec/0049_aish-command-output-replay-spec.md) | 設計確定（実装済み） | `aish` command output replay（`replay list/show/pick`、shell span 記録） |
| 0050 | [0050_client-provided-replay-tool-spec.md](spec/0050_client-provided-replay-tool-spec.md) | 設計確定（実装済み） | Client-Provided Replay Tool（`aish.replay_show`、turn-local read-only client tool、hybrid manifest + `shell_log_tail`） |
| 0051 | [0051_smart-observation-report-spec.md](spec/0051_smart-observation-report-spec.md) | 設計確定（実装済み） | Smart Preprocessor observation の read-only stats/recent/report CLI |
| 0052 | [0052_ai_work.md](spec/0052_ai_work.md) | 設計確定（実装済み） | `ai work` 作業文脈管理（start/status/list/switch/push/pop/defer/idea/note/decide/focus/finish） |
| 0053 | [0053_ai-suggested-command-recall-spec.md](spec/0053_ai-suggested-command-recall-spec.md) | 設計確定（実装済み） | `ai` 提案コマンド再呼び出し（bash / zsh、`aish shell` / `ai complete` hook） |
| 0054 | [0054_safe-file-write-tools-spec.md](spec/0054_safe-file-write-tools-spec.md) | 設計確定（実装済み） | Safe File Write Tools（`write_file` / `apply_patch`、承認・journal・SHA-256） |
| 0055 | [0055_collaborative-human-handoff-spec.md](spec/0055_collaborative-human-handoff-spec.md) | 設計確定（実装中 Phase 2–5） | Human-in-the-loop 協調作業（`ai --collaborative`、human shell handoff、side agent、復旧） |

### 状態ラベルの意味

| 状態 | 意味 |
|------|------|
| 設計確定 | spec のみ確定。実装は未着手または別途追跡 |
| 設計確定（実装中 …） | 実装指示書が `docs/tasks/` にあり、受け入れ条件が未完了 |
| 設計確定（実装済み） | `scripts/spec-acceptance.toml` の当該 spec がすべて `pending = false` かつ `docs/done/` へ移動済み |
| 実装済み（Phase N） | 当該 Phase の受け入れテストが `pending = false` で緑 |

## 実装指示書（docs/tasks/ — 進行中）

| 番号 | ファイル | 状態 | 概要 |
|------|----------|------|------|
| 0055 | [0055_collaborative-human-handoff-implementation-spec.md](tasks/0055_collaborative-human-handoff-implementation-spec.md) | 進行中 | Collaborative Human Handoff マスター（Phase 1–5） |
| 0055 | [0055_collaborative-human-handoff-phase1-implementation-spec.md](tasks/0055_collaborative-human-handoff-phase1-implementation-spec.md) | Phase 1 完了 | Phase 1 — Domain / 永続化 |
| 0055 | [0055_collaborative-human-handoff-phase2-implementation-spec.md](tasks/0055_collaborative-human-handoff-phase2-implementation-spec.md) | 未着手 | Phase 2 — 親 shell_exec handoff |
| 0055 | [0055_collaborative-human-handoff-phase3-implementation-spec.md](tasks/0055_collaborative-human-handoff-phase3-implementation-spec.md) | 未着手 | Phase 3 — Side agent |
| 0055 | [0055_collaborative-human-handoff-phase4-implementation-spec.md](tasks/0055_collaborative-human-handoff-phase4-implementation-spec.md) | 未着手 | Phase 4 — 復旧 |
| 0055 | [0055_collaborative-human-handoff-phase5-implementation-spec.md](tasks/0055_collaborative-human-handoff-phase5-implementation-spec.md) | 未着手 | Phase 5 — UX / docs |

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
| 0029 | [0029_ai-ux-polish-implementation-spec.md](done/0029_ai-ux-polish-implementation-spec.md) | 実装済み | `--yes-exec` 検証・history GC（設計: [0029](spec/0029_ai-ux-polish-spec.md)） |
| 0030 | [0030_ai-smart-entry-implementation-spec.md](done/0030_ai-smart-entry-implementation-spec.md) | 実装済み | `ai` スマート入口（設計: [0030](spec/0030_ai-smart-entry-spec.md)） |
| 0031 | [0031_hexagonal-effect-boundary-implementation-spec.md](done/0031_hexagonal-effect-boundary-implementation-spec.md) | 実装済み | Hexagonal effect boundary 検査（設計: [0031](spec/0031_hexagonal-effect-boundary-spec.md)） |
| 0032 | [0032_ai-console-hint-toggle-implementation-spec.md](done/0032_ai-console-hint-toggle-implementation-spec.md) | 実装済み | `ai` コンソールヒント切り替え（設計: [0032](spec/0032_ai-console-hint-toggle-spec.md)） |
| 0033 | [0033_ai-structured-output-stream-fix-implementation-spec.md](done/0033_ai-structured-output-stream-fix-implementation-spec.md) | 実装済み | `ai` structured output と streaming の衝突解消（設計: [0033](spec/0033_ai-structured-output-stream-fix-spec.md)） |
| 0034 | [0034_aibe-contextual-memory-implementation-spec.md](done/0034_aibe-contextual-memory-implementation-spec.md) | 実装済み | AIBE Contextual Memory MVP（設計: [0034](spec/0034_aibe-contextual-memory-spec.md)） |
| 0035 | [0035_aibe-memory-identity-split-implementation-spec.md](done/0035_aibe-memory-identity-split-implementation-spec.md) | 実装済み | AIBE Memory Identity Split（設計: [0035](spec/0035_aibe-memory-identity-split-spec.md)） |
| 0036 | [0036_shell-exec-approval-ux-implementation-spec.md](done/0036_shell-exec-approval-ux-implementation-spec.md) | 実装済み | `shell_exec` 承認 UX 拡張（設計: [0036](spec/0036_shell-exec-approval-ux-spec.md)） |
| 0037 | [0037-phase0-docs-drift-implementation-spec.md](done/0037-phase0-docs-drift-implementation-spec.md) | 実装済み（Phase 0） | Contextual Memory Runtime v1 Phase 0 — docs/spec drift 修正（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase1-builtin-memory-kind-registry-implementation-spec.md](done/0037-phase1-builtin-memory-kind-registry-implementation-spec.md) | 実装済み（Phase 1） | Contextual Memory Runtime v1 Phase 1 — Builtin MemoryKindRegistry（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase2-add-defaulting-memory-kind-list-implementation-spec.md](done/0037-phase2-add-defaulting-memory-kind-list-implementation-spec.md) | 実装済み（Phase 2） | Contextual Memory Runtime v1 Phase 2 — Add defaulting + MemoryKindList RPC（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase3-resolver-policy-implementation-spec.md](done/0037-phase3-resolver-policy-implementation-spec.md) | 実装済み（Phase 3） | Contextual Memory Runtime v1 Phase 3 — ResolverPolicy（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase4-memory-recipe-implementation-spec.md](done/0037-phase4-memory-recipe-implementation-spec.md) | 実装済み（Phase 4） | Contextual Memory Runtime v1 Phase 4 — MemoryRecipe（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase5-memory-subscribe-implementation-spec.md](done/0037-phase5-memory-subscribe-implementation-spec.md) | 実装済み（Phase 5） | Contextual Memory Runtime v1 Phase 5 — MemorySubscribe（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase6-capability-model-implementation-spec.md](done/0037-phase6-capability-model-implementation-spec.md) | 実装済み（Phase 6） | Contextual Memory Runtime v1 Phase 6 — Capability model（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0037 | [0037-phase7-multi-client-readiness-docs-implementation-spec.md](done/0037-phase7-multi-client-readiness-docs-implementation-spec.md) | 実装済み（Phase 7） | Contextual Memory Runtime v1 Phase 7 — Multi-client readiness docs（設計: [0037](spec/0037_aibe-contextual-memory-runtime-v1-spec.md)） |
| 0038 | [0038_contextual-memory-pack-phase-a-implementation-spec.md](done/0038_contextual-memory-pack-phase-a-implementation-spec.md) | 実装済み（Phase A） | Contextual Memory Pack Phase A — `[memory] enabled` による basic 切替（設計: [0038](spec/0038_contextual-memory-pack-phase-a-spec.md)） |
| 0038 | [0038_contextual-memory-pack-phase-b-implementation-spec.md](done/0038_contextual-memory-pack-phase-b-implementation-spec.md) | 実装済み（Phase B） | Contextual Memory Pack Phase B — TurnHook / RpcExtension trait 化（設計: [0038 Phase B](spec/0038_contextual-memory-pack-phase-b-spec.md)） |
| 0038 | [0038_contextual-memory-pack-phase-c-implementation-spec.md](done/0038_contextual-memory-pack-phase-c-implementation-spec.md) | 実装済み（Phase C） | Contextual Memory Pack Phase C — CLI / built-in kind pack 移行（設計: [0038 Phase C](spec/0038_contextual-memory-pack-phase-c-spec.md)） |
| 0038 | [0038_contextual-memory-pack-phase-d-implementation-spec.md](done/0038_contextual-memory-pack-phase-d-implementation-spec.md) | 実装済み（Phase D） | Contextual Memory Pack Phase D — `memory` feature / basic build（設計: [0038 Phase D](spec/0038_contextual-memory-pack-phase-d-spec.md)） |
| 0039 | [0039_aish-memory-pack-externalization-implementation-spec.md](done/0039_aish-memory-pack-externalization-implementation-spec.md) | 実装済み | AISH Memory Pack Externalization — builtin kind / recipe TOML 外出し（設計: [0039](spec/0039_aish-memory-pack-externalization-spec.md)） |
| 0040 | [0040_generic-recipe-cli-aish-name-cleanup-implementation-spec.md](done/0040_generic-recipe-cli-aish-name-cleanup-implementation-spec.md) | 実装済み | Generic Recipe CLI / AISH Name Cleanup（設計: [0040](spec/0040_generic-recipe-cli-aish-name-cleanup-spec.md)） |
| 0052 | [0052_ai-work-implementation-spec.md](done/0052_ai-work-implementation-spec.md) | 実装済み | `ai work` 作業文脈管理（設計: [0052](spec/0052_ai_work.md)） |
| 0053 | [0053_ai-suggested-command-recall-implementation-spec.md](done/0053_ai-suggested-command-recall-implementation-spec.md) | 実装済み | `ai` 提案コマンド再呼び出し（設計: [0053](spec/0053_ai-suggested-command-recall-spec.md)） |
| 0054 | [0054_safe-file-write-tools-implementation-spec.md](done/0054_safe-file-write-tools-implementation-spec.md) | 実装済み | Safe File Write Tools マスター（設計: [0054](spec/0054_safe-file-write-tools-spec.md)） |
| 0054 | [0054_safe-file-write-tools-phase1-implementation-spec.md](done/0054_safe-file-write-tools-phase1-implementation-spec.md) | 実装済み（Phase 1） | protocol / name / config / DTO |
| 0054 | [0054_safe-file-write-tools-phase2-implementation-spec.md](done/0054_safe-file-write-tools-phase2-implementation-spec.md) | 実装済み（Phase 2） | safe_path / SHA-256 / read_file 移行 |
| 0054 | [0054_safe-file-write-tools-phase3-implementation-spec.md](done/0054_safe-file-write-tools-phase3-implementation-spec.md) | 実装済み（Phase 3） | read_file metadata |
| 0054 | [0054_safe-file-write-tools-phase4-implementation-spec.md](done/0054_safe-file-write-tools-phase4-implementation-spec.md) | 実装済み（Phase 4） | diff / atomic / journal |
| 0054 | [0054_safe-file-write-tools-phase5-implementation-spec.md](done/0054_safe-file-write-tools-phase5-implementation-spec.md) | 実装済み（Phase 5） | FileChangeService / approval gate |
| 0054 | [0054_safe-file-write-tools-phase6-implementation-spec.md](done/0054_safe-file-write-tools-phase6-implementation-spec.md) | 実装済み（Phase 6） | @edit / write_file |
| 0054 | [0054_safe-file-write-tools-phase7-implementation-spec.md](done/0054_safe-file-write-tools-phase7-implementation-spec.md) | 実装済み（Phase 7） | apply_patch |
| 0054 | [0054_safe-file-write-tools-phase8-implementation-spec.md](done/0054_safe-file-write-tools-phase8-implementation-spec.md) | 実装済み（Phase 8） | ai 承認 UI |
| 0054 | [0054_safe-file-write-tools-phase9-implementation-spec.md](done/0054_safe-file-write-tools-phase9-implementation-spec.md) | 実装済み（Phase 9） | 統合 / docs |
| 0041 | [0041_ai-smart-feature-plan-implementation-spec.md](done/0041_ai-smart-feature-plan-implementation-spec.md) | 実装済み | `ai` Smart Feature Plan（設計: [0041](spec/0041_ai-smart-feature-plan-spec.md)） |
| 0042 | [0042_configurable-smart-features-implementation-spec.md](done/0042_configurable-smart-features-implementation-spec.md) | 実装済み | Configurable Smart Features（設計: [0042](spec/0042_configurable-smart-features-spec.md)） |
| 0043 | [0043_feature-pack-boundary-hardening-implementation-spec.md](done/0043_feature-pack-boundary-hardening-implementation-spec.md) | 実装済み（Phase 1） | Feature Pack Boundary Hardening（設計: [0043](spec/0043_feature-pack-boundary-hardening-spec.md)） |
| 0043 | [0043_feature-pack-boundary-hardening-phase2-implementation-spec.md](done/0043_feature-pack-boundary-hardening-phase2-implementation-spec.md) | 実装済み（Phase 2） | Feature Pack Boundary Hardening Phase 2（eligibility / generic memory / read-only tools） |
| 0043 | [0043_feature-pack-boundary-hardening-phase3-implementation-spec.md](done/0043_feature-pack-boundary-hardening-phase3-implementation-spec.md) | 実装済み（Phase 3） | Feature Pack Boundary Hardening Phase 3（FeaturePackConfig 分離 / composition root 解決） |
| 0044 | [0044_smart-preprocessor-implementation-spec.md](done/0044_smart-preprocessor-implementation-spec.md) | 実装済み（Phase 1–3） | AISH Smart Preprocessor / Local Intent Router（設計: [0044](spec/0044_smart-preprocessor-spec.md)） |
| 0044 | [0044_smart-preprocessor-phase2.6-implementation-spec.md](done/0044_smart-preprocessor-phase2.6-implementation-spec.md) | 実装済み（Phase 2.6） | Smart Preprocessor production 仕上げ（threshold 分離 / observation 拡張 / bundled model / failure_kind / context_needs+tool_hints） |
| 0044 | [0044_smart-preprocessor-phase2.7-implementation-spec.md](done/0044_smart-preprocessor-phase2.7-implementation-spec.md) | 実装済み（Phase 2.7） | Smart Preprocessor `route_turn` hint wire（3軸 gate 分離 / `RouteTurnPreprocessorHints` / observation 区別 / aibe advisory） |
| 0044 | [0044_smart-preprocessor-phase2.9-implementation-spec.md](done/0044_smart-preprocessor-phase2.9-implementation-spec.md) | 実装済み（Phase 2.9） | Smart Preprocessor local route fast path（`LocalRouteDecision` / tool enablement / observation metrics） |
| 0046 | [0046_aibe-graceful-restart-implementation-spec.md](done/0046_aibe-graceful-restart-implementation-spec.md) | 実装済み | aibe graceful restart（PID file / SIGTERM / stop / restart / status） |
| 0047 | [0047_ai-interactive-prompt-input-implementation-spec.md](done/0047_ai-interactive-prompt-input-implementation-spec.md) | 実装済み | `ai` 対話的プロンプト入力（bare `ai` / AI_EDITOR / reedline） |
| 0048 | [0048_ai-filter-streaming-fix-implementation-spec.md](done/0048_ai-filter-streaming-fix-implementation-spec.md) | 実装済み | `ai` output filter と assistant streaming の整合化（設計: [0048](spec/0048_ai-filter-streaming-fix-spec.md)） |
| 0049 | [0049_aish-command-output-replay-implementation-spec.md](done/0049_aish-command-output-replay-implementation-spec.md) | 実装済み | `aish` command output replay（設計: [0049](spec/0049_aish-command-output-replay-spec.md)） |
| 0050 | [0050_client-provided-replay-tool-implementation-spec.md](done/0050_client-provided-replay-tool-implementation-spec.md) | 実装済み | Client-Provided Replay Tool（設計: [0050](spec/0050_client-provided-replay-tool-spec.md)） |
| 0051 | [0051_smart-observation-report-implementation-spec.md](done/0051_smart-observation-report-implementation-spec.md) | 実装済み | Smart Preprocessor Observation Report（設計: [0051](spec/0051_smart-observation-report-spec.md)） |
実装順の目安（完了）: **0004** → **0005** → **0006** → **0007** → **0008** / **0009** → **0010** → **0011** → **0012** → **0013** → **0014** → **0015** → **0016**。
