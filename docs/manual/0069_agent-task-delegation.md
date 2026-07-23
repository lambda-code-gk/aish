# Agent Task Delegation 手動確認（0069）

自動受け入れ試験は deterministic fixture を使用する。実 Agent 製品の確認は任意であり、API keyやnetworkを自動試験の前提にしない。

## 前提と設定

1. Worker CLIの絶対path、固定argv、timeout、permission profileを決める。credential値は設定に書かず、必要な環境変数名だけを`env_allowlist`へ置く。
2. `~/.config/aibe/config.toml`へ次を追加し、aibeを再起動する。

```toml
[agent_task]
enabled = true

[[agent_task.workers]]
id = "local-agent"
executable = "/absolute/path/to/worker"
args = ["--configured-non-interactive-mode"]
timeout_secs = 300
permission_profile = "workspace-write"
env_allowlist = ["PATH"]
```

3. `ai ask --tools agent_task '委譲する小さな作業'` をTTYから実行する。Worker固有のログインやAPI key設定はリポジトリ外で行う。

## チェックリスト

- [ ] approvalにworker、canonical cwd、timeout、profile、objectiveが表示される
- [ ] 「Worker内部操作はAISHが個別承認しない」「cwdはOS sandboxではない」という警告が表示される
- [ ] `n`、空入力、non-TTYではWorkerが起動しない
- [ ] `--yes-exec`やshell allowlist/session cacheだけではAgent Taskが承認されない
- [ ] `y`で設定済みWorkerが指定cwdから一回だけ同期実行され、親Agentへ戻る
- [ ] Resultのstatus（completed/blocked/cancelled/failed/…）/blockers/stdout/stderr/exit/timeout/truncationとchanged path Evidenceがboundedに表示される
- [ ] Resultと全Evidenceが`verified=false`で、Workerの完了自己申告だけで親が完了断定しない
- [ ] timeout時にWorkerと子孫process（pipeを掴んだままの子孫含む）が残らず、成功扱いにならない
- [ ] approval/audit/error/tool resultにcredential値、raw env、file content、未redactな秘密が表示されない
- [ ] 相対`executable`の設定が起動時に拒否される
- [ ] 委譲先からの`agent_task`再呼び出しが実行前に拒否される
- [ ] `human_task`のHuman Shell、suspend/resume/continuationが従来どおり動く

## 実施状況

未実施。実製品・credential・networkを必要とするため、最終確認は利用者が行う。fixtureを使う自動ACはこの手動確認とは独立している。
