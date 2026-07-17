# 0066 Human Task recovery hardening 手動検証

## 前提

- disposableな`history_dir`を設定した`ai`を使う
- 実checkpointの手動破損試験は本番historyで行わない

## orphaned Running

1. Human Task起動中の`ai`をテスト環境で強制終了し、Running checkpointと解放済みroot lockを残す。
2. `ai human-task status`が`State: orphaned running`と`ai human-task recover`を案内することを確認する。
3. `ai human-task recover`の確認を一度拒否し、statusが変わらないことを確認する。
4. 再実行して承認し、`State: suspended`、reason `unexpected_process_termination`になることを確認する。
5. `ai human-task resume`でbashまたはzshのHuman Shellが保存cwdから再開することを確認する。

## stale Continuing

1. disposable環境でcontinuation中の`ai`を終了し、Continuing checkpointを残す。
2. statusのrecover案内後に`ai human-task recover --yes`を実行する。
3. statusが`result pending`とresume retryを案内し、`ai human-task resume`で既存continuation再試行へ進むことを確認する。

## invalid residue

1. disposable history内の単一checkpointを破損JSON、0600以外のmode、またはcheckpoint欠落task directoryにする。
2. statusがnon-zeroで安定診断と`ai human-task recover --force-invalid`を表示し、残骸を保持することを確認する。
3. force cleanupの確認を拒否して残骸が保持されることを確認する。
4. `ai human-task recover --force-invalid --yes`で単一残骸だけが削除され、statusがno-taskになることを確認する。
5. root lock保持中のrecoverがbusyで無変更になること、およびsymlink / nested directory / 複数task残骸がfail-closedになることを確認する。

## 未実施時の扱い

自動テストはapplication/store境界を検証する。実process強制終了と実bash/zsh PTYの確認を未実施の場合は、完了報告の残リスクに明記する。
