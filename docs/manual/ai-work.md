# `ai work` Phase 4 手動検証

## 対象

0052 Phase 4 の `push / pop`、stack 表示、disabled経路、通常 turn 注入を確認する。

## 前提

```bash
cargo build -p aibe -p ai
```

- API key は不要。
- `[memory] enabled = true` の aibe を起動する。
- 既存 Work state を避ける場合は、一時的な `AIBE_CONTEXT_ID` を使う。

## 手順

1. aibe を起動する。
2. 空の context を選ぶ。

```bash
export AIBE_CONTEXT_ID=work_phase1_manual
ai work
ai work status
ai work list
ai work --help
```

3. `ai work --help` に `start/status/list/switch/push/pop/defer/idea/note/decide/focus/finish` が表示されることを確認する。
4. `start` と `switch` と `finish` の表示を確認する。

```bash
ai work start "phase 2 manual"
ai work start "second root work"
ai work switch 1
ai work finish
```

5. 後続Phaseの操作が state を変更せず拒否されることを確認する。stack guard は自動テストで確認済みとする。

```bash
ai work push "child work"
ai work pop
ai work finish
```

6. `AI_MEMORY_ENABLED=0` で拒否されることを確認する。

```bash
AI_MEMORY_ENABLED=0 ai work --no-start
```

## 期待結果

- `ai work` は `No active work.` と開始導線を表示する。
- `start` で active が作られ、二度目の `start` で旧 active が `Paused` になり、旧 active 停止が表示される。
- `switch` は `Paused` / `Deferred` の work に対して active を切り替え、`Done` / missing は拒否される。
- `finish` は stack が空のときだけ active を `Done` にして unset する。
- `push` は active を stack に積んで child work を開始する。
- `pop` は child work を `Done` にして parent へ戻り、child entries を親へ自動 merge しない。
- `focus / idea / note / decide / finish` は active がないと拒否される。
- memory disabled は既存 contextual memory と同じ error で fail-closed になる。
- 通常の `ai ...` 実行時は active work の goal / focus / recent decisions が synthetic user context として注入され、`system_instruction` に重複しない。

## 注意

Work の goal / note / idea 等には API key、token、password を保存しない。Work 内容は自動 redaction されない。
