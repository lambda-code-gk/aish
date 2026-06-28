# `ai work` Phase 0 手動検証

## 対象

0052 Phase 0 の CLI 骨格、空状態表示、disabled 経路を確認する。`start` 等の mutation は Phase 1 以降の対象である。

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
export AIBE_CONTEXT_ID=work_phase0_manual
ai work
ai work status
ai work list
ai work --help
```

3. `ai work --help` に `start/status/list/switch/push/pop/defer/idea/note/decide/focus/finish` が表示されることを確認する。
4. Phase 0 では mutation が fail-closed で拒否されることを確認する。

```bash
ai work start "phase 0 manual"
```

5. `AI_MEMORY_ENABLED=0` で拒否されることを確認する。

```bash
AI_MEMORY_ENABLED=0 ai work --no-start
```

## 期待結果

- `ai work` は `No active work.` と開始導線を表示する。
- `ai work status` は active work がないことと `start` 導線を表示する。
- `ai work list` は Active / Paused / Deferred / Done を空分類で表示する。
- Phase 0 の mutation は non-zero で終了し、state file を作成・変更しない。
- memory disabled は既存 contextual memory と同じ error で fail-closed になる。

## 注意

Work の goal / note / idea 等には API key、token、password を保存しない。Work 内容は自動 redaction されない。
