# `ai work` 手動検証

## 対象

`ai work` の状態遷移・表示・turn 注入を実 aibe 上で確認する。

## 前提

```bash
cargo build -p aibe -p ai
```

- API key は不要。
- `[memory] enabled = true` の aibe を起動する。
- 既存 Work state を避ける場合は、一時的な `AIBE_CONTEXT_ID` を使う。

## 手順

### 1. 空状態と一覧

```bash
export AIBE_CONTEXT_ID=work_manual_verify
ai work
ai work status
ai work list
ai work --help
```

### 2. start / note / decide / focus

```bash
ai work start "phase 1 manual"
ai work note "note が status に出ることを確認"
ai work decide "status の表示件数には上限を設ける"
ai work idea "defer と idea の違いを manual に書く"
ai work focus "表示改善の確認"
ai work status
```

### 3. defer（active が変わらないこと）

```bash
ai work defer "ブラウザ拡張から同一work contextを参照する"
ai work status
```

`defer` 出力に `Active work remains` が含まれ、active が start した work のままであること。

### 4. push / pop

```bash
ai work push "一時的な派生作業"
ai work decide "push/pop は一時的な寄り道に使う"
ai work pop
ai work status
```

### 5. switch / finish

```bash
ai work start "second root work"
ai work switch 1
ai work finish
```

### 6. 拒否ケース

```bash
# stack が空でない finish の拒否（push 後に finish を試す）
ai work push "child"
ai work finish   # 拒否されること

# active なしでの操作拒否
ai work finish
ai work note "should fail"
```

### 7. disabled 経路

```bash
AI_MEMORY_ENABLED=0 ai work --no-start
```

### 8. turn 注入（任意・API key 必要）

通常の `ai ...` 実行時に active work の goal / focus / recent decisions が synthetic user context として注入され、`system_instruction` に重複しないことを確認する。

## 期待結果

- `ai work status` に Notes セクションと note 本文が表示される
- decisions / ideas / notes が 5 件超のとき `... and N more` が出る
- `defer` 後も active work が変わらない
- `push` で parent が stack に積まれ、`pop` で parent に戻る
- `finish` は stack が空のときだけ成功する
- `focus / idea / note / decide / pop / finish` は active がないと拒否される
- memory disabled は fail-closed
- 通常 turn に active work block が注入される（任意確認）

## よくある失敗

- 古い `work-state.json` が残っている → 別の `AIBE_CONTEXT_ID` を使う
- aibe が起動していない → `aibe status` で確認
