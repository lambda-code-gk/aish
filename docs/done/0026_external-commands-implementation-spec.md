# 0026 — 外部コマンド（CLI coding agent）実装指示書

> **種別**: 実装指示書（`docs/tasks/` → 完了後 `docs/done/`）  
> **状態**: 実装済み  
> **設計の正本**: [0026_external-commands-spec.md](../spec/0026_external-commands-spec.md)  
> **起票**: 2026-06-06

## 目的

0026 を本番経路に落とし込む。`feature/cli-subagent` の first-class 統合は採用しない。Codex CLI / Claude Code CLI を `shell_exec` 経路の **外部コマンド設定テンプレート**としてのみ扱う。

## 実装サマリ

### aibe

- `AppConfig.external_commands` / `ExternalCommandConfig` を追加
- `[[external_commands]]` を `toml_config` で読み込み、`allowed_commands` との整合を起動時検証
- `ShellExecTool` が一致する `command` に対し `approval_source=shell_exec_approval=<mode>;external_command=<name>` を付与
- 一致時は `timeout_secs` を subprocess timeout に使用
- 0024 / 0025 由来の runtime（`cli_subagent` / `invoke_*` / `artifacts` 等）を削除

### ai

- `aibe_external_commands` で設定から外部コマンド名のみ読み取り
- 起動時 `warning: ai: external commands registered: ...` を stderr に表示
- `cli-thread.json` / `invoke_*` / `artifacts` 経路を削除

### tests

- `aibe/tests/external_commands.rs` — 正常系・拒否系
- `aibe` / `ai` unit — config parse / approval_source / warning
- `aibe-client/tests/ensure_running_spawn.rs` — 隔離 `AIBE_CONFIG` で起動検証

### docs

- `docs/architecture.md` / `security.md` / `testing.md` / `aibe.config.example.toml`
- `docs/manual/external-commands-cli.md`（新規）

## 受け入れ条件（検証済み）

1. `[[external_commands]]` が設定として読める
2. `command` が `allowed_commands` に無いと aibe 起動失敗
3. 外部コマンドは `shell_exec` 経路のみ（新 tool / LlmProvider なし）
4. `tool_calls` に監査フィールドと `external_command` 付き `approval_source`
5. `ai` の `shell_exec` warning + 外部コマンド名 warning
6. 0024 / 0025 の first-class 統合なし
7. `./scripts/verify.sh` と `./scripts/smoke-mock.sh` 成功

## 品質ゲート

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
cargo test -p aibe --test external_commands -- --nocapture
cargo test -p ai --test ask_integration -- --nocapture
```
