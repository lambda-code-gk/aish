# 0046 — aibe Graceful Restart 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0046_aibe-graceful-restart-spec.md](../spec/0046_aibe-graceful-restart-spec.md)  
> **状態**: 実装前  
> **起票**: 2026-06-21  
> **注意**: 本番経路前提。仮実装・テスト専用分岐・stub は入れない。`aibe-protocol` の wire 変更は MVP では追加しない。

## 0. 目的

`aibe` に stop / restart / status を追加し、PID file と Unix socket を使った local-only の graceful restart を本番経路で実装する。設定変更の再反映、停止、状態確認を `aibe` 自身の責務として閉じ、`aibe-client` や wire protocol へ lifecycle control を持ち出さない。

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|------|------|------------------------------------------|
| 1 | CLI 追加、PID file の read/write、status の JSON 出力、live socket / stale PID の判定を固める。`aibe` 起動の既存 `ping` / `already running` 挙動を壊さない。 | `pid_file_roundtrip_preserves_identity_and_metadata`、`status_json_exposes_required_fields`、`already_running_short_circuits_on_live_socket` |
| 2 | `aibe stop` / `aibe restart` の control plane を実装し、config parse failure では旧 daemon を一切触らない。restart は新 daemon ready まで待つ。 | `restart_aborts_before_signaling_on_config_parse_failure`、`stop_signals_daemon_and_cleans_up_pid_file`、`restart_waits_for_new_daemon_readiness_before_returning` |
| 3 | SIGTERM / SIGINT shutdown を server 側へ配線し、accept 停止、active turn cancel、`MemorySubscribe` close、drain timeout 後の cleanup を実装する。mock/local の正常系導通と docs を揃える。 | `shutdown_cancels_active_turn_and_closes_memory_subscribe`、`status_reports_stale_pid_but_live_socket_as_running`、`./scripts/smoke-mock.sh` |

Phase を飛ばさない。各 Phase の gate が green になるまで次 Phase の実装へ進まない。

## 2. 受け入れ条件

設計書 §10 を、`scripts/spec-acceptance.toml` へ 1:1 で登録できる粒度に分解する。

| ID | 条件 | テスト関数 | 置き場所 | pending |
|----|------|------------|----------|---------|
| 0046-pidfile-roundtrip | PID file に `pid` / `config_path` / `socket_path` / 識別子を保存し、read 後も一致する | `pid_file_roundtrip_preserves_identity_and_metadata` | `aibe/src/daemon.rs` または新規 pid file helper の unit | true |
| 0046-pidfile-stale | PID 再利用または metadata 不一致を stale と判定し、signal を送らない | `pid_file_detects_stale_or_mismatched_identity` | `aibe/src/daemon.rs` または新規 pid file helper の unit | true |
| 0046-status-json | `aibe status --format json` が `state` / `pid_file_state` / `pid_file_path` / `pid` / `config_path` / `socket_path` / `socket_ping` を返す | `status_json_exposes_required_fields` | `aibe/tests/graceful_restart.rs` | true |
| 0046-status-live-socket | stale PID file が残っていても live socket が応答するなら running と判定する | `status_reports_stale_pid_but_live_socket_as_running` | `aibe/tests/graceful_restart.rs` | true |
| 0046-already-running | live socket が応答する既存 daemon に対して `aibe` 起動が `already running` で止まる | `already_running_short_circuits_on_live_socket` | `aibe/tests/graceful_restart.rs` | true |
| 0046-restart-parse-failure | 新 config の parse failure 時に旧 daemon へ SIGTERM を送らない | `restart_aborts_before_signaling_on_config_parse_failure` | `aibe/src/application/graceful_restart.rs` または control helper の unit | true |
| 0046-stop-cleanup | `aibe stop` が SIGTERM 送信、終了待ち、PID/socket の cleanup を行う | `stop_signals_daemon_and_cleans_up_pid_file` | `aibe/tests/graceful_restart.rs` | true |
| 0046-restart-ready | `aibe restart` が旧 daemon shutdown 後に新 daemon を起動し、ready を確認してから返る | `restart_waits_for_new_daemon_readiness_before_returning` | `aibe/tests/graceful_restart.rs` | true |
| 0046-shutdown-drain | SIGTERM / `aibe stop` で accept 停止、active turn cancel、`MemorySubscribe` close、drain timeout 後 cleanup が起きる | `shutdown_cancels_active_turn_and_closes_memory_subscribe` | `aibe/tests/graceful_restart.rs` と既存 `memory_subscribe` / `agent_turn` 系 | true |

`pending = true` の行は、対応する Rust テスト関数を先に追加してから実装を進める。`pending = false` へは、実装・テスト・docs 更新が揃った時点でのみ変更する。

## 3. テスト計画

### 3.1 unit

| 対象 | 観点 |
|------|------|
| `aibe/src/daemon.rs` または新規 pid file helper | PID file の write/read、metadata 検証、stale 判定、0600 相当の権限、壊れた file の fail-closed |
| `aibe/src/application/graceful_restart.rs` のような新規 application helper | restart の分岐、config parse failure 時の no-op、ready 待ちの制御 |
| `aibe/src/clap_cli.rs` | `stop` / `restart` / `status` の subcommand 定義、`status --format json` の引数解釈 |

### 3.2 integration

| ファイル | 観点 |
|---------|------|
| `aibe/tests/graceful_restart.rs` | temp HOME / temp config / temp socket で `status --format json`、`stop`、`restart`、stale PID、already-running を固定する |
| `aibe/tests/memory_subscribe.rs` | shutdown 時に subscribe 接続が閉じることを固定する |
| `aibe/tests/agent_turn_tools.rs` または専用 integration | active turn 中の SIGTERM で cancel が配線されることを固定する |
| 既存 `aibe/tests/ai_ask_e2e.rs` / `aibe-client` 側の既存 test | `ping` / `already running` / transport contract の regression を確認する |

integration は実ネットワークを使わず、`AIBE_CONFIG` / `AIBE_SOCKET_PATH` / `HOME` を temp directory に隔離した mock 正常系で回す。LLM は mock provider のみを使う。

### 3.3 manual

`docs/manual/aibe-graceful-restart.md` を新規作成し、`status` / `stop` / `restart` の手順と期待結果を記録する。`./scripts/smoke-mock.sh` の control-plane 追試をこの manual と同期させる。

## 4. docs 更新対象

| ファイル | 変更内容 |
|----------|----------|
| `docs/architecture.md` | aibe daemon lifecycle、PID file の意味、stop / restart / status の役割、SIGTERM shutdown の流れを追記 |
| `docs/testing.md` | 0046 節を、実際の test file / test function 名に合わせて更新 |
| `docs/security.md` | PID file の配置・権限、local-only control、stale 判定と no-remote-control を追記 |
| `docs/manual/aibe-graceful-restart.md` | 新規。temp HOME での `status` / `stop` / `restart` / stale PID 確認手順 |
| `docs/manual/README.md` | 新規 manual の目次追加 |
| `docs/0000_spec-index.md` | `tasks` セクションに 0046 の実装指示書を追加 |

## 5. 実装タスク分解

### 5.1 CLI と status の骨格

1. `aibe/src/clap_cli.rs` に `stop` / `restart` / `status` を追加する。
2. `status` に `--format json` を追加し、機械可読出力を正とする。
3. `main.rs` で control command を判別し、control command では daemonize しない。
4. 既存 `complete` と `--foreground` の契約は維持する。

### 5.2 PID file と control helper

1. PID file の保存先、read/write、metadata 構造を本番コードとして追加する。
2. PID / config path / socket path / 起動識別子の一致確認を実装する。
3. stale 判定では signal を送らず、必要なら cleanup のみ行う。
4. 権限は user-private を維持し、作成時の umask / chmod を明示する。

### 5.3 graceful shutdown

1. `aibe/src/application/server.rs` から shutdown orchestrator を起動する。
2. `aibe/src/adapters/inbound/unix_socket_server.rs` に accept 停止と connection drain の経路を足す。
3. `RequestService` の active turn cancel 入口を shutdown から呼べるようにする。
4. `MemorySubscribe` の長寿命接続を shutdown で閉じる。
5. drain timeout を過ぎたら best-effort cleanup に切り替える。

### 5.4 stop / restart の制御

1. `stop` は PID file を読み、識別子検証後に SIGTERM を送る。
2. `restart` は先に `AIBE_CONFIG` を parse し、成功した場合のみ旧 daemon を stop する。
3. 旧 daemon shutdown 完了後に新 daemon を起動し、ready 確認後に終了する。
4. 旧 daemon がいない場合は、そのまま start 相当の経路に倒す。

### 5.5 テストと docs

1. `aibe/tests/graceful_restart.rs` を新設し、status / stop / restart / stale PID / already-running を固定する。
2. 既存の `memory_subscribe` / `agent_turn` 系へ shutdown 観点を追加する。
3. `docs/manual/aibe-graceful-restart.md` と `docs/manual/README.md` を更新する。
4. `docs/architecture.md` / `docs/testing.md` / `docs/security.md` / `docs/0000_spec-index.md` を同じ変更で同期する。

## 6. Step 6 で実行する正常系コマンド

以下は temp HOME に隔離した mock 正常系の一巡で、`./scripts/verify.sh` の後に続けて実行する前提のコマンド列である。

```bash
tmp="$(mktemp -d)"
export HOME="$tmp/home"
export AIBE_CONFIG="$tmp/aibe.toml"
export AIBE_SOCKET_PATH="$tmp/run.sock"

cat >"$AIBE_CONFIG" <<'EOF'
[llm]
provider = "mock"
EOF

cargo build -q -p aibe

cargo run -q -p aibe -- -f &
AIBE_PID=$!

for _ in $(seq 1 100); do
  [ -S "$AIBE_SOCKET_PATH" ] && break
  sleep 0.1
done

cargo run -q -p aibe -- status --format json
cargo run -q -p aibe -- restart
cargo run -q -p aibe -- status --format json
cargo run -q -p aibe -- stop

./scripts/smoke-mock.sh
```

`restart` の途中で新 config を差し替える場合は、`AIBE_CONFIG` の中身を更新してから `cargo run -q -p aibe -- restart` を実行する。`status --format json` は、出力を `jq` で検証できる形にしておく。

## 7. 完了条件

1. `scripts/spec-acceptance.toml` の 0046 ケースがすべて `pending = false` になる。
2. `./scripts/verify.sh` が通る。
3. `./scripts/smoke-mock.sh` もしくは同等の mock/control-plane 正常系が通る。
4. `docs/architecture.md` / `docs/testing.md` / `docs/security.md` / `docs/manual/*` / `docs/0000_spec-index.md` が 0046 と整合する。
5. 本ファイルを `docs/done/0046_aibe-graceful-restart-implementation-spec.md` へ移動できる状態になる。

## 8. 仕様との差分

なし。設計書 0046 の受け入れ条件をそのまま実装手順へ落とす。

