# ai doctor 手動検証

実 API key は使わず、まず `./scripts/smoke-mock.sh` で mock aibe に対する human / JSON 経路を確認する。

1. 一時 HOME と socket を使う mock smoke が成功することを確認する。
2. `ai doctor --quiet` の先頭が `OK` または `WARN doctor` で、6 check ID が設計順に表示されることを確認する。
3. `ai doctor --quiet --format json` が `command/status/checks` だけからなる JSON report を返し、exit 0 になることを確認する。
4. 存在しない socket を `--socket` で指定し、6 checks が最後まで表示され、exit 1 になることを確認する。
5. `AI_FILTER` にダミー秘密 marker、session log に別 marker を置き、stdout/stderr の human / JSON / TSV / env のいずれにも marker 本文が出ないことを確認する。

`doctor` は aibe を自動起動せず、provider/LLM に接続せず、設定やログを書き換えない。数値 protocol version、`--full`、`--fix` は対象外。
