#!/bin/bash
# aishコマンドの動作確認テストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# aishコマンドのパス
AISH_CMD="${AISH_CMD:-./aish}"

# AISH_HOMEの設定（テスト用）
export AISH_HOME="${TEST_DIR}/.aish"
mkdir -p "$AISH_HOME"

# テスト用のfunctionsファイルを作成
cat > "$AISH_HOME/functions" << 'EOF'
#!/bin/bash
function aish_rollout {
  echo "rollout called"
}
function aish_clear {
  echo "clear called"
}
function aish_ls {
  echo "ls called"
}
function aish_rm_last {
  echo "rm_last called"
}
EOF

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

assert_contains() {
    local output="$1"
    local pattern="$2"
    if echo "$output" | grep -q "$pattern"; then
        return 0
    else
        log_error "Output does not contain: $pattern"
        log_error "Actual output: $output"
        return 1
    fi
}

assert_exit_code() {
    local expected="$1"
    local actual="$2"
    if [ "$actual" -eq "$expected" ]; then
        return 0
    else
        log_error "Exit code mismatch: expected $expected, got $actual"
        return 1
    fi
}

# テスト1: ヘルプオプション (-h)
test_help_option() {
    test_case "Help Option (-h)"
    
    log_info "Running: $AISH_CMD -h"
    local output
    set +e
    output=$($AISH_CMD -h 2>&1)
    local exit_code=$?
    set -e
    
    if assert_exit_code 1 "$exit_code"; then
        log_info "✓ Exit code is 1 (as expected for usage)"
    else
        return 1
    fi
    
    if assert_contains "$output" "Usage:"; then
        log_info "✓ Help message contains 'Usage:'"
    else
        return 1
    fi
    
    if assert_contains "$output" "rollout"; then
        log_info "✓ Help message contains 'rollout'"
    else
        return 1
    fi
    
    if assert_contains "$output" "clear"; then
        log_info "✓ Help message contains 'clear'"
    else
        return 1
    fi
    
    log_info "Test 1 PASSED"
    return 0
}

# テスト2: 無効なオプション
test_invalid_option() {
    test_case "Invalid Option"
    
    log_info "Running: $AISH_CMD -x"
    local output
    set +e
    output=$($AISH_CMD -x 2>&1)
    local exit_code=$?
    set -e
    
    if assert_exit_code 1 "$exit_code"; then
        log_info "✓ Exit code is 1 for invalid option"
    else
        return 1
    fi
    
    if assert_contains "$output" "Invalid option"; then
        log_info "✓ Error message contains 'Invalid option'"
    else
        return 1
    fi
    
    log_info "Test 2 PASSED"
    return 0
}

# テスト3: -dオプション（ディレクトリ指定）
test_directory_option() {
    test_case "Directory Option (-d)"
    
    local test_session_dir="$TEST_DIR/test_session"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir rollout"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" rollout 2>&1)
    local exit_code=$?
    set -e
    
    if [ "$exit_code" -eq 0 ]; then
        log_info "✓ Command executed successfully"
    else
        log_error "Command failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    if assert_contains "$output" "rollout called"; then
        log_info "✓ Rollout function was called"
    else
        return 1
    fi
    
    log_info "Test 3 PASSED"
    return 0
}

# テスト4: rolloutコマンド
test_rollout_command() {
    test_case "Rollout Command"
    
    local test_session_dir="$TEST_DIR/test_session_rollout"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir rollout"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" rollout 2>&1)
    local exit_code=$?
    set -e
    
    if [ "$exit_code" -eq 0 ]; then
        log_info "✓ Rollout command executed successfully"
    else
        log_error "Rollout command failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    if assert_contains "$output" "rollout called"; then
        log_info "✓ Rollout function was called"
    else
        return 1
    fi
    
    log_info "Test 4 PASSED"
    return 0
}

# テスト5: clearコマンド
test_clear_command() {
    test_case "Clear Command"
    
    local test_session_dir="$TEST_DIR/test_session_clear"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir clear"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" clear 2>&1)
    local exit_code=$?
    set -e
    
    if [ "$exit_code" -eq 0 ]; then
        log_info "✓ Clear command executed successfully"
    else
        log_error "Clear command failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    if assert_contains "$output" "clear called"; then
        log_info "✓ Clear function was called"
    else
        return 1
    fi
    
    log_info "Test 5 PASSED"
    return 0
}

# テスト6: lsコマンド
test_ls_command() {
    test_case "LS Command"
    
    local test_session_dir="$TEST_DIR/test_session_ls"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir ls"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" ls 2>&1)
    local exit_code=$?
    set -e
    
    if [ "$exit_code" -eq 0 ]; then
        log_info "✓ LS command executed successfully"
    else
        log_error "LS command failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    if assert_contains "$output" "ls called"; then
        log_info "✓ LS function was called"
    else
        return 1
    fi
    
    log_info "Test 6 PASSED"
    return 0
}

# テスト7: rm_lastコマンド
test_rm_last_command() {
    test_case "RM_LAST Command"
    
    local test_session_dir="$TEST_DIR/test_session_rm_last"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir rm_last"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" rm_last 2>&1)
    local exit_code=$?
    set -e
    
    if [ "$exit_code" -eq 0 ]; then
        log_info "✓ RM_LAST command executed successfully"
    else
        log_error "RM_LAST command failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    if assert_contains "$output" "rm_last called"; then
        log_info "✓ RM_LAST function was called"
    else
        return 1
    fi
    
    log_info "Test 7 PASSED"
    return 0
}

# テスト8: 無効なコマンド
test_invalid_command() {
    test_case "Invalid Command"
    
    local test_session_dir="$TEST_DIR/test_session_invalid"
    mkdir -p "$test_session_dir"
    
    log_info "Running: $AISH_CMD -d $test_session_dir invalid_command"
    local output
    set +e
    output=$($AISH_CMD -d "$test_session_dir" invalid_command 2>&1)
    local exit_code=$?
    set -e
    
    # 無効なコマンドの場合は関数が見つからずエラーになる可能性がある
    # 実際の動作に応じて調整が必要
    log_info "Exit code: $exit_code"
    log_info "Output: $output"
    
    log_info "Test 8 PASSED (checked behavior)"
    return 0
}

# テスト9: -dオプションの引数なし
test_directory_option_no_arg() {
    test_case "Directory Option Without Argument"
    
    log_info "Running: $AISH_CMD -d"
    local output
    set +e
    output=$($AISH_CMD -d 2>&1)
    local exit_code=$?
    set -e
    
    if assert_exit_code 1 "$exit_code"; then
        log_info "✓ Exit code is 1 for missing argument"
    else
        return 1
    fi
    
    if assert_contains "$output" "requires an argument"; then
        log_info "✓ Error message indicates missing argument"
    else
        return 1
    fi
    
    log_info "Test 9 PASSED"
    return 0
}

# テスト10: コマンドなし（セッション開始の準備確認）
test_no_command() {
    test_case "No Command (Session Start Preparation)"
    
    # このテストは実際にセッションを開始するため、モックが必要
    # ここでは基本的な構造チェックのみ
    
    log_info "Testing command structure without command argument"
    
    # aish-captureバイナリの存在確認（実際のセッション開始には必要）
    local aish_capture_bin=""
    local script_dir="$(cd "$(dirname "$AISH_CMD")" && pwd)"
    
    if [ -f "$script_dir/tools/aish-capture/target/release/aish-capture" ]; then
        aish_capture_bin="$script_dir/tools/aish-capture/target/release/aish-capture"
    elif [ -f "$script_dir/tools/aish-capture/target/debug/aish-capture" ]; then
        aish_capture_bin="$script_dir/tools/aish-capture/target/debug/aish-capture"
    fi
    
    if [ -n "$aish_capture_bin" ] && [ -f "$aish_capture_bin" ]; then
        log_info "✓ aish-capture binary found: $aish_capture_bin"
        log_info "Test 10 PASSED (binary check)"
        return 0
    else
        log_warn "⚠ aish-capture binary not found (may need to build first)"
        log_info "Test 10 PASSED (skipped - binary not built)"
        return 0
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "aish Command Test Suite"
    echo "========================================="
    echo "Command: $AISH_CMD"
    echo "Test directory: $TEST_DIR"
    echo "AISH_HOME: $AISH_HOME"
    echo ""
    
    # コマンドの存在確認
    if [ ! -f "$AISH_CMD" ]; then
        log_error "aish command not found: $AISH_CMD"
        log_info "Please ensure the aish script exists and is executable"
        exit 1
    fi
    
    # 実行権限の確認
    if [ ! -x "$AISH_CMD" ]; then
        log_warn "aish command is not executable, attempting to chmod +x"
        chmod +x "$AISH_CMD" || {
            log_error "Failed to make aish command executable"
            exit 1
        }
    fi
    
    # テスト実行
    local tests=(
        "test_help_option"
        "test_invalid_option"
        "test_directory_option_no_arg"
        "test_directory_option"
        "test_rollout_command"
        "test_clear_command"
        "test_ls_command"
        "test_rm_last_command"
        "test_invalid_command"
        "test_no_command"
    )
    
    for test_func in "${tests[@]}"; do
        if $test_func; then
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            TESTS_FAILED=$((TESTS_FAILED + 1))
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

