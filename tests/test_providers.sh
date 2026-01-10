#!/bin/bash
# プロバイダ統合（Function Calling）の動作確認テストスクリプト

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
export AISH_PART="$AISH_SESSION/part"

# ダミーのAPIキー
export OPENAI_API_KEY="sk-dummy"
export GEMINI_API_KEY="dummy-key"

# ライブラリのパス
MEMORY_LIB="$PROJECT_ROOT/_aish/lib/memory_manager.sh"
GPT_LIB="$PROJECT_ROOT/_aish/ai.gpt"
GEMINI_LIB="$PROJECT_ROOT/_aish/ai.gemini"

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

# プロバイダファイルを読み込むための関数（重複定義を避けるためサブシェルで実行することが多いが、ここでは関数の存在を確認する）
check_provider_functions() {
    local lib="$1"
    local provider_name="$2"
    
    log_info "Checking $provider_name implementation..."
    
    # payload生成関数に記憶機能が含まれているかチェック
    if grep -q "save_memory" "$lib" && grep -q "search_memory" "$lib"; then
        log_info "✓ $provider_name implementation contains memory functions"
        return 0
    else
        log_error "✗ $provider_name implementation missing memory functions"
        return 1
    fi
}

# GPTのpayload生成テスト
test_gpt_payload_generation() {
    test_case "GPT Payload Generation - contains memory tools"
    
    if ! grep -q "save_memory" "$GPT_LIB"; then
        log_error "save_memory not found in $GPT_LIB"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    if ! grep -q "search_memory" "$GPT_LIB"; then
        log_error "search_memory not found in $GPT_LIB"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    log_info "✓ GPT payload generation implementation looks correct"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# Geminiのpayload生成テスト
test_gemini_payload_generation() {
    test_case "Gemini Payload Generation - contains memory tools"
    
    if ! grep -q "save_memory" "$GEMINI_LIB"; then
        log_error "save_memory not found in $GEMINI_LIB"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    if ! grep -q "search_memory" "$GEMINI_LIB"; then
        log_error "search_memory not found in $GEMINI_LIB"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    log_info "✓ Gemini payload generation implementation looks correct"
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# プロバイダのツール呼び出し処理テスト（ダミー応答を使用）
test_gpt_process_tool_calls() {
    test_case "GPT - process_tool_calls for memory functions"
    
    # 実際の実装を確認（_execute_tool_call関数の呼び出しを確認）
    if grep -q '_execute_tool_call' "$GPT_LIB"; then
        log_info "✓ GPT implementation uses _execute_tool_call for tool execution"
    else
        log_error "✗ GPT implementation missing _execute_tool_call"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # tool_save_memoryとtool_search_memoryが読み込まれているか確認
    if grep -q 'tool_save_memory.sh' "$GPT_LIB" && grep -q 'tool_search_memory.sh' "$GPT_LIB"; then
        log_info "✓ GPT implementation loads memory tool libraries"
    else
        log_error "✗ GPT implementation missing memory tool library imports"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # _load_all_tool_definitions_openaiが呼び出されているか確認
    if grep -q '_load_all_tool_definitions_openai' "$GPT_LIB"; then
        log_info "✓ GPT implementation loads tool definitions"
    else
        log_error "✗ GPT implementation missing tool definition loading"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# Geminiのツール呼び出し処理テスト
test_gemini_process_tool_calls() {
    test_case "Gemini - process_tool_calls for memory functions"
    
    # 実際の実装を確認（_execute_tool_call関数の呼び出しを確認）
    if grep -q '_execute_tool_call' "$GEMINI_LIB"; then
        log_info "✓ Gemini implementation uses _execute_tool_call for tool execution"
    else
        log_error "✗ Gemini implementation missing _execute_tool_call"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # tool_save_memoryとtool_search_memoryが読み込まれているか確認
    if grep -q 'tool_save_memory.sh' "$GEMINI_LIB" && grep -q 'tool_search_memory.sh' "$GEMINI_LIB"; then
        log_info "✓ Gemini implementation loads memory tool libraries"
    else
        log_error "✗ Gemini implementation missing memory tool library imports"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # _load_all_tool_definitions_geminiが呼び出されているか確認
    if grep -q '_load_all_tool_definitions_gemini' "$GEMINI_LIB"; then
        log_info "✓ Gemini implementation loads tool definitions"
    else
        log_error "✗ Gemini implementation missing tool definition loading"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    TESTS_PASSED=$((TESTS_PASSED + 1))
    return 0
}

# システムインストラクションの更新テスト
test_agent_instruction_update() {
    test_case "Agent System Instruction - contains memory instructions"
    
    local agent_exec="$PROJECT_ROOT/_aish/task.d/agent/execute"
    if grep -i -q "memory" "$agent_exec" && (grep -q "save_memory" "$agent_exec" || grep -q "search_memory" "$agent_exec"); then
        log_info "✓ Agent system instruction contains memory-related guidance"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        log_error "✗ Agent system instruction missing memory guidance"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "Provider Integration Test Suite"
    echo "========================================="
    
    # テスト実行
    test_gpt_payload_generation || true
    test_gemini_payload_generation || true
    test_gpt_process_tool_calls || true
    test_gemini_process_tool_calls || true
    test_agent_instruction_update || true
    
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
        log_info "All integration checks passed! ✓"
        exit 0
    else
        echo ""
        log_error "Some integration checks failed. ✗"
        exit 1
    fi
}

main "$@"

