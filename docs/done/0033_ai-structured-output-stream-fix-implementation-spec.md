# 0033 — `ai` structured output と streaming の衝突解消 実装指示書

> **種別**: 実装指示書（`docs/done/`）  
> **設計の正本**: [0033_ai-structured-output-stream-fix-spec.md](../spec/0033_ai-structured-output-stream-fix-spec.md)  
> **状態**: 実装済み  
> **対象**: `ai` のみ。`aibe` プロトコル変更なし、`aibe-client` / `aish` 変更なし

## 変更ファイル一覧

| パス | 責務 |
|------|------|
| `ai/src/main.rs` | `resolve_turn_settings()` の progress 判定から `output_format` 依存を外す。turn スコープの `ProgressGuard` を導入し、`?` で抜けても spinner が残らないようにする。`run_agent_turn_*` の呼び出し前後で RAII を使う。 |
| `ai/src/adapters/outbound/stdout_presenter.rs` | `show_stream_chunk()` を structured output 時に no-op 化する。`show_response()` は structured output の唯一の stdout 出力点として維持する。単体テストをここに追加する。 |
| `ai/tests/phase_a_cli.rs` | mock socket server を拡張し、`AssistantStreaming` → `AgentTurnResult` の順で流れるケースを追加する。`--format json|tsv|env` で stdout が壊れないことを確認する。 |
| `ai/tests/ux_gap_closure.rs` | `chat` / `retry` / `rerun` の経路でも同じ structured output 抑止が効くことを確認する。turn 継続経路の回帰を防ぐ。 |
| `ai/src/main.rs` の `#[cfg(test)]` もしくは既存 unit テスト | `ProgressGuard` の drop で停止保証を検証する小さな unit テストを置く。早期 return と通常完了の両方を通す。 |
| `docs/architecture.md` | turn 進行表示の記述を更新し、`--format` が stderr progress の正本ではないこと、stdout の structured output と stderr progress が別責務であることを明記する。 |
| `docs/spec/0027_ai-ux-spec.md` | A-6 / C-2 / C-3 に追記し、structured output 時の assistant streaming 抑止が stdout 契約の保護であり、progress は stderr 側で独立に扱うことを明文化する。 |

## 実装手順

1. `ai/src/adapters/outbound/stdout_presenter.rs` で `show_stream_chunk()` を修正し、`output_format.is_some()` のときは stdout に一切書かないようにする。ここで streaming を止めるのではなく、表示経路だけを閉じる。
2. `show_response()` は structured output の最終出力だけを担当させる。`output_format` があるときは既存の structured renderer をそのまま使い、streamed turn でも最後に 1 回だけ stdout へ出す。
3. `ai/src/main.rs` に turn スコープの `ProgressGuard` を追加し、`begin_turn_progress()` / `end_turn_progress()` の手動対応をやめる。guard は生成時に開始し、`Drop` で必ず停止する。
4. `resolve_turn_settings()` の `progress_spinner` 判定から `output_format.is_none()` を除去する。`--format` は stdout 契約だけを決め、stderr progress の可否は `quiet` / `TTY` / `progress` で解く。
5. `run_agent_turn_sync()` / `run_agent_turn_async()` / `run_agent_turn_core()` の呼び出し順を見直し、`ProgressGuard` が `?` で崩れない構造にする。turn 本体の成功・失敗・途中 return のどれでも guard が drop される形に固定する。
6. unit テストで presenter の契約と guard の停止保証を固める。integration テストでは mock socket server を使い、`AssistantStreaming` が 1 回以上来ても `--format` の stdout が parse 可能なままであることを確認する。
7. `docs/architecture.md` と `docs/spec/0027_ai-ux-spec.md` を同じ変更で更新し、今回の責務分離を設計文書へ反映する。

## テスト追加

### unit

- `ai/src/adapters/outbound/stdout_presenter.rs`
  - `show_stream_chunk()` が `output_format = Some(Json|Tsv|Env)` のとき stdout を汚さないこと
  - `show_response()` が structured output を 1 回だけ出すこと
- `ai/src/main.rs`
  - `resolve_turn_settings()` の progress 判定が `output_format` に引きずられないこと
  - `ProgressGuard` の drop で spinner 停止が保証されること

### integration

- `ai/tests/phase_a_cli.rs`
  - mock socket server で `AssistantStreaming` を先に返し、その後に `AgentTurnResult` を返す scripted ケースを追加する
  - `ai --format json|tsv|env --no-start` で stdout が parse 可能で、stream chunk が混入しないことを確認する
  - `--quiet` は stderr を抑制しても structured stdout には影響しないことを確認する
- `ai/tests/ux_gap_closure.rs`
  - `chat` / `retry` / `rerun` でも同じ structured output 抑止が効くことを確認する
  - turn 継続経路でも stdout contract が壊れないことを確認する

## docs 更新

- `docs/architecture.md`
  - `turn 進行表示` の `progress_spinner` 解決から `output_format` 依存を外す
  - `assistant_streaming` と structured output の責務分離を追記する
  - `ProgressGuard` による turn スコープの停止保証を短く補足する
- `docs/spec/0027_ai-ux-spec.md`
  - A-6 に「`--format` は stdout 表現のみを決める。streaming chunk を stdout に流さないのは `ai` の表示層責務」と追記する
  - C-2 / C-3 に「progress は stderr、structured output は stdout、turn 終了保証は RAII」と追記する
  - 既存の 0027 の Phase 記述は崩さず、補足の追記に留める

## 受け入れ条件チェックリスト

- [ ] `ai --format json|tsv|env` で assistant streaming chunk が stdout に混ざらない
- [ ] structured output は最後に 1 回だけ出力され、parse 可能なまま維持される
- [ ] `show_stream_chunk()` は structured output 時に no-op になる
- [ ] `ProgressGuard` の drop で spinner が必ず停止する
- [ ] `run_agent_turn_*` の失敗や早期 return でも spinner が残らない
- [ ] `--format` が stderr progress の有効/無効を勝手に変えない
- [ ] `ai` 以外のクレートに変更が入らない
- [ ] unit / integration テストが追加される
- [ ] `docs/architecture.md` と `docs/spec/0027_ai-ux-spec.md` が同じ変更で更新される
- [ ] `./scripts/verify.sh` が通る
- [ ] `./scripts/smoke-mock.sh` が通る

## Step 6 で実行するコマンド

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

## 未確定・残リスク

- `smoke-mock.sh` は非 TTY なので、実機 TTY の spinner 視覚確認までは担保しない
- mock socket server の scripted streaming ケースは、`AssistantStreaming` の送受信順序を壊さない実装が前提になる
- `ProgressGuard` の実装位置を `main.rs` に置くか、小さな内部モジュールに分けるかは最小差分で判断する
- `docs/spec/0027_ai-ux-spec.md` への追記位置は、A-6 / C-2 / C-3 の近傍に収める前提で微調整が必要になる可能性がある
