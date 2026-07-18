#!/bin/zsh

set -u

SCRIPT_DIR="$(cd "$(dirname "${0:A}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ENV_FILE="$ROOT_DIR/config/env.local"
BIN_PATH="$ROOT_DIR/target/debug/future-quant"
CONFIG_FILE="$ROOT_DIR/config/local.toml"

cd "$ROOT_DIR" || exit 78

if [[ -f "$ENV_FILE" ]]; then
  set +u
  set -a
  source "$ENV_FILE"
  set +a
  set -u
fi

printf '[%s] 正在从 %s 启动 future-quant\n' "$(date '+%Y-%m-%d %H:%M:%S %Z')" "$ROOT_DIR"
exec "$BIN_PATH" --config "$CONFIG_FILE"
