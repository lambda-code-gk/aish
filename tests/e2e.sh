#!/bin/bash
# E2E（End-to-End）テストを実行するスクリプト
# 実際のLLM APIを呼び出してaiコマンドの動作を確認する

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# プロジェクトルートの取得
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# ビルドモード（デフォルトはrelease）
BUILD_MODE="${BUILD_MODE:-release}"
TARGET_DIR="$BUILD_MODE"

# テスト結果のカウント
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
FAILED_TESTS=()
SKIPPED_TESTS=()

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_skip() {
    echo -e "${BLUE}[SKIP]${NC} $*"
}

test_case() {
    echo ""
    echo "========================================="
    echo "Test: $1"
    echo "========================================="
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
    
    # プロジェクトディレクトリに移動してビルド
    (
        cd "$project_path"
        if [ "$BUILD_MODE" == "debug" ]; then
            cargo build >&2
        else
            cargo build --release >&2
        fi
    )
    
    local binary_path="$project_path/target/$TARGET_DIR/$binary_name"
    if [ ! -f "$binary_path" ]; then
        log_error "Binary not found after build: $binary_path" >&2
        return 1
    fi
    
    echo "$binary_path"
}

# プロバイダのAPIキーが設定されているかチェック
check_provider_available() {
    local provider="$1"
    case "$provider" in
        gemini)
            [ -n "${GEMINI_API_KEY:-}" ]
            ;;
        gpt|openai)
            [ -n "${OPENAI_API_KEY:-}" ]
            ;;
        sakura-qwen)
            [ -n "${SAKURA_API_KEY:-}" ]
            ;;
        echo)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

# 個別プロバイダのE2Eテスト
test_provider_e2e() {
    local binary_path="$1"
    local provider="$2"
    local test_name="ai -p $provider 'say hello'"
    
    test_case "$test_name"
    
    if ! check_provider_available "$provider"; then
        log_skip "Skipping $provider: API key is not set"
        TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
        SKIPPED_TESTS+=("$provider (API key not set)")
        return 0
    fi
    
    log_info "Running: $binary_path -p $provider 'say hello'"
    
    local output_file="$TEST_DIR/e2e_${provider}.stdout"
    local error_file="$TEST_DIR/e2e_${provider}.stderr"
    
    if timeout 30 "$binary_path" -p "$provider" 'say hello' > "$output_file" 2> "$error_file"; then
        local output
        output=$(cat "$output_file")
        
        if [ -n "$output" ]; then
            log_info "✓ $provider: Response received"
            log_info "  Response preview: ${output:0:100}..."
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ $provider: Empty response"
            cat "$error_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("$provider (empty response)")
            return 1
        fi
    else
        local exit_code=$?
        if [ $exit_code -eq 124 ]; then
            log_error "✗ $provider: Timeout (30s)"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("$provider (timeout)")
        else
            log_error "✗ $provider: Command failed (exit code: $exit_code)"
            echo "--- stderr ---"
            cat "$error_file"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            FAILED_TESTS+=("$provider (exit code $exit_code)")
        fi
        return 1
    fi
}

main() {
    echo "========================================="
    echo "E2E Test Suite"
    echo "========================================="
    echo "Project root: $PROJECT_ROOT"
    echo "Build mode: $BUILD_MODE"
    echo "Test directory: $TEST_DIR"
    echo ""
    
    # aiバイナリをビルド
    log_info "Building ai binary..."
    local binary_path
    if ! binary_path=$(build_binary "ai" "$PROJECT_ROOT/core/ai" "ai"); then
        log_error "Failed to build ai binary"
        exit 1
    fi
    
    log_info "Binary path: $binary_path"
    echo ""
    
    # テスト実行
    log_info "Running E2E tests..."
    test_provider_e2e "$binary_path" "echo" || true
    test_provider_e2e "$binary_path" "gemini" || true
    test_provider_e2e "$binary_path" "gpt" || true
    test_provider_e2e "$binary_path" "sakura-qwen" || true
    
    # 結果表示
    echo ""
    echo "========================================="
    echo "E2E Test Summary"
    echo "========================================="
    echo "Passed:  $TESTS_PASSED"
    echo "Failed:  $TESTS_FAILED"
    echo "Skipped: $TESTS_SKIPPED"
    echo "Total:   $((TESTS_PASSED + TESTS_FAILED + TESTS_SKIPPED))"
    
    if [ $TESTS_FAILED -eq 0 ]; then
        log_info "\nAll executed E2E tests passed! ✓"
        exit 0
    else
        log_error "\nSome E2E tests failed. ✗"
        exit 1
    fi
}

main "$@"
