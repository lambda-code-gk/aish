#!/bin/bash
# ファイル編集ツールの動作確認テストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)

# クリーンアップ関数（テスト終了時に呼ばれる）
cleanup_test_data() {
    rm -rf "$TEST_DIR"
}

trap cleanup_test_data EXIT

# プロジェクトルートに移動
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# AISH_HOMEの設定（テスト用）
export AISH_HOME="${AISH_HOME:-$PROJECT_ROOT/_aish}"
TEST_AISH_HOME="$TEST_DIR/.aish"
mkdir -p "$TEST_AISH_HOME/lib"

# AISH_SESSIONの設定（テスト用）
export AISH_SESSION="$TEST_DIR/session"
mkdir -p "$AISH_SESSION"

# ライブラリのパス
TOOL_LIB_DIR="${TOOL_LIB_DIR:-$PROJECT_ROOT/_aish/lib}"

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
        if [ -f '$TOOL_LIB_DIR/tool_helper.sh' ]; then
            . '$TOOL_LIB_DIR/tool_helper.sh'
        fi
        if [ -f '$TOOL_LIB_DIR/../functions' ]; then
            . '$TOOL_LIB_DIR/../functions'
        fi
        $cmd
    "
}

# テスト1: read_file - ファイル全体を読む
test_read_file_full() {
    test_case "read_file - read full file"
    
    local test_file="$TEST_DIR/test.txt"
    echo -e "line1\nline2\nline3" > "$test_file"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_read_file.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    local func_args=$(echo "{\"path\": \"$test_file\"}" | jq -c .)
    local result=$(_tool_read_file_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    
    local content=$(echo "$result" | jq -r '.content // empty' 2>/dev/null || echo "")
    
    if [ "$content" = $'line1\nline2\nline3' ]; then
        log_info "✓ Correctly read full file"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to read full file. Expected: line1\\nline2\\nline3, Got: $content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト2: read_file - 指定した範囲を読む
test_read_file_range() {
    test_case "read_file - read specified range"
    
    local test_file="$TEST_DIR/test.txt"
    echo -e "line1\nline2\nline3\nline4\nline5" > "$test_file"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_read_file.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    local func_args=$(echo "{\"path\": \"$test_file\", \"start_line\": 2, \"end_line\": 4}" | jq -c .)
    local result=$(_tool_read_file_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    
    local content=$(echo "$result" | jq -r '.content // empty' 2>/dev/null || echo "")
    
    if [ "$content" = $'line2\nline3\nline4' ]; then
        log_info "✓ Correctly read specified range"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to read specified range. Expected: line2\\nline3\\nline4, Got: $content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト3: write_file - ファイルを書き込む
test_write_file() {
    test_case "write_file - write content to file"
    
    local test_file="$TEST_DIR/test_write.txt"
    local content="hello world"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_write_file.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    local func_args=$(echo "{\"path\": \"$test_file\", \"content\": \"$content\"}" | jq -c .)
    local result=$(_tool_write_file_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    
    local success=$(echo "$result" | jq -r '.success // false' 2>/dev/null || echo "false")
    local actual_content=$(cat "$test_file" 2>/dev/null || echo "")
    
    if [ "$success" = "true" ] && [ "$actual_content" = "$content" ]; then
        log_info "✓ Correctly wrote file"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to write file. success=$success, content=$actual_content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト4: replace_block - ブロックを置換
test_replace_block() {
    test_case "replace_block - replace a block of code"
    
    local test_file="$TEST_DIR/test_replace.txt"
    echo -e "line1\nline2\nline3" > "$test_file"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_replace_block.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    local func_args=$(echo "{\"path\": \"$test_file\", \"old_block\": \"line2\", \"new_block\": \"newline2\"}" | jq -c .)
    local result=$(_tool_replace_block_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    
    local success=$(echo "$result" | jq -r '.success // false' 2>/dev/null || echo "false")
    local actual_content=$(cat "$test_file" 2>/dev/null || echo "")
    local diff=$(echo "$result" | jq -r '.diff // empty' 2>/dev/null || echo "")
    
    if [ "$success" = "true" ] && echo "$actual_content" | grep -q "newline2" && [ ! -z "$diff" ]; then
        log_info "✓ Correctly replaced block"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to replace block. success=$success, content=$actual_content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト5: diff_files - 二つのファイルを比較
test_diff_files() {
    test_case "diff_files - compare two files"
    
    local file1="$TEST_DIR/file1.txt"
    local file2="$TEST_DIR/file2.txt"
    echo -e "line1\nline2\nline3" > "$file1"
    echo -e "line1\nmodified\nline3" > "$file2"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_diff_files.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    local func_args=$(echo "{\"before\": \"$file1\", \"after\": \"$file2\"}" | jq -c .)
    local result=$(_tool_diff_files_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    
    local diff=$(echo "$result" | jq -r '.diff // empty' 2>/dev/null || echo "")
    
    if [ ! -z "$diff" ] && echo "$diff" | grep -q "modified"; then
        log_info "✓ Correctly generated diff"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to generate diff"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト6: run_validation - configured mode
test_run_validation_configured() {
    test_case "run_validation - configured mode"
    
    local project_dir="$TEST_DIR/project"
    mkdir -p "$project_dir/.aish"
    
    # validate.jsonを作成
    cat > "$project_dir/.aish/validate.json" <<'EOF'
{
  "commands": [
    {"command": "echo test", "description": "test command"}
  ]
}
EOF
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_run_validation.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    cd "$project_dir"
    local func_args=$(echo '{"mode": "configured"}' | jq -c .)
    local result=$(_tool_run_validation_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    cd - > /dev/null
    
    local success=$(echo "$result" | jq -r '.success // false' 2>/dev/null || echo "false")
    
    if [ "$success" = "true" ]; then
        log_info "✓ Correctly ran validation in configured mode"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to run validation in configured mode. success=$success"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト7: run_validation - auto mode (Makefileがある場合)
test_run_validation_auto_makefile() {
    test_case "run_validation - auto mode with Makefile"
    
    local project_dir="$TEST_DIR/project"
    mkdir -p "$project_dir"
    
    # Makefileを作成
    cat > "$project_dir/Makefile" <<'EOF'
test:
	echo "test"
EOF
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_run_validation.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    cd "$project_dir"
    local func_args=$(echo '{"mode": "auto"}' | jq -c .)
    local result=$(_tool_run_validation_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    cd - > /dev/null
    
    local success=$(echo "$result" | jq -r '.success // false' 2>/dev/null || echo "false")
    local mode=$(echo "$result" | jq -r '.mode // empty' 2>/dev/null || echo "")
    
    if [ "$success" = "true" ] || [ "$mode" = "auto" ]; then
        log_info "✓ Correctly detected Makefile in auto mode"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to detect Makefile in auto mode. success=$success, mode=$mode"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト8: run_validation - none mode
test_run_validation_none() {
    test_case "run_validation - none mode"
    
    local project_dir="$TEST_DIR/project"
    mkdir -p "$project_dir"
    
    # ツール定義関数を読み込む
    . "$TOOL_LIB_DIR/tool_run_validation.sh" 2>/dev/null || true
    
    # テスト用の実行関数を呼び出す
    cd "$project_dir"
    local func_args=$(echo '{"mode": "none"}' | jq -c .)
    local result=$(_tool_run_validation_execute "" "$func_args" "openai" 2>/dev/null || echo '{"error":"function not found"}')
    cd - > /dev/null
    
    local success=$(echo "$result" | jq -r '.success // false' 2>/dev/null || echo "false")
    local mode=$(echo "$result" | jq -r '.mode // empty' 2>/dev/null || echo "")
    local message=$(echo "$result" | jq -r '.message // empty' 2>/dev/null || echo "")
    
    if [ "$mode" = "none" ] && [ ! -z "$message" ]; then
        log_info "✓ Correctly handled none mode"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Failed to handle none mode. mode=$mode"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "File Editing Tools Test Suite"
    echo "========================================="
    echo "Project root: $(pwd)"
    echo "Test directory: $TEST_DIR"
    echo ""
    
    # テスト実行
    local tests=(
        "test_read_file_full"
        "test_read_file_range"
        "test_write_file"
        "test_replace_block"
        "test_diff_files"
        "test_run_validation_configured"
        "test_run_validation_auto_makefile"
        "test_run_validation_none"
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

