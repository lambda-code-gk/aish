# 0062 Collaborative Mode Human Task Tool 手動確認

## 前提

- `aibe` と `aish` が同じ workspace の最新ビルドであること。
- 対話端末から実行すること。

## 正式導線

1. `ai collab --tools none "作業ディレクトリを確認して戻ってください"` を実行する。
2. `human_task` だけが公開され、Human Shell に `Objective:` が表示されることを確認する。
3. 任意の操作後に Ctrl+D で戻り、追加の summary/status 入力なしで親 agent が継続することを確認する。

`--tools @exec` は `shell_exec` を追加する指定であり、`human_task` の公開条件ではない。`ai collab --tools @exec "..."` では両 tool が独立して公開される。

## 互換導線

`ai ask --collaborative --tools @exec "..."` は旧 0055 の `shell_exec` handoff 互換導線として維持される。正式な新規利用は `ai collab` とする。

## 表示と安全性

- `reason` がない場合は `Why this is a Human Task:` を表示しない。
- `instructions` が空なら `Suggested actions:`、`completion_criteria` が空なら `Done when:` を表示しない。
- `instructions` があるときは `Alt+.` / `Alt+,` で suggested action を prompt に挿入できる（実行はしない。Enter で実行）。複数あるときは巡回する。
- `done` は作業達成や自動検証済みを意味しない。必要なら親 agent が環境を再観測する。
- `AISH_HANDOFF_TASK_JSON` が子 shell 環境に残らないことを `env | rg AISH_HANDOFF` で確認する。
- Human Shell 中は `ai: / running human_task…` スピナーが消え、対話プロンプトが見えること。

この実 PTY 手順の最終確認は人間が行う。
