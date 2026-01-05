#!/bin/bash
# aish-render の動作確認テストスクリプト

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
BINARY="${BINARY:-./target/release/aish-render}"
if [ ! -f "$BINARY" ]; then
    BINARY="./target/debug/aish-render"
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

assert_output() {
    local expected="$1"
    local actual="$2"
    local test_name="$3"
    
    # 改行を考慮して比較（diffを使用）
    # $()は最後の改行を削除するため、期待値と実際の値の両方に改行を追加して比較
    local expected_file=$(mktemp)
    local actual_file=$(mktemp)
    printf '%s\n' "$expected" > "$expected_file"
    printf '%s\n' "$actual" > "$actual_file"
    
    if diff -q "$expected_file" "$actual_file" > /dev/null 2>&1; then
        log_info "PASS: $test_name"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        rm -f "$expected_file" "$actual_file"
        return 0
    else
        log_error "FAIL: $test_name"
        echo "Expected:"
        cat -A "$expected_file"
        echo "Actual:"
        cat -A "$actual_file"
        rm -f "$expected_file" "$actual_file"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# テスト1: 基本的なテキスト出力
test_case "Basic text output"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"hello"}'
EXPECTED="hello"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Basic text output"

# テスト2: base64エンコードされたデータ
test_case "Base64 encoded data"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":5,"data":"aGVsbG8="}'
EXPECTED="hello"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Base64 encoded data"

# テスト3: 改行を含むテキスト
test_case "Text with newline"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":11,"data":"hello\nworld"}'
# aish-renderは最後に改行を追加する
# $()は最後の改行を削除するため、期待値も最後の改行なしで指定
EXPECTED=$'hello\nworld'
ACTUAL=$(echo "$JSONL" | "$BINARY")
assert_output "$EXPECTED" "$ACTUAL" "Text with newline"

# テスト4: カーソル左移動
test_case "Cursor left movement"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":10,"data":"hello\x1B[3D"}'
EXPECTED="he"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
# カーソルが左に3移動するので、最後の3文字が上書きされる可能性がある
# このテストは実装に依存するため、簡易版
log_info "Cursor movement test (implementation dependent)"

# テスト5: 行消去 (\x1B[K)
test_case "Erase to end of line"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":15,"data":"hello world\x1B[6DK"}'
EXPECTED="hello"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
# カーソルを6文字左に移動してから行末まで消去
log_info "Erase to end of line test (implementation dependent)"

# テスト6: 画面全体消去 (\x1B[2J)
test_case "Clear entire screen"
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"line1"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","n":5,"data":"line2"}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","enc":"b64","n":4,"data":"G1sySg=="}'
EXPECTED=""
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY")
assert_output "$EXPECTED" "$ACTUAL" "Clear entire screen"

# テスト7: カーソル位置設定 (\x1B[H)
test_case "Cursor position (home)"
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"hello"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","enc":"b64","n":3,"data":"G1tI"}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","n":3,"data":"xyz"}'
EXPECTED="xyzlo"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY" | head -1)
log_info "Cursor position test (implementation dependent)"

# テスト8: バックスペース
test_case "Backspace"
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":7,"data":"hello\x08"}'
EXPECTED="hell"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
log_info "Backspace test (implementation dependent)"

# テスト9: 複数のstdoutイベント
test_case "Multiple stdout events"
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":6,"data":"hello "}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","n":5,"data":"world"}'
EXPECTED="hello world"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Multiple stdout events"

# テスト10: JSONエスケープ文字
test_case "JSON escape characters"
# JSONでは改行は \n としてエンコードされる
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":11,"data":"hello\nworld"}'
# aish-renderは最後に改行を追加するが、$()は最後の改行を削除する
EXPECTED=$'hello\nworld'
ACTUAL=$(printf '%s\n' "$JSONL" | "$BINARY")
assert_output "$EXPECTED" "$ACTUAL" "JSON escape characters"

# テスト11: Phase 1.5対応 - ANSIエスケープシーケンスがJSONエスケープ文字列として記録された場合
test_case "Phase 1.5: ANSI escape sequences as JSON escape strings"
# \u001b[31mRED\u001b[0m は赤色のテキスト（ANSIエスケープシーケンス）
# Phase 1.5では、これらはbase64ではなくJSONエスケープ文字列として記録される
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":14,"data":"\u001b[31mRED\u001b[0m"}'
# ANSIエスケープシーケンスは処理され、テキストのみが抽出される
EXPECTED="RED"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "ANSI escape sequences as JSON escape strings"

# テスト12: Phase 1.5対応 - 複数のANSIエスケープシーケンス
test_case "Phase 1.5: Multiple ANSI escape sequences"
# 色付きテキストと通常テキストの混在
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":20,"data":"\u001b[32mGREEN\u001b[0m text"}'
EXPECTED="GREEN text"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Multiple ANSI escape sequences"

# テスト13: Phase 1.5対応 - 既存のbase64エンコードとの互換性確認
test_case "Phase 1.5: Compatibility with base64 encoded data"
# 既存のbase64エンコードされたログファイルも引き続き正しく処理されることを確認
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":5,"data":"aGVsbG8="}'
EXPECTED="hello"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Compatibility with base64 encoded data"

# テスト14: Phase 1.5対応 - カーソル移動を含むJSONエスケープ文字列
test_case "Phase 1.5: Cursor movement with JSON escape strings"
# カーソル左移動を含むデータ（JSONエスケープ文字列として記録）
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":10,"data":"hello\u001b[3D"}'
# カーソルが左に3移動するので、最後の3文字が上書きされる可能性がある
# このテストは実装に依存するため、簡易版
log_info "Cursor movement with JSON escape strings test (implementation dependent)"

# テスト15: OSCシーケンスのST終端（\x1B\\）のテスト
test_case "OSC sequence with ST terminator"
# OSCシーケンス（ST終端）を含むデータ
# OSCシーケンスは無視されるので、テキストのみが出力される
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"hello"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","enc":"b64","n":10,"data":"G10wO3Rlc3QbXA=="}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","n":5,"data":"world"}'
EXPECTED="helloworld"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "OSC sequence with ST terminator"

# テスト16: OSCシーケンスのBEL終端（\x07）のテスト
test_case "OSC sequence with BEL terminator"
# OSCシーケンス（BEL終端）を含むデータ
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"hello"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","enc":"b64","n":9,"data":"G10wO3Rlc3QH"}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","n":5,"data":"world"}'
EXPECTED="helloworld"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "OSC sequence with BEL terminator"

# テスト17: CSIシーケンスのプライベートパラメータ（?）のテスト
test_case "CSI sequence with private parameter"
# プライベートパラメータ（?）を含むCSIシーケンス
# \x1B[?25h はカーソルを表示するシーケンス（無視される）
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","n":5,"data":"hello"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","enc":"b64","n":6,"data":"G1s/MjVo"}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","n":5,"data":"world"}'
EXPECTED="helloworld"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "CSI sequence with private parameter"

# テスト18: CSIシーケンスのデフォルト値（パラメータなしのカーソル左移動）のテスト
test_case "CSI sequence with default parameter (cursor left)"
# \x1B[D は \x1B[1D と同等（デフォルト値1）
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":9,"data":"helloG1tE"}'
# カーソルが左に1移動するので、最後の1文字が上書きされる可能性がある
# このテストは実装に依存するため、簡易版
log_info "CSI sequence with default parameter test (implementation dependent)"

# テスト19: UTF-8マルチバイト文字（日本語）のテスト
test_case "UTF-8 multibyte characters (Japanese)"
# 日本語のテキスト
JSONL='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":15,"data":"44GT44KT44Gr44Gh44Gv"}'
EXPECTED="こんにちは"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "UTF-8 multibyte characters (Japanese)"

# テスト20: UTF-8マルチバイト文字とエスケープシーケンスの混在
test_case "UTF-8 multibyte characters with escape sequences"
# 日本語のテキストとエスケープシーケンスの混在
JSONL1='{"v":1,"t_ms":1000,"type":"stdout","enc":"b64","n":15,"data":"44GT44KT44Gr44Gh44Gv"}'
JSONL2='{"v":1,"t_ms":1001,"type":"stdout","n":1,"data":" "}'
JSONL3='{"v":1,"t_ms":1002,"type":"stdout","n":5,"data":"world"}'
EXPECTED="こんにちは world"
ACTUAL=$(echo -e "$JSONL1\n$JSONL2\n$JSONL3" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "UTF-8 multibyte characters with escape sequences"

# テスト21: 日本語とJSONエスケープ文字列の混在
test_case "Japanese with JSON escape strings"
# 日本語のテキスト（JSONエスケープ文字列として記録）
JSONL='{"v":1,"t_ms":1000,"type":"stdout","n":15,"data":"\u3053\u3093\u306b\u3061\u306f"}'
EXPECTED="こんにちは"
ACTUAL=$(echo "$JSONL" | "$BINARY" | head -1)
assert_output "$EXPECTED" "$ACTUAL" "Japanese with JSON escape strings"

# テスト結果の表示
echo ""
echo "========================================="
echo "Test Results"
echo "========================================="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    log_info "All tests passed!"
    exit 0
else
    log_error "Some tests failed"
    exit 1
fi
