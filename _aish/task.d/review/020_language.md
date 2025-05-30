## 対象言語と考慮事項

このセクションは、レビュー対象のコードのプログラミング言語と、その言語固有の考慮事項に関する情報を提供します。この情報を用いて、言語の特性やベストプラクティスに基づいた適切なコードレビューを実行してください。


---

### 1. レビュー対象のプログラミング言語

*   **指示:** レビュー対象のコードが書かれているプログラミング言語を識別してください。
*   (例: Python)
*   (例: JavaScript)
*   (例: Java)

### 2. その言語に固有の考慮事項とベストプラクティス

*   **指示:** 上記で指定されたプログラミング言語において、コードレビュー時に特に考慮すべき一般的な事項や、その言語コミュニティで広く受け入れられているベストプラクティス、イディオム、あるいはよくある落とし穴があれば、それらを考慮してください。
*   (例:
    *   **Pythonの場合:** PEP 8スタイルガイドの推奨事項（詳細は`rules.md`参照）。Pythonicなコード記述（例: リスト内包表記、ジェネレータ）。適切なエラーハンドリングと例外処理。リソース管理（`with`文など）。
    *   **JavaScriptの場合:** 非同期処理（Promise, async/await）の正しい使い方。スコープとクロージャの理解。厳密等価演算子（`===`）の使用。一般的なリンティングルールの考慮（詳細は`rules.md`参照）。
    *   **Javaの場合:** オブジェクト指向設計原則の適用。適切な例外処理とロギング。null安全性の考慮。スレッド安全性（並行処理）。標準ライブラリの効果的な利用。)

### 3. 特定のライブラリやフレームワークに関する考慮事項 (任意)

*   **指示:** もしレビュー対象のコードが特定の重要なライブラリやフレームワークに強く依存しており、その使用に関する特有の注意点や推奨事項があれば、それらを考慮してください。
*   (例: Reactを使用している場合、Hooksの正しい使い方、コンポーネントの設計原則。Djangoを使用している場合、ORMの効率的な使い方、セキュリティ対策。)
*   **[ここに、特定のライブラリやフレームワークに関する考慮事項を記述してください。該当しない場合はこの項目を削除しても構いません。]**

---

上記の言語固有の情報を踏まえ、コードレビューを実行してください。
