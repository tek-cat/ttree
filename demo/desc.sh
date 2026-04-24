#!/usr/bin/env bash
# Usage: desc.sh "Title" "subtitle"
printf '\033[2J\033[H'
printf '\n  \033[1;32m▶\033[0m  \033[1;97m%s\033[0m\n' "$1"
printf '     \033[90m%s\033[0m\n' "$2"
