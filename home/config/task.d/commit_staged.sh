#!/usr/bin/env bash

git --no-pager diff --cached
kill -usr1 $AISH_PID
ai -p gemini '上記のステージングされたファイルの差分を見て、修正意図を反映した英語のコミットメッセージを生成し、run_shellを使って`git commit`して'
