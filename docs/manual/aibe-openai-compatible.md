# aibe OpenAI 互換プロバイダ 手動検証

## 前提

- `cargo build -p aibe -p ai`
- ローカル LM Studio / vLLM 等、または OpenAI API キー（**リポジトリに置かない**）
- 設定: `docs/aibe.config.example.toml` を `~/.config/aibe/config.toml` にコピーして編集

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
- ベース URL の末尾 `/v1` の重複・欠落 → HTTP 404

**本手順は AI 未実施時は完了報告に「未実施」と明記する。**
