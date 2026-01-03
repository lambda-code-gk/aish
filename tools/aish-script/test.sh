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

# テスト1: 基本的な文字列マッチングとFIFO送信
test_basic_string_match() {
    test_case "Basic string matching and FIFO send"
    
    local log_file="$TEST_DIR/test1.jsonl"
    local fifo_path="$TEST_DIR/test1.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1002,"type":"stdout","n":5,"data":"Done\n"}
{"v":1,"t_ms":1003,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test1.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test1.stdout" 2> "$TEST_DIR/test1.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if [ "$fifo_content" = "mypass" ] || [ "$fifo_content" = $'mypass\n' ]; then
                log_info "✓ FIFO received correct data"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test1.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト2: 複数ルールのマッチング
test_multiple_rules() {
    test_case "Multiple rules matching"
    
    local log_file="$TEST_DIR/test2.jsonl"
    local fifo_path="$TEST_DIR/test2.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":11,"data":"Username: "}
{"v":1,"t_ms":1002,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1003,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test2.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（複数ルール）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Username:\" then send \"user\\n\"; match \"Password:\" then send \"pass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Username:" then send "user\n"; match "Password:" then send "pass\n"' > "$TEST_DIR/test2.stdout" 2> "$TEST_DIR/test2.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認（userとpassの両方）
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if echo "$fifo_content" | grep -q "user" && echo "$fifo_content" | grep -q "pass"; then
                log_info "✓ FIFO received both user and pass"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test2.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト3: スクリプトファイルからの読み込み
test_script_file() {
    test_case "Script file reading"
    
    local log_file="$TEST_DIR/test3.jsonl"
    local script_file="$TEST_DIR/test3.script"
    local fifo_path="$TEST_DIR/test3.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # スクリプトファイルを作成
    echo 'match "Password:" then send "fromfile\n"' > "$script_file"
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test3.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（スクリプトファイルから）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -s $script_file"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -s "$script_file" > "$TEST_DIR/test3.stdout" 2> "$TEST_DIR/test3.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if echo "$fifo_content" | grep -q "fromfile"; then
                log_info "✓ FIFO received data from script file"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test3.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト4: ANSIエスケープシーケンスを含むログファイル（Phase 1.5対応）
test_ansi_escape_sequence() {
    test_case "ANSI escape sequence in log file (Phase 1.5)"
    
    local log_file="$TEST_DIR/test4.jsonl"
    local fifo_path="$TEST_DIR/test4.fifo"
    
    # テスト用のJSONLログファイルを作成（ANSIエスケープシーケンスを\u001b形式で記録）
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":20,"data":"\u001b[31mPassword: \u001b[0m"}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test4.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（ANSIエスケープシーケンスを含むパターンでもマッチする）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test4.stdout" 2> "$TEST_DIR/test4.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if [ "$fifo_content" = "mypass" ] || [ "$fifo_content" = $'mypass\n' ]; then
                log_info "✓ FIFO received correct data (ANSI escape sequence handled)"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test4.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト5: リアルタイム監視（--followオプション）
test_realtime_follow() {
    test_case "Real-time file monitoring (--follow option)"
    
    local log_file="$TEST_DIR/test5.jsonl"
    local fifo_path="$TEST_DIR/test5.fifo"
    
    # 初期ログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":11,"data":"Username: "}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test5.fifo_output"
    timeout 3s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptをバックグラウンドで実行（--followオプション付き）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path --follow -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    timeout 3s $BINARY -f "$log_file" --input-fifo "$fifo_path" --follow -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test5.stdout" 2> "$TEST_DIR/test5.stderr" &
    local script_pid=$!
    
    # 少し待ってから新しい行を追加
    sleep 0.5
    echo '{"v":1,"t_ms":1002,"type":"stdout","n":13,"data":"Password: "}' >> "$log_file"
    
    # 少し待つ
    sleep 0.5
    
    # プロセスを終了
    kill "$script_pid" 2>/dev/null || true
    wait "$script_pid" 2>/dev/null || true
    kill "$fifo_reader_pid" 2>/dev/null || true
    wait "$fifo_reader_pid" 2>/dev/null || true
    
    # FIFOに正しいデータが送信されたか確認
    if [ -f "$fifo_output" ]; then
        local fifo_content=$(cat "$fifo_output")
        if echo "$fifo_content" | grep -q "mypass"; then
            log_info "✓ FIFO received data from real-time monitoring"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ FIFO received incorrect data: '$fifo_content'"
            log_error "stderr:"
            cat "$TEST_DIR/test5.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        log_error "✗ FIFO output file not created"
        log_error "stderr:"
        cat "$TEST_DIR/test5.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト6: 正規表現パターンのマッチング（Phase 4）
test_regex_pattern() {
    test_case "Regex pattern matching (Phase 4)"
    
    local log_file="$TEST_DIR/test6.jsonl"
    local fifo_path="$TEST_DIR/test6.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":15,"data":"Enter password: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test6.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（正規表現パターン）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match /password:\s*/i then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match /password:\s*/i then send "mypass\n"' > "$TEST_DIR/test6.stdout" 2> "$TEST_DIR/test6.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if [ "$fifo_content" = "mypass" ] || [ "$fifo_content" = $'mypass\n' ]; then
                log_info "✓ FIFO received correct data (regex pattern matched)"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                cat "$TEST_DIR/test6.stderr"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            cat "$TEST_DIR/test6.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test6.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト7: 正規表現フラグ `i` (大文字小文字無視) の動作確認（Phase 4）
test_regex_flag_i() {
    test_case "Regex flag 'i' (case-insensitive) test (Phase 4)"
    
    local log_file="$TEST_DIR/test7.jsonl"
    local fifo_path="$TEST_DIR/test7.fifo"
    
    # テスト用のJSONLログファイルを作成（大文字小文字が混在）
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":15,"data":"PASSWORD: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test7.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（正規表現パターン、iフラグ付き）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match /password:/i then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match /password:/i then send "mypass\n"' > "$TEST_DIR/test7.stdout" 2> "$TEST_DIR/test7.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if [ "$fifo_content" = "mypass" ] || [ "$fifo_content" = $'mypass\n' ]; then
                log_info "✓ FIFO received correct data (case-insensitive regex matched)"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                cat "$TEST_DIR/test7.stderr"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            cat "$TEST_DIR/test7.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test7.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト8: 文字列マッチと正規表現マッチの混合（Phase 4）
test_mixed_string_and_regex() {
    test_case "Mixed string and regex matching (Phase 4)"
    
    local log_file="$TEST_DIR/test8.jsonl"
    local fifo_path="$TEST_DIR/test8.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":11,"data":"Username: "}
{"v":1,"t_ms":1002,"type":"stdout","n":15,"data":"Enter pass: "}
{"v":1,"t_ms":1003,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test8.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（文字列マッチと正規表現マッチの混合）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Username:\" then send \"user\\n\"; match /pass:/i then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Username:" then send "user\n"; match /pass:/i then send "mypass\n"' > "$TEST_DIR/test8.stdout" 2> "$TEST_DIR/test8.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認（userとmypassの両方）
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if echo "$fifo_content" | grep -q "user" && echo "$fifo_content" | grep -q "mypass"; then
                log_info "✓ FIFO received both user and mypass (mixed string and regex)"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                cat "$TEST_DIR/test8.stderr"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            cat "$TEST_DIR/test8.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test8.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト9: タイムアウト機能（Phase 5）
test_timeout() {
    test_case "Timeout functionality (Phase 5)"
    
    local log_file="$TEST_DIR/test9.jsonl"
    local fifo_path="$TEST_DIR/test9.fifo"
    
    # テスト用のJSONLログファイルを作成（パターンが含まれない）
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":10,"data":"Hello\n"}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行（ハング防止のため）
    cat "$fifo_path" > /dev/null 2>&1 &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（タイムアウト付き、パターンが見つからない）
    # 通常モードでは、ファイルを読み終わった後にタイムアウトをチェックする
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Password:\" timeout 1 then send \"mypass\\n\"'"
    
    # タイムアウトでエラー終了することを期待（exit code != 0）
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Password:" timeout 1 then send "mypass\n"' > "$TEST_DIR/test9.stdout" 2> "$TEST_DIR/test9.stderr"; then
        # 成功した場合は失敗（タイムアウトでエラー終了すべき）
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "✗ Command succeeded but should have timed out"
        cat "$TEST_DIR/test9.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        # タイムアウトエラーが表示されているか確認
        if grep -q -i "timeout" "$TEST_DIR/test9.stderr" && [ $exit_code -eq 1 ]; then
            log_info "✓ Timeout handled correctly (exit code: $exit_code)"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Timeout not handled correctly (exit code: $exit_code)"
            cat "$TEST_DIR/test9.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    fi
}

# テスト10: 不正なJSONL行の処理（Phase 5）
test_invalid_jsonl() {
    test_case "Invalid JSONL line handling (Phase 5)"
    
    local log_file="$TEST_DIR/test10.jsonl"
    local fifo_path="$TEST_DIR/test10.fifo"
    
    # テスト用のJSONLログファイルを作成（不正な行を含む）
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
invalid json line
{"v":1,"t_ms":1001,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test10.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（不正な行を無視して継続することを期待）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test10.stdout" 2> "$TEST_DIR/test10.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # FIFOに正しいデータが送信されたか確認（不正な行を無視して処理が継続された）
        if [ -f "$fifo_output" ]; then
            local fifo_content=$(cat "$fifo_output")
            if [ "$fifo_content" = "mypass" ] || [ "$fifo_content" = $'mypass\n' ]; then
                log_info "✓ FIFO received correct data (invalid JSONL line ignored)"
                TESTS_PASSED=$((TESTS_PASSED + 1))
                return 0
            else
                log_error "✗ FIFO received incorrect data: '$fifo_content'"
                TESTS_FAILED=$((TESTS_FAILED + 1))
                return 1
            fi
        else
            log_error "✗ FIFO output file not created"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test10.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト11: デバッグモードの動作確認（Phase 5）
test_debug_mode() {
    test_case "Debug mode functionality (Phase 5)"
    
    local log_file="$TEST_DIR/test11.jsonl"
    local fifo_path="$TEST_DIR/test11.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test11.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（--debugオプション付き）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path --debug -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" --debug -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test11.stdout" 2> "$TEST_DIR/test11.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # デバッグ情報が出力されているか確認
        if grep -q -i "Parsed rules\|Matched pattern\|Sent to FIFO" "$TEST_DIR/test11.stderr"; then
            log_info "✓ Debug information was output"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Debug information not found in stderr"
            cat "$TEST_DIR/test11.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test11.stderr"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト12: verboseモードの動作確認（Phase 5）
test_verbose_mode() {
    test_case "Verbose mode functionality (Phase 5)"
    
    local log_file="$TEST_DIR/test12.jsonl"
    local fifo_path="$TEST_DIR/test12.fifo"
    
    # テスト用のJSONLログファイルを作成
    cat > "$log_file" << 'EOF'
{"v":1,"t_ms":1000,"type":"start","cols":80,"rows":24,"argv":["/bin/bash"],"cwd":"/tmp","pid":12345}
{"v":1,"t_ms":1001,"type":"stdout","n":13,"data":"Password: "}
{"v":1,"t_ms":1002,"type":"exit","how":"code","code":0}
EOF
    
    # FIFOを作成
    mkfifo "$fifo_path" || {
        log_error "Failed to create FIFO: $fifo_path"
        return 1
    }
    
    # FIFOからの読み取りをバックグラウンドで実行
    local fifo_output="$TEST_DIR/test12.fifo_output"
    timeout 2s cat "$fifo_path" > "$fifo_output" 2>/dev/null || true &
    local fifo_reader_pid=$!
    
    # aish-scriptを実行（--verboseオプション付き）
    log_info "Running: $BINARY -f $log_file --input-fifo $fifo_path --verbose -e 'match \"Password:\" then send \"mypass\\n\"'"
    
    if $BINARY -f "$log_file" --input-fifo "$fifo_path" --verbose -e 'match "Password:" then send "mypass\n"' > "$TEST_DIR/test12.stdout" 2> "$TEST_DIR/test12.stderr"; then
        # FIFO読み取りを終了
        kill "$fifo_reader_pid" 2>/dev/null || true
        wait "$fifo_reader_pid" 2>/dev/null || true
        
        # verbose情報が出力されているか確認（マッチング情報など）
        if grep -q -i "Matched pattern\|Sent to FIFO" "$TEST_DIR/test12.stderr"; then
            log_info "✓ Verbose information was output"
            TESTS_PASSED=$((TESTS_PASSED + 1))
            return 0
        else
            log_error "✗ Verbose information not found in stderr"
            cat "$TEST_DIR/test12.stderr"
            TESTS_FAILED=$((TESTS_FAILED + 1))
            return 1
        fi
    else
        local exit_code=$?
        kill "$fifo_reader_pid" 2>/dev/null || true
        log_error "Command failed with exit code: $exit_code"
        cat "$TEST_DIR/test12.stderr"
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
        "test_ansi_escape_sequence"
        "test_realtime_follow"
        "test_regex_pattern"
        "test_regex_flag_i"
        "test_mixed_string_and_regex"
        "test_timeout"
        "test_invalid_jsonl"
        "test_debug_mode"
        "test_verbose_mode"
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
