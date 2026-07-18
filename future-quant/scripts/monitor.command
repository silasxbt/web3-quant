#!/bin/zsh

set -u
SCRIPT_DIR="$(cd "$(dirname "${0:A}")" && pwd)"
source "$SCRIPT_DIR/fq_lib.sh"

while true; do
  fq_monitor_once
  printf '\n每 5 秒刷新一次。按 Ctrl-C 退出。\n'
  sleep 5
done
