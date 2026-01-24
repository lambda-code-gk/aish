#!/usr/bin/env bash

# AISH環境のセットアップ（必要に応じて）
export AISH_HOME="$(pwd)/_aish"
. "$AISH_HOME/lib/tool_helper.sh"
. "$AISH_HOME/lib/tool_write_file.sh"
. "$AISH_HOME/lib/tool_replace_block.sh"

test_file="test_edit.txt"

echo "Testing write_file..."
_tool_write_file_execute "" '{"path": "'$test_file'", "content": "line1\nline2\nline3\n"}' "gemini"
if [ "$(cat $test_file)" = "line1
line2
line3" ]; then
    echo "write_file success"
else
    echo "write_file failed"
    cat $test_file
    exit 1
fi

echo "Testing replace_block (success case)..."
_tool_replace_block_execute "" '{"path": "'$test_file'", "old_block": "line2\n", "new_block": "line2 modified\n"}' "gemini"
if grep -q "line2 modified" "$test_file"; then
    echo "replace_block success"
else
    echo "replace_block failed"
    cat $test_file
    exit 1
fi

echo "Testing replace_block (not found)..."
_tool_replace_block_execute "" '{"path": "'$test_file'", "old_block": "nonexistent", "new_block": "error"}' "gemini" 2>/dev/null
if [ $? -ne 0 ]; then
    echo "replace_block (not found) handled correctly"
else
    echo "replace_block (not found) should have failed"
    exit 1
fi

echo "Testing replace_block (multiple found)..."
echo "duplicate" >> $test_file
echo "duplicate" >> $test_file
_tool_replace_block_execute "" '{"path": "'$test_file'", "old_block": "duplicate", "new_block": "error"}' "gemini" 2>/dev/null
if [ $? -ne 0 ]; then
    echo "replace_block (multiple found) handled correctly"
else
    echo "replace_block (multiple found) should have failed"
    exit 1
fi

rm "$test_file"
echo "All tests passed!"
