# 0048 — `ai` output filter と assistant streaming の整合化 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-06-22  
> **関連**: [0022_ai-filter-spec.md](../done/0022_ai-filter-spec.md)、[0033_ai-structured-output-stream-fix-spec.md](0033_ai-structured-output-stream-fix-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)、[ai-ask-tools.md](../manual/ai-ask-tools.md)

## 0. 目的

`AI_FILTER` / `[ask].filter` が assistant streaming 導入後に効かなくなっている問題を、`ai` クレート内の出力制御で解消する。

本仕様の目的は次のとおり。

1. filter 有効時は assistant streaming chunk を stdout に出さない
2. turn 終了時に `AgentTurnResult.assistant_message.content` 全文へ、0022 の filter を 1 回だけ適用する
3. progress spinner は stderr の責務のまま維持する
4. 0022 の batch filter 契約を維持しつつ、streaming UX は filter 有効時のみ犠牲にする

本書は方式 A を採る。つまり、streaming 中の chunk を filter pipe に逐次流す方式は採用しない。

## 1. 非目標

- 方式 B による chunk 逐次 filter
- `aibe` の挙動変更
- `aish` の挙動変更
- filter コマンド仕様そのものの変更
- assistant streaming の wire 変更
- `ai` 以外のクレートへの影響拡大

## 2. パック構成の適用

**No**。この変更は optional 機能束を core から脱着する話ではなく、`ai` の presenter / turn 終了処理にある stdout 契約を修正するだけである。`AI_FILTER` / `[ask].filter` は設定駆動の出力整形であり、Active Pack / Basic Pack を持つ独立した optional runtime にはならない。composition root で pack を選ぶ構造も不要で、通常の ports & adapters と turn 終了時の後処理として扱うのが正しい。

## 3. 現状の問題

### 3.1 streaming chunk が filter を通らない

0022 では `AI_FILTER` が実装済みだが、`StdoutPresenter::show_stream_chunk()` は filter を通さず chunk をそのまま stdout に出している。assistant streaming が有効な turn では、途中の chunk がそのままユーザー stdout に流れ、最終的な filter 結果と整合しない。

### 3.2 `show_response(..., streamed=true)` が filter 経路を飛ばす

`show_response()` は `streamed=true` のとき `emit_assistant_stdout` をスキップしている。0022 の filter は `AgentTurnResult.assistant_message.content` に対して 1 回だけ適用するのが正しいが、streamed 判定があるせいで filter 有効時の最終出力でも bypass が起きる。

### 3.3 aibe はほぼ常に AssistantStreaming を送る

aibe はほぼ常に `AssistantStreaming` を送る。非 streaming プロバイダでも synthetic な 1 delta に変換されるため、`ai` 側は streaming を前提にして stdout 契約を分け直さないと、filter が再び効かなくなる。

### 3.4 0033 と同じ種類の stdout 契約分離が必要

0033 では `--format json|tsv|env` 時に streaming chunk を抑止し、最終 structured 出力だけを stdout に出す整理をした。今回も同様に、filter 有効時は streaming chunk を stdout に出さず、最終 turn 完了時の本文だけを filter に通す必要がある。

## 4. 決定事項

### 4.1 filter 有効時は assistant streaming chunk を stdout に出さない

- `AI_FILTER` または `[ask].filter` が有効な turn では、assistant streaming chunk を stdout に書かない
- streaming event 自体は受信するが、表示は抑止する
- progress spinner は stderr 側の表示として継続する

### 4.2 turn 終了時に全文へ 1 回だけ filter を適用する

- turn の終わりに `AgentTurnResult.assistant_message.content` 全文を取り出す
- 0022 と同じ filter 解決・実行経路で 1 回だけ filter を適用する
- filter の stdout / stderr / warning / spawn failure の扱いは 0022 の契約を維持する

### 4.3 streaming UX の扱い

- filter が無効な場合は、現行の streaming UX を維持する
- filter が有効な場合は、streaming の逐次可視化を犠牲にしてでも、stdout の整合性を優先する
- これは batch filter 契約の維持と、streaming 表示の両立が同時にできないための明示的なトレードオフである

## 5. `StdoutPresenter` と `main.rs` の責務

### 5.1 `StdoutPresenter::show_stream_chunk`

- `show_stream_chunk()` は assistant streaming の表示入口だが、filter 有効時は no-op にする
- `output_format` の有無と同じく、filter 有効かどうかでも stdout への反映を分岐する
- `show_stream_chunk()` が filter を通したり、filter 用の最終出力を先出ししたりしてはいけない

### 5.2 `StdoutPresenter::show_response`

- `show_response()` は turn 終了時の最終出力点とする
- `streamed=true` であっても、filter 有効時は `emit_assistant_stdout` をスキップしない
- `streamed` を理由に filter 実行や filter 後 stdout を抑止してはならない
- filter 有効時は、`show_stream_chunk()` で chunk を stdout に出していないため、`show_response()` 側で全文 filter を必ず適用する

### 5.3 `main.rs`

- `main.rs` は turn 実行の結果を受けて、最終 assistant message を presenter に渡す
- `streamed` は「実際に assistant streaming chunk を stdout に表示したか」の事実記録に限定する
- `streamed = streamed || settings.progress || settings.timeout_secs.is_some()` のように、`progress` や `timeout_secs` を `streamed` に混ぜてはならない
- `progress` は stderr の spinner 制御にのみ使う
- `timeout_secs` は stderr 側の cancel / timeout 通知にのみ使う
- filter 有効時の stdout 抑止は presenter 側で統一する

## 6. 0022 仕様との関係

0022 の batch filter 契約は維持する。

- filter 対象は `AgentTurnResult.assistant_message.content` のみ
- `/bin/sh -c`、stdin pipe、stdout `write_all`、stderr 透過、非 0 終了 warning、spawn failure フォールバックは維持
- 変更点は「いつ filter を呼ぶか」だけであり、「どう filter を呼ぶか」は変えない

したがって、0022 の意味は次のように補強される。

- batch filter は turn 終了時に 1 回だけ適用する
- assistant streaming 中の chunk は filter 契約の対象にしない
- streaming UX は filter 有効時のみ落とす

## 7. 仕様

### 7.1 出力契約

`ai` の stdout 契約は次のように固定する。

- filter 無効: 現行どおり streaming chunk を表示しうる
- filter 有効: streaming chunk を stdout に出さず、turn 完了後の最終本文だけを filter に通す
- progress は stderr に残す

### 7.2 filter 解決

filter の優先順位は 0022 と同じく `非空 AI_FILTER` > `非空 [ask].filter` > なし とする。

空文字は未設定として扱う。設定・環境変数の解決順を変えない。

### 7.3 streamed フラグの扱い

`streamed` は「assistant streaming chunk を stdout に実際に表示したか」を示す内部フラグであり、filter 実行の抑止条件ではない。

特に次を禁止する。

- `streamed=true` を理由に最終本文への filter 適用を飛ばすこと
- `progress=true` や `timeout_secs` の有無を理由に filter 後 stdout を飛ばすこと
- `streamed || progress || timeout_secs` により filter 経路を丸ごと落とすこと
- `streamed` を「chunk を stdout に出した事実」以外に使うこと

### 7.4 progress spinner

progress spinner は stderr の責務として維持する。

- assistant streaming の有無と独立に、TTY / quiet / progress 設定に従う
- filter 有効時でも spinner の停止・再開の契約は崩さない

## 8. 受け入れ条件

### 8.1 unit

- `StdoutPresenter::show_stream_chunk()` は filter 有効時に stdout を汚さない
- `show_response()` は `streamed=true` でも filter 有効時に最終本文へ 1 回だけ filter を適用する
- `streamed || progress || timeout_secs` の条件で filter が飛ばされない
- `progress` と `timeout_secs` は stdout 抑止条件ではなく、それぞれ stderr spinner / cancel のみを制御する
- filter 無効時は従来の streaming 表示が維持される

### 8.2 integration

- `AI_FILTER` 有効時の assistant turn で、streaming chunk が stdout に出ない
- turn 終了後の stdout が、全文 filter 後の結果になる
- progress spinner は stderr に残り、filter 有効時も動作する
- `timeout_secs` は stdout 抑止に影響せず、stderr 側の cancel / timeout 経路のみを発火させる
- 非 streaming 相当の synthetic 1 delta でも同じ契約になる

### 8.3 manual

- `docs/manual/ai-ask-tools.md` の output filter 手順で、filter 有効時に streaming chunk が stdout に出ないことを確認できる
- 既存の filter 失敗・spawn failure の manual 手順は維持する

## 9. 影響範囲

### 9.1 変更対象クレート

- `ai` のみ

### 9.2 主対象

- `ai/src/adapters/outbound/stdout_presenter.rs`
- `ai/src/main.rs`
- `ai/tests/*` の filter / streaming 系

### 9.3 非対象

- `aibe`
- `aish`
- `aibe-protocol`

## 10. 補足

本件は streaming を止める話ではない。filter 有効時だけ stdout 契約を守るために、assistant streaming の可視化を最終出力から切り離す話である。  
0033 と同じく、`ai` の最終出力面を機械的に壊さないことを優先する。
