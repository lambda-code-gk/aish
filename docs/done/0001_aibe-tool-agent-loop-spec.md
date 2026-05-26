# 0001 — aibe ツール付きエージェントループ — 仕様ドラフト

> **出典**: Codex `spec`（2026-05-22）。session id: `019e51d6-e66f-79b0-974a-7d490a1aa2fc`  
> **状態**: 実装済み（2026-05-23）。正本の要約は `../architecture.md` の `agent_turn` 節。本ファイルは詳細仕様として残す。

## 確定した設計判断（ユーザー）

| 項目 | 方針 |
|------|------|
| **ツール失敗** | **LLM に tool result として返し、同一 `agent_turn` 内で再推論させる。** turn 全体を即 `error` にしない（後述の turn レベル例外を除く）。 |
| **`shell_exec`** | **設定ファイルに列挙されたコマンドのみ実行可能。** リスト外は実行せず、失敗結果を LLM へ返す。 |
| **将来** | コマンドごとの **インタラクティブ許可フロー** を追加予定。MVP では実装しないが、設定スキーマと `ShellExecutor` 境界は拡張しやすくする。 |

## 目的

aibe に、単発の `llm.complete` だけではなく、**LLM がツール呼び出しを返した場合に aibe がローカルでツールを実行し、結果を LLM に返して再度推論するループ**を実装する。

本仕様は、既存の NDJSON socket プロトコルを維持し、`agent_turn` / `tools` / `tool_calls` の流れを拡張する。

## スコープ

### 対象

- aibe 内のエージェントループ
- aibe 内のツール実行アダプタ
- `LlmProvider` の拡張
- request / response プロトコルの詳細化
- エラー、タイムアウト、回数制限
- テスト方針
- セキュリティ、ログ、秘密情報の扱い

### 対象外

- ai のツール列挙 UI / 動的ツールディスカバリ
- aish との直接連携追加
- aish 側でのツール実行
- **`shell_exec` のインタラクティブ許可 UI**（将来。MVP は設定 allowlist のみ）
- 新しい transport や `conversation_id` ベースの別プロトコル
- streaming の逐次イベント化
- 複数ツールの並列実行最適化
- Gemini / OpenAI 互換ごとの差分の最適化を先回りして詰めること

## 前提

- 既存プロトコルは **NDJSON 1 行 1 JSON**。
- `agent_turn` リクエストの `tools` は **`Vec<String>` のツール名**。
- `agent_turn_result` の `tool_calls` は **`Vec<serde_json::Value>`**。
- 本仕様はこの現行形に合わせる。**新しい conversation API は作らない。**
- `LlmProvider` は、OpenAI chat/completions の `tools` 形式に相当する情報を扱えるよう拡張する。

## 受け入れ条件

### 1. 既存の単発応答は壊さない

- `tools: []` の `agent_turn` は、従来どおり 1 回の LLM 呼び出しで完了する。
- `tool_calls` は空のままでもよい。
- 既存の `ping` は変更しない。

### 2. ツール付きループが動く

- `tools` に許可されたツール名が含まれ、LLM が tool call を返した場合、aibe はそのツールを実行する。
- ツール結果を LLM の次回入力に追加し、最終応答または上限到達までループする。
- 最終レスポンスには、実行したツール呼び出しの記録が `tool_calls` に含まれる。

### 3. 未許可・未知ツールは実行しない

- **リクエスト**の `tools` に含まれないツール名はリクエスト時点で turn `error`（実行しない）。
- **モデル**がリクエスト allowlist 外・未実装のツール名を返した場合は実行せず、tool result で LLM に返してループ継続する。
- 「知らないツールを勝手に実行する」ことはない。

### 4. 無限ループしない

- 最大ツールラウンド数を超えたら打ち切る。
- ツール実行タイムアウトがあれば、タイムアウト時に打ち切る。
- LLM が同じ tool call を繰り返しても、上限で必ず止まる。

### 5. 失敗が観測可能

- クライアントは `id` を使って要求と応答を対応付けられる。
- **ツール実行失敗**（引数不正、allowlist 不一致、subprocess 非ゼロ終了、タイムアウト等）は `tool_calls` 記録と LLM 向け tool result の両方で追跡できる。
- **turn レベル `error`** は、LLM 再試行では回復不能なものに限定する（下記「エラー設計」）。

### 6. ツール失敗は LLM が再試行できる

- ツールが失敗しても、最大ラウンド数の範囲内でループを継続する。
- LLM は失敗内容を見て、別コマンド・別引数・説明のみの応答へ切り替えられる。
- `tool_calls` には `status: "error"`（または同等）と `error` / `message` を含め、クライアントも監査できる。

## プロトコル詳細

### リクエスト（現行維持）

```json
{
  "type": "agent_turn",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "messages": [
    { "role": "user", "content": "..." }
  ],
  "tools": ["shell_exec", "read_file"],
  "context": {
    "shell_log_tail": "..."
  }
}
```

**意味論**

- `messages`: LLM に渡す会話履歴の起点
- `tools`: この turn で **許可するツール名の allowlist**
- `context.shell_log_tail`: aish ログ由来の補助コンテキスト

**ルール**

- `tools` が空なら、ツールは使わない。
- `tools` に unknown 名が含まれる場合は、リクエスト時点で弾くか、少なくともツール実行前に失敗させる。
- `context.shell_log_tail` はそのまま LLM に渡さず、必要なら aibe 側で明示的にコンテキスト化する（現行実装は `[shell log tail]` プレフィックス付き user メッセージ — 実装時に本仕様と整合させる）。

### レスポンス（現行維持、`tool_calls` の意味を明確化）

```json
{
  "type": "agent_turn_result",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "ok",
  "assistant_message": { "role": "assistant", "content": "..." },
  "tool_calls": []
}
```

**`tool_calls` の意味**

- **aibe が実際に実行した**ツール呼び出しの記録。
- OpenAI 互換の tool call 概念に寄せた JSON object の配列。

**推奨形（成功）**

```json
{
  "id": "call_1",
  "name": "read_file",
  "arguments": { "path": "README.md" },
  "status": "ok",
  "output": "..."
}
```

**推奨形（失敗 — LLM へは tool result、クライアント記録は `tool_calls`）**

```json
{
  "id": "call_2",
  "name": "shell_exec",
  "arguments": { "command": "curl", "args": ["https://example.com"] },
  "status": "error",
  "error": "command_not_allowed",
  "message": "command not in allowed_commands"
}
```

**エラー応答**

```json
{
  "type": "error",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "code": "provider_error",
  "message": "..."
}
```

**エラーコード案**（`ErrorCode` enum 拡張）

| コード | 用途 |
|--------|------|
| `invalid_request` | リクエスト不正 |
| `provider_error` | LLM API 失敗 |
| `tool_error` | ツール実行・引数失敗 |
| `tool_timeout` | ツールタイムアウト |
| `tool_not_allowed` | リクエスト `tools` に未実装名、または設定でツール全体が無効 |
| `max_tool_rounds` | ループ上限到達（LLM 再試行の余地なし） |
| `internal_error` | 内部エラー |

**turn レベル `error` と tool result の使い分け**

| 状況 | 扱い |
|------|------|
| プロバイダ API 失敗 | turn `error`（`provider_error`） |
| `messages` 空、JSON 不正 | turn `error`（`invalid_request`） |
| リクエスト `tools` に未知のツール名 | turn `error`（`tool_not_allowed`） |
| 最大ツールラウンド到達 | `agent_turn_result`（`status: "max_tool_rounds"`）+ 取得済み tool result を根拠にした最終 assistant 応答。`tool_calls` に実行記録を含める |
| ツール引数不正、実行失敗、タイムアウト | **tool result を LLM へ返してループ継続** |
| `shell_exec` が設定 allowlist 外 | **tool result（拒否理由）を LLM へ返してループ継続** |
| `read_file` がパス制限外 | **tool result を LLM へ返してループ継続** |
| モデルがリクエスト `tools` 外のツール名を返す | **tool result（`tool_not_allowed`）を LLM へ返してループ継続** |
| モデルが未実装ツール名を返す | **tool result（`tool_not_implemented`）を LLM へ返してループ継続** |

## エージェントループ

### 基本フロー

1. リクエストを受ける
2. `messages` を正規化する
3. `tools` を allowlist として登録する
4. LLM に `tools` 定義を渡して初回推論する
5. LLM が tool call を返したら、aibe がツールを実行する
6. ツール結果を会話に追加する
7. 追加した結果をもとに LLM へ再問い合わせする
8. tool call が無くなるか、上限到達で終了する

### 停止条件

- LLM が tool call を返さない（正常終了）
- 最大ツールラウンド数に到達
- プロバイダエラー（turn `error`）
- リクエスト段階の致命的不正（turn `error`）

**ループ継続（停止しない）**

- 個別ツールの実行失敗・タイムアウト・`shell_exec` allowlist 不一致 → tool result を LLM に渡して次ラウンドへ

### 実行順序

- MVP は **逐次実行**。1 応答内の複数 tool call も順番に処理。
- 並列化は将来拡張。

### コンテキスト組み立て

- `shell_log_tail` は必要最小限。
- ツール結果は LLM 向けの **tool result** メッセージとして追加（OpenAI `role: tool` + `tool_call_id` 相当。内部表現は実装時に `ChatMessage` 拡張で定義）。
- 成功・失敗どちらも同じ経路で LLM に返す。失敗時は `is_error: true` 相当のフラグまたは明示的なエラーテキストを含める。
- クライアントへの `assistant_message` は **最終応答のみ**。

## ツール定義（MVP）

| ツール | 責務 |
|--------|------|
| `shell_exec` | aibe 内 subprocess（**aish 経由しない**）。**設定 allowlist のコマンドのみ** |
| `read_file` | パス指定でテキスト読取（ベースパス制限必須） |

- ツール実装は **aibe** の `ports/outbound` + `adapters`。
- LLM へ渡す定義: 名前、説明、JSON Schema 引数。
- 引数は JSON object。パース不能・必須欠落は **tool result（エラー）→ LLM 再試行**。

**カレントディレクトリ（全ツール共通・必須）**

- 相対パス・`.` 付き `allowed_roots` の基準は **クライアント cwd**（`agent_turn.context.cwd`）。`ai` は `std::env::current_dir()` を毎回送る。
- 実装は `ToolExecutionContext::base_dir` / `resolve_path` を使う。aibe プロセスの `std::env::current_dir()` をツール内で直接使わない。
- `read_file` / `shell_exec` は準拠済み。今後追加するツールも同様。

**引数例**

- `read_file`: `path`（任意: `offset`, `limit`）
- `shell_exec`: `command`（必須、実行ファイルまたはコマンド名）, `args`（任意: 文字列配列）

### `shell_exec` — 設定 allowlist（MVP）

`~/.config/aibe/config.toml` に **事前許可コマンド** を列挙する。LLM が要求した `command` がリストに無い場合は subprocess を起動せず、tool result で拒否する。

**設定例（案）**

```toml
[tools]
max_rounds = 8
exec_timeout_ms = 30_000

[tools.shell_exec]
enabled = true
# 実行を許可するコマンド。MVP は次のいずれかで一致:
# - ベース名一致（例: "git" は PATH 上の git）
# - 設定に書いた絶対パスとの完全一致
allowed_commands = ["ls", "git", "cargo", "/usr/bin/rg"]
```

**マッチング規則（MVP）**

1. `command` を正規化（先頭の `./` 除去、パス区切りは OS 規則に従う）。
2. `allowed_commands` の各エントリと比較:
   - エントリに `/` が無い → **ベース名** が一致すれば許可。
   - エントリが絶対パス → **正規化後の `command` パス** と完全一致で許可。
3. 一致なし → 実行しない。tool result: `command not in allowed_commands`（LLM が別手段を検討可能）。
4. `args` は allowlist 通過後にそのまま subprocess へ渡す（MVP では引数パターンの追加制限は **未確定**。過剰な自由度を避けるなら将来 `allowed_commands` をオブジェクト化）。

**`enabled = false`**

- リクエスト `tools` に `shell_exec` が含まれていても、実行前に tool result で「無効」と返す（turn は継続可能。LLM が代替を選べる）。

### `shell_exec` — 将来: インタラクティブ許可（スコープ外）

- ユーザーが実行直前に y/n するフロー、セッション単位の一時許可、監査ログ。
- MVP では **port**（例: `CommandPolicy`）に `ConfigAllowlist` 実装のみ置き、後から `InteractiveApproval` アダプタを差し替え可能にする。
- 設定に `approval_mode = "config_only"`（MVP 既定）→ 将来 `"interactive"` を追加する想定でよい（**推測**: キー名は実装時に確定）。

### `read_file`（要約）

- 許可ルート外・秘密ファイルは tool result で拒否し LLM へ返す（turn 即死にしない）。
- 相対 `path` と `allowed_roots` の `.` は `agent_turn` の `context.cwd`（`ai` は自身のカレントディレクトリを送る）を基準に解決する。aibe プロセスの cwd は使わない。

## `LlmProvider` 拡張

- `complete()` はツールなし経路として残してよい。
- ツール付き経路は OpenAI `tools` / `tool_calls` 相当を扱う新メソッドまたは拡張応答型を追加。
- adapter が provider 固有表現を aibe 共通モデルへ正規化する。

**返却情報（1 ステップあたり）**

- assistant message（テキスト、tool call のみ、または両方）
- 0 個以上の tool call（順序保持）

## 設定（aibe `config.toml` 追加案）

| キー | 説明 |
|------|------|
| `[tools] max_rounds` | 1 `agent_turn` あたりの最大 LLM↔ツール ラウンド（**1 以上**。`0` は TOML 読み込み拒否。プログラム上 0 のみ 1 に補正 — `ToolsConfig::effective_max_rounds`） |
| `[tools] exec_timeout_ms` | ツール 1 回あたりのタイムアウト |
| `[tools] max_tool_output_bytes` | `tool_calls` / LLM 向け tool result の最大バイト（超過分は切り詰め） |
| `[tools.shell_exec] enabled` | `false` なら常に拒否（tool result） |
| `[tools.shell_exec] allowed_commands` | 事前許可コマンド一覧（**必須**。空なら常に拒否） |
| `[tools.read_file] allowed_roots` | 読取可能ディレクトリ（未確定: 実装時に既定値を決める） |

- `allowed_commands` が未設定または空のとき、`shell_exec` は常に allowlist 不一致として tool result を返す（安全側）。

## テスト方針

| 種別 | 内容 |
|------|------|
| 単体 | no-tool 回帰、tool 解析、unknown 拒否、max round、timeout |
| 単体 | allowlist 外 `shell_exec` → tool result → MockLlm が再推論 |
| 単体 | subprocess 失敗 → tool result → ループ継続 |
| 統合 | Unix socket + MockLlm で tool call → error result → retry → final |
| 回帰 | `ping`、既存 `agent_turn`、serde 互換 |

## セキュリティ・ログ

- tool 引数・stdout/stderr・戻り値の無制限ログ禁止。`max_tool_output_bytes` で `tool_calls` と LLM 向け tool result を切り詰める（実装済み）。パターン別マスクは将来検討。
- `shell_exec` / `read_file` は高リスク。`shell_exec` は **allowlist が唯一の実行ゲート**（MVP）。
- allowlist は git にコミットしない実設定のみ。例示は `*.example.toml`。
- aibe プロセス権限＝許可されたコマンドでも持つ権限。allowlist は **緩和ではなく必須の絞り込み**。
- 将来のインタラクティブ許可は、allowlist 通過後の追加ゲートとして設計する。

## 影響クレート

| クレート | 変更 |
|---------|------|
| **aibe** | `agent_turn`, `llm`, protocol, tool adapters, tests |
| **ai** | 今回は対象外（将来 `tools` 列挙） |
| **aish** | 対象外（ログ tail 継続） |
| **docs** | `architecture.md`, `security.md`, `testing.md` |

## 未確定・推測

| 種別 | 内容 |
|------|------|
| **確定** | ツール失敗は LLM に返して再試行（上記） |
| **確定** | `shell_exec` は設定 `allowed_commands` のみ（インタラクティブは将来） |
| **推測** | `tool_calls` JSON の field 名は OpenAI 寄せで実装時微調整 |
| **推測** | `read_file` は `allowed_roots` で複数ルート可 |
| **推測** | `approval_mode` キーは将来追加。MVP は `config_only` 相当のみ |
| **未確定** | 内部 tool result の `ChatMessage` 表現（`role: tool` 等） |
| **未確定** | provider ごとの tool call 正規化の細部 |
| **未確定** | 最大ラウンド到達時に `error` とするか `agent_turn_result` とするか |
| **未確定** | `shell_exec` の `args` に対する追加制限（ワイルドカード許可など） |

## 残リスク（Codex 指摘）

- `shell_exec` 有効化時のプロセス権限リスク
- `read_file` による秘密ファイル読取
- tool call 解析不備による可用性低下
- 既存マスクが tool 出力を十分覆えない可能性
