# aish command output replay 手動検証

設計: [0049_aish-command-output-replay-spec.md](../spec/0049_aish-command-output-replay-spec.md)

## 前提

- `cargo build -p aish`
- `aish shell` の手動確認は **TTY** が必要
- `aish replay pick` は stdin / stdout / stderr が TTY のときのみ

## 手順（`aish exec`）

```bash
tmp_log="$(mktemp)"
cargo run -p aish -- exec --log "$tmp_log" -- echo hello
cargo run -p aish -- replay list --log "$tmp_log"
cargo run -p aish -- replay show --log "$tmp_log" 1
cargo run -p aish -- replay show --log "$tmp_log" -1
cargo run -p aish -- replay show --log "$tmp_log" 1 | rg hello
```

## 手順（`aish shell` + `AISH_SESSION_DIR`）

1. `cargo run -p aish -- shell` を TTY で起動
2. シェル内で `echo hello` と `exit`
3. 退出後:

```bash
cargo run -p aish -- replay list
cargo run -p aish -- replay show 1
cargo run -p aish -- replay show -1
cargo run -p aish -- replay pick
```

（`AISH_SESSION_DIR` は `aish shell` 子シェル内、または export 済みの環境で実行）

## 期待結果

- `replay list` に `echo hello` 相当の span が出る（`kind=shell`）
- `replay show` は **再実行せず** 記録済みの `hello` を stdout に出す
- `replay show --index N | rg ...` が成立する
- `replay pick` は TTY で動作し、`fzf` が PATH にあれば優先される
- パイプのみの環境で `replay pick` はエラーになり、`list` + `show --index` を案内する

## smoke-mock 相当（exec のみ・非対話）

```bash
tmp_log="$(mktemp)"
cargo run -p aish -- exec --log "$tmp_log" -- echo hello
cargo run -p aish -- replay list --log "$tmp_log" --format json
cargo run -p aish -- replay show --log "$tmp_log" --index 1
```

**本手順の `aish shell` / `pick` 部分は AI 未実施時は完了報告に「未実施」と明記する。**
