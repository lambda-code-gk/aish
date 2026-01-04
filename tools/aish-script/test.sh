#!/bin/bash
# aish-script の動作確認テストスクリプト

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
BINARY="${BINARY:-./target/release/aish-script}"
if [ ! -f "$BINARY" ]; then
    BINARY="./target/debug/aish-script"
fi

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

# テスト1: 基本的な文字列マッチング
test_basic_string_match() {
    test_case "Basic string matching"
    
    local input="Password: "
    local expected_output="mypass"
    
    log_info "Running: echo \"$input\" | $BINARY -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY -e 'match "Password:" then send "mypass\n"' 2> "$TEST_DIR/test1.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Correct output received"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test1.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test1.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト2: 複数ルールのマッチング
test_multiple_rules() {
    test_case "Multiple rules matching"
    
    local input=$'Username: \nPassword: '
    local expected_user="user"
    local expected_pass="pass"
    
    log_info "Running: echo -e \"$input\" | $BINARY -e 'match \"Username:\" then send \"user\\n\"; match \"Password:\" then send \"pass\\n\"'"
    
    local output=$(echo -e "$input" | $BINARY -e 'match "Username:" then send "user\n"; match "Password:" then send "pass\n"' 2> "$TEST_DIR/test2.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_user" && echo "$output" | grep -q "$expected_pass"; then
            log_info "✓ Both rules matched correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output'"
            cat "$TEST_DIR/test2.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test2.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト3: スクリプトファイルからの読み込み
test_script_file() {
    test_case "Script file reading"
    
    local script_file="$TEST_DIR/test3.script"
    local input="Password: "
    local expected_output="fromfile"
    
    # スクリプトファイルを作成
    echo 'match "Password:" then send "fromfile\n"' > "$script_file"
    
    log_info "Running: echo \"$input\" | $BINARY -s $script_file"
    
    local output=$(echo -n "$input" | $BINARY -s "$script_file" 2> "$TEST_DIR/test3.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Script file read correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test3.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test3.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト4: 正規表現パターンのマッチング
test_regex_pattern() {
    test_case "Regex pattern matching"
    
    local input="Enter password: "
    local expected_output="mypass"
    
    log_info "Running: echo \"$input\" | $BINARY -e 'match /password:\s*/i then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY -e 'match /password:\s*/i then send "mypass\n"' 2> "$TEST_DIR/test4.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Regex pattern matched correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test4.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test4.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト5: 正規表現フラグ `i` (大文字小文字無視) の動作確認
test_regex_flag_i() {
    test_case "Regex flag 'i' (case-insensitive) test"
    
    local input="PASSWORD: "
    local expected_output="mypass"
    
    log_info "Running: echo \"$input\" | $BINARY -e 'match /password:/i then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY -e 'match /password:/i then send "mypass\n"' 2> "$TEST_DIR/test5.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Case-insensitive regex matched correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test5.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test5.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト6: 文字列マッチと正規表現マッチの混合
test_mixed_string_and_regex() {
    test_case "Mixed string and regex matching"
    
    local input=$'Username: \nEnter pass: '
    local expected_user="user"
    local expected_pass="mypass"
    
    log_info "Running: echo -e \"$input\" | $BINARY -e 'match \"Username:\" then send \"user\\n\"; match /pass:/i then send \"mypass\\n\"'"
    
    local output=$(echo -e "$input" | $BINARY -e 'match "Username:" then send "user\n"; match /pass:/i then send "mypass\n"' 2> "$TEST_DIR/test6.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_user" && echo "$output" | grep -q "$expected_pass"; then
            log_info "✓ Mixed string and regex matched correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output'"
            cat "$TEST_DIR/test6.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test6.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト7: タイムアウト機能
test_timeout() {
    test_case "Timeout functionality"
    
    local input="Hello"
    # パターンが含まれない入力でタイムアウトが発生することを確認
    
    log_info "Running: echo \"$input\" | $BINARY -e 'match \"Password:\" timeout 1 then send \"mypass\\n\"'"
    
    # タイムアウトでエラー終了することを期待（exit code != 0）
    set +e  # 一時的にエラーを許可
    echo -n "$input" | timeout 2s $BINARY -e 'match "Password:" timeout 1 then send "mypass\n"' > "$TEST_DIR/test7.stdout" 2> "$TEST_DIR/test7.stderr"
    local exit_code=$?
    set -e  # エラーを再び有効化
    
    if [ $exit_code -ne 0 ]; then
        # タイムアウトエラーが表示されているか確認
        if grep -q -i "timeout" "$TEST_DIR/test7.stderr"; then
            log_info "✓ Timeout handled correctly (exit code: $exit_code)"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Timeout not handled correctly (exit code: $exit_code)"
            cat "$TEST_DIR/test7.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "✗ Command succeeded but should have timed out"
        cat "$TEST_DIR/test7.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト8: デバッグモードの動作確認
test_debug_mode() {
    test_case "Debug mode functionality"
    
    local input="Password: "
    local expected_output="mypass"
    
    log_info "Running: echo \"$input\" | $BINARY --debug -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY --debug -e 'match "Password:" then send "mypass\n"' 2> "$TEST_DIR/test8.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        # デバッグ情報が出力されているか確認
        if grep -q -i "Parsed.*rules\|Matched pattern\|Sent to stdout" "$TEST_DIR/test8.stderr"; then
            if echo "$output" | grep -q "$expected_output"; then
                log_info "✓ Debug information was output and correct output received"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ Debug info found but incorrect output: '$output'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ Debug information not found in stderr"
            cat "$TEST_DIR/test8.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test8.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト9: verboseモードの動作確認
test_verbose_mode() {
    test_case "Verbose mode functionality"
    
    local input="Password: "
    local expected_output="mypass"
    
    log_info "Running: echo \"$input\" | $BINARY --verbose -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY --verbose -e 'match "Password:" then send "mypass\n"' 2> "$TEST_DIR/test9.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        # verbose情報が出力されているか確認
        if grep -q -i "Matched pattern\|Sent to stdout" "$TEST_DIR/test9.stderr"; then
            if echo "$output" | grep -q "$expected_output"; then
                log_info "✓ Verbose information was output and correct output received"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ Verbose info found but incorrect output: '$output'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ Verbose information not found in stderr"
            cat "$TEST_DIR/test9.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test9.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト10: 複数行入力の処理
test_multiline_input() {
    test_case "Multiline input processing"
    
    local input=$'Line 1\nLine 2\nPassword: \nLine 4'
    local expected_output="mypass"
    
    log_info "Running: echo -e \"$input\" | $BINARY -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    local output=$(echo -e "$input" | $BINARY -e 'match "Password:" then send "mypass\n"' 2> "$TEST_DIR/test10.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Multiline input processed correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test10.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test10.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト11: パターンがマッチしない場合
test_no_match() {
    test_case "No pattern match"
    
    local input="Hello World"
    # パターンがマッチしない場合、何も出力されない（正常終了）
    
    log_info "Running: echo \"$input\" | $BINARY -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    local output=$(echo -n "$input" | $BINARY -e 'match "Password:" then send "mypass\n"' 2> "$TEST_DIR/test11.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if [ -z "$output" ]; then
            log_info "✓ No output when pattern doesn't match (correct behavior)"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Unexpected output: '$output'"
            cat "$TEST_DIR/test11.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test11.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト12: 逐次処理（状態遷移）
test_sequential_state_transition() {
    test_case "Sequential state transition"
    
    local input=$'Step 1\nStep 2\nStep 3'
    local expected_output=$'response1\nresponse2\nresponse3\n'
    
    log_info "Running: echo -e \"$input\" | $BINARY -e 'state \"start\"; in state \"start\" match \"Step 1\" then send \"response1\\n\" goto \"state2\"; in state \"state2\" match \"Step 2\" then send \"response2\\n\" goto \"state3\"; in state \"state3\" match \"Step 3\" then send \"response3\\n\"'"
    
    local output=$(echo -e "$input" | $BINARY -e 'state "start"; in state "start" match "Step 1" then send "response1\n" goto "state2"; in state "state2" match "Step 2" then send "response2\n" goto "state3"; in state "state3" match "Step 3" then send "response3\n"' 2> "$TEST_DIR/test12.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "response1" && echo "$output" | grep -q "response2" && echo "$output" | grep -q "response3"; then
            log_info "✓ Sequential state transition worked correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output'"
            cat "$TEST_DIR/test12.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test12.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト13: 条件分岐（マッチしたパターンに基づく分岐）
test_conditional_branch() {
    test_case "Conditional branch based on pattern match"
    
    local input="Choice: yes"
    local expected_output="yes_response"
    
    log_info "Running: echo \"$input\" | $BINARY -e 'state \"start\"; in state \"start\" match \"Choice: yes\" then send \"yes_response\\n\" goto \"yes_state\"; in state \"start\" match \"Choice: no\" then send \"no_response\\n\" goto \"no_state\"'"
    
    local output=$(echo -n "$input" | $BINARY -e 'state "start"; in state "start" match "Choice: yes" then send "yes_response\n" goto "yes_state"; in state "start" match "Choice: no" then send "no_response\n" goto "no_state"' 2> "$TEST_DIR/test13.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ Conditional branch worked correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test13.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test13.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト14: ステートブロック構文（一つのステート内で複数のパターン）
test_state_block_syntax() {
    test_case "State block syntax (multiple patterns in one state)"
    
    local input="Choice: yes"
    local expected_output="yes_response"
    
    log_info "Running: echo \"$input\" | $BINARY -e 'state \"start\" { match \"Choice: yes\" then send \"yes_response\\n\" goto \"yes_state\"; match \"Choice: no\" then send \"no_response\\n\" goto \"no_state\"; }'"
    
    local output=$(echo -n "$input" | $BINARY -e 'state "start" { match "Choice: yes" then send "yes_response\n" goto "yes_state"; match "Choice: no" then send "no_response\n" goto "no_state"; }' 2> "$TEST_DIR/test14.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "$expected_output"; then
            log_info "✓ State block syntax worked correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output' (expected: '$expected_output')"
            cat "$TEST_DIR/test14.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test14.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト15: ステートブロック構文での逐次処理
test_state_block_sequential() {
    test_case "Sequential processing with state block syntax"
    
    local input=$'Step 1\nStep 2\nStep 3'
    local expected_output=$'response1\nresponse2\nresponse3\n'
    
    log_info "Running: echo -e \"$input\" | $BINARY -e 'state \"start\" { match \"Step 1\" then send \"response1\\n\" goto \"state2\"; }; state \"state2\" { match \"Step 2\" then send \"response2\\n\" goto \"state3\"; }; state \"state3\" { match \"Step 3\" then send \"response3\\n\"; }'"
    
    local output=$(echo -e "$input" | $BINARY -e 'state "start" { match "Step 1" then send "response1\n" goto "state2"; }; state "state2" { match "Step 2" then send "response2\n" goto "state3"; }; state "state3" { match "Step 3" then send "response3\n"; }' 2> "$TEST_DIR/test15.stderr")
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        if echo "$output" | grep -q "response1" && echo "$output" | grep -q "response2" && echo "$output" | grep -q "response3"; then
            log_info "✓ Sequential processing with state block syntax worked correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output: '$output'"
            cat "$TEST_DIR/test15.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test15.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト16: 一つのステート内で複数のパターン（条件分岐）
test_multiple_patterns_in_state() {
    test_case "Multiple patterns in one state (conditional branching)"
    
    # テスト1: "Step 1"が来た場合
    local input1="Step 1"
    local expected_output1="response1"
    
    log_info "Running: echo \"$input1\" | $BINARY -e 'state \"start\" { match \"Step 1\" then send \"response1\\n\" goto \"state2\"; match \"Finish\" then send \"finish\\n\" goto \"state3\"; }'"
    
    local output1=$(echo -n "$input1" | $BINARY -e 'state "start" { match "Step 1" then send "response1\n" goto "state2"; match "Finish" then send "finish\n" goto "state3"; }' 2> "$TEST_DIR/test16a.stderr")
    local exit_code1=$?
    
    if [ $exit_code1 -eq 0 ]; then
        if echo "$output1" | grep -q "$expected_output1"; then
            log_info "✓ Pattern 'Step 1' matched correctly"
        else
            log_error "✗ Incorrect output for 'Step 1': '$output1' (expected: '$expected_output1')"
            cat "$TEST_DIR/test16a.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code1"
        cat "$TEST_DIR/test16a.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # テスト2: "Finish"が来た場合
    local input2="Finish"
    local expected_output2="finish"
    
    log_info "Running: echo \"$input2\" | $BINARY -e 'state \"start\" { match \"Step 1\" then send \"response1\\n\" goto \"state2\"; match \"Finish\" then send \"finish\\n\" goto \"state3\"; }'"
    
    local output2=$(echo -n "$input2" | $BINARY -e 'state "start" { match "Step 1" then send "response1\n" goto "state2"; match "Finish" then send "finish\n" goto "state3"; }' 2> "$TEST_DIR/test16b.stderr")
    local exit_code2=$?
    
    if [ $exit_code2 -eq 0 ]; then
        if echo "$output2" | grep -q "$expected_output2"; then
            log_info "✓ Pattern 'Finish' matched correctly"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Incorrect output for 'Finish': '$output2' (expected: '$expected_output2')"
            cat "$TEST_DIR/test16b.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code2"
        cat "$TEST_DIR/test16b.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト17: sendを省略した構文（then gotoのみ）
test_then_goto_without_send() {
    test_case "then goto without send (state transition only)"
    
    # テスト1: "Step 1"が来た場合（sendあり）
    local input1="Step 1"
    local expected_output1="response1"
    
    log_info "Running: echo \"$input1\" | $BINARY -e 'state \"start\" { match \"Step 1\" then send \"response1\\n\" goto \"state2\"; match \"Finish\" then goto \"state3\"; }'"
    
    local output1=$(echo -n "$input1" | $BINARY -e 'state "start" { match "Step 1" then send "response1\n" goto "state2"; match "Finish" then goto "state3"; }' 2> "$TEST_DIR/test17a.stderr")
    local exit_code1=$?
    
    if [ $exit_code1 -eq 0 ]; then
        if echo "$output1" | grep -q "$expected_output1"; then
            log_info "✓ Pattern 'Step 1' with send matched correctly"
        else
            log_error "✗ Incorrect output for 'Step 1': '$output1' (expected: '$expected_output1')"
            cat "$TEST_DIR/test17a.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code1"
        cat "$TEST_DIR/test17a.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    
    # テスト2: "Finish"が来た場合（sendなし、状態遷移のみ）
    local input2="Finish"
    
    log_info "Running: echo \"$input2\" | $BINARY -e 'state \"start\" { match \"Step 1\" then send \"response1\\n\" goto \"state2\"; match \"Finish\" then goto \"state3\"; }'"
    
    local output2=$(echo -n "$input2" | $BINARY -e 'state "start" { match "Step 1" then send "response1\n" goto "state2"; match "Finish" then goto "state3"; }' 2> "$TEST_DIR/test17b.stderr")
    local exit_code2=$?
    
    if [ $exit_code2 -eq 0 ]; then
        # sendがないので、出力は空であるべき
        if [ -z "$output2" ]; then
            log_info "✓ Pattern 'Finish' without send matched correctly (no output, state transition only)"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Unexpected output for 'Finish': '$output2' (expected: empty)"
            cat "$TEST_DIR/test17b.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "Command failed with exit code: $exit_code2"
        cat "$TEST_DIR/test17b.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# メイン実行
main() {
    echo "========================================="
    echo "aish-script Test Suite"
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
    
    # テスト実行
    local tests=(
        "test_basic_string_match"
        "test_multiple_rules"
        "test_script_file"
        "test_regex_pattern"
        "test_regex_flag_i"
        "test_mixed_string_and_regex"
        "test_timeout"
        "test_debug_mode"
        "test_verbose_mode"
        "test_multiline_input"
        "test_no_match"
        "test_sequential_state_transition"
        "test_conditional_branch"
        "test_state_block_syntax"
        "test_state_block_sequential"
        "test_multiple_patterns_in_state"
        "test_then_goto_without_send"
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
