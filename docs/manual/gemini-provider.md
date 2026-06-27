# Gemini プロバイダ 手動検証

## 前提

- **0010 実装済み**の aibe / ai バイナリ（未実装時は本手順はスキップし、完了報告に「未実施」と明記）
- `cargo build -p aibe -p ai`
- Gemini API key を持っていること
- API key をリポジトリやログに書かないこと
- `~/.config/aibe/config.toml` と `~/.config/ai/config.toml` を編集できること
- tool 検証時は **絶対パス cwd** が `agent_turn` に送られること（`ai ask` をツール対象ディレクトリで実行する。詳細は [ai-ask-tools.md](ai-ask-tools.md)）

## 設定例

`provider = "gemini"` に切り替えるとき、**以前 OpenAI 互換用に設定した `AIBE_BASE_URL` がシェル env に残っていないか確認する**。残っていると Gemini 以外の URL に POST して 404 になる。

- 対処: TOML に `base_url = "https://generativelanguage.googleapis.com/v1beta"` を明示する、または `unset AIBE_BASE_URL` して aibe 既定に任せる

`~/.config/aibe/config.toml`:

```toml
[llm]
provider = "gemini"
api_key = "YOUR_API_KEY"
model = "gemini-3.5-flash"
# base_url = "https://generativelanguage.googleapis.com/v1beta"

[tools.read_file]
allowed_roots = ["."]
```

`~/.config/ai/config.toml`:

```toml
[ask]
tools = "@read-only"
```

## 手順

1. aibe をフォアグラウンドで起動する。
   ```bash
   cargo run -p aibe -- -f
   ```
2. 別ターミナルで、まずは単発応答を確認する。
   ```bash
   cargo run -p ai -- ask "say hello in one word"
   ```
3. 次に tool calling を確認する。
   ```bash
   cargo run -p ai -- ask --tools @read-only --verbose-tools \
     "Read a file named README.md from the current directory and reply with only its first line."
   ```
4. `aish shell` 内または有効な replay session から同じ操作を行い、既定の `shell_log_mode=hybrid` で `aish.replay_show` が広告されても HTTP 400 にならないことを確認する。
5. 任意で `max_rounds = 1` にして、max-round 終端が落ちないことを確認する。

## 期待結果

- `agent_turn_result` の assistant 本文が表示される
- `--verbose-tools` 使用時に tool 呼び出しの詳細が見える
- `functionCall` / `functionResponse` の対応で turn が継続する
- tool declaration は `parametersJsonSchema` を使い、`aish.replay_show` の `additionalProperties: false` を含んでも Gemini に拒否されない
- `aish.replay_show` 実行後も `functionCall.name` / `functionResponse.name` がともに `aish_replay_show` となり、次ラウンドが HTTP 400 にならない
- API key や Bearer がターミナルやログに平文で出ない
- `max_rounds = 1` の場合も `status: max_tool_rounds` で終端する

## よくある失敗

- `api_key` 未設定 → aibe 起動時に設定エラー
- `model` 未指定時は `gemini-3.5-flash` を使う想定なので、古いモデル名を残していると 404 や 400 になる
- `base_url` の末尾 `/` や `/v1beta` の抜けがあると HTTP 404 になりやすい
- **`AIBE_BASE_URL` が OpenAI 互換用のまま**（例: `http://127.0.0.1:1234/v1`）→ Gemini 利用時は上書きまたは unset
- `allowed_roots` 外のファイルを読ませると tool エラーになる
- tool 利用時に cwd 未送信 → `invalid_request`（0003 / 0005）

**本手順は AI 未実施時は完了報告に「未実施」と明記する。**
