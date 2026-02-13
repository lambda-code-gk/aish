#!/usr/bin/env bash

git --no-pager diff --cached
aish rollout
ai -p gemini '上記のステージングされたファイルの差分を見て、修正意図を反映した英語のコミットメッセージを生成しコミットして'
