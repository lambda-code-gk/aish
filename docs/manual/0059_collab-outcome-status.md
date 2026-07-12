# Collaborative Mode Outcome 手動確認

実端末から mock aibe を使う既存 0055 E2E 相当の構成で `ai ask --collaborative` を起動する。Human Shell を `exit` で終了した後、親端末に `作業結果を選択してください` が表示されることを確認する。

1. `d` / `done` を選ぶと、追加入力なしで親へ戻り `collab_outcome.status=done` が返ることを確認する。
2. `b` / `blocked` でも追加入力なしで `status=blocked` が返ることを確認する。
3. `c` / `cancelled` でも追加入力なしで `status=cancelled` が返ることを確認する。
4. Human Shell を起動できない `AISH_BIN` を指定し、outcome prompt が表示されず `human_handoff_failed` になることを確認する。
5. `human_shell_exit_code` と選択 status が独立していることを確認する。
6. summary / 理由の入力欄が出ないことを確認する。

API key は不要。自動検証は `cargo test -p ai --test 0055_collaborative_handoff_vertical_e2e -j 1` と `cargo test -p ai --test 0059_collab_outcome_status -j 1` で行える。
