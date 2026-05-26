# LLM プロファイル（手動検証）

`docs/done/0011_llm-profiles-spec.md` の受け入れ確認用チェックリスト。

## 前提

- `aibe` / `ai` をビルド済み（`cargo build -p aibe -p ai`）
- テスト用 `~/.config/aibe/config.toml` を新形式で用意（`docs/aibe.config.example.toml` 参照）

## 1. 起動と既定プロファイル

1. 既存 aibe を停止（socket ファイルがあれば削除）
2. `aibe --foreground` を起動（または `ai ask` で自動起動）
3. `ai ask "hello"` — 応答が返る（`default_profile` 使用）

## 2. `--profile` 切り替え

1. 設定に `profiles.fast` / `profiles.local` 等を定義
2. `ai ask "hello" --profile local` — ローカルモデル経由の応答（または接続エラーが provider 側であること）
3. 存在しない名前: `ai ask "hi" --profile no-such` — `invalid_request` 相当のエラー表示

## 3. ai 設定の既定

`~/.config/ai/config.toml`:

```toml
[ask]
default_profile = "fast"
```

`ai ask "hello"` で wire に `llm_profile: "fast"` が載ること（デバッグ時は aibe ログまたは tcpdump 不要 — 統合テスト `aibe_client_sends_llm_profile` で代替可）。

## 4. 後方互換（legacy フラット `[llm]`）

1. 新形式をコメントアウトし、example 末尾のフラット `[llm]` のみ有効化
2. aibe 再起動後 `ai ask "hello"` — 従来どおり動作

## 5. 自動テスト

```bash
cargo test -p aibe llm_profiles
cargo test -p aibe toml_config::
cargo test -p ai llm_profile
cargo test -p aibe --test llm_profiles_socket
```
