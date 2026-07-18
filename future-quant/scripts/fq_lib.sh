#!/bin/zsh

set -u

ROOT_DIR="$(cd "$(dirname "${0:A}")/.." && pwd)"
CONFIG_FILE="$ROOT_DIR/config/local.toml"
ENV_FILE="$ROOT_DIR/config/env.local"
BIN_PATH="$ROOT_DIR/target/debug/future-quant"
RUN_ROOT="$HOME/Library/Application Support/FutureQuant"
RUN_BIN="$RUN_ROOT/bin/future-quant"
RUN_CONFIG="$RUN_ROOT/config/local.toml"
RUN_ENV="$RUN_ROOT/config/env.local"
RUNNER_PATH="$RUN_ROOT/bin/run_service.sh"
PID_FILE="$RUN_ROOT/data/future-quant.pid"
LAUNCH_LABEL="com.future-quant.local"
PLIST_FILE="$RUN_ROOT/$LAUNCH_LABEL.plist"
LOG_FILE="$RUN_ROOT/logs/future-quant.log"
WEB_URL="http://127.0.0.1:8787"

fq_banner() {
  printf '\nFuture Quant\n'
  printf '项目目录：%s\n' "$ROOT_DIR"
  printf 'Web 控制台：%s\n\n' "$WEB_URL"
}

fq_is_running() {
  local pid
  pid="$(fq_pid)"
  [[ "$pid" == <-> ]] || return 1
  kill -0 "$pid" 2>/dev/null
}

fq_pid() {
  local pid
  pid="$(launchctl print "$(fq_launch_domain)/$LAUNCH_LABEL" 2>/dev/null | awk '/pid =/ {print $3; exit}')"
  if [[ "$pid" == <-> ]]; then
    printf '%s\n' "$pid"
    return 0
  fi
}

fq_build() {
  cd "$ROOT_DIR" || return 1
  cargo build
}

fq_load_env() {
  if [[ -f "$ENV_FILE" ]]; then
    set +u
    set -a
    source "$ENV_FILE"
    set +a
    set -u
  fi
}

fq_launch_domain() {
  printf 'gui/%s' "$(id -u)"
}

fq_is_loaded() {
  launchctl print "$(fq_launch_domain)/$LAUNCH_LABEL" >/dev/null 2>&1
}

fq_install_runtime() {
  mkdir -p "$RUN_ROOT/bin" "$RUN_ROOT/config" "$RUN_ROOT/data" "$RUN_ROOT/logs" "$RUN_ROOT/reports"
  cp "$BIN_PATH" "$RUN_BIN"
  chmod +x "$RUN_BIN"
  if [[ -f "$CONFIG_FILE" ]]; then
    cp "$CONFIG_FILE" "$RUN_CONFIG"
  else
    cp "$ROOT_DIR/config/local.example.toml" "$RUN_CONFIG"
  fi
  if [[ -f "$ENV_FILE" ]]; then
    cp "$ENV_FILE" "$RUN_ENV"
  elif [[ ! -f "$RUN_ENV" ]]; then
    printf 'RUST_LOG=info\n' > "$RUN_ENV"
  fi
  cat > "$RUNNER_PATH" <<EOF
#!/bin/zsh

set -u

RUN_ROOT="$RUN_ROOT"
RUN_ENV="$RUN_ENV"
RUN_BIN="$RUN_BIN"
RUN_CONFIG="$RUN_CONFIG"

cd "\$RUN_ROOT" || exit 78

if [[ -f "\$RUN_ENV" ]]; then
  set +u
  set -a
  source "\$RUN_ENV"
  set +a
  set -u
fi

printf '[%s] 正在从 %s 启动 future-quant\n' "\$(date '+%Y-%m-%d %H:%M:%S %Z')" "\$RUN_ROOT"
exec "\$RUN_BIN" --config "\$RUN_CONFIG"
EOF
  chmod +x "$RUNNER_PATH"
}

fq_write_plist() {
  mkdir -p "$RUN_ROOT" "$RUN_ROOT/logs"
  cat > "$PLIST_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LAUNCH_LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/zsh</string>
    <string>$RUNNER_PATH</string>
  </array>
  <key>WorkingDirectory</key>
  <string>$ROOT_DIR</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RUST_LOG</key>
    <string>${RUST_LOG:-info}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>StandardOutPath</key>
  <string>$LOG_FILE</string>
  <key>StandardErrorPath</key>
  <string>$LOG_FILE</string>
</dict>
</plist>
EOF
}

fq_export_launch_env() {
  local name value
  for name in \
    RUST_LOG \
    TELEGRAM_BOT_TOKEN \
    BINANCE_API_KEY \
    BINANCE_API_SECRET \
    OKX_API_KEY \
    OKX_API_SECRET \
    OKX_API_PASSPHRASE \
    OPENAI_API_KEY; do
    value="${(P)name:-}"
    if [[ -n "$value" ]]; then
      launchctl setenv "$name" "$value"
    fi
  done
}

fq_start() {
  mkdir -p "$RUN_ROOT/data" "$RUN_ROOT/logs" "$RUN_ROOT/reports"
  if fq_is_running; then
    printf '服务已经在运行。PID=%s\n' "$(fq_pid)"
    return 0
  fi
  if [[ ! -x "$BIN_PATH" ]]; then
    fq_build || return 1
  fi
  fq_load_env
  fq_export_launch_env
  fq_install_runtime
  fq_write_plist
  printf '正在启动 Future Quant...\n'
  if fq_is_loaded; then
    launchctl bootout "$(fq_launch_domain)/$LAUNCH_LABEL" >/dev/null 2>&1 || true
  fi
  launchctl bootstrap "$(fq_launch_domain)" "$PLIST_FILE" || return 1
  for _ in {1..20}; do
    if fq_is_running; then
      fq_pid > "$PID_FILE"
      printf '启动成功。PID=%s\n' "$(fq_pid)"
      return 0
    fi
    sleep 0.5
  done
  printf '启动失败，请查看日志：%s\n' "$LOG_FILE"
  launchctl print "$(fq_launch_domain)/$LAUNCH_LABEL" 2>/dev/null | tail -n 40 || true
  return 1
}

fq_stop() {
  if ! fq_is_running; then
    printf '服务当前未运行。\n'
    if fq_is_loaded; then
      launchctl bootout "$(fq_launch_domain)/$LAUNCH_LABEL" >/dev/null 2>&1 || true
    fi
    rm -f "$PID_FILE"
    return 0
  fi
  local pid
  pid="$(fq_pid)"
  printf '正在停止 Future Quant。PID=%s\n' "$pid"
  launchctl bootout "$(fq_launch_domain)/$LAUNCH_LABEL" >/dev/null 2>&1 || \
    launchctl bootout "$(fq_launch_domain)" "$PLIST_FILE" >/dev/null 2>&1 || \
    kill "$pid" 2>/dev/null || true
  for _ in {1..20}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$PID_FILE"
      printf '已停止。\n'
      return 0
    fi
    sleep 0.2
  done
  printf '强制停止 PID=%s\n' "$pid"
  kill -9 "$pid" 2>/dev/null || true
  rm -f "$PID_FILE"
}

fq_restart() {
  fq_stop
  fq_build || return 1
  fq_start
}

fq_status_json() {
  curl -sS --max-time 5 "$WEB_URL/api/state"
}

fq_status() {
  if ! fq_is_running; then
    printf '服务：已停止\n'
    return 1
  fi
  printf '服务：运行中 PID=%s\n' "$(fq_pid)"
  python3 - "$WEB_URL/api/state" <<'PY'
import json
import sys
import urllib.request

url = sys.argv[1]
try:
    with urllib.request.urlopen(url, timeout=5) as resp:
        state = json.load(resp)
except Exception as exc:
    print(f"API：不可用（{exc}）")
    raise SystemExit(1)

status = state["status"]
stats = state["stats"]
kind_zh = {
    "UsdtPerp": "U 本位永续",
    "CoinPerp": "币本位永续",
    "USDT_PERP": "U 本位永续",
    "COIN_PERP": "币本位永续",
}
level_zh = {
    "WatchOnly": "仅观察",
    "Normal": "正常机会",
    "High": "高优先级",
    "Extreme": "极端机会",
    "WATCH_ONLY": "仅观察",
    "NORMAL": "正常机会",
    "HIGH": "高优先级",
    "EXTREME": "极端机会",
}
ok = lambda value: "正常" if value else "异常"
print("API：正常")
print(f"币安：{ok(status['binance_ok'])}  OKX：{ok(status['okx_ok'])}")
print(f"共同合约数量：{stats['discovered_common_symbols']}")
print(f"活跃机会数量：{len(state['active_opportunities'])}")
print(f"最近机会记录：{len(state['recent_opportunities'])}")
print(f"最近轮询耗时：{status['last_poll_ms']} ms")
print(f"最近错误：{status['last_error'] or '无'}")
if state["active_opportunities"]:
    print("")
    print("活跃机会 Top：")
    for item in state["active_opportunities"][:5]:
        net = item["estimated_net_spread_bps"] / 100
        kind = kind_zh.get(item["kind"], item["kind"])
        level = level_zh.get(item["alert_level"], item["alert_level"])
        print(f"- {item['base']} {kind} 净价差={net:.2f}% 等级={level} 建议仓位={item['suggested_notional_usdt']:.2f}U")
PY
}

fq_open_web() {
  open "$WEB_URL" >/dev/null 2>&1 || true
}

fq_tail_log() {
  touch "$LOG_FILE"
  tail -n 120 -f "$LOG_FILE"
}

fq_write_report() {
  mkdir -p "$RUN_ROOT/reports"
  local now path
  now="$(date '+%Y-%m-%d-%H%M%S')"
  path="$RUN_ROOT/reports/manual-status-$now.json"
  fq_status_json > "$path"
  printf '已写出 %s\n' "$path"
}

fq_monitor_once() {
  clear
  fq_banner
  date '+%Y-%m-%d %H:%M:%S %Z'
  printf '\n'
  fq_status || true
  printf '\n最近日志：\n'
  tail -n 12 "$LOG_FILE" 2>/dev/null || true
}
