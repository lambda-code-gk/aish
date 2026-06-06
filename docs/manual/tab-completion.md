# CLI Tab 補完 — 手動検証

`docs/0021_tab-completion-spec.md` の bash / zsh / PATH / `cargo run` / `aish shell` 経路を実機で確認する。

## 前提

```bash
cargo build -p aish -p ai -p aibe
export PATH="$PWD/target/debug:$PATH"
```

## A. PATH バイナリ（bash）

```bash
source <(aish complete bash)
source <(ai complete bash)
source <(aibe complete bash)
```

| # | 操作 | 期待 |
|---|------|------|
| A1 | `aish <Tab>` | `exec`, `shell`, `session`, `complete` |
| A2 | `aish exec --format <Tab>` | `tsv`, `json`, `env` |
| A3 | `ai ask --profile <Tab>` | `~/.config/aibe/config.toml` の `[profiles.*]` キー（設定がある場合） |
| A4 | `ai ask --session <Tab>` | `log_dir` 配下の 12 桁 hex ディレクトリ名 |
| A5 | `ai chat --<Tab>` | `--progress`, `--timeout`, `--yes-exec` 等 |
| A6 | `aibe --<Tab>` | `--foreground`, `--help` 等 |

## B. 破壊的変更

```bash
ai ask hello --log /tmp/x --no-start 2>&1
```

| # | 期待 |
|---|------|
| B1 | 終了コード非 0、`options must appear before message` |

```bash
ai ask --log /tmp/nope --no-start "hello" 2>&1
```

| # | 期待 |
|---|------|
| B2 | オプション先行は受理（ログパスが無くても引数順エラーにならない） |

## C. `cargo run` 委譲

bash:

```bash
source scripts/completions/bash/cargo-run-delegate.bash
source <(cargo run -q -p ai -- complete bash)
```

| # | 操作 | 期待 |
|---|------|------|
| C1 | `cargo run -p ai -- ask --pro<Tab>` | `--profile` 等が補完される |

## D. `aish shell` 内

```bash
aish shell
# 子シェル内
ai ask --tools <Tab>
aish session --format <Tab>
```

| # | 期待 |
|---|------|
| D1 | 一時 rcfile 経由で補完が有効（bash / zsh それぞれ確認） |
| D2 | 終了後も `~/.bashrc` / `~/.zshrc` は改変されていない |

## E. `aish exec --` 以降

```bash
aish exec --format tsv -- ls <Tab>
```

| # | 期待 |
|---|------|
| E1 | `aish` の補完が止まり、シェル標準のファイル名補完になる |

## F. 生成スクリプト

```bash
aish complete bash | head
ai complete zsh | head
aibe complete bash | head
```

| # | 期待 |
|---|------|
| F1 | 各コマンドが 0 終了し、shell 向けスクリプト断片を stdout に出す |
