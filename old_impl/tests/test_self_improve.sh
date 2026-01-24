#!/bin/bash
# 自己改善（履歴からの知見抽出・記録）の動作確認テストスクリプト

set -euo pipefail

# プロジェクトルートに移動
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# AISH環境のシミュレーション
export AISH_HOME="$PROJECT_ROOT/_aish"
export AISH_SESSION="$TEST_DIR/session"
mkdir -p "$AISH_SESSION/part"
export AISH_LOGFILE="$AISH_SESSION/log.jsonl"

# ライブラリのパス
SELF_IMPROVE_LIB="$PROJECT_ROOT/_aish/lib/self_improve.sh"
MEMORY_LIB="$PROJECT_ROOT/_aish/lib/memory_manager.sh"

# テスト結果のカウント
TESTS_PASSED=0
TESTS_FAILED=0

# ヘルパー関数
log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

test_case() {
    local name="$1"
    echo ""
    echo "========================================="
    echo "Test: $name"
    echo "========================================="
}

# 擬似的なログファイルを生成
create_mock_log() {
    cat <<EOF > "$AISH_LOGFILE"
{"role": "user", "content": "How do I fix the 'permission denied' error when running test.sh?"}
{"role": "assistant", "content": "You need to add execute permissions to the file.", "tool_calls": [{"id": "call_1", "function": {"name": "execute_shell_command", "arguments": "{\"command\": \"chmod +x test.sh\"}"}}]}
{"role": "tool", "tool_call_id": "call_1", "content": "{\"exit_code\": 0, \"stdout\": \"\", \"stderr\": \"\"}"}
{"role": "assistant", "content": "I have added execute permissions to test.sh. You can now run it with ./test.sh."}
EOF
}

# テスト1: ログの抽出
test_log_extraction() {
    test_case "Log Extraction - extract relevant interactions"
    
    create_mock_log
    
    # self_improve.sh がまだ存在しないか、関数が未定義の場合は失敗することを期待
    if [ ! -f "$SELF_IMPROVE_LIB" ]; then
        log_error "Library not found: $SELF_IMPROVE_LIB"
        return 1
    fi
    
    source "$SELF_IMPROVE_LIB"
    
    local log_summary=$(extract_session_history "$AISH_LOGFILE")
    
    if [[ "$log_summary" == *"chmod +x test.sh"* ]]; then
        log_info "✓ Successfully extracted relevant command from logs"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to extract command from logs"
        echo "Log summary: $log_summary"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト2: 自己改善プロセスの実行（モックLLMを使用）
test_self_improve_execution() {
    test_case "Self-Improve Process Execution"
    
    create_mock_log
    
    # 記憶ディレクトリの初期化
    local memory_dir="$TEST_DIR/memory"
    # AISH_HOMEをテスト用ディレクトリに設定し、ライブラリを再読み込み
    export AISH_HOME_TEST="$TEST_DIR/.aish"
    mkdir -p "$AISH_HOME_TEST/memory"
    
    (
        export AISH_HOME="$AISH_HOME_TEST"
        source "$MEMORY_LIB"
        init_memory_directory "$AISH_HOME/memory"
    )
    
    # 擬似的なLLM応答を返す query 関数のモック
    query() {
        echo '{"content": "To fix permission denied for scripts, use chmod +x <script_name>.", "category": "error_solution", "keywords": ["permission", "chmod", "bash"]}'
    }
    export -f query
    
    # 実行（AISH_HOMEを偽装して実行）
    # プロジェクトの.aish/memoryを拾わないように一時ディレクトリに移動して実行
    (
        mkdir -p "$TEST_DIR/work"
        cd "$TEST_DIR/work"
        export AISH_HOME="$AISH_HOME_TEST"
        source "$SELF_IMPROVE_LIB"
        run_self_improvement "$AISH_LOGFILE" "$AISH_HOME/memory"
    )
    
    # 記憶が保存されたか確認
    if [[ $(ls "$AISH_HOME_TEST/memory/entries/"*.json 2>/dev/null | wc -l) -gt 0 ]]; then
        log_info "✓ Memory was automatically saved from session history"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ No memory was saved in $AISH_HOME_TEST/memory"
        ls -R "$AISH_HOME_TEST"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Self-Improvement Test Suite"
    echo "========================================="
    
    # ライブラリ読み込みテスト
    if [ ! -f "$SELF_IMPROVE_LIB" ]; then
        log_error "Library $SELF_IMPROVE_LIB does not exist. Implementation required."
        # TDD: 一度失敗させる
    fi
    
    test_log_extraction || true
    test_self_improve_execution || true
    
    # 結果サマリー
    echo ""
    echo "========================================="
    echo "Test Summary"
    echo "========================================="
    echo "Passed: $TESTS_PASSED"
    echo "Failed: $TESTS_FAILED"
    echo "Total:  $((TESTS_PASSED + TESTS_FAILED))"
    
    if [ $TESTS_FAILED -eq 0 ] && [ $TESTS_PASSED -gt 0 ]; then
        echo ""
        log_info "All self-improvement tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some tests failed. ✗"
        exit 1
    fi
}

main "$@"

