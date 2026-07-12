# 0060 Collab Mode Human Task Briefing（手動検証）

実端末で Collaborative Mode の Human Shell を開き、開始時 briefing と終了後の即時 return を確認する。

## 手順

1. mock aibe または既存 0055 E2E 相当の構成で `ai ask --collaborative` を起動する。
2. Human Shell 開始直後の stderr に次が含まれることを確認する。
   - `AISH Collaborative Mode`
   - `Human Task`
   - `Objective:`
   - `Suggested first action:`
   - `Done when:`
   - `Edit, run, replace, or ignore`
   - `Ctrl+D` / `exit`
3. 候補コマンドが自動実行されていないことを確認する。
4. `exit` または Ctrl+D で Human Shell を終了する。
5. 終了後に `作業結果を選択してください` やサマリ入力が出ず、ただちに親へ制御が戻ることを確認する。
6. 通常の `ai ask`（非 collaborative）や `shell_exec` 承認に Human Task briefing が出ないことを確認する。

API key は不要。自動検証は次で行える。

```bash
cargo test -p aish --test 0060_collab_mode_human_task_briefing -j 1
cargo test -p ai --test 0055_collaborative_handoff_vertical_e2e -j 1
./scripts/verify.sh
```
