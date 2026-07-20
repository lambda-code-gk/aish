# 0068 Task Completion Phase 1 手動確認

自動 E2E が Unix socket syscall を許可しない sandbox では direct production handler fallback を使うため、通常ホストで次を確認する。実 API key や本番設定はリポジトリへ保存しない。

1. 一時ディレクトリと mock provider 設定で aibe を起動し、変更後に `read_file` 再観測が必要な `ai ask` を1回実行する。
2. provider trace の2回目 query が元要求の再送ではなく、固定 Contract、unsatisfied criterion、required Evidence、単一 `next_objective` を含むことを確認する。
3. `completion_report.outcome=done`、`queries_used=2`、effect Evidence が unverified、effect 後 observation が verified であることを確認する。
4. plan-only、approval 拒否、同一 failure、budget 到達でそれぞれ Done にならず、NeedsUser / Blocked / BudgetExhausted の理由付き report になることを確認する。
5. stdout / stderr / trace に assistant control envelope、raw tool output、command/path、秘密値が出ないことを確認する。

Codex sandbox 内では Unix socket bind/connect が `EPERM` となるため未実施。通常ホストまたは CI smoke で実施する。
