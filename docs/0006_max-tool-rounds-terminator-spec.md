# 0006 — max_tool_rounds 終端戦略の改善 — 指示書

> **出典**: Codex `review`（2026-05-24）中優先度指摘。0003 で `ToolExecutionSummary` + `tool_round_terminator.rs` に分離済み。  
> **状態**: **未実装**（0003 はプレーンテキスト要約 + 単一 `complete()` 経路のみ）。

## 目的

ツールラウンド上限到達時の終端処理を、**プロバイダ差** と **回答品質** に耐えるよう拡張可能にする。

0003 現状の課題（レビュー指摘）:

- 会話履歴中の `role: tool` を捨て、要約テキストだけを再送している
- 一部プロバイダは tool メッセージを無視するため要約は必要だが、**要約の癖** に最終回答が引きずられる
- 終端戦略が 1 実装に固定で、OpenAI / Gemini / OpenAI 互換の差を吸収できない

## スコープ

### 対象

- `ToolRoundTerminator` **port**（application が依存、adapter / 内蔵実装が提供）
- 戦略の最低 2 種:
  1. **SummaryPrompt**（現行相当 — `ToolExecutionSummary` を user メッセージに埋め込み）
  2. **ConversationReplay**（可能なプロバイダ向け — 要約せず会話履歴を `complete()` に渡す。失敗時 SummaryPrompt にフォールバック）
- プロバイダ capability フラグ（例: `LlmProvider::supports_tool_role_in_plain_complete()`）

### 対象外

- `max_rounds` 値自体の動的変更
- 並列ツール実行
- streaming イベント

## 設計判断（実装前に確定）

| 項目 | 論点 |
|------|------|
| port 置き場 | `ports/outbound/tool_round_terminator.rs` vs application 内 trait |
| 既定戦略 | SummaryPrompt のまま（後方互換） |
| フォールバック | Replay 失敗 → SummaryPrompt → それでも ProviderError |
| 結果型 | `TerminationOutcome { conversation_used, strategy, assistant }` でテスト可能に |

## 受け入れ条件

1. `tool_round_terminator.rs` が **port 経由** で終端処理を委譲する。
2. 既定（未設定）挙動は **0003 と同一**（SummaryPrompt）。既存 `agent_turn_loop` max-round テストが無変更で通る。
3. ConversationReplay は MockLlm / 設定で有効化した統合テストが 1 本以上。
4. 終端戦略名または outcome がログ / テストで観測可能（デバッグ用。wire protocol 変更は不要）。

## 実装マップ（案）

```
aibe/src/ports/outbound/tool_round_terminator.rs   # trait ToolRoundTerminator
aibe/src/application/tool_round_terminator.rs        # 委譲・composition
aibe/src/adapters/outbound/terminator/               # summary.rs, replay.rs
aibe/src/ports/outbound/llm.rs                       # capability（必要なら）
```

## テスト

| 種別 | 内容 |
|------|------|
| 単体 | SummaryPrompt が `ToolExecutionSummary` を含む user メッセージを生成 |
| 単体 | Replay が tool 付き conversation をそのまま渡す |
| 統合 | ScriptedMockLlm で Replay → 最終 assistant が tool output を反映 |
| 手動 | `docs/manual/ai-ask-tools.md` に max-round 到達例（実 LLM・任意） |

## 0003 との関係

0003 の `ToolExecutionSummary` / `tool_round_terminator` は **第一実装**。0006 はその **差し替え可能化**。

## 未確定・残リスク

- 実プロバイダごとの tool role 無視挙動は実測が必要（手動 + 0001 プロバイダテスト拡張）
- Replay 経路がトークン上限を超える場合の切り詰め（0006 スコープ外なら 0006 に「未対応」と明記）
