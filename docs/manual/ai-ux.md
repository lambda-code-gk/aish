# ai UX 手動検証

`ai chat`、`--progress`、assistant streaming、`--timeout`、`--yes-exec` の確認手順。

## 前提

```bash
cargo build -p aibe -p ai
export PATH="$PWD/target/debug:$PATH"
```

## 手順

1. mock `aibe` を起動して `ai ask` の基本経路が動くことを確認する。
2. `ai chat` を起動し、複数行入力して会話が継続することを確認する。
3. `ai ask --progress ...` を実行し、stderr に progress 行が出ることを確認する。
4. `ai ask --timeout 1 ...` を実行し、タイムアウトで cancel 経路に入ることを確認する。
5. `ai ask --yes-exec ...` を実行し、`shell_exec` 承認がセッション内で再利用されることを確認する。

## 期待結果

- `chat` は各 turn の応答を順に表示する。
- `--progress` は stderr に phase を出す。
- `--timeout` は turn を打ち切る。
- `--yes-exec` は `shell_exec_approval=ask` のみを bypass し、`never` は越えない。

