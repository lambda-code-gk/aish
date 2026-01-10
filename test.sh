#!/bin/bash
# プロジェクト全体のテストを実行するスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# テスト結果のカウント
TESTS_PASSED=0
TESTS_FAILED=0
FAILED_TESTS=()

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

# テストスクリプトを実行する関数
run_test() {
    local test_name="$1"
    local test_script="$2"
    
    echo ""
    echo "========================================="
    echo "Running: $test_name"
    echo "========================================="
    
    if [ ! -f "$test_script" ]; then
        log_error "Test script not found: $test_script"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("$test_name (not found)")
        return 1
    fi
    
    if [ ! -x "$test_script" ]; then
        log_warn "Making test script executable: $test_script"
        chmod +x "$test_script" || {
            log_error "Failed to make test script executable"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("$test_name (not executable)")
            return 1
        }
    fi
    
    # テスト実行
    if bash "$test_script"; then
        log_info "✓ $test_name PASSED"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ $test_name FAILED"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("$test_name")
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Running All Tests"
    echo "========================================="
    echo "Project root: $PROJECT_ROOT"
    echo ""
    
    # testsディレクトリのテストスクリプトを実行
    log_info "Running tests from tests/ directory..."
    for test_file in "$PROJECT_ROOT/tests"/test_*.sh; do
        if [ -f "$test_file" ]; then
            test_name=$(basename "$test_file")
            run_test "$test_name" "$test_file" || true
        fi
    done
    
    # Rustプロジェクトのテストスクリプトを実行
    log_info "Running Rust project tests..."
    
    # aish-capture
    if [ -f "$PROJECT_ROOT/tools/aish-capture/test.sh" ]; then
        cd "$PROJECT_ROOT/tools/aish-capture"
        run_test "aish-capture/test.sh" "./test.sh" || true
        cd "$PROJECT_ROOT"
    else
        log_warn "aish-capture/test.sh not found, skipping"
    fi
    
    # aish-render
    if [ -f "$PROJECT_ROOT/tools/aish-render/test_render.sh" ]; then
        cd "$PROJECT_ROOT/tools/aish-render"
        run_test "aish-render/test_render.sh" "./test_render.sh" || true
        cd "$PROJECT_ROOT"
    else
        log_warn "aish-render/test_render.sh not found, skipping"
    fi
    
    # aish-script
    if [ -f "$PROJECT_ROOT/tools/aish-script/test.sh" ]; then
        cd "$PROJECT_ROOT/tools/aish-script"
        run_test "aish-script/test.sh" "./test.sh" || true
        cd "$PROJECT_ROOT"
    else
        log_warn "aish-script/test.sh not found, skipping"
    fi
    
    # 結果サマリー
    echo ""
    echo "========================================="
    echo "Test Summary"
    echo "========================================="
    echo "Passed: $TESTS_PASSED"
    echo "Failed: $TESTS_FAILED"
    echo "Total:  $((TESTS_PASSED + TESTS_FAILED))"
    
    if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
        echo ""
        log_error "Failed tests:"
        for failed_test in "${FAILED_TESTS[@]}"; do
            echo "  - $failed_test"
        done
    fi
    
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
