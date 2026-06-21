# ai UX 手動検証

`ai chat`、`--progress`、assistant streaming、`--timeout`、`--yes-exec`、終了コードの確認手順。

## 前提

```bash
cargo build -p aibe -p ai
export PATH="$PWD/target/debug:$PATH"
```

## 手順

1. mock `aibe` を起動して `ai ask` の基本経路が動くことを確認する。
2. `ai chat` を起動し、複数行入力して会話が継続することを確認する。
3. TTY 上で `ai ask ...` を実行し、stderr にスピナー（`ai: | thinking…` 等）が出ることを確認する。`--no-progress` で消えることも確認する。
4. `ai ask --timeout 1 ...` を実行し、タイムアウトで cancel 経路に入ることを確認する。
5. `ai ask --yes-exec ...` を実行し、`shell_exec` 承認がセッション内で再利用されることを確認する。
6. `ai chat --dry-run --format json` を実行し、`command=chat` で aibe に接続しないことを確認する。
7. `ai chat` で 2 回以上 turn を打ち、各 turn の assistant 応答が逐次表示されることと、`/exit` で終了することを確認する。
8. `ai chat` で日本語を入力し、Backspace・左右矢印で編集しても文字化けや送信エラーにならないこと。
9. `ai history --command chat` で同一 `conversation_id` が記録されていること、`ai rerun <2nd_id>` で会話文脈が復元されることを確認する。
10. bare `ai` を TTY で実行し、内蔵ミニエディタまたは `AI_EDITOR` で入力した内容が `ai ask` と同様に送信されること。内蔵ミニエディタでは `Enter` で改行、`↑`/`↓` で上下の行へ移動して編集できること。`Ctrl+Enter`（対応端末）または `Alt+Enter` で送信、`Ctrl+C` または空入力では AI を呼ばずキャンセルメッセージが出ること。
11. `echo hello | ai` が pipe 入力のまま動き、prompt UI を出さないこと。

## 期待結果

- `chat` は TTY 上で Unicode 対応の行編集（Backspace・カーソル移動）を使う。
- TTY では progress は既定 ON。stderr の単行スピナーで phase を示し、assistant streaming 開始時に行を消す。`--no-progress` / `--progress` で上書きできる。
- `--timeout` は turn を打ち切る。
- `--yes-exec` は `shell_exec_approval=ask` のみを bypass し、`never` は越えない。
- `ai` の exit code は概ね `0` / `2` / `3` / `4` / `5` / `130` に分かれ、`130` は SIGINT、`2` は入力/引数の不正、`3` は内部/中断系、`4` は provider エラー、`5` は tool 系エラーを表す。
