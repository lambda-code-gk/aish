# 0065 Human Task agent continuation 手動確認

## 前提

- `cargo build --workspace` 済み
- Collaborative Mode が利用するテスト用 aibe profile を設定済み（本番 API key をログやリポジトリへ置かない）
- 0064 の手順で Suspended checkpoint を作れる

## 正常導線

1. `ai human-task resume` で Human Shell を再開する。
2. Human Shell で安全な観測コマンドを実行し、`exit` または Ctrl+D で Done にする。
3. `State: result pending` と `Continuing Collaborative Mode...` の後、新しい agent 応答が表示されることを確認する。
4. agent が Human Task の結果だけで完了断定せず、必要な環境再観測と completion criteria の確認を行うことを確認する。
5. `ai human-task status` が `No suspended Human Task.` を返すことを確認する。

## ResultPending retry

1. テスト用 aibe を停止した状態で Human Task を Done にし、continuation を失敗させる。
2. `ai human-task status` が `State: result pending` と `ai human-task resume` を案内することを確認する。
3. aibe を復旧し、`ai human-task resume [TASK_ID]` を実行する。
4. Human Shell が起動せず、保存済み結果から continuation だけが実行されることを確認する。
5. 成功後に checkpoint が削除されることを確認する。

## fail-closed 確認

- continuation 失敗後も ResultPending と同じ `continuation_turn_id` が checkpoint に残る。
- 同じ aibe process へ成功済み continuation turn ID を再送すると `invalid_request` で拒否される。
- ai / aibe を Continuing 中に強制終了した場合の自動復旧は未実装である。0066の`ai human-task recover`による明示回復だけを使用し、checkpointを手動編集・削除しない。

## 実施状況

- 未実施。自動テストでは fake LLM / file store / RequestService により正常・失敗・重複拒否を検証する。
