以下は AISH Smart Preprocessor の観測ログ集計レポートです。

このレポートをもとに、Smart Preprocessor が日常利用に耐える状態かを評価してください。

重要な前提:

* このレポートには raw user text は含まれていません。
* そのため、intent 分類が意味的に正しかったかどうかは直接評価できません。
* 代わりに、観測可能なメトリクスから以下を評価してください。

  * assist / gate の使われ方は妥当か
  * short-circuit が過剰に発生していないか
  * route_turn fallback が多すぎないか
  * route_turn hints が出ているのに注入されていないケースが多くないか
  * local_route が実際に latency / LLM call 削減に寄与しているか
  * intent / context_needs / tool_hints / reason_codes に偏りや異常がないか
  * gate を広げてもよいか、それとも assist 運用に留めるべきか
  * 次に収集すべき追加メトリクスは何か

評価では、以下の形式で答えてください。

## 総合判定

以下から1つ選んでください。

* A: assist / gate ともに日常利用を広げてよい
* B: assist は常用してよいが、gate は限定運用が妥当
* C: shadow / assist で観測継続すべき
* D: Smart Preprocessor の設計または実装を見直すべき

## 根拠

数値に基づいて判断してください。
特に以下を見てください。

* total_records / valid_records / invalid_lines
* by_mode
* by_intent
* by_gate
* by_decision_path
* by_fallback_reason
* route_turn_used_count
* local_route_used_count
* route_turn_skipped_count
* route_turn_fallback_count
* route_turn_hints_present_count
* route_turn_hints_injected_count
* short_circuit_allowed_count
* estimated_tokens_saved_sum
* latency avg / p50 / p95
* llm_call_count_estimated_sum
* context_needs / tool_hints / reason_codes

## 懸念点

観測ログから見えるリスクを列挙してください。

例:

* fallback が多い
* confidence が高いのに route_turn_required が多い
* short_circuit_allowed が多すぎる
* local_route_fallback が多い
* specific intent に偏りすぎている
* route_turn_hints_present に対して injected が少ない
* latency 削減効果が小さい
* invalid_lines が多い
* 判断に必要なメトリクスが足りない

## 次にやるべきこと

優先順位付きで提案してください。

分類は以下に分けてください。

1. すぐ修正すべきバグ・欠陥
2. 設定変更で試すべきこと
3. 追加すべき観測メトリクス
4. gate 対象を広げる/狭める判断
5. replay / memory / route_turn 連携で改善できること

## gate 運用方針

以下のどれが妥当か提案してください。

* gate はまだ使わず shadow/assist のみ
* gate は simple_chat のみ許可
* gate は simple_chat + 一部 inspect まで許可
* gate をさらに広げてよい
* gate は設計見直しが必要

## 最後に

このレポートだけでは判断できないことを明示してください。
また、次回レポートで追加すると評価精度が上がる項目があれば提案してください。

以下がレポートです。

```markdown
ここに `ai smart report --limit 1000 --include-recent 30` の出力を貼る
```

