# 0071 Task Completion hardening 手動確認

この手順は自動testの補助であり、実API key・network・workspace外path・秘密fileを使わない。local mock / fixtureだけを対象とし、実行結果にcontrol envelopeやraw command outputが含まれないことを目視確認する。

## 事前確認

```bash
cargo test -p aibe --test 0071_task_completion_hardening_red -j 1 -- --test-threads=1
./scripts/smoke-mock.sh
```

## 確認項目

1. Contract固定前と固定後にTask Completion turnをcancelし、top-level responseが`cancelled`のまま、reportがある場合は`verification_terminal=cancelled`、JSON全体が128 KiB以下であることを確認する。通常turnとHuman Task cancelでは`completion_report`が省略されることも確認する。
2. fixed verification executableを存在しないfixtureへ向け、top-level `error`と`verification_terminal=failed`を確認する。schema不正は`invalid_request`、開始済みcommandのnonzeroとcleanup成功timeoutはFailedではなく未検証Evidence / Gapになることを確認する。
3. 初回Workerが未達artifactを返すfixtureで、各Gapの`observed`が実Evidence summary、`required_work`がcriterion固有、`verify_with`が元plan itemであることを確認する。`required_evidence`文言やglobal next objectiveがobserved / required_workへ複製されていないことを確認する。
4. 64 KiB+1 byteまたは1025 deltaのstreamをfixtureから返し、overflow前prefixを含めcontrol JSONがhuman / JSON / TSV / env出力へ表示されないこと、progress終了とfinal通知が失われないことを確認する。
5. 同一byte長のfileを同一workspace内で書き換え、changed pathに出るか`observation_incomplete=true`になることを確認する。symlink targetやfile内容をObserverが読んだ形跡がなく、秘密値を含むpathがResultへ出ないことも確認する。

## 記録

- 実施日時・OS/filesystem:
- cancel（Contract前/後）:
- spawn failure / nonzero / timeout:
- Gap表示:
- buffer overflow表示:
- same-size rewrite / observation_incomplete:
- 未実施項目と理由:
