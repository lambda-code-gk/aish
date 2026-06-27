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

`codex doctor`、bwrap、Landlock、CLI / MCP 経路をまとめて確認する。

## 本リポジトリの設定

| 経路 | 設定 |
|------|------|
| CLI | `scripts/codex-cli.sh`（bwrap優先、Landlockフォールバック） |
| Cursor MCP | `scripts/codex-mcp-wrapper.sh`（同上、`workspace-write`、network off） |

`mcp.json` 変更後は **Cursor 再起動**または MCP `codex` 再接続。

## AppArmor プロファイル

Landlock はCodexで非推奨のため、Ubuntuではbwrap用profileを有効化する。

```bash
sudo apt-get install -y apparmor-profiles apparmor-utils bubblewrap
sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict
sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict
./scripts/codex-fix-linux-sandbox.sh
```

`kernel.apparmor_restrict_unprivileged_userns=0` によるシステム全体の制限解除は行わない。
