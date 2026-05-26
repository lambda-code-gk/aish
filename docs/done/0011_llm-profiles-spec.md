# 0011 — LLM プロファイル（接続定義 + 能力プリセット）— 指示書

> **出典**: 本タスク（2026-05-24）— 複数 LLM 接続と名前付きプロファイルの再導入。Codex `review` 反映（2026-05-24）。  
> **状態**: **実装済み**

## 目的

aibe 設定で **LLM 接続定義**（プロバイダ・API キー・エンドポイント）と **プロファイル**（モデル・生成パラメータのプリセット）を分離し、`ai ask` 実行時にプロファイル名で利用する LLM 能力を選べるようにする。

同一 LLM 接続（例: 同じ Gemini API キー）に対し、モデルや温度などを変えた **複数プロファイル** を定義できること。

## スコープ

### 対象

- aibe 設定の 2 段構造（`[llm.<name>]` + `[profiles.<name>]`）
- 起動時 **eager** なプロファイルレジストリ構築
- リクエストごとのプロファイル選択（`agent_turn.llm_profile`）
- `ai ask --profile` / `~/.config/ai/config.toml` / `AI_LLM_PROFILE` の既定プロファイル
- 後方互換（従来のフラット `[llm]` 単一設定）
- 生成パラメータ（`temperature`, `max_output_tokens`）のプロファイル単位指定
- テスト・`docs/` 同期

### 対象外

- プロファイルごとに **別 socket / 別 aibe デーモン** を起動する方式
- `ai` / `aish` から LLM HTTP を直接呼ぶこと
- プロファイルの動的ホットリロード（設定変更は aibe 再起動が必要）
- プロファイルの **lazy** 初回構築（本 PR では起動時 eager のみ）
- Vertex AI / streaming / プロバイダ固有の高度パラメータ（第 2 段階）
- プロファイル名の自動推論（CLI 未指定時は既定のみ）

## 確定した設計判断

| 項目 | 方針 |
|------|------|
| **2 段構成** | **LLM 定義** = 接続（`provider`, `api_key`, `base_url`）。**プロファイル** = 利用プリセット（`llm` 参照名, `model`, 生成パラメータ）。 |
| **LLM 定義の TOML 形** | `[llm.<name>]`。テーブルキー `<name>` が LLM 名（接続 ID）。`name` フィールドは設けない。 |
| **プロファイルの TOML 形** | トップレベル `[profiles.<name>]`。テーブルキー `<name>` がプロファイル名。 |
| **既定プロファイル** | トップレベル `default_profile = "<name>"`（aibe 設定）。**省略時は `"default"`**（その名前のプロファイルが存在すること）。新形式で `"default"` も `default_profile` も無く、かつ `[profiles.*]` が `"default"` を含まない場合は読み込みエラー。 |
| **モデルの所在** | **プロファイルのみ**。LLM 定義に `model` を書いてはならない（後方互換の例外は後述）。 |
| **実行時選択** | クライアントは **プロファイル名** のみ送る。API キー・provider 種別は aibe 内に閉じる。 |
| **プロトコル** | `agent_turn` に任意フィールド `llm_profile: string` を追加。省略時は aibe の `default_profile`。 |
| **後方互換** | 従来のフラット `[llm]`（`provider` + `api_key` + `base_url` + `model`）は、暗黙の LLM 名 `default` + プロファイル名 `default` として解釈する。 |
| **レジストリ構築** | **起動時 eager**。全プロファイルの `LlmProvider` を起動前に構築し、参照解決・構築失敗は **起動失敗**（`ConfigError`）。リクエスト時の lazy 構築は行わない。 |
| **環境変数（aibe 起動時）** | `AIBE_LLM_PROVIDER` / `AIBE_API_KEY` / `AIBE_MODEL` / `AIBE_BASE_URL` は **legacy フラット `[llm]` のみ** で有効。新形式 `[llm.<name>]` がある設定では **無視**（0010 の env 仕様を legacy に限定して上書き）。 |
| **環境変数（ai 実行時）** | `AI_LLM_PROFILE` — プロファイル名。CLI `--profile` より低優先。aibe 側の `AIBE_LLM_PROFILE` は **設けない**（ai が解決して wire に載せる）。 |
| **TerminationCapability** | プロファイル名 → 参照 backend の provider 種別から決定。`ProfileRegistry` が profile 名ごとに保持（0010 と同様、Gemini / OpenAI 互換は初期 `false`）。 |
| **HTTP クライアント共有** | **backend 名** 単位で `reqwest::Client` + `base_url` + `api_key` を 1 セット共有。**profile 単位**で `model` と生成パラメータを持つ別 `LlmProvider` インスタンスを生成する。 |
| **Mock backend** | `provider = "mock"` の backend は **backend 名ごとに 1 つの `Arc<dyn LlmProvider>`（`MockLlm`）を共有**。参照する全プロファイルが同じインスタンスを使う（stateless のため model 差は無視）。 |
| **境界** | `ai` はプロファイル名のみ知る。`aish` は LLM 設定を持たない。 |

## 設定読み込みの判定順（aibe）

TOML パース後、次の順で形式を判定する。

1. **legacy フラット `[llm]`** — `[llm]` 直下に `provider` 等があり、かつ `[llm.<name>]` 形式のサブテーブルが **1 つもない**
   - → 暗黙 backend `default` + プロファイル `default` を生成
   - → `AIBE_LLM_*` env オーバーライドを適用（0010 互換）
2. **新形式** — 1 つ以上の `[llm.<name>]` がある
   - → `[profiles.<name>]` が **1 つ以上必須**
   - → `AIBE_LLM_*` env は **すべて無視**
   - → `[llm]` 直下のフラットキーと `[llm.<name>]` の **混在はエラー**
3. **無効** — `[profiles.*]` が 0 件 **かつ** legacy フラット `[llm]` でもない → `ConfigError::Invalid`

## 設定ファイル（aibe）

### 新形式（推奨）

```toml
# ~/.config/aibe/config.toml

# socket_path = "~/.local/share/aibe/run.sock"

default_profile = "fast"

# --- LLM 接続定義（認証・エンドポイントのみ）---

[llm.gemini-studio]
provider = "gemini"
api_key = "YOUR_KEY"
base_url = "https://generativelanguage.googleapis.com/v1beta"

[llm.lmstudio]
provider = "openai_compatible"
api_key = "lm-studio"
base_url = "http://127.0.0.1:1234/v1"

[llm.mock]
provider = "mock"

# --- プロファイル（能力プリセット）---

[profiles.fast]
llm = "gemini-studio"
model = "gemini-3.5-flash"

[profiles.reasoning]
llm = "gemini-studio"          # 同一 LLM 接続を再利用
model = "gemini-3-pro"
temperature = 0.2
max_output_tokens = 8192

[profiles.local-chat]
llm = "lmstudio"
model = "qwen-7b"
temperature = 0.7

[tools]
max_rounds = 8
# ... 従来どおり
```

### LLM 定義 `[llm.<name>]`

| キー | 必須 | 説明 |
|------|------|------|
| `provider` | ○ | `mock` / `openai_compatible` / `openai-compatible` / `gemini` |
| `api_key` | provider 依存 | `mock` 以外。TOML 必須（新形式では env フォールバックなし） |
| `base_url` | 任意 | 省略時は provider 既定（0010 参照） |

**禁止**: LLM 定義に `model` / `temperature` 等を書く（新形式）。パース時に検出したら `ConfigError::Invalid`。

### プロファイル `[profiles.<name>]`

| キー | 必須 | 説明 |
|------|------|------|
| `llm` | ○ | 参照する LLM 定義名（`[llm.<name>]` の `<name>`） |
| `model` | ○ | そのプロファイルで使うモデル ID |
| `temperature` | 任意 | `f32`。省略時は JSON に含めない |
| `max_output_tokens` | 任意 | `u32`（0〜`4294967295`）。範囲外は読み込みエラー。省略時は JSON に含めない |

**検証**（読み込み時・起動前）:

- `llm` が存在しない LLM 名 → 読み込みエラー
- 新形式で `[profiles.*]` が 0 件 → 読み込みエラー（legacy フラット `[llm]` は除外）
- `default_profile` が存在しないプロファイル名 → 読み込みエラー
- プロファイル名の重複 → TOML パースエラー（テーブルキーで一意）
- いずれかのプロファイルの `LlmProvider` 構築失敗 → 起動失敗

### 後方互換（フラット `[llm]`）

```toml
[llm]
provider = "openai_compatible"
api_key = "..."
base_url = "http://127.0.0.1:1234/v1"
model = "local-model"
```

解釈:

| 生成される内部オブジェクト | 内容 |
|---------------------------|------|
| LLM 定義 `default` | `provider`, `api_key`, `base_url`（`model` 除く）。`AIBE_LLM_*` env で上書き可 |
| プロファイル `default` | `llm = "default"`, `model` = 上記 `model`（env `AIBE_MODEL` で上書き可） |
| `default_profile` | `"default"`（未指定時） |

`[llm.<name>]` テーブルとフラット `[llm]` を **同一ファイルに混在させない**。混在時は読み込みエラー。

## 環境変数

### aibe（legacy `[llm]` のみ）

| 変数 | 用途 |
|------|------|
| `AIBE_LLM_PROVIDER` | legacy フラット `[llm]` の `provider` 上書き |
| `AIBE_API_KEY` | legacy の `api_key` 上書き |
| `AIBE_MODEL` | legacy プロファイル `default` の `model` 上書き |
| `AIBE_BASE_URL` | legacy の `base_url` 上書き |

新形式（`[llm.<name>]` あり）では **すべて無視**。0010 の env 記述は legacy 限定として本仕様が上書きする（0010 本文は実装 PR で脚注追記可）。

### ai（実行時プロファイル選択）

| 優先順位（高い順） | ソース |
|-------------------|--------|
| 1 | CLI `ai ask ... --profile NAME` |
| 2 | 環境変数 `AI_LLM_PROFILE` |
| 3 | `~/.config/ai/config.toml` の `[ask].default_profile` |
| 4 | wire に `llm_profile` を **載せない**（aibe の `default_profile` に委譲） |

`ai` は解決したプロファイル名を wire に載せる。**存在確認・backend 解決は aibe** が行う。

## 設定ファイル（ai）

```toml
# ~/.config/ai/config.toml

[ask]
default_profile = "fast"
# tools = "@read-only"  # 従来どおり
```

## ai 側データフロー

プロファイル名は **aibe 設定を読まず**、次の経路のみで wire に載せる。

```text
main.rs (--profile / AI_LLM_PROFILE)
  → AskRunOptions.resolved_profile: Option<String>
  → AskInput.llm_profile
  → AskRequest.llm_profile          # into_request() で引き継ぎ
  → AibeUnixClient.agent_turn()
  → ClientRequest::AgentTurn { llm_profile, ... }
```

| 段階 | 型 / 場所 | 責務 |
|------|-----------|------|
| CLI / config 解決 | `main.rs` | 優先順位どおり `Option<String>` を決定 |
| 入力収集 | `AskInput.llm_profile` | cwd・tools と並列で保持 |
| 送信ペイロード | `AskRequest.llm_profile` | aibe 送信直前の DTO |
| wire | `ClientRequest::AgentTurn.llm_profile` | JSON 任意フィールド |

優先順位 4（aibe 既定委譲）のときは `llm_profile` を **省略**（`None` → JSON にキーなし）。aibe が `default_profile` を使う。

## プロトコル

### リクエスト（追加フィールド）

```json
{
  "type": "agent_turn",
  "id": "...",
  "llm_profile": "reasoning",
  "messages": [{ "role": "user", "content": "..." }],
  "tools": [],
  "context": {}
}
```

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `llm_profile` | 任意 | 使用するプロファイル名。省略時は aibe の `default_profile`。 |

**後方互換**: 既存クライアント（フィールドなし）は従来どおり aibe の `default_profile` を使用。

### エラー

| 条件 | `code` | 備考 |
|------|--------|------|
| 未知の `llm_profile` | `invalid_request` | メッセージにプロファイル名を含める。**LLM 呼び出し前** |

起動時 eager 構築のため、プロファイル構築失敗は **リクエスト時には発生しない**（起動失敗に統一）。

## ランタイム

### 起動（aibe — composition root: `lib.rs` / `server.rs`）

1. TOML を読み、判定順（上記）で legacy / 新形式を確定
2. LLM backend マップとプロファイルマップを構築
3. backend 名ごとに `BackendContext { client, base_url, api_key, provider_kind }` を 1 セット生成
4. 各プロファイルについて `build_llm_from_profile(backend_ctx, profile)` で `Arc<dyn LlmProvider>` を生成
5. 各プロファイルについて `termination_capability(backend.provider_kind)` を算出
6. `ProfileRegistry { providers, capabilities, default_profile }` を組み立て
7. 未知 backend 参照・プロファイル構築失敗・`default_profile` 不在 → **起動失敗**（プロセス終了、`ConfigError` 表示）
8. `RequestService::new(registry, tools_config, ...)` に注入

### ProfileRegistry の責務

| 責務 | 担当 |
|------|------|
| 起動時の全プロファイル eager 構築 | `llm_factory` + composition root |
| `default_profile` 名の保持 | `ProfileRegistry` |
| リクエストの profile 名 → `LlmProvider` 解決 | `RequestService`（registry 参照） |
| 未知 profile 名 → `invalid_request` | `RequestService`（registry に無いキー） |
| profile 名 → `TerminationCapability` 解決 | `RequestService`（registry 参照） |
| 1 ターンの agent loop 実行 | 選択された provider で `AgentTurnService` / `ToolRoundExecutor` を都度組み立て、または registry から取得した provider を注入 |

`application/server.rs` は registry を組み立てて `RequestService` に渡すだけ。profile 名検証の **ビジネスルール** は `RequestService` に置く。

### リクエスト（`agent_turn`）

1. `llm_profile` を解決（省略 → registry の `default_profile`）
2. registry から `Arc<dyn LlmProvider>` と `TerminationCapability` を取得。未登録なら `invalid_request`
3. 0003 どおり **cwd 検証を tool 名検証より先** に行う（プロファイル選択の前後は問わないが、LLM 呼び出しより前）
4. 選択された provider で 1 ターン実行

```text
ai --profile reasoning
  → agent_turn { llm_profile: "reasoning" }
  → ProfileRegistry.providers["reasoning"]
  → GeminiLlm(shared_backend=gemini-studio, model=gemini-3-pro, temp=0.2)
```

### HTTP クライアント共有（具体）

```text
BackendContext["gemini-studio"]
  ├── reqwest::Client          ← 1 インスタンス（backend 共有）
  ├── base_url, api_key
  └── provider_kind: Gemini

Profile "fast"     → GeminiLlm { backend: shared, model: "gemini-3.5-flash", params: {} }
Profile "reasoning"→ GeminiLlm { backend: shared, model: "gemini-3-pro", params: { temp: 0.2 } }
```

各 `GeminiLlm` / `OpenAiCompatibleLlm` は `BackendContext` への参照（または clone 可能な軽量ハンドル）を持ち、HTTP POST 時に自 profile の `model` / 生成パラメータを body に載せる。

## ドメイン / ports 型（案）

```rust
/// LLM 接続定義（認証・エンドポイント）
pub struct LlmBackend {
    pub provider: LlmProviderKind,
    pub api_key: String,
    pub base_url: String,
}

/// 生成パラメータ（プロファイル単位）
pub struct LlmGenerationParams {
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

/// プロファイル
pub struct LlmProfile {
    pub llm: String,
    pub model: String,
    pub params: LlmGenerationParams,
}

pub struct LlmProfilesConfig {
    pub backends: HashMap<String, LlmBackend>,
    pub profiles: HashMap<String, LlmProfile>,
    pub default_profile: String,
}

/// 起動時構築。リクエスト時は参照のみ。
pub struct ProfileRegistry {
    pub providers: HashMap<String, Arc<dyn LlmProvider>>,
    pub capabilities: HashMap<String, TerminationCapability>,
    pub default_profile: String,
}
```

`AppConfig.llm: LlmConfig`（単一 enum）は **`llm: LlmProfilesConfig`** に置き換える。

`LlmProvider` trait は変更しない。生成パラメータは各 adapter 構造体のフィールドとして保持し、HTTP body 組み立て時に反映する。

### アダプタへの生成パラメータ反映

| パラメータ | OpenAI 互換 (`ChatRequest`) | Gemini (`GenerateContentRequest`) |
|-----------|----------------------------|-----------------------------------|
| `temperature` | `temperature: Option<f32>` | `generationConfig.temperature` |
| `max_output_tokens` | `max_tokens: Option<u32>` | `generationConfig.maxOutputTokens` |

省略時は JSON に含めない（プロバイダ既定）。

## 受け入れ条件

1. 新形式 TOML（複数 `[llm.*]` + 複数 `[profiles.*]` + `default_profile`）を読み込める。
2. 同一 LLM 定義を参照する複数プロファイルが、異なる `model` / 生成パラメータで動作する。
3. 同一 backend を参照する複数プロファイルが **1 つの `reqwest::Client`**（backend 単位）を共有する。
4. フラット `[llm]` のみの既存設定が **変更なし** で動作する（暗黙 `default` / `default`）。`AIBE_LLM_*` env も legacy で従来どおり効く。
5. 新形式設定で `AIBE_LLM_*` env が **無視**される。
6. `agent_turn` の `llm_profile` 省略時は aibe の `default_profile` が使われる。
7. `agent_turn` の未知 `llm_profile` は `invalid_request` で返る（LLM 呼び出し前）。
8. プロファイル構築失敗は **起動失敗**（リクエスト時 `provider_error` にはならない）。
9. `ai ask --profile NAME` が wire に `llm_profile` を載せる。
10. `AI_LLM_PROFILE` が CLI より低優先で効く。
11. `[ask].default_profile` が env より低優先で効く。
12. 優先順位 4 のとき wire に `llm_profile` キーが **含まれない**。
13. `ai` / `aish` に API キー・provider 分岐が増えない。
14. `cargo test --workspace` / `clippy` / `check-architecture.sh` が通る。
15. `../aibe.config.example.toml` と manual が新形式を反映する。

## テスト

### 単体（`aibe` — `toml_config.rs`）

| テスト（想定名） | 観点 |
|-----------------|------|
| `parses_multi_backend_and_profiles` | 新形式の完全パース |
| `same_backend_two_profiles_different_models` | 同一 `llm` 参照・異なる model |
| `rejects_model_in_llm_backend_section` | LLM 定義への `model` 禁止 |
| `rejects_unknown_llm_reference_in_profile` | 存在しない `llm` 参照 |
| `rejects_mixed_flat_and_named_llm` | フラット `[llm]` と `[llm.x]` 混在 |
| `rejects_new_format_without_profiles` | `[llm.x]` のみで `[profiles.*]` 0 件 |
| `legacy_flat_llm_backward_compat` | 従来 `[llm]` → 暗黙 default |
| `env_ignored_for_named_llm_backends` | 新形式 + `AIBE_MODEL` 設定でも env 無視 |

### 統合（`aibe/tests/`）

| テスト（想定名） | 観点 |
|-----------------|------|
| `agent_turn_selects_profile_from_request` | mock LLM 2 プロファイルで model が切り替わる |
| `agent_turn_unknown_profile_invalid_request` | 未知名で LLM 未呼び出し |
| `generation_params_in_request_body` | wiremock で temperature / max_tokens 確認 |
| `profile_registry_startup_fails_on_bad_backend` | 不正 backend 参照で起動失敗 |

### ai

| テスト（想定名） | 観点 |
|-----------------|------|
| `cli_profile_overrides_env_and_config` | `--profile` > `AI_LLM_PROFILE` > config |
| `env_profile_overrides_config_default` | `AI_LLM_PROFILE` > `[ask].default_profile` |
| `aibe_client_omits_llm_profile_when_unset` | 優先順位 4 で JSON にキーなし |
| `aibe_client_sends_llm_profile` | 指定時 JSON に `llm_profile` が含まれる |

### 手動

- [../manual/llm-profiles.md](../manual/llm-profiles.md)（実装時に追加）

## 実装マップ

| ファイル | 変更 |
|----------|------|
| `aibe/src/ports/outbound/config.rs` | `LlmBackend`, `LlmProfile`, `LlmProfilesConfig`, `ProfileRegistry` |
| `aibe/src/adapters/outbound/toml_config.rs` | 2 段パース・legacy 判定・env ルール |
| `aibe/src/adapters/outbound/llm_factory.rs` | `BackendContext`, `build_profile_registry` |
| `aibe/src/adapters/outbound/openai_compatible.rs` | backend 共有 + `temperature`, `max_output_tokens` |
| `aibe/src/adapters/outbound/gemini.rs` | 同上（generationConfig） |
| `aibe/src/application/server.rs` | `ProfileRegistry` 注入 |
| `aibe/src/application/request_service.rs` | リクエストごとのプロファイル選択 |
| `aibe/src/protocol/request.rs` | `llm_profile` フィールド |
| `aibe/src/lib.rs` | 起動時レジストリ構築 |
| `ai/src/main.rs` | `--profile`, 優先順位解決 |
| `ai/src/application/ask.rs` | `AskRunOptions.resolved_profile` |
| `ai/src/adapters/outbound/toml_config.rs` | `[ask].default_profile` |
| `ai/src/adapters/outbound/aibe_client.rs` | `llm_profile` 送信 |
| `ai/src/domain/ask.rs` | `AskInput` / `AskRequest.llm_profile` |
| `../aibe.config.example.toml` | 新形式例 |
| `../architecture.md` | 設定・プロトコル節 |
| `0010_gemini-provider-spec.md` | env 節に legacy 限定の脚注 |
| `../manual/llm-profiles.md` | 手動検証 |
| `../0000_spec-index.md` | 0011 行 |
| `AGENTS.md` | 0011 行 |

## 0010 / 0003 / 0008 との関係

| ドキュメント | 関係 |
|-------------|------|
| **0010** | Gemini adapter はそのまま。`model` / 生成パラメータはプロファイルから注入。`provider_extras` 等は不変。**env 上書き**は legacy フラット `[llm]` に限定（0010 本文へ脚注追記）。 |
| **0003** | `context.cwd` 必須規則は不変。LLM 呼び出し前に cwd / tool 検証。 |
| **0008** | wire の `messages` / `ToolCall` は不変。追加は `llm_profile` のみ（任意フィールド）。 |

## docs 同期（実装 PR に含める）

- `../architecture.md` — 設定 2 段構造、プロトコル `llm_profile`、env ルール
- `../aibe.config.example.toml` — 新形式を正とし、legacy をコメントで記載
- `0010_gemini-provider-spec.md` — env 節に「0011 以降 legacy 限定」の脚注
- `../manual/llm-profiles.md` — 手動検証手順
- 本ファイルの **状態** を「実装済み」に更新

## 未確定（実装前に決めてよい細部）

- `top_p` 等の追加パラメータは第 2 段階（本 PR では `temperature` / `max_output_tokens` のみ）
- プロファイル名・backend 名の命名規則（英数字 + `_` `-` のみ許可するか）は実装時にパーサで固定

## レビュー反映履歴（Codex 2026-05-24）

| 指摘 | 対応 |
|------|------|
| `provider_error` と起動失敗の矛盾 | eager 構築に統一。構築失敗は起動失敗のみ |
| `ai` 送信経路の未定義 | 「ai 側データフロー」節を追加 |
| 0010 env との衝突 | legacy 限定を明記。0010 へ脚注追記を実装マップに追加 |
| `profiles` 空の曖昧さ | 「設定読み込みの判定順」節で legacy / 新形式 / 無効を分離 |
| `TerminationCapability` 注入点 | `ProfileRegistry` 責務表で profile 名 → capability を明記 |
| HTTP 共有の受け入れ条件不足 | 受け入れ条件 3 + 「HTTP クライアント共有（具体）」節 |
| ai 優先順位テスト不足 | 受け入れ条件 9–12、ai テスト表を拡充 |
