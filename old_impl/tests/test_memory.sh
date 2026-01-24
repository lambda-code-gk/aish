#!/bin/bash
# 記憶管理ライブラリの動作確認テストスクリプト (刷新版)

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)

# 実際のプロジェクトの記憶ディレクトリをバックアップ (省略または一時的な隔離)
# ...

# クリーンアップ関数
cleanup_test_data() {
    rm -rf "$TEST_DIR"
}

trap cleanup_test_data EXIT

# AISH_HOMEの設定（テスト用）
export AISH_HOME="$TEST_DIR/.aish"
mkdir -p "$AISH_HOME/memory"

# ライブラリのパス
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

# 関数を実行するヘルパー
run_with_lib() {
    local cmd="$@"
    bash -c "
        set -e
        source '$MEMORY_LIB'
        $cmd
    "
}

# テスト1: init_memory_directory
test_init_memory_directory() {
    test_case "init_memory_directory"
    local mem_dir="$TEST_DIR/init_test"
    
    run_with_lib "init_memory_directory '$mem_dir'"
    
    if [ -d "$mem_dir/entries" ] && [ -f "$mem_dir/metadata.json" ]; then
        log_info "✓ Correct directories created"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        log_error "✗ Initialization failed"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# テスト2: save_memory and structure
test_save_memory_structure() {
    test_case "save_memory structure"
    local proj_dir="$TEST_DIR/proj"
    mkdir -p "$proj_dir/.aish/memory"
    
    local res=$(run_with_lib "cd '$proj_dir' && save_memory 'test content' 'cat' 'kw' 'subject'")
    local id=$(echo "$res" | jq -r '.memory_id')
    
    if [ -f "$proj_dir/.aish/memory/entries/$id.json" ]; then
        log_info "✓ Entry file created"
        # Check metadata
        if jq -e '.memories[] | select(.id == "'$id'") | has("content") | not' "$proj_dir/.aish/memory/metadata.json" > /dev/null; then
            log_info "✓ Metadata does NOT contain content"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            log_error "✗ Metadata contains content!"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else
        log_error "✗ Entry file not found"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# テスト3: search and get_content
test_search_and_get() {
    test_case "search and get_content"
    local proj_dir="$TEST_DIR/search_proj"
    mkdir -p "$proj_dir/.aish/memory"
    
    run_with_lib "cd '$proj_dir' && save_memory 'detailed info' 'cat' 'kw' 'SearchMe'" > /dev/null
    
    # Search
    local search_res=$(run_with_lib "cd '$proj_dir' && search_memory_efficient 'SearchMe' '' 5 true")
    if echo "$search_res" | jq -e '.[0].content == "detailed info"' > /dev/null; then
        log_info "✓ Search with content works"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        log_error "✗ Search failed to return content"
        echo "$search_res"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

main() {
    test_init_memory_directory
    test_save_memory_structure
    test_search_and_get
    
    echo ""
    echo "Passed: $TESTS_PASSED, Failed: $TESTS_FAILED"
    [ $TESTS_FAILED -eq 0 ]
}

main "$@"
