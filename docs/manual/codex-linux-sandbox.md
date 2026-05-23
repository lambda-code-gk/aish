# Codex（Linux）サンドボックス / bwrap 手動確認

## 症状

Codex MCP または CLI でシェル・ファイル読取時:

```text
bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted
```

Ubuntu 24.04 で `kernel.apparmor_restrict_unprivileged_userns = 1` のときに多い。

## 確認

```bash
./scripts/codex-fix-linux-sandbox.sh
```

両方 OK なら問題なし。

## 本リポジトリの設定

| 経路 | 設定 |
|------|------|
| CLI | `.codex/config.toml` — `use_legacy_landlock = true` |
| Cursor MCP | `scripts/codex-mcp-wrapper.sh`（`danger-full-access`、Landlock オフ） |

`mcp.json` 変更後は **Cursor 再起動**または MCP `codex` 再接続。

## ユーザー層（任意）

`~/.codex/config.toml` にも同じ `[features]` を入れると、他プロジェクトの Codex でも有効。

## AppArmor プロファイル（Landlock でも失敗するとき）

`docs/codex-delegation.md` の「Linux: bwrap」節を参照。
