# 0063 Human Task suspend checkpoint 手動検証

## 前提

- `cargo build --workspace`
- mockまたは検証用aibe profileを使い、実API keyを手順やログへ記録しない
- bashとzshを利用できるUnix端末
- 検証専用`AI_CONFIG`で一時`history_dir`を指定する

## 手順

1. checkpointがない状態で`AI_CONFIG="$TMPDIR/ai.toml" ai human-task status`を実行し、`No suspended Human Task.`、exit 0、aibe未起動でも成功することを確認する。
2. `ai collab '人間の確認が必要な作業を依頼する'`を開始し、agentが明示`human_task`を呼んだHuman Shell内で`type human-task`を確認する。
3. `human-task suspend '承認を確認してから続ける'`を実行する。shellが終了し、親turnが`Human Task suspended.`とtask ID、`ai human-task cancel --yes`案内を表示して正常終了することを確認する。
4. aibe/aiのturn終了後に別の`ai human-task status`を実行し、task ID、state、objective、local time、reason、cwd、同じcancel案内が表示されることを確認する。
5. `ai human-task cancel`を実行し、TTYの確認で拒否した場合はnon-zeroでstatus内容が残ることを確認する。続けて承認するか`--yes`を使い、削除成功後のstatus/cancelが`No suspended Human Task.`とexit 0になることを確認する。
6. cancel後に新しい`ai collab`から明示Human Taskを開始できることを確認する。
7. bashとzshで手順2–4を繰り返し、通常の`~/.bashrc` / `~/.zshrc`とPATHが変更されていないことを確認する。
8. 4097 bytesのreason、改行を含むreason、読み手のないcontrol FIFO相当の送信失敗を試し、commandがnon-zeroでshellを終了しないことを確認する。
9. 通常のCtrl+D / `exit`ではDoneとして親agentが継続し、checkpointが残らないことを確認する。
10. Human Shell所有processの異常終了を模したRunning checkpointでstatusがorphanedを示すこと、`ai human-task cancel --yes`で削除後に新しいHuman Taskを開始できることを確認する。

## 期待結果

- Running checkpoint保存後だけHuman Shellが起動し、suspend後は同roundの後続toolや追加LLM callを実行しない。
- status/cancelはsocket不要で同じroot lockを使い、破損・未知version・権限不正・checkpoint欠落を「taskなし」へ丸めず、lock取得後のorphaned Runningだけを確認付きcancelで復旧できる。
- 旧`shell_exec` handoffには`human-task suspend`を公開せず、既存のreturn動作を維持する。

## 未実装

`ai human-task resume`、continuation、crash recovery、lease/heartbeat、schema migrationは未実装である。cancelはlocal checkpoint削除だけでagent continuationを行わない。
