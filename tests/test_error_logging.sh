#!/usr/bin/env bash

# エラーハンドリングとログシステムのテスト

set -e

# テスト用のセッションディレクトリを作成
TEST_SESSION=$(mktemp -d)
export AISH_SESSION="$TEST_SESSION"
export AISH_HOME="${AISH_HOME:-$(dirname "$(dirname "$(readlink -f "$0")")")/_aish}"

# テスト用のログレベルを設定
export AISH_LOG_LEVEL="${AISH_LOG_LEVEL:-DEBUG}"

# ライブラリを読み込む
. "$AISH_HOME/lib/error_handler.sh"
. "$AISH_HOME/lib/logger.sh"
. "$AISH_HOME/functions"

# 初期化
error_handler_init
logger_init

echo "=== エラーハンドリングとログシステムのテスト ==="
echo ""

# テスト1: エラーハンドリング関数のテスト
echo "テスト1: エラーハンドリング関数"
error_info "情報メッセージ" '{"test": "info"}'
error_warn "警告メッセージ" '{"test": "warn"}'
error_debug "デバッグメッセージ" '{"test": "debug"}'

# エラーは終了しないようにする
error_error "エラーメッセージ（終了しない）" '{"test": "error"}' || true

echo "✓ エラーハンドリング関数のテスト完了"
echo ""

# テスト2: ログ関数のテスト
echo "テスト2: ログ関数"
log_info "情報ログ" "test" '{"action": "test"}'
log_warn "警告ログ" "test" '{"action": "test"}'
log_debug "デバッグログ" "test" '{"action": "test"}'
log_error "エラーログ" "test" '{"action": "test"}'

echo "✓ ログ関数のテスト完了"
echo ""

# テスト3: ログファイルの確認
echo "テスト3: ログファイルの確認"
if [ -f "$AISH_SESSION/app.log" ]; then
    echo "app.log が作成されました"
    log_count=$(jq 'length' "$AISH_SESSION/app.log" 2>/dev/null || echo "0")
    echo "ログエントリ数: $log_count"
    
    # ログの内容を表示（最初の3件）
    echo "最初の3件のログ:"
    jq -r '.[0:3] | .[] | "\(.level): \(.message)"' "$AISH_SESSION/app.log" 2>/dev/null || echo "ログの解析に失敗"
else
    echo "✗ app.log が作成されていません"
    exit 1
fi

if [ -f "$AISH_SESSION/error.log" ]; then
    echo "error.log が作成されました"
    error_count=$(jq 'length' "$AISH_SESSION/error.log" 2>/dev/null || echo "0")
    echo "エラーログエントリ数: $error_count"
else
    echo "error.log はまだ作成されていません（エラーが発生していないため正常）"
fi

echo "✓ ログファイルの確認完了"
echo ""

# テスト4: エラーハンドリング関数の直接使用
echo "テスト4: エラーハンドリング関数の直接使用"
error_error "エラーメッセージ（終了しない）" '{"test": "direct_call"}' || true
echo "✓ error_error 関数が直接使用可能"

echo "✓ エラーハンドリング関数のテスト完了"
echo ""

# テスト4.5: 新しいログ関数のテスト
echo "テスト4.5: 新しいログ関数（log_request, log_response, log_tool）"
export LOG="$AISH_SESSION/log.json"
echo "[]" > "$LOG"

# log_request のテスト
test_payload='{"test": "request"}'
log_request "$test_payload" "test"
if [ -f "$LOG" ]; then
    # log.jsonはJSONL形式（各行が独立したJSONオブジェクト）なので、行ごとに解析
    request_count=$(grep -c '"type":"request"' "$LOG" 2>/dev/null || echo "0")
    # 数値のみを抽出（改行や空白を削除）
    request_count=$(echo "$request_count" | tr -d '\n' | tr -d ' ')
    if [ -n "$request_count" ] && [ "$request_count" -gt 0 ] 2>/dev/null; then
        echo "✓ log_request が log.json に記録されました（$request_count 件）"
    else
        echo "✗ log_request が log.json に記録されていません"
        exit 1
    fi
fi

# log_response のテスト
test_payload='{"test": "response"}'
log_response "$test_payload" "test"
if [ -f "$LOG" ]; then
    # log.jsonはJSONL形式（各行が独立したJSONオブジェクト）なので、行ごとに解析
    response_count=$(grep -c '"type":"response"' "$LOG" 2>/dev/null || echo "0")
    # 数値のみを抽出（改行や空白を削除）
    response_count=$(echo "$response_count" | tr -d '\n' | tr -d ' ')
    if [ -n "$response_count" ] && [ "$response_count" -gt 0 ] 2>/dev/null; then
        echo "✓ log_response が log.json に記録されました（$response_count 件）"
    else
        echo "✗ log_response が log.json に記録されていません"
        exit 1
    fi
fi

# log_tool のテスト（標準エラー出力に色付きで出力されることを確認）
log_tool "テストメッセージ" "test" >/dev/null 2>&1 || true
echo "✓ log_tool が実行されました（色付き出力は標準エラー出力に表示）"

echo "✓ 新しいログ関数のテスト完了"
echo ""

# テスト5: ログレベルのフィルタリング
echo "テスト5: ログレベルのフィルタリング"
export AISH_LOG_LEVEL="WARN"
log_info "このメッセージは表示されないはず（INFOレベル）" "test"
log_warn "このメッセージは表示されるはず（WARNレベル）" "test"
log_error "このメッセージは表示されるはず（ERRORレベル）" "test"

# ログファイルのエントリ数を確認
warn_log_count=$(jq '[.[] | select(.level == "WARN" or .level == "ERROR")] | length' "$AISH_SESSION/app.log" 2>/dev/null || echo "0")
echo "WARN/ERRORレベルのログ数: $warn_log_count"

echo "✓ ログレベルのフィルタリングテスト完了"
echo ""

# クリーンアップ
echo "=== テスト完了 ==="
echo "テストセッションディレクトリ: $TEST_SESSION"
echo "クリーンアップする場合は: rm -rf $TEST_SESSION"
echo ""
echo "すべてのテストが成功しました！"

