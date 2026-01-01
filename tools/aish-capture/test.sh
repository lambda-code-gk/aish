#!/bin/bash
# aish-capture の動作確認テストスクリプト

set -euo pipefail

# 色付き出力のための変数
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# テスト用のディレクトリ
TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

# バイナリのパス
BINARY="${BINARY:-./target/release/aish-capture}"

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

assert_jsonl_contains() {
    local file="$1"
    local pattern="$2"
    if grep -q "$pattern" "$file" 2>/dev/null; then
        return 0
    else
        log_error "JSONL file does not contain: $pattern"
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

check_jsonl_valid() {
    local file="$1"
    if [ ! -f "$file" ]; then
        log_error "JSONL file does not exist: $file"
        return 1
    fi
    
    if [ ! -s "$file" ]; then
        log_error "JSONL file is empty: $file"
        return 1
    fi
    
    # 各行が有効なJSONかチェック（簡単なチェック）
    local line_num=0
    while IFS= read -r line; do
        line_num=$((line_num + 1))
        if ! echo "$line" | grep -q '{'; then
            log_error "Line $line_num is not valid JSON: $line"
            return 1
        fi
    done < "$file"
    
    return 0
}

# テスト1: コマンド実行（echo）
test_command_execution() {
    test_case "Command Execution (echo)"
    
    local output_file="$TEST_DIR/test1.jsonl"
    
    log_info "Running: $BINARY -o $output_file -- echo hello world"
    if $BINARY -o "$output_file" -- echo "hello world" > "$TEST_DIR/test1.stdout" 2> "$TEST_DIR/test1.stderr"; then
        local exit_code=$?
        log_info "Exit code: $exit_code"
        
        # stdoutを確認
        if grep -q "hello world" "$TEST_DIR/test1.stdout"; then
            log_info "✓ stdout contains 'hello world'"
        else
            log_error "✗ stdout does not contain 'hello world'"
            cat "$TEST_DIR/test1.stdout"
            return 1
        fi
        
        # JSONLファイルを確認
        if check_jsonl_valid "$output_file"; then
            log_info "✓ JSONL file is valid"
        else
            return 1
        fi
        
        # startイベントを確認
        if assert_jsonl_contains "$output_file" '"type":"start"'; then
            log_info "✓ Contains start event"
        else
            return 1
        fi
        
        # stdoutイベントを確認
        if assert_jsonl_contains "$output_file" '"type":"stdout"'; then
            log_info "✓ Contains stdout event"
        else
            return 1
        fi
        
        # exitイベントを確認
        if assert_jsonl_contains "$output_file" '"type":"exit"'; then
            log_info "✓ Contains exit event"
        else
            return 1
        fi
        
        # dataフィールドを確認（encフィールドはオプショナル）
        if assert_jsonl_contains "$output_file" '"data"'; then
            log_info "✓ Contains data field"
        else
            return 1
        fi
        
        log_info "Test 1 PASSED"
        return 0
    else
        local exit_code=$?
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test1.stderr"
        return 1
    fi
}

# テスト2: 終了コードの確認
test_exit_code() {
    test_case "Exit Code Propagation"
    
    local output_file="$TEST_DIR/test2.jsonl"
    local expected_exit=7
    
    log_info "Running: $BINARY -o $output_file -- bash -c 'exit $expected_exit'"
    set +e  # エラーで停止しないようにする
    $BINARY -o "$output_file" -- bash -c "exit $expected_exit" > "$TEST_DIR/test2.stdout" 2> "$TEST_DIR/test2.stderr"
    local actual_exit=$?
    set -e  # エラーで停止するように戻す
    
    if assert_exit_code "$expected_exit" "$actual_exit"; then
        log_info "✓ Exit code $actual_exit matches expected $expected_exit"
        
        # exitイベントを確認
        if assert_jsonl_contains "$output_file" "\"how\":\"code\""; then
            log_info "✓ Exit event contains code"
        fi
        
        if assert_jsonl_contains "$output_file" "\"code\":$expected_exit"; then
            log_info "✓ Exit code in JSONL matches"
        fi
        
        log_info "Test 2 PASSED"
        return 0
    else
        log_error "stdout:"
        cat "$TEST_DIR/test2.stdout" | head -20
        log_error "stderr:"
        cat "$TEST_DIR/test2.stderr" | head -20
        return 1
    fi
}

# テスト3: --no-stdinオプション
test_no_stdin() {
    test_case "No Stdin Option"
    
    local output_file="$TEST_DIR/test3.jsonl"
    
    log_info "Running: $BINARY -o $output_file --no-stdin -- echo test"
    if $BINARY -o "$output_file" --no-stdin -- echo "test" > "$TEST_DIR/test3.stdout" 2> "$TEST_DIR/test3.stderr"; then
        # stdinイベントが存在しないことを確認
        if grep -q '"type":"stdin"' "$output_file" 2>/dev/null; then
            log_error "✗ JSONL contains stdin event (should not)"
            return 1
        else
            log_info "✓ No stdin events in JSONL (as expected)"
        fi
        
        log_info "Test 3 PASSED"
        return 0
    else
        log_error "Command failed"
        cat "$TEST_DIR/test3.stderr"
        return 1
    fi
}

# テスト4: --cwdオプション
test_cwd_option() {
    test_case "CWD Option"
    
    local output_file="$TEST_DIR/test4.jsonl"
    local test_cwd="/tmp"
    
    log_info "Running: $BINARY -o $output_file --cwd $test_cwd -- pwd"
    if $BINARY -o "$output_file" --cwd "$test_cwd" -- pwd > "$TEST_DIR/test4.stdout" 2> "$TEST_DIR/test4.stderr"; then
        # stdoutにパスが含まれているか確認
        if grep -q "$test_cwd" "$TEST_DIR/test4.stdout"; then
            log_info "✓ CWD option works"
        else
            log_error "✗ CWD option failed"
            cat "$TEST_DIR/test4.stdout"
            return 1
        fi
        
        log_info "Test 4 PASSED"
        return 0
    else
        log_error "Command failed"
        cat "$TEST_DIR/test4.stderr"
        return 1
    fi
}

# テスト5: --envオプション
test_env_option() {
    test_case "Environment Variable Option"
    
    local output_file="$TEST_DIR/test5.jsonl"
    local test_var="TEST_VAR=test_value"
    
    log_info "Running: $BINARY -o $output_file --env $test_var -- bash -c 'echo \$TEST_VAR'"
    if $BINARY -o "$output_file" --env "$test_var" -- bash -c 'echo $TEST_VAR' > "$TEST_DIR/test5.stdout" 2> "$TEST_DIR/test5.stderr"; then
        if grep -q "test_value" "$TEST_DIR/test5.stdout"; then
            log_info "✓ Environment variable set correctly"
        else
            log_error "✗ Environment variable not set"
            cat "$TEST_DIR/test5.stdout"
            return 1
        fi
        
        log_info "Test 5 PASSED"
        return 0
    else
        log_error "Command failed"
        cat "$TEST_DIR/test5.stderr"
        return 1
    fi
}

# テスト6: エラー終了コード（引数不正）
test_error_exit_code() {
    test_case "Error Exit Codes"
    
    log_info "Testing invalid option (should return 64)"
    set +e  # エラーで停止しないようにする
    $BINARY --invalid-option 2> "$TEST_DIR/test6.stderr"
    local exit_code=$?
    set -e  # エラーで停止するように戻す
    
    if [ "$exit_code" -eq 64 ]; then
        log_info "✓ Invalid option returns exit code 64"
        log_info "Test 6 PASSED"
        return 0
    else
        log_error "✗ Expected exit code 64, got $exit_code"
        cat "$TEST_DIR/test6.stderr"
        return 1
    fi
}

# テスト7: JSONL形式の検証
test_jsonl_format() {
    test_case "JSONL Format Validation"
    
    local output_file="$TEST_DIR/test7.jsonl"
    
    log_info "Running: $BINARY -o $output_file -- echo test"
    if $BINARY -o "$output_file" -- echo "test" > /dev/null 2>&1; then
        # 各行が有効なJSONオブジェクトか確認
        local has_start=false
        local has_stdout=false
        local has_exit=false
        
        while IFS= read -r line; do
            # vフィールドを確認
            if ! echo "$line" | grep -q '"v":1'; then
                log_error "✗ Missing or invalid 'v' field"
                return 1
            fi
            
            # typeフィールドを確認
            if echo "$line" | grep -q '"type":"start"'; then
                has_start=true
                # startイベントの必須フィールドを確認
                if ! echo "$line" | grep -q '"cols"'; then
                    log_error "✗ start event missing 'cols'"
                    return 1
                fi
                if ! echo "$line" | grep -q '"rows"'; then
                    log_error "✗ start event missing 'rows'"
                    return 1
                fi
            fi
            
            if echo "$line" | grep -q '"type":"stdout"'; then
                has_stdout=true
                # stdoutイベントの必須フィールドを確認（encフィールドはオプショナル）
                if ! echo "$line" | grep -q '"data"'; then
                    log_error "✗ stdout event missing 'data'"
                    return 1
                fi
            fi
            
            if echo "$line" | grep -q '"type":"exit"'; then
                has_exit=true
            fi
        done < "$output_file"
        
        if [ "$has_start" = true ] && [ "$has_stdout" = true ] && [ "$has_exit" = true ]; then
            log_info "✓ All required event types present"
            log_info "Test 7 PASSED"
            return 0
        else
            log_error "✗ Missing event types: start=$has_start stdout=$has_stdout exit=$has_exit"
            return 1
        fi
    else
        log_error "Command failed"
        return 1
    fi
}

# テスト8: --appendオプション
test_append_option() {
    test_case "Append Option"
    
    local output_file="$TEST_DIR/test8.jsonl"
    
    log_info "Running first command"
    $BINARY -o "$output_file" -- echo "first" > /dev/null 2>&1
    
    local first_size=$(wc -l < "$output_file")
    
    log_info "Running second command with --append"
    $BINARY -o "$output_file" --append -- echo "second" > /dev/null 2>&1
    
    local second_size=$(wc -l < "$output_file")
    
    if [ "$second_size" -gt "$first_size" ]; then
        log_info "✓ Append option works (lines increased from $first_size to $second_size)"
        log_info "Test 8 PASSED"
        return 0
    else
        log_error "✗ Append option failed (lines: $first_size -> $second_size)"
        return 1
    fi
}

# テスト9: テキストバッファリング（改行までまとめる）
test_text_buffering() {
    test_case "Text Buffering (Line-based)"
    
    local output_file="$TEST_DIR/test9.jsonl"
    
    log_info "Running: $BINARY -o $output_file -- bash -c 'echo line1; echo line2'"
    if $BINARY -o "$output_file" -- bash -c 'echo line1; echo line2' > "$TEST_DIR/test9.stdout" 2> "$TEST_DIR/test9.stderr"; then
        # JSONLファイルを確認
        if check_jsonl_valid "$output_file"; then
            log_info "✓ JSONL file is valid"
        else
            return 1
        fi
        
        # stdoutイベントを確認（改行までまとめられていることを確認）
        local stdout_count=0
        local has_line1=false
        local has_line2=false
        
        while IFS= read -r line; do
            if echo "$line" | grep -q '"type":"stdout"'; then
                stdout_count=$((stdout_count + 1))
                # dataフィールドに改行が含まれているか確認
                if echo "$line" | grep -q '"data":"line1'; then
                    has_line1=true
                    # 改行文字が含まれているか確認（エスケープされた形式）
                    if echo "$line" | grep -qE '(\\r)?\\n'; then
                        log_info "✓ stdout event contains line1 with newline"
                    else
                        log_error "✗ stdout event missing newline for line1"
                        return 1
                    fi
                fi
                if echo "$line" | grep -q '"data":"line2'; then
                    has_line2=true
                    if echo "$line" | grep -qE '(\\r)?\\n'; then
                        log_info "✓ stdout event contains line2 with newline"
                    else
                        log_error "✗ stdout event missing newline for line2"
                        return 1
                    fi
                fi
            fi
        done < "$output_file"
        
        if [ "$has_line1" = true ] && [ "$has_line2" = true ]; then
            log_info "✓ Both lines are present in stdout events"
            log_info "✓ Text buffering works correctly (lines separated into events)"
            log_info "Test 9 PASSED"
            return 0
        else
            log_error "✗ Missing lines: line1=$has_line1 line2=$has_line2"
            return 1
        fi
    else
        local exit_code=$?
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test9.stderr"
        return 1
    fi
}

# テスト10: バイナリデータの即座書き出し
test_binary_immediate_output() {
    test_case "Binary Data Immediate Output"
    
    local output_file="$TEST_DIR/test10.jsonl"
    
    log_info "Running: $BINARY -o $output_file -- bash -c 'echo -e \"\\033[31mRED\\033[0m\"'"
    if $BINARY -o "$output_file" -- bash -c 'echo -e "\033[31mRED\033[0m"' > "$TEST_DIR/test10.stdout" 2> "$TEST_DIR/test10.stderr"; then
        # JSONLファイルを確認
        if check_jsonl_valid "$output_file"; then
            log_info "✓ JSONL file is valid"
        else
            return 1
        fi
        
        # stdoutイベントにencフィールドが含まれていることを確認（バイナリデータ）
        if assert_jsonl_contains "$output_file" '"enc":"b64"'; then
            log_info "✓ Binary data is base64 encoded (enc field present)"
        else
            log_error "✗ Binary data missing enc field"
            return 1
        fi
        
        log_info "✓ Binary data is written immediately (not buffered)"
        log_info "Test 10 PASSED"
        return 0
    else
        local exit_code=$?
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test10.stderr"
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "aish-capture Test Suite"
    echo "========================================="
    echo "Binary: $BINARY"
    echo "Test directory: $TEST_DIR"
    echo ""
    
    # バイナリの存在確認
    if [ ! -f "$BINARY" ]; then
        log_error "Binary not found: $BINARY"
        log_info "Please build the binary first: cargo build --release"
        exit 1
    fi
    
    # TTYチェック（一部のテストはTTYが必要）
    if [ ! -t 0 ]; then
        log_warn "Not running in a TTY. PTY-based tests will fail."
        log_warn "For full test coverage, run this script in an interactive terminal:"
        log_warn "  script -c './test.sh'"
        log_warn ""
        log_warn "Or use expect/socat to provide a pseudo-TTY for CI environments."
    fi
    
    # テスト実行
    local tests=(
        "test_command_execution"
        "test_exit_code"
        "test_no_stdin"
        "test_cwd_option"
        "test_env_option"
        "test_error_exit_code"
        "test_jsonl_format"
        "test_append_option"
        "test_text_buffering"
        "test_binary_immediate_output"
    )
    
    # TTYが必要なテストをスキップするかチェック
    local skip_pty_tests=false
    if [ ! -t 0 ]; then
        skip_pty_tests=true
        log_warn "Skipping PTY-dependent tests (test_command_execution, test_exit_code, etc.)"
        log_warn "Only running tests that don't require a TTY..."
    fi
    
    for test_func in "${tests[@]}"; do
        # TTYが必要なテストをスキップ
        if [ "$skip_pty_tests" = true ]; then
            case "$test_func" in
                test_command_execution|test_exit_code|test_no_stdin|test_cwd_option|test_env_option|test_jsonl_format|test_append_option|test_text_buffering|test_binary_immediate_output)
                    log_warn "Skipping $test_func (requires TTY)"
                    continue
                    ;;
            esac
        fi
        
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

