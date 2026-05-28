# P2: 安全なツール体系

[← 索引](README.md)

正式指示書: [0018_safe-tools-policy-spec.md](../../0018_safe-tools-policy-spec.md)

[concerns.md](concerns.md) §4 の方向性。`shell_exec` は万能すぎるため、専用ツールを増やして「安全にできること」を広げ、`shell_exec` は明示指定時のみ、という提案。

## 追加順（読み取り中心）

```text
read_file          # 既存
list_dir
grep / rg
git_diff
git_status
```

## その後（書き込み系）

**dry-run + 承認** を必須とする:

```text
write_file
replace_file
apply_patch
```

スプリントへの落とし込み: [sprints.md](sprints.md) Sprint 3
