#!/usr/bin/env bash

# Test script for Agent Interruption and Resumption logic

# Mock environment
export AISH_HOME=$(mktemp -d)
mkdir -p "$AISH_HOME/sessions"
export AISH_SESSION="$AISH_HOME/sessions/test_session"
mkdir -p "$AISH_SESSION"

# Mock REQUEST_FILE
REQUEST_FILE="$AISH_SESSION/request.txt"
cat > "$REQUEST_FILE" <<EOF
{
  "messages": [
    {"role": "user", "content": "test message"},
    {"role": "assistant", "tool_calls": [{"function": {"name": "execute_shell_command"}}]}
  ]
}
EOF

# Define the interrupt handler logic for testing
_on_interrupt_agent_test() {
    local iteration=$1
    echo "Interrupting (mock)..."
    if [ -f "$REQUEST_FILE" ]; then
        cp "$REQUEST_FILE" "$AISH_SESSION/checkpoint_request.json"
        local last_tool=$(jq -r '.messages | map(select(.role == "assistant" and .tool_calls)) | last | .tool_calls[0].function.name // "none"' "$REQUEST_FILE" 2>/dev/null)
        cat > "$AISH_SESSION/checkpoint_summary.txt" <<EEOF
[Interrupted Session Summary]
- Session ID: $(basename "$AISH_SESSION")
- Last Iteration: $iteration
- Last Action: $last_tool
EEOF
    fi
}

echo "Testing Interruption Handler..."
_on_interrupt_agent_test 5

# Verify checkpoint files
if [ ! -f "$AISH_SESSION/checkpoint_request.json" ]; then
    echo "FAILED: checkpoint_request.json not found"
    exit 1
fi

if [ ! -f "$AISH_SESSION/checkpoint_summary.txt" ]; then
    echo "FAILED: checkpoint_summary.txt not found"
    exit 1
fi

echo "✓ Interruption files generated correctly"

# Test Resumption Logic (query_entry_prepare part)
echo "Testing Resumption logic..."
export AISH_CONTINUE="true"

# Mock functions needed by query_entry.sh
function aish_rollout() { :; }
function find_memory_directory() { echo ""; }
function search_memory_efficient() { echo "[]"; }
function detail.aish_list_parts() { echo ""; }
function detail.aish_security_check() { echo ""; }
function memory_system_load_all() { echo "[]"; }

# Source query_entry.sh (we need to be careful about side effects)
# Instead of sourcing the whole file which might have top-level calls,
# let's just test if query_entry_prepare sets the system instruction correctly.
. _aish/lib/query_entry.sh

# Run query_entry_prepare
# We need to set some args
query_entry_prepare "new task"

if [[ "$_query_system_instruction" == *"[Interrupted Session Summary]"* ]]; then
    echo "✓ System instruction contains checkpoint summary"
else
    echo "FAILED: System instruction does not contain checkpoint summary"
    echo "DEBUG: _query_system_instruction=$_query_system_instruction"
    exit 1
fi

echo "All tests passed!"
rm -rf "$AISH_HOME"