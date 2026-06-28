# aibe graceful restart 手動検証

設計: [spec/0046_aibe-graceful-restart-spec.md](../spec/0046_aibe-graceful-restart-spec.md)

## 前提

- mock provider で可（実 API キー不要）
- temp HOME に隔離すること

```bash
tmp="$(mktemp -d)"
export HOME="$tmp/home"
mkdir -p "$HOME"
export AIBE_CONFIG="$tmp/aibe.toml"
export AIBE_SOCKET_PATH="$tmp/run.sock"

cat >"$AIBE_CONFIG" <<'EOF'
[llm]
provider = "mock"
EOF

cargo build -p aibe
AIBE=target/debug/aibe
```

## 手順

1. **起動** — `"$AIBE" -f` を別ターミナルで実行し、socket ができることを確認する
2. **status** — `"$AIBE" status --format json` が `"state":"running"` と `"socket_ping":true` を返す
3. **設定変更** — `AIBE_CONFIG` に `default_profile` 等を追記・変更する
4. **restart** — `"$AIBE" restart` が成功し、再度 status で running になる
5. **stop** — `"$AIBE" stop` 後、socket と `$HOME/.local/share/aibe/run.pid` が消える

## 期待結果

- restart 前に無効 TOML を置いた場合、`restart` はエラーになり旧 daemon は継続する
- 進行中の `agent_turn` がある状態で `stop` しても 5s 以内に daemon が終了する
