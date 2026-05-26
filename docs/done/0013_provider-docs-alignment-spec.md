# 0013 — provider 表記と OpenAI 公式 API の docs 整合指示書 — 仕様ドラフト

> **出典**: ChatGPT レビュー 4 代目の P0 / Sprint 1 第 2 項目。`docs/todo/chatgpt-review-4th-gen/concerns.md` §1、`p0-stabilization.md`、`implementation-order.md`、`verification.md` を前提にした実装前仕様化。  
> **状態**: **実装済み**（2026-05-26。Codex レビュー反映: 索引 §5・残存検索 §6・確認対象の明記）

## 目的

`provider = "openai"` が別 provider であるかのような誤期待を防ぎ、設定ミスとセキュリティ事故の芽を減らす。P0 では **新機能を足さず**、既存の実装正本に合わせて docs を整えることだけを狙う。

この指示書は、次を明確にする。

- 実装の正本は `aibe/src/adapters/outbound/toml_config.rs` の `parse_provider_kind` と `parse_backend_fields`
- 公式 OpenAI API も provider 名は `openai_compatible` で扱う
- `https://api.openai.com/v1` はその実装の既定 `base_url` と一致させて説明する
- `provider = "openai"` の alias 実装は **しない**

## スコープ

### 対象

- `README.md` の Supported LLM providers 表と関連説明
- `docs/architecture.md` の LLM provider 節と設定説明
- `docs/aibe.config.example.toml` の例示コメントと provider 表記
- `docs/manual/aibe-openai-compatible.md`
- `docs/manual/ai-ask-tools.md`
- `docs/manual/README.md`
- `docs/testing.md`
- `docs/0000_spec-index.md`（0013 の状態を実装済みに更新）

### 残存確認対象（変更不要ならスキップ可）

実装の主対象外だが、表記の取りこぼしがないか `rg` で確認する。

- `AGENTS.md`
- `docs/security.md`
- `docs/manual/gemini-provider.md`
- `docs/done/*`（過去指示書の歴史的記述は変更しない）

### 対象外

- `aibe/src/adapters/outbound/toml_config.rs` の Rust 実装変更
- `provider = "openai"` alias の追加
- `parse_provider_kind` の受け入れ値追加やエラーメッセージ変更
- API キーや実設定の追加
- 仕様外の新 provider 名や互換表記の増殖
- この項目単独でのテストコード追加
- `docs/todo/chatgpt-review-4th-gen/**` のバックログ文言（意図的に `openai` を残す）

## 確定した設計判断

| 項目 | 方針 | 理由 |
|------|------|------|
| **canonical provider 名** | docs 上の正本は `openai_compatible` に統一する | 実装の `parse_provider_kind` が既にその名前を受けるため |
| **OpenAI 公式 API の扱い** | 公式 OpenAI API も `openai_compatible` として説明する | `api_key` と `base_url` を渡すだけでよく、別 provider を増やす必要がない |
| **既定 base_url** | `https://api.openai.com/v1` を正として明記する | 実装の `parse_backend_fields` の既定値と一致させる |
| **`openai` alias** | 実装しない。docs でも別 provider のように書かない | P0 では新機能を足さず、誤期待の温床を消すことを優先する |
| **エラーメッセージ** | Rust の `unknown llm provider: ...` は変更しない | この項目は docs 整合が主であり、文言変更は別判断に分離する |
| **表記方針** | docs では `openai_compatible` を優先し、`openai-compatible` は前面に出さない | 省略不能な互換説明を減らし、設定例の一貫性を高める |
| **境界** | この項目は docs の整合に限定する | セキュリティ優先の P0 でも、コード変更は別タスクに分ける方が安全 |

> **推測**: README の `openai` 行は「OpenAI 公式 API」の意図で書かれた可能性が高い。だが、現行実装の正本に合わせるなら、その意図を `openai_compatible` に言い換えるべきである。

## 受け入れ条件

### 1. README の provider 表が実装と一致する

- `README.md` の provider 表から `openai` 行が消える
- 代わりに `openai_compatible` が canonical として説明される
- 「OpenAI 公式 API も `openai_compatible`」と明記される

### 2. architecture の正本説明が一致する

- `docs/architecture.md` の LLM provider 節が `parse_provider_kind` の受け入れ値と矛盾しない
- 既定 `base_url` が `https://api.openai.com/v1` と説明される
- `openai` を別 provider と読める文言が残らない

### 3. example config が canonical 表記に揃う

- `docs/aibe.config.example.toml` で OpenAI 系の例示が `openai_compatible` に統一される
- OpenAI 公式 API を使うときの `provider` / `api_key` / `base_url` の関係が読み取れる
- `openai` という provider 名の例示を新たに増やさない

### 4. manual / testing の案内が一貫する

- `docs/manual/aibe-openai-compatible.md` が canonical provider 名を前提に説明する
- `docs/manual/ai-ask-tools.md` と `docs/manual/README.md` が、OpenAI 公式 API の扱いを `openai_compatible` に寄せる
- `docs/testing.md` の manual 参照が、誤った provider 名を補強しない

### 5. 索引が実装済みと整合する

- `docs/0000_spec-index.md` に 0013 の行があり、状態が **実装済み** である
- 指示書本体は `docs/done/0013_provider-docs-alignment-spec.md` に移す（完了後）

### 6. 残存検索が通る

次のコマンドで、ユーザー向け docs に誤った provider 表記が残らないこと（ヒット 0 件）:

```bash
rg -n --glob '*.md' --glob '*.toml' \
  --glob '!docs/done/**' \
  --glob '!docs/todo/**' \
  --glob '!docs/0013_provider-docs-alignment-spec.md' \
  --glob '!docs/done/0013_provider-docs-alignment-spec.md' \
  'provider\s*=\s*"openai"|\| `openai` \|' \
  README.md docs AGENTS.md
```

- `openai_compatible` やファイル名 `aibe-openai-compatible` は残ってよい
- `docs/todo/chatgpt-review-4th-gen/**` はバックログのため除外する

## docs 更新一覧

- `README.md`
  - provider 表から `openai` 行を削除する
  - OpenAI 公式 API も `openai_compatible` で使うと明記する
- `docs/architecture.md`
  - LLM provider の説明を canonical 表記に合わせる
  - `base_url` の既定値を正しく説明する
- `docs/aibe.config.example.toml`
  - OpenAI 系の例示コメントを canonical 表記に揃える
  - 必要なら「公式 OpenAI API も `openai_compatible`」の補足を足す
- `docs/manual/aibe-openai-compatible.md`
  - 公式 OpenAI API を含む OpenAI 系の前提を canonical 表記に寄せる
- `docs/manual/ai-ask-tools.md`
  - OpenAI 系 manual 参照の文言を canonical 表記に合わせる
- `docs/manual/README.md`
  - manual 一覧の説明が `openai_compatible` の説明と整合するようにする
- `docs/testing.md`
  - 手動検証の案内が誤った provider 名を補強しないようにする
- `docs/0000_spec-index.md`
  - 0013 を実装済みとして `done/` に登録する

## 実装手順の目安

1. `feature/v0.1-stabilization` ブランチで着手する
2. `README.md` と `docs/architecture.md` から先に、canonical provider 名と既定 `base_url` の説明を揃える
3. `docs/aibe.config.example.toml` と manual 類を同期する
4. `docs/0000_spec-index.md` を更新し、指示書を `docs/done/` へ移す
5. docs-only の変更なので、この項目単独では `cargo fmt --all -- --check` / `cargo clippy --workspace -- -D warnings` / `cargo test --workspace` / `./scripts/check-architecture.sh` は **対象外** とする
6. 代わりに、§6 の `rg` と人手の diff 確認で整合を確認する

## 未確定・見送り

| 種別 | 内容 |
|------|------|
| **見送り** | `provider = "openai"` alias の追加はしない |
| **見送り** | `parse_provider_kind` の受け入れ値やエラーメッセージの変更はしない |
| **見送り** | `openai-compatible` を docs の主表記にはしない。canonical は `openai_compatible` に固定する |
| **見送り** | この項目単独での Rust テスト追加はしない |
| **見送り** | API キーや実設定の新規例は追加しない |

## 残リスク

- docs と実装の差分は、今後 provider 周りに変更が入ると再び発生しうる
- `docs/manual/*` は手動検証用なので、実運用の変化に追随できない期間が生じる
- `openai-compatible` の hyphen 表記は実装ではまだ受けるため、docs から完全に消してもコードとの表現差は残る
