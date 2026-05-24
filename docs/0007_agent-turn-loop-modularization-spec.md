# 0007 — agent_turn ループ本体のモジュール分割 — 指示書

> **出典**: Codex `review`（2026-05-24）境界分割提案。0003 で `tool_round_terminator` のみ分離。  
> **状態**: **未実装**。

## 目的

`AgentTurnService` に集中している **1 ラウンド分の LLM↔ツール実行** を独立モジュールに切り出し、単体テスト・将来の並列化・ラウンド上限ポリシー変更を容易にする。

0003 時点の `agent_turn.rs` 責務:

- リクエスト検証・context 組み立て
- ツールなし `complete()` 経路
- ツール付き for-loop（LLM step → tool 実行 → conversation 更新）
- max-round 分岐 → terminator 委譲

本指示書は **ループ本体（1 iteration）** の抽出を定義する。

## スコープ

### 対象

- `ToolRoundExecutor`（仮称）— 入力: conversation, allowed_tools, tool_ctx, registry → 出力: 更新 conversation + 実行記録 + 「続行 / 完了 / 上限」
- `AgentTurnService` — ラウンドカウンタと terminator 呼び出しのみ
- `rejected_tool_result` 等のヘルパを `application/tool_round/` 配下に集約

### 対象外

- 0006 の終端 **戦略** 差し替え（0006 が terminator port を担当）
- プロトocol 変更
- 新ツール追加

## 提案 API（案）

```rust
pub enum RoundOutcome {
    Completed { assistant: ChatMessage, executed: Vec<ExecutedToolCall> },
    Continue { conversation: Vec<ChatMessage>, executed: Vec<ExecutedToolCall> },
}

pub struct ToolRoundExecutor { /* llm + registry */ }

impl ToolRoundExecutor {
    pub async fn run_one_round(
        &self,
        conversation: &[ChatMessage],
        allowed_tools: &[ToolName],
        tool_ctx: &ToolExecutionContext,
        tool_defs: &[ToolDefinition],
    ) -> Result<RoundOutcome, LlmError>;
}
```

## 受け入れ条件

1. `agent_turn.rs` の行数・分岐が **明確に減る**（目安: ループ本体の 80 行以上を別ファイルへ）。
2. `ToolRoundExecutor` に対する **MockLlm 単体テスト** が 3 ケース以上（tool なし完了、1 tool 実行、許可外 tool）。
3. 既存 `aibe/tests/agent_turn_loop.rs` が **無変更または最小変更** で通る（挙動不変）。
4. hexagonal: executor は `application` 内。adapters 直接参照なし。

## ディレクトリ案

```
aibe/src/application/
  agent_turn.rs              # オーケストレーションのみ
  tool_round/
    mod.rs
    executor.rs              # run_one_round
    rejected.rs              # rejected_tool_result
```

## 0003 / 0006 との関係

| ドキュメント | 分割単位 |
|-------------|----------|
| 0003 | max-round **終端**（terminator） |
| 0006 | terminator **戦略** |
| 0007 | ループ **1 ラウンド** |

## 未確定・残リスク

- 過度な抽象化（1 ラウンドすら port 化）は見送り。application 内 concrete で十分
- 将来並列 tool 実行時は executor 内部を拡張（0007 スコープ外）
