# Smart Preprocessor observation report 手動確認

## 前提

Smart Preprocessor observation log が既定パス、または任意の JSONL パスに存在すること。コマンドはログを read-only で扱う。

## 確認

1. ai smart stats --format tsv を実行し、total_records と invalid_lines を確認する。
2. ai smart stats --format json --limit 1000 を実行し、JSON と distribution / latency を確認する。
3. ai smart recent --limit 30 を実行し、既知フィールドだけが表示されることを確認する。
4. ai smart report --limit 1000 --include-recent 30 を実行し、Markdown を AI 評価へ貼り付けられることを確認する。
5. 任意パスでは --path ~/path/to/observation.jsonl を使い、HOME 展開を確認する。

raw user text、未知フィールド、秘密情報らしい任意追加フィールドが出力されないことを確認する。分類精度そのものの評価には元入力と正解ラベルが別途必要である。
