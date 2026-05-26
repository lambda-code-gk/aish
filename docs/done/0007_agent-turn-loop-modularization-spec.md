# 0007 — agent_turn ループ本体のモジュール分割 — 指示書

> **出典**: Codex `review`（2026-05-24）境界分割提案。0003 で `tool_round_terminator` のみ分離。  
> **レビュー**: Codex `spec`（2026-05-24）— API 責務・0006 境界・受け入れ条件を反映済み。  
> **状態**: **実装済み**（2026-05-24）。

## 目的

`AgentTurnService` に集中している **1 ラウンド分の LLM↔ツール実行** を独立モジュールに切り出し、単体テスト・将来の並列化を容易にする。

**0006 との分担**: `max_rounds` 到達時の **終端処理**（terminator 委譲）は 0006 のまま。0007 は 1 ラウンド抽出のみ。上限判定は `AgentTurnService` の for-loop 側に残す。

0003 時点の `agent_turn.rs` 責務（参考）:

- リクエスト検証・context 組み立て（**0007 対象外 — service に残す**）
- ツールなし `complete()` 経路（**0007 対象外**）
- ツール付き for-loop の **1 iteration**（LLM step → tool 実行 → conversation 更新）→ **0007 対象**
- `round + 1 >= max_rounds` 分岐 → `finish_after_max_tool_rounds`（terminator 委譲）→ **0007 対象外 — service に残す**

## スコープ

### 対象

- `ToolRoundExecutor`（application 内 concrete）— 1 回の `complete_with_tools` と続く tool 実行まで。戻り値は **続行 / ツールなし完了** のみ（上限は含めない）
- `application/tool_round/` — `executor.rs`, `rejected.rs`（`rejected_tool_result` 等）
- `AgentTurnService` — 上記前処理・for-loop（カウンタ・max-round 分岐・terminator 呼び出し）のオーケストレーション

### 対象外

- 0006 の終端 **戦略** 差し替え・terminator port の変更
- `max_rounds` 到達判定と `finish_after_max_tool_rounds` の実装移動
- プロトコル変更
- 新ツール追加
- executor の port 化（application 内 struct で十分）

## 確定した設計判断

| 項目 | 決定 |
|------|------|
| 1 ラウンドの境界 | `complete_with_tools` から、assistant 追加・全 `tool_calls` 処理・conversation 更新まで。次ラウンドへ進むかは `RoundOutcome` で表す |
| `RoundOutcome` | `Completed`（モデルが tool を返さなかった）/ `Continue`（tool 実行後、ループ継続）のみ。**上限・terminator は含めない** |
| LLM エラー | `Result<RoundOutcome, LlmError>`。service が `client_response_for_llm_error` に変換 |
| `tool_defs` | **executor 内**で `definitions_for(&allowed_tools)` を生成。呼び出し側は `allowed_tools` のみ渡す（二重入力禁止） |
| `ToolsConfig` | executor 構築時に保持（少なくとも `exec_timeout_ms`）。`registry.get(...).execute(..., timeout, ctx)` の現行挙動を維持 |
| 出力上限など | 現行どおり registry / tool 実装側。0007 では `ToolsConfig` 以外を executor に増やさない |
| hexagonal | executor は `application` 内。`ports` の trait のみ依存（`LlmProvider`, `ToolRegistry`）。**adapters 直接参照禁止** |
| composition root | `application/server.rs` が `ToolRoundExecutor` を組み立て、`AgentTurnService` に注入（0006 の terminator 注入と同型） |
| `AgentTurnService` に残すもの | 空 `messages` 検査、`inject_shell_log_tail`、tools 空経路、`validate_tools_enabled`、`ToolExecutionContext` 生成、for-loop、`finish_after_max_tool_rounds` |

## 提案 API

```rust
/// 1 ラウンドの結果。max-round 終端は AgentTurnService + terminator が担当。
pub enum RoundOutcome {
    Completed {
        assistant: ChatMessage,
        executed: Vec<ExecutedToolCall>,
    },
    Continue {
        conversation: Vec<ChatMessage>,
        executed: Vec<ExecutedToolCall>,
    },
}

pub struct ToolRoundExecutor {
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    tools_config: ToolsConfig,
}

impl ToolRoundExecutor {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        registry: Arc<dyn ToolRegistry>,
        tools_config: ToolsConfig,
    ) -> Self { /* ... */ }

    /// 1 回: LLM（tools 付き）→ tool 実行 → conversation 更新。
    /// `executed` は呼び出し側が持つ累積リストに、本ラウンド分を追記して返す。
    pub async fn run_one_round(
        &self,
        conversation: &[ChatMessage],
        allowed_tools: &[ToolName],
        tool_ctx: &ToolExecutionContext,
        executed_so_far: &[ExecutedToolCall],
    ) -> Result<RoundOutcome, LlmError>;
}
```

**service 側ループ（イメージ）**:

```rust
let mut executed = Vec::new();
for round in 0..max_rounds {
    match self.executor.run_one_round(&conversation, &allowed_tools, &tool_ctx, &executed).await {
        Ok(RoundOutcome::Completed { assistant, executed: round_executed }) => { /* AgentTurnResult */ }
        Ok(RoundOutcome::Continue { conversation: next, executed: round_executed }) => {
            conversation = next;
            executed = round_executed;
            if round + 1 >= max_rounds {
                return finish_after_max_tool_rounds(/* terminator 委譲 — 0006 のまま */).await;
            }
        }
        Err(e) => return client_response_for_llm_error(id, e),
    }
}
```

（上記は責務分担の説明用。実装時のシグネチャ・所有権は Rust 慣習に合わせて調整可。）

## 受け入れ条件

### 振る舞い（必須）

1. **挙動不変**: 既存 `aibe/tests/agent_turn_loop.rs`（max-round / replay / terminator 連携を含む）が **無変更または import・組み立てのみの最小変更** で通る。
2. **リファクタの可観測性**: ループ 1 iteration の本体（LLM step + tool 分岐 + conversation push）が `application/tool_round/executor.rs` に移り、`agent_turn.rs` はオーケストレーション中心になる（行数削減は副次指標とし、単独の合格条件にしない）。

### `ToolRoundExecutor` 単体テスト（MockLlm、最低 5 ケース）

| # | ケース | 期待 |
|---|--------|------|
| 1 | tool なし完了 | `RoundOutcome::Completed`、executed 空 |
| 2 | 1 tool 実行 | `RoundOutcome::Continue`、conversation に assistant + tool メッセージ、executed 1 件 |
| 3 | 許可外 tool（`tool_not_allowed`） | `Continue`、エラー tool result でループ継続可能 |
| 4 | 未実装 tool（`tool_not_implemented`） | `Continue`、registry に無い許可ツール名 |
| 5 | 複数 tool（同一ラウンド） | 呼び出し順で conversation / executed が並ぶ |

### アーキテクチャ

3. `cargo test --workspace` / `clippy` / `./scripts/check-architecture.sh` 成功。
4. `ToolRoundExecutor` の生成は `application/server.rs` のみ（テストは test helper または直接 `new` 可）。

## ディレクトリ案

```
aibe/src/application/
  agent_turn.rs              # run / 前処理 / for-loop + terminator 委譲
  tool_round/
    mod.rs
    executor.rs              # ToolRoundExecutor::run_one_round
    rejected.rs              # rejected_tool_result
```

## 0003 / 0006 との関係

| ドキュメント | 分割単位 |
|-------------|----------|
| 0003 | max-round **終端**（terminator モジュール） |
| 0006 | terminator **戦略**・`TerminationCapability` |
| 0007 | ループ **1 ラウンド**（LLM + tool 実行） |

```text
AgentTurnService
  ├─ 前処理（messages, shell log, validate, cwd）
  ├─ for round in 0..max_rounds
  │    └─ ToolRoundExecutor::run_one_round  ← 0007
  └─ round 上限 → finish_after_max_tool_rounds  ← 0003 / 0006（変更なし）
```

## `max_rounds` の扱い（Codex レビュー反映）

| 経路 | `max_rounds = 0` の挙動 |
|------|-------------------------|
| `config.toml` | **読み込み拒否**（`ConfigError::Invalid` — `max_rounds must be at least 1`） |
| プログラム（`ToolsConfig { max_rounds: 0, .. }`） | `ToolsConfig::effective_max_rounds()` が **1 に補正**（無限ループ防止。テスト・誤設定の安全網） |
| `AgentTurnService` ループ | `effective_max_rounds()` のみ参照（生の `max_rounds` は使わない） |

実装: `ports/outbound/config.rs`（`MIN_MAX_TOOL_ROUNDS`）、`adapters/outbound/toml_config.rs`（検証）、`agent_turn.rs`（ループ上限）。

テスト: `config` 単体（補正）、`toml_config`（TOML 拒否）、`agent_turn_loop`（プログラム 0 → 1 ラウンド上限）。

## 未確定・残リスク

- `run_one_round` の `executed_so_far` の受け渡し方（引数で累積 vs 戻り値のみで差分）— 実装時に所有権の単純な方を選ぶ
- 将来の並列 tool 実行は executor 内部を拡張（0007 スコープ外）
- port 化は見送り（過度な抽象化を避ける）
