# ai client-provided replay tool 手動検証

設計: [0050_client-provided-replay-tool-spec.md](../spec/0050_client-provided-replay-tool-spec.md)

## 前提

- `cargo build -p ai -p aibe -p aish -p aish-replay`
- `aish shell` の手動確認は **TTY** が必要
- `AISH_SESSION_DIR` がある場合とない場合の両方を試す
- `shell_log_mode` の既定は `hybrid`

## 手順 1: manifest と tail の確認

1. `aish shell` で session を作る。
2. `echo hello` と `cargo test -j 1` のような span を作る。
3. `ai ask` を実行し、`shell_log_mode=manifest` または `hybrid` のときに replay manifest が system instruction 側へ合成されることを確認する。
4. `shell_log_mode=tail` のときは replay manifest を出さず、通常の `shell log tail` だけで turn が進むことを確認する。

## 手順 2: client tool 往復の確認

1. `ai ask` で `aish.replay_show` が広告される条件を作る。
2. `ClientToolCallRequested` が来たら `aish.replay_show` を実行し、`[untrusted terminal output]` wrapper 付きの出力がモデルへ戻ることを確認する。
3. `stream=stderr` を shell span に対して要求したとき、`InvalidArguments` 相当で拒否されることを確認する。

## 手順 3: fallback の確認

1. `AISH_SESSION_DIR` を外した状態で `ai ask` を実行する。
2. `shell_log_mode=off` でも `ai ask` 自体は継続できることを確認する。
3. `manifest` mode では manifest が欠けたときに `shell_log_tail` へ倒れることを確認する。

## 手順 4: shared parser の確認

1. `aish replay list/show` と `ai` 側の `aish.replay_show` が同じ `index` を指すことを確認する。
2. shell span の stdout で prompt echo prefix の扱いが一致することを確認する。

## 補足

- `aibe` は `AISH_SESSION_DIR` を直接読まない。replay source の解決は `ai` 側の責務。
- `aish.replay_show` は read-only client tool としてのみ使う。
