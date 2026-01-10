#!/bin/bash
# 記憶管理ライブラリの動作確認テストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)

# 実際のプロジェクトの記憶ディレクトリをバックアップ
PROJECT_MEMORY_DIR="$(pwd)/_aish/memory"
PROJECT_BACKUP_DIR=$(mktemp -d)
PROJECT_METADATA_BACKUP="$PROJECT_BACKUP_DIR/metadata.json"
PROJECT_CATEGORY_BACKUP="$PROJECT_BACKUP_DIR/by_category"
PROJECT_KEYWORD_BACKUP="$PROJECT_BACKUP_DIR/keywords.json"

if [ -d "$PROJECT_MEMORY_DIR" ]; then
    [ -f "$PROJECT_MEMORY_DIR/metadata.json" ] && cp "$PROJECT_MEMORY_DIR/metadata.json" "$PROJECT_METADATA_BACKUP" 2>/dev/null || true
    [ -d "$PROJECT_MEMORY_DIR/by_category" ] && cp -r "$PROJECT_MEMORY_DIR/by_category" "$PROJECT_CATEGORY_BACKUP" 2>/dev/null || true
    [ -f "$PROJECT_MEMORY_DIR/by_keyword/keywords.json" ] && cp "$PROJECT_MEMORY_DIR/by_keyword/keywords.json" "$PROJECT_KEYWORD_BACKUP" 2>/dev/null || true
fi

# クリーンアップ関数（テスト終了時に呼ばれる）
cleanup_test_data() {
    # 実際のプロジェクトの記憶ディレクトリをバックアップから復元
    if [ -d "$PROJECT_BACKUP_DIR" ]; then
        if [ -f "$PROJECT_METADATA_BACKUP" ]; then
            cp "$PROJECT_METADATA_BACKUP" "$PROJECT_MEMORY_DIR/metadata.json" 2>/dev/null || true
        else
            # バックアップが存在しない場合、テストで作成された可能性があるので削除
            rm -f "$PROJECT_MEMORY_DIR/metadata.json" 2>/dev/null || true
        fi
        
        if [ -d "$PROJECT_CATEGORY_BACKUP" ]; then
            rm -rf "$PROJECT_MEMORY_DIR/by_category" 2>/dev/null || true
            mkdir -p "$PROJECT_MEMORY_DIR/by_category" 2>/dev/null || true
            cp -r "$PROJECT_CATEGORY_BACKUP/"* "$PROJECT_MEMORY_DIR/by_category/" 2>/dev/null || true
        else
            # バックアップが存在しない場合、テストで作成された可能性があるので削除
            rm -rf "$PROJECT_MEMORY_DIR/by_category" 2>/dev/null || true
        fi
        
        if [ -f "$PROJECT_KEYWORD_BACKUP" ]; then
            mkdir -p "$PROJECT_MEMORY_DIR/by_keyword" 2>/dev/null || true
            cp "$PROJECT_KEYWORD_BACKUP" "$PROJECT_MEMORY_DIR/by_keyword/keywords.json" 2>/dev/null || true
        else
            # バックアップが存在しない場合、テストで作成された可能性があるので削除
            rm -rf "$PROJECT_MEMORY_DIR/by_keyword" 2>/dev/null || true
        fi
        
        # バックアップディレクトリを削除
        rm -rf "$PROJECT_BACKUP_DIR"
    fi
    
    # 一時ディレクトリを削除
    rm -rf "$TEST_DIR"
}

trap cleanup_test_data EXIT

# AISH_HOMEの設定（テスト用）
export AISH_HOME="${AISH_HOME:-$HOME/.aish}"
TEST_AISH_HOME="$TEST_DIR/.aish"
mkdir -p "$TEST_AISH_HOME/memory"

# ライブラリのパス
MEMORY_LIB="${MEMORY_LIB:-_aish/lib/memory_manager.sh}"

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

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

test_case() {
    local name="$1"
    echo ""
    echo "========================================="
    echo "Test: $name"
    echo "========================================="
}

# 関数を実行するヘルパー（環境変数を設定してライブラリを読み込んでから実行）
run_with_lib() {
    local aish_home="$1"
    shift
    local cmd="$@"
    
    AISH_HOME="$aish_home" bash -c "
        set -e
        if [ -f '$MEMORY_LIB' ]; then
            . '$MEMORY_LIB'
        fi
        $cmd
    "
}

# テスト1: find_memory_directory - 現在のディレクトリに.aish/memoryが存在する場合
test_find_memory_directory_current_dir() {
    test_case "find_memory_directory - .aish/memory in current directory"
    
    local test_project_dir="$TEST_DIR/project1"
    mkdir -p "$test_project_dir/.aish/memory"
    
    local result=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && find_memory_directory" 2>/dev/null || echo "")
    
    if [ "$result" = "$test_project_dir/.aish/memory" ]; then
        log_info "✓ Correctly found .aish/memory in current directory"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Expected: $test_project_dir/.aish/memory, got: '$result'"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト2: find_memory_directory - 親ディレクトリに.aish/memoryが存在する場合
test_find_memory_directory_parent_dir() {
    test_case "find_memory_directory - .aish/memory in parent directory"
    
    local test_project_dir="$TEST_DIR/project2"
    local test_sub_dir="$test_project_dir/sub/deep"
    mkdir -p "$test_project_dir/.aish/memory"
    mkdir -p "$test_sub_dir"
    
    local result=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_sub_dir' && find_memory_directory" 2>/dev/null || echo "")
    
    if [ "$result" = "$test_project_dir/.aish/memory" ]; then
        log_info "✓ Correctly found .aish/memory in parent directory"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Expected: $test_project_dir/.aish/memory, got: '$result'"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト3: find_memory_directory - .aish/memoryが存在しない場合
test_find_memory_directory_not_found() {
    test_case "find_memory_directory - .aish/memory not found, fallback to global"
    
    local test_dir="$TEST_DIR/no_project"
    mkdir -p "$test_dir"
    mkdir -p "$TEST_AISH_HOME/memory"
    
    local result=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_dir' && find_memory_directory" 2>/dev/null || echo "")
    
    if [ "$result" = "$TEST_AISH_HOME/memory" ]; then
        log_info "✓ Correctly fell back to global memory directory"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Expected: $TEST_AISH_HOME/memory, got: '$result'"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト4: init_memory_directory - 新規ディレクトリの作成
test_init_memory_directory_new() {
    test_case "init_memory_directory - new directory creation"
    
    local memory_dir="$TEST_DIR/new_memory"
    
    run_with_lib "$TEST_AISH_HOME" "init_memory_directory '$memory_dir'" > /dev/null 2>&1
    
    if [ ! -d "$memory_dir" ]; then
        log_error "✗ Memory directory was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    if [ ! -d "$memory_dir/by_category" ]; then
        log_error "✗ by_category subdirectory was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    if [ ! -d "$memory_dir/by_keyword" ]; then
        log_error "✗ by_keyword subdirectory was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    if [ ! -f "$memory_dir/metadata.json" ]; then
        log_error "✗ metadata.json was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # metadata.jsonの内容を確認
    local metadata_content=$(cat "$memory_dir/metadata.json")
    if echo "$metadata_content" | jq -e '.memories == [] and .memory_dir == "'"$memory_dir"'"' > /dev/null 2>&1; then
        log_info "✓ Memory directory initialized correctly"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ metadata.json has incorrect content"
        echo "$metadata_content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト5: save_memory - 基本的な保存
test_save_memory_basic() {
    test_case "save_memory - basic save"
    
    local test_project_dir="$TEST_DIR/save_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    # プロジェクトディレクトリで記憶を保存
    local content="Test memory content"
    local category="test_category"
    local keywords="keyword1,keyword2"
    
    local result=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory '$(echo "$content" | sed "s/'/\\'/g")' '$category' '$keywords'" 2>/dev/null)
    local memory_id=$(echo "$result" | jq -r '.memory_id // empty' 2>/dev/null || echo "")
    
    if [ -z "$memory_id" ]; then
        log_error "✗ save_memory did not return memory_id"
        echo "Result: $result"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    local memory_dir="$test_project_dir/.aish/memory"
    
    # カテゴリ別ファイルの存在確認
    if [ ! -f "$memory_dir/by_category/${category}_${memory_id}.json" ]; then
        log_error "✗ Category file was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # メタデータの更新確認
    local metadata_count=$(cat "$memory_dir/metadata.json" | jq '.memories | length')
    if [ "$metadata_count" -ne 1 ]; then
        log_error "✗ Metadata was not updated correctly (expected 1, got $metadata_count)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # 記憶内容の確認
    local saved_content=$(cat "$memory_dir/by_category/${category}_${memory_id}.json" | jq -r '.content')
    if [ "$saved_content" = "$content" ]; then
        log_info "✓ Memory saved correctly"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Saved content mismatch (expected: $content, got: $saved_content)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト6: save_memory - キーワードインデックスの更新
test_save_memory_keyword_index() {
    test_case "save_memory - keyword index update"
    
    local test_project_dir="$TEST_DIR/keyword_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    local content="Test content with keywords"
    local category="test"
    local keywords="python,api,test"
    
    local result=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory '$(echo "$content" | sed "s/'/\\'/g")' '$category' '$keywords'" 2>/dev/null)
    local memory_id=$(echo "$result" | jq -r '.memory_id // empty' 2>/dev/null || echo "")
    
    if [ -z "$memory_id" ]; then
        log_error "✗ save_memory failed"
        echo "Result: $result"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    local memory_dir="$test_project_dir/.aish/memory"
    
    # キーワードインデックスの確認
    if [ ! -f "$memory_dir/by_keyword/keywords.json" ]; then
        log_error "✗ keywords.json was not created"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # 各キーワードがインデックスに含まれているか確認
    local keyword_file="$memory_dir/by_keyword/keywords.json"
    for keyword in python api test; do
        if ! cat "$keyword_file" | jq -e ".[\"$keyword\"] | index(\"$memory_id\")" > /dev/null 2>&1; then
            log_error "✗ Keyword '$keyword' not found in index"
            cat "$keyword_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    done
    
    log_info "✓ Keyword index updated correctly"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# テスト7: search_memory_efficient - 基本的な検索
test_search_memory_basic() {
    test_case "search_memory_efficient - basic search"
    
    local test_project_dir="$TEST_DIR/search_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    # テスト用の記憶を保存
    run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory 'Python API tutorial' 'tutorial' 'python,api'" > /dev/null 2>&1
    run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory 'JavaScript guide' 'guide' 'javascript'" > /dev/null 2>&1
    
    # 検索実行
    local results=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && search_memory_efficient 'python' '' 10" || echo "[]")
    local result_count=$(echo "$results" | jq 'length' 2>/dev/null || echo "0")
    
    if [ "$result_count" -ge 1 ]; then
        # 結果に"Python"が含まれているか確認
        if echo "$results" | jq -r '.[].content' 2>/dev/null | grep -q "Python"; then
            log_info "✓ Basic search works correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Search result does not contain expected content"
            echo "Results: $results"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "✗ Search returned no results (expected at least 1)"
        echo "Results: $results"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト8: search_memory_efficient - カテゴリフィルタ
test_search_memory_category_filter() {
    test_case "search_memory_efficient - category filter"
    
    local test_project_dir="$TEST_DIR/category_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    # 異なるカテゴリの記憶を保存
    run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory 'Python tutorial' 'tutorial' 'python'" > /dev/null 2>&1
    run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory 'Python error fix' 'error' 'python'" > /dev/null 2>&1
    
    # カテゴリフィルタで検索
    local results=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && search_memory_efficient 'python' 'tutorial' 10" || echo "[]")
    local result_count=$(echo "$results" | jq 'length' 2>/dev/null || echo "0")
    
    if [ "$result_count" -eq 1 ]; then
        local result_category=$(echo "$results" | jq -r '.[0].category' 2>/dev/null || echo "")
        if [ "$result_category" = "tutorial" ]; then
            log_info "✓ Category filter works correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Wrong category in result: $result_category"
            echo "Results: $results"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "✗ Expected 1 result, got $result_count"
        echo "Results: $results"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト9: search_memory_efficient - limitパラメータ
test_search_memory_limit() {
    test_case "search_memory_efficient - limit parameter"
    
    local test_project_dir="$TEST_DIR/limit_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    # 複数の記憶を保存
    for i in {1..5}; do
        run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && save_memory 'Test memory $i' 'test' 'test'" > /dev/null 2>&1
    done
    
    # limit=2で検索
    local results=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && search_memory_efficient 'test' '' 2" || echo "[]")
    local result_count=$(echo "$results" | jq 'length' 2>/dev/null || echo "0")
    
    if [ "$result_count" -eq 2 ]; then
        log_info "✓ Limit parameter works correctly"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Expected 2 results, got $result_count"
        echo "Results: $results"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト10: search_memory_efficient - プロジェクト優先（重複時）
test_search_memory_project_priority() {
    test_case "search_memory_efficient - project priority on duplicates"
    
    local project_dir="$TEST_DIR/project_priority"
    local project_memory="$project_dir/.aish/memory"
    local global_memory="$TEST_AISH_HOME/memory"
    
    mkdir -p "$project_memory"
    mkdir -p "$global_memory"
    
    # プロジェクト固有の記憶を保存
    run_with_lib "$TEST_AISH_HOME" "cd '$project_dir' && save_memory 'Project-specific Python guide' 'guide' 'python'" > /dev/null 2>&1
    
    # グローバル記憶を保存（別のディレクトリから）
    run_with_lib "$TEST_AISH_HOME" "cd '$TEST_DIR' && save_memory 'Global Python guide' 'guide' 'python'" > /dev/null 2>&1
    
    # 検索実行（プロジェクトディレクトリから）
    local results=$(run_with_lib "$TEST_AISH_HOME" "cd '$project_dir' && search_memory_efficient 'python' '' 10" || echo "[]")
    local result_count=$(echo "$results" | jq 'length' 2>/dev/null || echo "0")
    
    # プロジェクト固有の記憶が含まれているか確認
    if echo "$results" | jq -r '.[].content' 2>/dev/null | grep -q "Project-specific"; then
        log_info "✓ Project-specific memory is included in results"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Project-specific memory not found in results"
        echo "Results: $results"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト11: search_memory_efficient - 空の検索結果
test_search_memory_empty() {
    test_case "search_memory_efficient - empty search results"
    
    local test_project_dir="$TEST_DIR/empty_test_project"
    mkdir -p "$test_project_dir/.aish/memory"
    
    # 検索実行（存在しないキーワード）
    local results=$(run_with_lib "$TEST_AISH_HOME" "cd '$test_project_dir' && search_memory_efficient 'nonexistent_keyword_xyz' '' 10" 2>/dev/null || echo "[]")
    local result_count=$(echo "$results" | jq 'length' 2>/dev/null || echo "-1")
    
    if [ "$result_count" = "0" ] || [ "$results" = "[]" ]; then
        log_info "✓ Empty search results handled correctly"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Expected empty results, got count: $result_count"
        echo "Results: $results"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Memory Manager Test Suite"
    echo "========================================="
    echo "Memory Library: $MEMORY_LIB"
    echo "Test directory: $TEST_DIR"
    echo "Test AISH_HOME: $TEST_AISH_HOME"
    echo ""
    
    # jqの存在確認
    if ! command -v jq > /dev/null 2>&1; then
        log_error "jq command is required but not found"
        log_info "Please install jq: sudo apt install jq (or equivalent for your system)"
        exit 1
    fi
    
    # ライブラリの存在確認
    if [ ! -f "$MEMORY_LIB" ]; then
        log_error "Memory library not found: $MEMORY_LIB"
        log_info "This is expected if the library hasn't been created yet"
        log_info "Tests will be created, but will fail until the library is implemented"
    fi
    
    # テスト実行
    local tests=(
        "test_find_memory_directory_current_dir"
        "test_find_memory_directory_parent_dir"
        "test_find_memory_directory_not_found"
        "test_init_memory_directory_new"
        "test_save_memory_basic"
        "test_save_memory_keyword_index"
        "test_search_memory_basic"
        "test_search_memory_category_filter"
        "test_search_memory_limit"
        "test_search_memory_project_priority"
        "test_search_memory_empty"
    )
    
    for test_func in "${tests[@]}"; do
        if $test_func; then
            :
        else
            log_error "Test failed: $test_func"
        fi
    done
    
    # 結果サマリー
    echo ""
    echo "========================================="
    echo "Test Summary"
    echo "========================================="
    echo "Passed: $TESTS_PASSED"
    echo "Failed: $TESTS_FAILED"
    echo "Total:  $((TESTS_PASSED + TESTS_FAILED))"
    
    if [ $TESTS_FAILED -eq 0 ]; then
        echo ""
        log_info "All tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some tests failed. ✗"
        exit 1
    fi
}

main "$@"
