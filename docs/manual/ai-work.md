# `ai work` — 作業文脈の使い方

## `ai work` とは

`ai work` は、現在の作業・注目点・決定事項・思いつき・後回し項目を扱うための **高レベルな作業文脈機能** です。

推奨される日常導線は `ai work` です。`ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` は、既存の低レベル Contextual Memory 操作として残っています（互換性と詳細操作用）。

**重要**: `ai work` は goal/now/idea/mem の単なるエイリアスではありません。

- `ai work start` しても `ai goal show` に必ず反映されるわけではない
- `ai work focus` しても `ai now show` に必ず反映されるわけではない
- 両者は無理に同期しない（二重書き込みを避けるため）
- 通常の `ai ...` turn では、active work の goal/focus/recent decisions と、既存 contextual memory の関連情報が必要に応じて注入される

## 典型フロー

```bash
ai work start "今回の作業目標"
ai work focus "今やっていること"
ai work decide "こうすることにした"
ai work note "覚えておきたい観察"
ai work idea "あとで試す案"
ai work status
# ... 作業 ...
ai work finish
```

## コマンド一覧

| コマンド | 説明 |
|---------|------|
| `ai work` | ダッシュボード（active work の概要） |
| `ai work status` | active work の詳細（focus / stack / decisions / ideas / notes / deferred） |
| `ai work list` | 全 work を状態別に一覧 |
| `ai work start <goal>` | 新しい作業を開始（既存 active は Paused） |
| `ai work focus <text>` | active work の注目点を更新 |
| `ai work decide <text>` | 決定事項を active work に追加 |
| `ai work note <text>` | メモを active work に追加 |
| `ai work idea <text>` | 作業に関連する思いつきを active work に追加 |
| `ai work defer <text>` | 後回し work を新規作成（active は変えない） |
| `ai work push <goal>` | 一時的な寄り道として child work を開始（親は stack へ） |
| `ai work pop` | child work を閉じて親 work に戻る |
| `ai work switch <id>` | Paused / Deferred の work へ本格的に切り替え |
| `ai work finish` | active work を完了（stack が空のときのみ） |

## `idea` と `defer` の違い

| | `ai work idea` | `ai work defer` |
|---|----------------|-----------------|
| 対象 | **現在の active work** に紐づく | **独立した後回し work** として新規作成 |
| active work | 変わらない | **変わらない** |
| 用途 | 今の作業に関連する思いつき | 今はやらない別タスク・別テーマ |

## `push` と `switch` の違い

| | `ai work push` | `ai work switch` |
|---|----------------|------------------|
| 性質 | 一時的な寄り道 | 本格的な作業切り替え |
| 戻り方 | `ai work pop` で親に戻る | `switch` または `start` で別 work へ |
| stack | 親を stack に積む | stack は使わない（空である必要あり） |

## `defer` の出力

`defer` は active work を変えません。成功時は次のように表示されます。

```text
Deferred work #44:
  ブラウザ拡張からaibeの同一work contextを参照する

Active work remains:
  #42 Contextual memoryの運用導線を設計する
```

active work がない場合は `No active work.` と表示されます。

## `status` の表示

`ai work status`（および引数なし `ai work`）では、各セクションに表示上限があります。

- Decisions / Ideas / Notes: 最大 5 件
- Deferred: 最大 10 件
- Stack: 最大 5 件

上限を超える場合は `... and N more` と表示されます。

## 注意

- Work の goal / note / idea 等に API key、token、password を保存しない（自動 redaction されない）
- `[memory] enabled = false` または feature-off 時は fail-closed で拒否される
- 手動検証手順は [`docs/testing/ai-work-manual-verification.md`](../testing/ai-work-manual-verification.md) を参照
