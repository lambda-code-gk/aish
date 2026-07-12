# 0061 Collab Mode Human Task Evidence 手動検証

実ターミナル上で、Human Shell の command span が追加入力なしに親へ構造化返却されることを確認する。破損ログ・巨大 range・境界値・秘密値の負の assertion は自動テストを正本とし、手動でログを改変しない。

## 前提

- `aibe` が起動済みで、`ai ask --collaborative` から `shell_exec` 承認 prompt を返せること
- `aish` と `ai` が同じ workspace の最新 build であること
- API key は aibe の設定だけに置き、画面やログへ転記しないこと

## 手順

1. 実ターミナルで `ai ask --collaborative 'Human Shell で成功コマンドと失敗コマンドを実行して結果を観測して'` を実行する。
2. Human Shell が開いたら次を入力する。

   ```sh
   printf 'evidence-ok\n'
   false
   exit
   ```

3. `exit` 後、summary、reason、status、outcome の入力 prompt が出ず、親 agent へ制御が戻ることを確認する。
4. debug/mock protocol で synthetic handoff result を確認できる環境では、`observation.human_task_evidence.commands` に `printf` と `false` が時系列順で入り、exit code がそれぞれ `0` と `1` であることを確認する。
5. 同じ result の `requested_command_completion` が `unknown` であり、Human Task 全体の成功・失敗が Evidence から推定されていないことを確認する。

## 期待結果

- Evidence 収集のための終了後入力はない。
- command はログに記録された sanitized / redacted 表現で返り、秘密の原値は含まれない。
- handoff 開始前・終了後の range 外 command は Evidence に混ざらない。
- Evidence 収集に失敗しても、安定 error code とともに親への制御返却と他の observation が継続する。

## 自動検証の正本

```sh
cargo test -p ai --test 0061_collab_mode_human_task_evidence -j 1
cargo test -p ai --test 0055_collaborative_handoff_vertical_e2e -j 1
```
