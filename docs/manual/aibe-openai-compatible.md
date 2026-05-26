# aibe `openai_compatible` プロバイダ 手動検証

## 前提

- `cargo build -p aibe -p ai`
- OpenAI 公式 API キー、またはローカル LM Studio / vLLM 等（**リポジトリに置かない**）
- 設定: `docs/aibe.config.example.toml` を `~/.config/aibe/config.toml` にコピーして編集
- 設定の `provider` は **`openai_compatible`**（`openai` という provider 名はない）

### OpenAI 公式 API

```toml
[llm.openai]
provider = "openai_compatible"
api_key = "YOUR_API_KEY"
# base_url を省略すると https://api.openai.com/v1
```

### ローカル OpenAI 互換

```toml
[llm.lmstudio]
provider = "openai_compatible"
api_key = "YOUR_API_KEY"
base_url = "http://127.0.0.1:1234/v1"
```

## 手順

1. フォアグラウンドで aibe を起動:
   ```bash
   cargo run -p aibe -- -f
   ```
2. 別ターミナルで:
   ```bash
   cargo run -p ai -- ask "say hello in one word"
   ```
3. 応答が表示され、aibe ログに HTTP エラーが出ていないこと。

## 期待結果

- `agent_turn_result` の assistant 本文が表示される
- キー・Bearer がターミナルや aish ログに出ないこと

## よくある失敗

- `api_key` 未設定 → aibe 起動時に設定エラー
- `provider = "openai"` と書く → `unknown llm provider`（未対応）
- ベース URL の末尾 `/v1` の重複・欠落 → HTTP 404

**本手順は AI 未実施時は完了報告に「未実施」と明記する。**
