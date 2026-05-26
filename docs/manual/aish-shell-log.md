# aish 対話シェル + JSONL ログ 手動検証

## 前提

- `cargo build -p aish`
- ターミナルが TTY であること（パイプのみの stdin では対話できない）

## 手順

1. ログパスを決める（例: `/tmp/aish-shell-test.jsonl`）。
2. 対話シェルを起動:
   ```bash
   cargo run -p aish -- shell --log /tmp/aish-shell-test.jsonl
   ```
3. シェルで `echo hello` と `exit` を実行する。
4. ログを確認:
   ```bash
   cat /tmp/aish-shell-test.jsonl
   ```

## 期待結果

- ターミナルに `hello` が表示される
- ログに `command_start`（`interactive_shell`）、`stdout`（`hello`）、`exit` が含まれる
- ログファイルのパーミッションが `600`（`ls -l`）であること
- `command_start` の `args` に API キー形式（`sk-...`）や `Bearer ...` を含む文字列を意図的に入れた場合、JSONL に **平文が残らない**こと（`sk-[REDACTED]` 等に置換されていること）。確認例: 設定のシェル起動引数に相当する値がログに載るため、`--log` 付きで起動し `cat` で `command_start` 行を見る

## よくある失敗

- `--log` 未指定でもデフォルトパス（`~/.local/share/aish/sessions/session-<pid>.jsonl`）に書かれる
- 非 TTY では入出力が期待どおりでないことがある

**本手順は AI 未実施時は完了報告に「未実施」と明記する。**
