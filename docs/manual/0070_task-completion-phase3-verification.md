# 0070 Task Completion Phase 3 手動確認

## 目的

ローカルの設定済みWorkerを使い、親AISHが完了自己申告を信用せず、固定Verification Planで再観測し、未達を同じWorkerへ一度だけ戻すことを確認する。実API、API key、networkは不要で、自動ACの代替にはしない。

## 前提

1. `agent_task`を有効化し、workspace-write相当のローカルfixture Workerを絶対pathで1件設定する。
2. Workerは初回に不完全なartifactと`status=done`を返し、instructionsに`Gap`がある2回目だけartifactを修正する。
3. `shell_exec`の検証commandをallowlistへ追加し、`read_file`と対象workspace rootを許可する。

秘密値をWorker入力、artifact名、command outputへ入れない。Worker cwdはOS sandboxではない。

## 手順

1. `ai ask --task-completion`でAgent Task、固定command、成果物readを含む依頼を開始する。
2. 初回とfollow-upのAgent Task approvalをそれぞれ確認し、同じWorker/cwd/timeoutであることを確認して承認する。
3. 初回Workerの`done`後も直ちに親TaskがDoneにならず、親のcommand→read観測後にGapが生成されることを確認する。
4. follow-up approval後、Gapが既存objective/instructions/completion criteriaへ渡され、wireに`gap` fieldがないことを確認する。
5. 同じVerification Planが再実行され、最終reportにcriterion状態、Verification provenance、Worker ID、`follow_up_count=1`、`verification_terminal=done`が出ることを確認する。
6. fixtureを修正しないmodeに変え、follow-up後は追加Worker起動せず`stagnated`となることを確認する。ユーザー操作が必要な評価では`needs_user`となることも確認する。
7. 通常のplan外shellを挟み、Verificationへ昇格せず過去観測がstaleになることを確認する。
8. report、stderr、保存会話にcredential、raw Worker output、workspace外pathが複製されていないことを確認する。

## 期待結果

- Worker Resultと全Worker Evidenceは`verified=false`。
- Agent Task起動は最大2回、approvalも2回、外側queryは最大2。別Worker、再帰委譲、3回目queryはない。
- `Done / NeedsUser / Blocked / Stagnated / BudgetExhausted`はcompletion reportで型付き表示され、`Failed / Cancelled`は既存top-level responseで表示される。
- Human Taskの通常Done、Suspended、checkpoint/resume/continuation外部挙動は変わらない。

## 実施記録

- 実施日:
- 実施者:
- Worker設定:
- 結果:
- 未確認事項:
