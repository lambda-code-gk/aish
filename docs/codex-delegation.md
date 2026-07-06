# Codex 委譲（サブエージェント・別 LLM）

Cursor 親と **Codex MCP サブエージェント**の役割分担。

## 目的

1. **別 LLM・別スレッド** — 調査・仕様・実装・横断確認を Codex に任せ、Cursor のコンテキストを節約する。
2. **自律調査** — 親が載せ忘れたファイルも、**リポジトリ内なら Codex が自分で読める**（パケット必須ではない）。
3. **パス境界** — **cwd（リポジトリ）内の読書き**（`sandbox_mode = workspace-write`）。リポジトリ外は原則不可（将来 `workspace_roots` で明示）。

## モデル

```mermaid
flowchart LR
  subgraph cursor [Cursor 親]
    U[ユーザー要求]
    T[タスク文 + 短いヘッダ]
    I[統合・最終判断・git commit]
    S[Codex 要約 + threadId]
  end
  subgraph codex [Codex MCP]
    P[sandbox: workspace-write]
    X[読取・編集・shell]
  end
  U --> T
  T -->|prompt| codex
  codex -->|要約| S
  S --> I
```

| 層 | 保持するもの |
|----|--------------|
| **Cursor** | ユーザー意図、Codex **要約**、`threadId` |
| **Codex** | スレッド内の調査・編集・会話全文 |

親は Codex 全文を履歴に貼らない。追い込みは `codex-reply` + 同じ `threadId`。

## 権限

リポジトリ直下の [`.codex/config.toml`](../.codex/config.toml):

| 設定元 | 意味 |
|--------|------|
| `scripts/codex-mcp-wrapper.sh` | `sandbox_mode = "workspace-write"` と network off を強制 |
| `[sandbox_workspace_write] network_access = false` | プロジェクト設定でもシェルからのネットワークを無効化 |

CLI / MCP wrapper は `umask 077` を設定し、新しく作成するセッション履歴をユーザー本人だけが読める権限にする。

**将来**: Codex の `[permissions.aish-subagent]`（beta）例は [codex.config.example.toml](./codex.config.example.toml)。

MCP の `codex` 呼び出しでは `sandbox: workspace-write` を渡す。`danger-full-access` は使わない。

## MCP 呼び出し（親エージェント）

### 1. prompt を組み立てる（既定）

```bash
{
  ./scripts/codex-mcp-prompt.sh
  echo
  cat <<'EOF'
  （ここにタスク。例: aibe の agent_turn と hexagonal チェックを横断確認し、
  問題があれば修正して cargo test まで通して。）
  EOF
} 
# → 連結した全文を MCP codex の prompt に渡す
```

**やらないこと**: prompt に「`target/xxx.txt` を読め」だけ書く（ファイルパス参照だけにしない。タスク文を必ず含める）。

### 2. オプション: 親がコンテキストを絞る（パケット）

```bash
CODEX_USE_PACKET=1 CODEX_TASK=review ./scripts/codex-mcp-prompt.sh
```

`codex-context.sh` の diff・抜粋を同梱。レビュー深度は [codex-review.md](./codex-review.md)。

### 3. 追加許可パス（そのターンだけ）

```bash
CODEX_EXTRA_ROOTS="$HOME/.config/aibe,$HOME/.local/share/aish" ./scripts/codex-mcp-prompt.sh
```

恒久に許可するなら `.codex/config.toml` の `workspace_roots` に追記。

### 4. MCP 引数

| 引数 | 値 |
|------|-----|
| `cwd` | リポジトリルート |
| `approval-policy` | `never` |
| `config` | 通常: `{"approval_policy":"never","model_reasoning_effort":"medium"}`、review: effort `low` |
| `sandbox` | `workspace-write` |
| `developer-instructions` | `.cursor/rules/50-codex-subagent.mdc` 参照 |

### 5. 続き

- `codex-reply` + `threadId`
- 再開時もタスク文を短く添える。大きな差分だけ `CODEX_USE_PACKET=1` でよい。

## タスク種別（`CODEX_TASK`）

| 値 | 用途 |
|----|------|
| `subagent` | 既定。調査・実装・修正・検証など自由記述（reasoning effort: medium） |
| `spec` | **設計書**を `docs/spec/` に出力。実装指示は `docs/tasks/`（effort: medium）。**0056 以降**は [`_feature-spec-template.md`](spec/_feature-spec-template.md) の必須節（Core outcome / Fault model / Complexity Gate 等）を含める |
| `review` | 変更の監査（パケット任意、effort: low） |
| `audit` | 境界・セキュリティ横断（effort: medium） |
| `spike` | 設計比較・調査のみ |

`CODEX_TASK` は `codex-mcp-prompt.sh` の Role 行に反映されるだけ。推論強度はMCPの `config` 引数で明示し、実際の作業内容は **prompt 本文**で伝える。プロジェクトローカルの `[profiles.*]` はCodex 0.134以降無視されるため使わない。

## Codex に任せてよいこと

- リポジトリ内の読取・編集・`cargo fmt` / `clippy` / `test` / `./scripts/check-architecture.sh`
- 実装中の `./scripts/verify-targeted.sh` と、完了直前の `./scripts/verify.sh`
- 設計書（`docs/spec/`）・実装指示書（`docs/tasks/`）
- 横断調査・指摘・修正案の実装（サブエージェントとして）

## 親（Cursor）が担うこと

- ユーザー意図の最終判断
- Codex 要約の保持と統合
- **`git commit` / `push` はユーザー明示時のみ**（Codex に任せない運用を推奨）
- feature ブランチ運用時の **commit 整理（soft reset → 意味単位で commit し直し）** は Codex 完了後に親が行う（`.cursor/rules/05-git-workflow.mdc`）
- MCP 障害時のフォールバック
- **完了監査**: Codex の「完了」報告後、設計書の受け入れ条件と [`scripts/spec-acceptance.toml`](../scripts/spec-acceptance.toml) を照合。`pending` が残る spec では `docs/done/` 移動・index の「実装済み」更新をしない（`.cursor/rules/45-spec-completion-gates.mdc`）

## 完了ゲート（実装タスク共通）

検証は二段階で行う。実装中は変更対象だけを `./scripts/verify-targeted.sh` で確認し、完了報告の直前に `./scripts/verify.sh` を実行する。全体ゲート失敗後は失敗した検査だけで修正を回し、最後に全体ゲートを再実行する。targeted検証だけでは完了扱いにしない。

| 段階 | 条件 |
|------|------|
| 着手 | 実装指示書が `docs/tasks/` にある |
| Phase 完了 | 当該 Phase の `spec-acceptance.toml` エントリが `pending = false` かつテスト緑 |
| 全体完了 | 全 Phase の `pending = false` + `./scripts/verify.sh` + 実装指示書を `docs/done/` へ |

`verify.sh` だけ緑でも、受け入れレジストリに `pending = true` が残っていれば **仕様未完了** とする。

## Feature Scope（0056 以降）

新規 spec では次を守る。

**仕様作成時** — 設計書に Core outcome / Minimum vertical slice / Fault model / Non-goals / Complexity inventory / Complexity Gate / Complexity budget / Split triggers / Deferred specs を含め、`scripts/feature-scope.toml` に entry を追加する。

**実装時** — 新しい複雑性要因を発見しても自動実装しない。`STOP-THE-LINE` 形式で報告する（詳細: [feature-development-policy.md](./feature-development-policy.md)）。

**レビュー時** — 指摘に `BLOCKER_ORIGINAL_AC` / `REGRESSION` / `SAFETY_WITHIN_FAULT_MODEL` / `NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` を付ける。`NEW_REQUIREMENT` / `HARDENING` / `OUT_OF_FAULT_MODEL` は別 spec 候補として分離する。

## Linux: `bwrap: loopback: Operation not permitted`

Ubuntu 24.04 などで AppArmor が unprivileged user namespace を制限していると、Codex の **bwrap** サンドボックスが失敗し、MCP からの `cat` / `rg` も同じエラーになる。

**CLI / Cursor MCP**

```bash
./scripts/codex-cli.sh …
```

- CLI / MCP wrapper はbwrapを優先し、利用不能な間だけLandlockへフォールバック
- [`.cursor/mcp.json`](../.cursor/mcp.json) → `scripts/codex-mcp-wrapper.sh`（認証は `~/.codex`、`workspace-write`）
- 事前に `codex login`（ChatGPT または API キー）
- **設定変更後は MCP 再接続が必須**

診断: `./scripts/codex-fix-linux-sandbox.sh`

bwrap用AppArmor profileを有効化:

```bash
sudo apt-get install -y apparmor-profiles apparmor-utils bubblewrap
sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict
sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict
```

`kernel.apparmor_restrict_unprivileged_userns=0` によるシステム全体の制限解除は行わない。

## MCP がそれでも動かないとき

1. Cursor の **Codex MCP** にシェル実行権限（Settings → MCP）
2. 手元で `codex` CLI（MCP 外）
3. 親が代替し **Codex 未実施**を明記

## 関連

- ルール: `.cursor/rules/50-codex-subagent.mdc`
- オプションの厚いパケット: [codex-review.md](./codex-review.md)
- 入口: `AGENTS.md`
