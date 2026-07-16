# 0063 Human Task suspend checkpoint 手動検証

## 前提

- `cargo build --workspace`
- mockまたは検証用aibe profileを使い、実API keyを手順やログへ記録しない
- bashとzshを利用できるUnix端末
- 検証専用`AI_CONFIG`で一時`history_dir`を指定する

## 手順

1. checkpointがない状態で`AI_CONFIG="$TMPDIR/ai.toml" ai human-task status`を実行し、`No suspended Human Task.`、exit 0、aibe未起動でも成功することを確認する。
2. `ai collab '人間の確認が必要な作業を依頼する'`を開始し、agentが明示`human_task`を呼んだHuman Shell内で`type human-task`を確認する。
3. `human-task suspend '承認を確認してから続ける'`を実行する。shellが終了し、親turnが`Human Task suspended.`とtask ID、将来のresume案内を表示して正常終了することを確認する。
4. aibe/aiのturn終了後に別の`ai human-task status`を実行し、task ID、state、objective、local time、reason、cwd、`ai human-task resume <TASK_ID>`案内が表示されることを確認する。
5. bashとzshで手順2–4を繰り返し、通常の`~/.bashrc` / `~/.zshrc`とPATHが変更されていないことを確認する。
6. 4097 bytesのreason、改行を含むreason、読み手のないcontrol FIFO相当の送信失敗を試し、commandがnon-zeroでshellを終了しないことを確認する。
7. 通常のCtrl+D / `exit`ではDoneとして親agentが継続し、checkpointが残らないことを確認する。

## 期待結果

- Running checkpoint保存後だけHuman Shellが起動し、suspend後は同roundの後続toolや追加LLM callを実行しない。
- statusはsocket不要かつread-onlyで、破損・未知version・権限不正・Runningを「taskなし」へ丸めない。
- 旧`shell_exec` handoffには`human-task suspend`を公開せず、既存のreturn動作を維持する。

## 未実装

`ai human-task resume` / `cancel`、continuation、crash recovery、file lock/lease、schema migrationは案内文字列以外は未実装である。
