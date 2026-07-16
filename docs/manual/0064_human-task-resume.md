# 0064 Human Task resume 手動検証

## 前提

- `cargo build --workspace`
- 検証専用`AI_CONFIG`で一時`history_dir`を指定する
- 0063のsuspend経路でSuspended checkpointを用意できること

## 手順

1. `ai collab '...'`から明示`human_task`を開始し、Human Shell内で`human-task suspend '途中で中断'`する。
2. turn終了後に`ai human-task status`を実行し、Resume / Cancel 案内の両方があることを確認する。
3. `ai human-task resume`を実行し、保存cwdで新しいHuman Shellが起動し、briefingが表示されることを確認する。
4. Human Shell内で作業後に`human-task suspend '再開後の中断'`を実行し、exit 0で終了することを確認する。
5. `ai human-task status`で最新reasonとcwdが更新され、checkpointが残ることを確認する。
6. 再度`ai human-task resume`し、Ctrl+Dまたは`exit`で終了する。completion deferred の案内が出てnon-zeroになり、`status`ではSuspendedのまま（再suspend前の内容）であることを確認する。
7. cwdを削除したfixture相当でresumeするとshellが起動せずSuspendedが維持されることを確認する（任意）。
8. `ai human-task cancel --yes`で削除できることを確認する。

## 期待結果

- resumeはaibe未起動でも動作する。
- 再suspendでsegmentが積み上がる。
- Done後のagent continuationはまだ案内・実行されない。

## 未実装

- Done → `ResultPending` / Collaborative Mode continuation（0063-D）
- crash recovery / lease（0063-E）
