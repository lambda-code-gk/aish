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
10. bare `ai` を TTY で実行し、内蔵ミニエディタまたは `AI_EDITOR` で入力した内容が `ai ask` と同様に送信されること。内蔵ミニエディタでは `Enter` で改行、`↑`/`↓` で上下の行へ移動して編集できること。`Ctrl+D`（本文あり）または `Alt+Enter` で送信、`Ctrl+C` または空入力では AI を呼ばずキャンセルメッセージが出ること。
11. `echo hello | ai` が pipe 入力のまま動き、prompt UI を出さないこと。

## 提案コマンド再呼び出し（0053）

### 前提

- `cargo build -p ai -p aish` と TTY 前提
- `aish shell` 経由、または通常 bash / zsh で `eval "$(ai complete bash)"`（zsh は `zsh`）を読み込んだ状態

### 手順

1. `aish shell`（または hook 済み bash / zsh）で `ai commit` 等を実行し、assistant が ```bash ブロックで shell コマンドを提案させる。
2. stderr に `ai: N suggested command(s) ready. Alt+. / Alt+, cycle proposals.` が出ることを確認する。
3. プロンプトで `Alt+.` を押し、提案コマンドが入力欄に挿入され、Enter するまで実行されないことを確認する。
4. 複数 block がある場合、`Alt+.` 連打で候補が順に巡回し、末尾の次は先頭に戻る（ラップアラウンド）ことを確認する。
5. `Alt+,` で逆方向に巡回でき、先頭の前は末尾に戻ることを確認する。
6. `ai ask -q ...` では hint が消え、cache は維持されること（その後 `Alt+.` / `Alt+,` で挿入可能）を確認する。
7. `ai ask --format json ...` では hint / cache が無効化されることを確認する。
8. bash / zsh のそれぞれで候補を `Alt+.` / `Alt+,` から挿入し、`←` / `→` で候補内を移動して文字を追加でき、`↑` / `↓` で直前履歴と候補 buffer を往復できることを確認する。
9. 候補なし、cache ファイル不在、`ai recall` の失敗をそれぞれ作り、入力済み buffer が変わらず、その直後も左右 cursor と上下 history が動くことを確認する。
10. `Alt+.` / `Alt+,` と矢印キーを間を空けずに連続入力し、完全な shortcut と CSI がそれぞれ解釈され、次の prompt 入力を継続できることを確認する。
11. 0055 Human Shell を handoff 候補ありで起動し、bash / zsh の両方で `Alt+.` / `Alt+,` 挿入後の左右 cursor と上下 history を確認する。handoff 候補なしでは既存の通常 recall binding が上書きされないことも確認する。

### 期待結果

- recall は prompt への挿入のみで、shell history を汚さない
- `aish shell` と `ai complete` が同じ hook 文面を使う
- recall subprocess は widget の stdin を消費せず、空・失敗後も line editor が入力可能な状態へ戻る
- zsh は候補挿入後に現在の ZLE buffer を再表示し、terminal 全体を固定値へ reset しない

## 期待結果

- `chat` は TTY 上で Unicode 対応の行編集（Backspace・カーソル移動）を使う。
- TTY では progress は既定 ON。stderr の単行スピナーで phase を示し、assistant streaming 開始時に行を消す。`--no-progress` / `--progress` で上書きできる。
- `--timeout` は turn を打ち切る。
- `--yes-exec` は `shell_exec_approval=ask` のみを bypass し、`never` は越えない。
- `ai` の exit code は概ね `0` / `2` / `3` / `4` / `5` / `130` に分かれ、`130` は SIGINT、`2` は入力/引数の不正、`3` は内部/中断系、`4` は provider エラー、`5` は tool 系エラーを表す。
