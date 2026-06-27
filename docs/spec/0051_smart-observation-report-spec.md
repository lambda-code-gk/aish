# 0051 — Smart Preprocessor Observation Report 設計書

## 0. 目的

既存の append-only observation.jsonl を read-only で集計し、Smart Preprocessor の利用傾向、fallback、latency、hint 注入、推定削減量を人間と AI が評価できる CLI を ai crate に追加する。

## 1. 非目標

- Smart Preprocessor の分類、gate、short-circuit 条件の変更
- observation schema や書き込み経路の変更
- raw user text の読み取り DTO への追加または出力
- aibe protocol の変更

## 2. パック構成の適用

No。単一クライアント内の軽量な read-only reader と CLI であり、core service への横断 hook、optional runtime、重い依存、別デプロイ単位のいずれもない。通常の domain / outbound adapter 分離で実装する。

## 3. CLI

- ai smart stats: 既定 1000 非空行を対象に tsv/json/env で集計する
- ai smart recent: 既定 20 非空行を安全な既知フィールドだけで一覧化する
- ai smart report: 集計と recent を Markdown で出す
- path は既定 observation path。先頭の ~/ は既存 config と同じ HOME 規則で展開する
- limit は末尾の非空物理行へ先に適用し、その範囲内で invalid_lines を数える
- session / since-hours は正常に parse できた行へ適用する

## 4. 安全性・互換性

Deserialize DTO は既知フィールドだけを宣言し、未知フィールドを無視する。したがって将来 schema と raw 相当の未知フィールドは再出力されない。不正 JSON と不正 UTF-8 は invalid_lines として扱い、他の正常行を維持する。TSV/ENV/Markdown の制御文字を単一行へ正規化する。ログは一切更新しない。

## 5. 集計規則

latency の avg/p50/p95 は 0 より大きいサンプルだけを使う。percentile は nearest-rank、空サンプルは 0、JSON timestamp 欠損は null、TSV/ENV/Markdown は 0 とする。distribution は値が存在する既知フィールドだけを数える。

## 6. 受け入れ条件

1. reader が空、欠損、不正行、未知・欠損フィールド、末尾 limit を安全に処理する
2. stats が distribution、route/local counts、token、hint、LLM call、latency を集計する
3. session と since cutoff が正常行へ適用される
4. stats JSON/TSV と recent JSON が実 CLI で動く
5. report が Markdown を出し、未知の raw user text 相当値を含めない
6. docs と受け入れ条件レジストリを同期し verify を通す
