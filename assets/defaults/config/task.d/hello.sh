#!/usr/bin/env bash

echo ========================
# AISH_HOME がある場合はその bin/ai、なければ PATH の ai を使用
if [ -n "$AISH_HOME" ]; then
    "$AISH_HOME/bin/ai" 'hello world'
else
    ai 'hello world'
fi
echo ========================
