# Codex（Linux）サンドボックス / bwrap 手動確認

## 症状

Codex MCP または CLI でシェル・ファイル読取時:

```text
bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted
```

Ubuntu 24.04 で `kernel.apparmor_restrict_unprivileged_userns = 1` のときに多い。

別症状（ツールだけ死ぬ）:

```text
Unable to spawn .../codex-linux-sandbox because it doesn't exist
```

`~/.codex/tmp/arg0` を MCP 稼働中に消すと起きる。MCP Restart / Cursor 再起動で復旧。

## 確認

```bash
./scripts/codex-fix-linux-sandbox.sh
```

`codex doctor`、bwrap、Landlock、CLI / MCP 経路をまとめて確認する。

### 誤診に注意（Cursor エージェント sandbox）

Cursor 親エージェントのシェル（`cursorsandbox` + ローカル `HTTP_PROXY`）内で診断すると、次が **誤って壊れているように見える**ことがある。

| 見え方 | ホスト上の実態 |
|--------|----------------|
| `~/.codex` が root 所有 | UID remap。実ファイルは通常ユーザー所有 |
| bwrap / uid_map Permission denied | エージェント sandbox 制限 |
| WebSocket / reachability FAIL | ローカル proxy 経由の失敗 |

**正本はホスト端末**（通常の gnome-terminal 等）での再実行。スクリプトはエージェント sandbox を検知すると WARN を出し、bwrap 失敗時は exit 2 にする。

ホストで健全なら、MCP の遅さの主因は環境劣化ではなくタスク運用（冷スタート・reasoning・verify）側を疑う。詳細は [`docs/codex-delegation.md`](../codex-delegation.md)「速さ」。

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
