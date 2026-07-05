# Collaborative Human Handoff 手動検証

0055 協調作業（`ai --collaborative`）の手動チェックリスト。

## 前提

- `aibe` が起動可能（または `ai` が自動起動する）
- `aish` / `ai` がビルド済み
- 協調 handoff 用の API キーは **aibe 設定のみ**（`ai` / ログへ token を出さない）

## チェックリスト

1. **`ai --collaborative "…"`** — 親エージェントを開始し、親の `shell_exec` が human shell handoff になる
2. **Alt+. / Alt+,** — handoff 由来の command candidate をプロンプトへ挿入（自動実行されない）
3. **編集・実行・非実行** — 人間が自由にコマンドを選べる
4. **Ctrl+D / `exit`** — 親へ制御が戻り、親が再観測して継続する
5. **human shell 内 `ai`** — 同一 side conversation を継続（入れ子 `--collaborative` は拒否）
6. **side 人間待ち** — `request_human_action` 後、プロンプトに waiting 表示。裸 `ai` で side を再開
7. **`ai --standalone`** — handoff 環境変数を除去した独立セッション
8. **異常終了 → `ai resume`** — ORPHANED を復旧（token rotation、旧 token 拒否）
9. **`ai status`** — handoff あり/なし。token は表示されない

## プロンプト表示

human shell プロンプト先頭に `[collab:…]` が常時付く（無効化不可）。`SIDE_AGENT_WAITING_FOR_HUMAN` では `run 'ai' to resume` ヒントが出る。

human shell 起動直後（親 handoff 直後・`ai resume` 復旧時）は、handoff store から **目的・依頼・候補コマンド** を stderr に briefing 表示する（`ai:` プレフィックス付き）。side agent が `request_human_action` を返したときの `ai` 実行結果も同様に stderr 整形表示する（raw JSON は出さない）。

## ログ redaction

`AISH_HANDOFF_TOKEN` および handoff token 平文は shell log / replay に残らない。`ai status` / LLM 入力にも出さない。

## 関連

- 設計: [docs/spec/0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)
- アーキテクチャ: [docs/architecture.md](../architecture.md)「Collaborative human handoff」
