# 0045 — パック構成（Pack Composition）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-21  
> **関連**: [0038_contextual-memory-pack-phase-b-spec.md](0038_contextual-memory-pack-phase-b-spec.md)、[0038_contextual-memory-pack-phase-d-spec.md](0038_contextual-memory-pack-phase-d-spec.md)、[0043_feature-pack-boundary-hardening-spec.md](0043_feature-pack-boundary-hardening-spec.md)、[architecture.md](../architecture.md)、[AGENTS.md](../../AGENTS.md)

## 0. 目的

本書は **パック構成（Pack Composition）** という機構そのものを定義する。Contextual Memory Pack（0038）で実証済みのパターンを、今後の optional 機能開発の **共通雛形** として再利用できるようにする。

パック構成の目的は次のとおり。

1. optional 機能を **core から切り離し**、無効時も basic runtime を壊さない
2. **同一プロセス内** で機能束を差し替え可能にする（動的ロードは不要）
3. **ランタイム切替**（設定）と **コンパイル時パッケージング**（Cargo feature）を **別軸** として明示する
4. 新機能設計時に「パック構成が適用できるか」を機械的に検討できるチェックリストを提供する

## 1. 非目標

- `dlopen` / `libloading` による **動的プラグインロード**
- サードパーティがバイナリ実行時に差し込む **拡張 API**
- wire protocol（`aibe-protocol`）の breaking change を前提とした設計
- すべての機能をパック化すること（core に残すべきものの判定も本書の範囲）

**用語上の非目標**: 本機構を「プラグイン機構」と呼ばない。動的ロード・外部拡張を連想させ、実態（静的合成）とずれるため。

## 2. 用語

| 用語（英） | 日本語 | 意味 |
|------------|--------|------|
| **Pack Composition** | **パック構成** | optional 機能束を composition root で選び配線する機構全体 |
| **Pack** | **パック** | 関連する機能群をまとめて差し替える in-process の単位 |
| **Pack Boundary** | **パック境界** | core が optional 機能に依存する trait 接点（例: `TurnHook`, `RpcExtension`） |
| **Composition Root** | **構成ルート** | pack を選び具体実装を注入する唯一の場所 |
| **Active Pack** | **Active Pack** | 機能有効時の具体 pack 実装 |
| **Basic Pack** | **Basic Pack** | 無効時の no-op / fail-closed 実装（Null Object） |
| **Runtime Toggle** | **ランタイム切替** | 設定 `[<name>] enabled` + 環境変数による ON/OFF（同一バイナリ） |
| **Compile-time Packaging** | **コンパイル時パッケージング** | Cargo feature + `#[cfg(feature = "...")]` によるリンク単位の除外 |

### 2.1 混同しない用語

| 用語 | 意味 | パック構成との関係 |
|------|------|-------------------|
| **Contextual Memory Pack** | contextual memory 機能の具体 pack | パック構成の **参照実装** |
| **Feature Pack**（0043） | smart feature の registry / eligibility（`features.toml` 等） | **別概念**。routing 用の定義束。パック構成機構そのものではない |
| **`plugin_memory/`** | memory 実装の in-crate モジュール名 | 歴史的命名。機構名は **パック構成** を使う |

## 3. パック構成の考え方

### 3.1 本質

パックは「**同じ機能群をまとめて差し替える in-process の単位**」である（0038 Phase B より）。

core（`RequestService`, `AgentTurnService`, CLI dispatch 等）は pack の **trait のみ** を consume し、optional 機能の有無・具体実装・設定参照は **composition root に閉じる**。

```text
[Composition Root]  ← enabled / cfg(feature) をここだけで参照
        │
        ├─ Active Pack  （機能有効）
        └─ Basic Pack   （無効: no-op / fail-closed）
                │
                ▼
        [Pack Boundary traits]  ← core が依存する唯一の接点
                │
                ▼
        [Core Services]  ← pack 名・enabled bool を知らない
```

### 3.2 2 軸（混同禁止）

| 軸 | 手段 | 効果 | 例 |
|----|------|------|-----|
| **ランタイム切替** | config + env | 同一バイナリ内で Active ↔ Basic | `[memory] enabled = false` |
| **コンパイル時パッケージング** | Cargo feature | optional 実装を dependency graph から除外 | `cargo build --no-default-features` |

両方を独立に設計する。ランタイム OFF でも Active Pack のコードがリンクされる build と、feature OFF で実装ごと外す build は **別の運用形態** である。

### 3.3 プラグインとの違い

| 観点 | 一般的なプラグイン | パック構成 |
|------|-------------------|-----------|
| ロード | 実行時に外部から差し込み | 起動前に composition root が **静的配線** |
| 所在 | 別プロセス / `.so` / 別バイナリが多い | **同一プロセス**（現状は同一 crate 内モジュールも可） |
| 無効化 | 未インストール | **Basic Pack** で core 契約を維持 |
| 切替 | ホットスワップもあり得る | **再起動**（設定変更 or 再ビルド）前提 |

## 4. 構成要素

### 4.1 Pack 境界（trait）

optional 機能が core に割り込む接点を **trait** として `ports`（または同等の境界モジュール）に定義する。

要件:

- core service は trait object（`Arc<dyn ...>`）のみ保持する
- trait は **最小 surface** に留める（1 機能 = 1 束の trait が理想）
- wire DTO の変更は最後の手段。既存 protocol を維持し、pack 内で解釈する

参照（aibe / memory）:

- `TurnHook` — `agent_turn` の prompt 組み立てへの割り込み
- `RpcExtension` — memory 系 RPC 5 種の分岐

参照（ai / memory）:

- CLI policy 層（`MemoryCliPack` / `MemoryCommandPolicy`）— server pack の相棒

### 4.2 Composition Root

pack の選択・組み立て・注入は **1 か所** に集約する。

| クレート | 構成ルート（代表） | 責務 |
|---------|-------------------|------|
| **aibe** | `application/server.rs` | Active / Basic pack 選択、`RequestService` へ注入 |
| **ai** | `main.rs`（+ 薄い application facade） | CLI gate、client-side pack 選択 |

禁止:

- `RequestService` / `AgentTurnService` / 各 handler が `memory.enabled` 等を直接参照して分岐を増やす
- pack 選択ロジックの複数箇所への散在

### 4.3 Active Pack / Basic Pack

各 optional 機能は **最低 2 実装** を持つ。

| 実装 | 役割 |
|------|------|
| **Active Pack** | 機能有効時の本実装 |
| **Basic Pack** | 無効時: 注入 no-op、RPC/CLI fail-closed、registry empty 等 |

Basic Pack は **スタブ成功禁止**（`.cursor/rules/40-no-stubs.mdc`）。無効時は明示拒否または no-op で、core の契約を維持する。

### 4.4 実装の置き場所

現行の慣例（0038 Phase D）:

```text
<crate>/src/
  application/           # facade, BasicPack, composition 用 factory
  plugin_<name>/         # Active Pack 実装（#[cfg(feature = "<name>")]）
  ports/outbound/        # Pack 境界 trait
```

将来、循環依存が解消できれば optional crate（例: `aibe-pack-memory`）へ昇格可能。Phase D 時点では in-crate モジュールで十分である。

## 5. 参照実装: Contextual Memory Pack

0038 Phase A〜D がパック構成の参照実装である。

| Phase | 内容 | パック構成上の位置づけ |
|-------|------|------------------------|
| A | `[memory] enabled` | **ランタイム切替** |
| B | `TurnHook` / `RpcExtension`, `BasicPack` / `ContextualMemoryPack` | **Pack 境界 + Active/Basic** |
| C | `MemoryCliPack`（ai 側） | **client-side pack** |
| D | `memory` Cargo feature, `plugin_memory/` | **コンパイル時パッケージング** |

関連設計書:

- [0038 Phase A](0038_contextual-memory-pack-phase-a-spec.md)
- [0038 Phase B](0038_contextual-memory-pack-phase-b-spec.md)
- [0038 Phase C](0038_contextual-memory-pack-phase-c-spec.md)
- [0038 Phase D](0038_contextual-memory-pack-phase-d-spec.md)

## 6. 新機能開発時の適用検討

**新規機能の設計・実装指示書を書く前に**、次のチェックリストでパック構成の適用可否を検討する（AI エージェント含む）。結果は設計書の「パック構成の適用」節に **Yes / No / 部分適用** と理由を 1 段落で記載する。

### 6.1 適用候補になりやすい条件

次の **2 つ以上** に該当する場合、パック構成を **第一候補** とする。

- [ ] 無効化した basic runtime を **別プロファイル** として維持したい
- [ ] core service（`agent_turn`, `route_turn`, 共通 RPC dispatch 等）へ **横断的な割り込み** がある
- [ ] 専用 RPC / 専用 CLI / turn hook のいずれかを **束ねて** 外したい
- [ ] 依存が重く、**basic build からリンク除外** したい
- [ ] 将来、設定・クレート・デプロイ単位で **optional 配備** する可能性がある

### 6.2 パック構成を採用する場合の最低要件

1. **Pack 境界 trait** を定義し、core は trait のみ consume
2. **Active Pack** + **Basic Pack** の 2 実装
3. **Composition Root 1 か所** で enabled / feature を参照
4. **ランタイム切替** が必要なら config + env（命名: `[<feature>] enabled`、env は `<CRATE>_<FEATURE>_ENABLED` 等で統一）
5. **コンパイル時除外** が必要なら Cargo feature（default 有効 / `--no-default-features` で basic build）
6. disabled / feature-off の **拒否経路テスト**（fail-closed or no-op）
7. `docs/architecture.md` と当該機能 spec へ **パック構成の記載**

### 6.3 パック構成を採用しない／部分適用とする例

| 状況 | 判断 |
|------|------|
| 全クライアント・全 build で常に必要な core 機能 | core に残す |
| 単一関数・単一 adapter に閉じる軽量機能 | 通常の port/adapter で十分 |
| wire protocol 変更が pack 分離の主目的 | 先に protocol 設計。pack は二次 |
| `aish` に載せる機能 | **aish は LLM/aibe 非依存** のためパック構成対象外（クレート境界で除外） |
| smart feature registry のみ（0043） | **Feature Pack** として設定面分離。server pack 全体の差し替えが不要ならパック構成は部分適用 |

### 6.4 部分適用

次のように **境界だけ** パック構成に寄せることもある。

- ランタイム toggle のみ（compile-time 除外は不要）
- server-side pack のみ（client は薄い gate のみ）
- registry / config 面の分離のみ（0043 Feature Pack Config パターン）

部分適用時も設計書に **何を pack 化し、何を core に残すか** を明示する。

## 7. クレート別方針

| クレート | パック構成 |
|---------|-----------|
| **aibe** | server-side pack の主戦場。`TurnHook`, `RpcExtension` 等 |
| **ai** | client-side pack（CLI policy, context 解決等）。`aibe` 本体へ依存しない |
| **aish** | 対象外（シェル + ログのみ） |
| **aibe-protocol** | leaf。pack 境界 trait は置かない（DTO は必要最小限） |

## 8. テスト方針

パック構成を採用した機能は、最低限次をカバーする。

| テスト種別 | 内容 |
|-----------|------|
| **Basic Pack 単体** | no-op / fail-closed が core 契約を満たす |
| **Active Pack 単体** | pack 境界経由の代表経路 |
| **disabled 統合** | runtime OFF で injection なし・RPC/CLI 拒否 |
| **feature-off build**（該当時） | `--no-default-features` で basic build が compile し、拒否経路が動く |

既存: `aibe/tests/memory_disabled.rs`, `ai/tests/memory_disabled_cli.rs`, `aibe/tests/memory_pack_turn_hook.rs`

## 9. ドキュメント・ハーネス

| 更新対象 | 内容 |
|---------|------|
| 機能 spec | 「パック構成の適用」節（Yes/No/部分 + 理由） |
| `docs/architecture.md` | パック構成節または contextual memory 節から本書へリンク |
| `AGENTS.md` | 新機能開発時の検討義務 |
| `.cursor/rules/46-pack-composition.mdc` | Cursor 向け要約 |
| `scripts/codex-mcp-prompt.sh` | Codex サブエージェント向け要約 |

## 10. 受け入れ条件（本書）

- [x] 本設計書（`docs/spec/0045_pack-composition-spec.md`）が存在する
- [x] `docs/0000_spec-index.md` に 0045 が登録されている
- [x] `docs/architecture.md` にパック構成節があり本書へリンクしている
- [x] `AGENTS.md` に新機能開発時のパック構成検討が記載されている
- [x] AI ハーネス（Cursor rule / Codex prompt）に同旨の指示がある

## 11. 未確定・将来

- optional crate 分離（`aibe-pack-*`）は `aibe-core` leaf 化後に再検討（0038 Phase D 参照）
- `plugin_<name>/` ディレクトリ名を `pack_<name>/` へリネームするかは **別タスク**（互換性・大規模 diff のため本書では強制しない）
- パック構成採用の機械チェック（lint / architecture script）は将来追加可能。現時点は **設計書チェックリスト + レビュー** で担保する
