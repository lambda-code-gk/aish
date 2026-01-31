#!/bin/bash
# プロジェクト全体の結合テストを実行するスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# ビルドモード（デフォルトはrelease）
BUILD_MODE="${BUILD_MODE:-release}"
TARGET_DIR="$BUILD_MODE"

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

test_case() {
    local name="$1"
    echo ""
    echo "========================================="
    echo "Test: $name"
    echo "========================================="
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

# バイナリをビルドする関数
build_binary() {
    local project_name="$1"
    local project_path="$2"
    local binary_name="$3"
    
    log_info "Building $project_name..." >&2
    
    if [ ! -d "$project_path" ]; then
        log_error "Project directory not found: $project_path" >&2
        return 1
    fi
    
    if [ ! -f "$project_path/Cargo.toml" ]; then
        log_error "Cargo.toml not found: $project_path/Cargo.toml" >&2
        return 1
    fi
    
    cd "$project_path"
    if [ "$BUILD_MODE" == "debug" ]; then
        cargo build >&2
    else
        cargo build --release >&2
    fi
    
    cd "$PROJECT_ROOT"
    
    local binary_path="$project_path/target/$TARGET_DIR/$binary_name"
    if [ ! -f "$binary_path" ]; then
        log_error "Binary not found after build: $binary_path" >&2
        return 1
    fi
    
    echo "$binary_path"
}

# aiコマンドの結合テスト
test_ai_binary() {
    test_case "ai command integration test"
    
    # バイナリをビルド
    local binary_path
    if ! binary_path=$(build_binary "ai" "$PROJECT_ROOT/core/ai" "ai"); then
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai (build failed)")
        return 1
    fi
    
    log_info "Binary path: $binary_path"
    
    # テスト1: 引数なしで実行した場合、適切なエラーメッセージが表示されること
    log_info "Test 1: Error handling (no query)"
    if "$binary_path" > "$TEST_DIR/ai_test1.stdout" 2> "$TEST_DIR/ai_test1.stderr"; then
        log_error "✗ Expected error for no query, but command succeeded"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai (should fail without query)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Correctly failed with no query (exit code: $exit_code)"
        else
            log_error "✗ Expected exit code 64, got $exit_code"
            cat "$TEST_DIR/ai_test1.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai (wrong exit code)")
            return 1
        fi
    fi
    
    # テスト2: エラーハンドリング（存在しないオプション）
    log_info "Test 2: Error handling (invalid option)"
    if "$binary_path" --unknown-option > "$TEST_DIR/ai_test2.stdout" 2> "$TEST_DIR/ai_test2.stderr"; then
        log_error "✗ Expected error for unknown option, but command succeeded"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("ai (should fail with unknown option)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Correctly failed with unknown option (exit code: $exit_code)"
        else
            log_error "✗ Expected exit code 64, got $exit_code"
            cat "$TEST_DIR/ai_test2.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("ai (wrong exit code for unknown option)")
            return 1
        fi
    fi
    
    log_info "ai integration test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# aishコマンドの結合テスト
test_aish_binary() {
    test_case "aish command integration test"
    
    # バイナリをビルド
    local binary_path
    if ! binary_path=$(build_binary "aish" "$PROJECT_ROOT/core/aish" "aish"); then
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (build failed)")
        return 1
    fi
    
    log_info "Binary path: $binary_path"
    
    # テスト用のホームディレクトリを作成
    local test_home_dir="$TEST_DIR/aish_home"
    mkdir -p "$test_home_dir"
    
    # テスト1: バイナリが実行できること（パイプで入力を与えてシェルが動作し、正常終了することを確認）
    log_info "Test 1: Binary execution with pipe input"
    local test_output
    # シェルが終了するように、最後にexitを送る
    if test_output=$(printf 'echo test\nexit\n' | "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test1.stderr"); then
        local exit_code=$?
        if echo "$test_output" | grep -q "test"; then
            log_info "✓ Binary executed successfully and shell output is correct (exit code: $exit_code)"
        else
            log_error "✗ Shell output is incorrect. Expected 'test', got: $test_output"
            cat "$TEST_DIR/aish_test1.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish (shell output incorrect)")
            return 1
        fi
    else
        local exit_code=$?
        log_error "✗ Binary execution failed (exit code: $exit_code)"
        cat "$TEST_DIR/aish_test1.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (execution failed)")
        return 1
    fi
    
    # テスト2: エラーハンドリング（存在しないオプション）
    log_info "Test 2: Error handling (invalid option)"
    if "$binary_path" -d "$test_home_dir" --invalid-option 2> "$TEST_DIR/aish_test2.stderr"; then
        log_error "✗ Should have failed with invalid option"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (error handling)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Correctly failed with invalid option (exit code: $exit_code)"
        else
            log_error "✗ Wrong exit code for invalid option: expected 64, got $exit_code"
            cat "$TEST_DIR/aish_test2.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish (error handling)")
            return 1
        fi
    fi

    # テスト3: 未実装コマンドは非0で終了すること
    log_info "Test 3: Unimplemented command exits non-zero"
    if "$binary_path" -d "$test_home_dir" resume 2> "$TEST_DIR/aish_test_unimpl.stderr"; then
        log_error "✗ Unimplemented command 'resume' should exit non-zero"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (unimplemented command should fail)")
        return 1
    else
        local exit_code=$?
        if [ $exit_code -eq 64 ]; then
            log_info "✓ Unimplemented command correctly failed (exit code: $exit_code)"
        else
            log_error "✗ Wrong exit code for unimplemented command: expected 64, got $exit_code"
            cat "$TEST_DIR/aish_test_unimpl.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("aish (unimplemented command exit code)")
            return 1
        fi
    fi

    # テスト4: デフォルトで2回起動すると別セッションになる（同居しない）
    log_info "Test 4: Two launches use different session dirs (no mixing)"
    local session1 session2
    # AISH_SESSION と AISH_HOME をクリアして、-d オプションのみで動作確認
    # state/session/ を含むパスを抽出（プロンプトや改行の有無に依存しない）
    session1=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test3a.stderr" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1 || true)
    session2=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" 2> "$TEST_DIR/aish_test3b.stderr" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1 || true)
    if [ -z "$session1" ] || [ -z "$session2" ]; then
        log_error "✗ Could not get AISH_SESSION from runs"
        [ -f "$TEST_DIR/aish_test3a.stderr" ] && log_error "Run 1 stderr:" && cat "$TEST_DIR/aish_test3a.stderr"
        [ -f "$TEST_DIR/aish_test3b.stderr" ] && log_error "Run 2 stderr:" && cat "$TEST_DIR/aish_test3b.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (session dir not printed)")
        return 1
    fi
    if [ "$session1" = "$session2" ]; then
        log_error "✗ Two launches shared the same session dir (expected unique): $session1"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (sessions should not mix by default)")
        return 1
    fi
    log_info "✓ Two launches used different session dirs"

    # テスト5: -s 指定で同一セッションに入れる（再開）
    log_info "Test 5: -s specifies same session dir (resume)"
    local resume_dir="$test_home_dir/state/session/resume_test"
    mkdir -p "$resume_dir"
    local out4
    # AISH_SESSION と AISH_HOME をクリアして、-s オプションで動作確認
    out4=$(printf 'echo "$AISH_SESSION"\nexit\n' | env -u AISH_SESSION -u AISH_HOME "$binary_path" -d "$test_home_dir" -s "$resume_dir" 2> "$TEST_DIR/aish_test4.stderr")
    local got_session
    got_session=$(echo "$out4" | tr -d '\r' | grep -oE '/[^[:space:]]*state/session/[^[:space:]]+' | head -1)
    if [ -z "$got_session" ]; then
        log_error "✗ Could not get AISH_SESSION with -s"
        cat "$TEST_DIR/aish_test4.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (-s session dir)")
        return 1
    fi
    # 正規化して比較（got_session はセッションディレクトリのフルパス）
    local want_canon got_canon
    want_canon=$(cd "$resume_dir" && pwd)
    got_canon=$(cd "$got_session" 2>/dev/null && pwd)
    if [ -z "$got_canon" ] || [ "$got_canon" != "$want_canon" ]; then
        log_error "✗ -s session dir mismatch: got $got_canon, want $want_canon"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        FAILED_TESTS+=("aish (-s should use specified dir)")
        return 1
    fi
    log_info "✓ -s uses specified session dir for resume"
    
    log_info "aish integration test PASSED"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# メイン実行
main() {
    echo "========================================="
    echo "Integration Test Suite"
    echo "========================================="
    echo "Project root: $PROJECT_ROOT"
    echo "Build mode: $BUILD_MODE"
    echo "Test directory: $TEST_DIR"
    echo ""
    
    # 結合テストを実行
    log_info "Running integration tests..."
    
    # aiコマンドのテスト
    test_ai_binary || true
    
    # aishコマンドのテスト
    test_aish_binary || true
    
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
        log_info "All integration tests passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some integration tests failed. ✗"
        exit 1
    fi
}

main "$@"

