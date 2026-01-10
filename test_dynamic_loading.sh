#!/bin/bash
# 動的な記憶読み込みの動作確認テストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# AISH環境のシミュレーション
export AISH_HOME="$(pwd)/_aish"
export AISH_SESSION="$TEST_DIR/session"
mkdir -p "$AISH_SESSION/part"
export AISH_PART="$AISH_SESSION/part"

# ライブラリのパス
QUERY_ENTRY_LIB="_aish/lib/query_entry.sh"
MEMORY_LIB="_aish/lib/memory_manager.sh"

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

# 記憶を事前に作成
setup_test_memories() {
    local memory_dir="$TEST_DIR/memory"
    mkdir -p "$memory_dir"
    
    # 共通関数を読み込むためにAISH_HOMEを設定
    (
        export AISH_HOME="$(pwd)/_aish"
        source "$MEMORY_LIB"
        init_memory_directory "$memory_dir"
        save_memory "To fix permission denied for scripts, use chmod +x <script_name>." "error_solution" "permission,chmod,denied" > /dev/null
    )
}

# ダミーのaish_rollout関数（query_entry.shで呼ばれる）
aish_rollout() {
    :
}
export -f aish_rollout

# ダミーのdetail.aish_list_parts関数
detail.aish_list_parts() {
    echo ""
}
export -f detail.aish_list_parts

# ダミーのdetail.aish_security_check関数
detail.aish_security_check() {
    cat
}
export -f detail.aish_security_check

# テスト1: 関連する記憶がシステムインストラクションに注入されるか
test_memory_injection() {
    test_case "Memory Injection into System Instruction"
    
    local memory_dir="$TEST_DIR/memory"
    setup_test_memories
    
    # query_entry_prepare を実行
    # 本来は AISH_HOME/lib から読み込まれるが、テスト用に直接sourceする
    (
        export AISH_HOME="$(pwd)/_aish"
        # 記憶ディレクトリを指すように細工（find_memory_directoryがここを見つけるようにする）
        export AISH_HOME_MEMORY="$memory_dir" 
        # project-specific memoryとして認識させるためにカレントディレクトリに .aish/memory を作る
        mkdir -p "$TEST_DIR/.aish/memory"
        cp -r "$memory_dir/"* "$TEST_DIR/.aish/memory/"
        
        cd "$TEST_DIR"
        source "$AISH_HOME/lib/memory_manager.sh"
        source "$AISH_HOME/lib/query_entry.sh"
        
        # クエリを実行
        query_entry_prepare -s "Original instruction" "How to fix permission denied?"
        
        # インストラクションに記憶が含まれているか確認
        if [[ "$_query_system_instruction" == *"Relevant Knowledge"* ]] && [[ "$_query_system_instruction" == *"chmod +x"* ]]; then
            echo "Modified instruction: $_query_system_instruction"
            log_info "✓ Successfully injected relevant memory into system instruction"
            return 0
        else
            log_error "✗ Memory was not injected"
            echo "Current instruction: $_query_system_instruction"
            return 1
        fi
    )
    
    if [ $? -eq 0 ]; then
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Dynamic Loading Test Suite"
    echo "========================================="
    
    test_memory_injection || true
    
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
        log_info "All dynamic loading tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some tests failed. ✗"
        exit 1
    fi
}

main "$@"

