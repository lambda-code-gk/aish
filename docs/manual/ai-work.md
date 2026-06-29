# `ai work` Phase 1 手動検証

## 対象

0052 Phase 1 の基本mutation、dashboard/status/list、disabled経路を確認する。

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
4. 基本操作と表示を確認する。

```bash
ai work start "phase 1 manual"
ai work focus "current focus"
ai work idea "try another approach"
ai work note "observed behavior"
ai work decide "keep one atomic store mutation"
ai work defer "later task"
ai work status
ai work list
ai work start "second root work"
```

5. 後続Phaseの操作がstateを変更せず拒否されることを確認する。

```bash
ai work switch 1
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
- `start`でactiveが作られ、二度目の`start`で旧activeがPausedになる。
- `focus / idea / note / decide`はactive workへ保存される。
- `defer`はactiveを変えずDeferred workを追加する。
- populated statusはActive/Focus/Stack/Decisions/Ideas/Deferred/Suggested nextを表示する。
- listはActive/Paused/Deferred/Doneへ分類する。
- `switch / push / pop / finish`はnon-zeroで終了し、stateを変更しない。
- memory disabled は既存 contextual memory と同じ error で fail-closed になる。

## 注意

Work の goal / note / idea 等には API key、token、password を保存しない。Work 内容は自動 redaction されない。
