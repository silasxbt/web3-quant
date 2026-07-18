#!/bin/zsh

set -u
SCRIPT_DIR="$(cd "$(dirname "${0:A}")" && pwd)"
source "$SCRIPT_DIR/scripts/fq_lib.sh"

fq_banner
printf '正在准备本地运行环境...\n'
fq_build || {
  printf '\n构建失败，按 Enter 关闭。\n'
  read _
  exit 1
}

fq_start || {
  printf '\n启动失败，按 Enter 关闭。\n'
  read _
  exit 1
}

fq_open_web

printf '\n本地内容和任务已启动。\n'
printf 'Web 控制台：%s\n' "$WEB_URL"
printf '日志文件：    %s\n' "$LOG_FILE"
printf 'PID 文件：    %s\n' "$PID_FILE"

while true; do
  printf '\n'
  printf '可用命令：\n'
  printf '  [s] 查看状态\n'
  printf '  [m] 打开监控面板\n'
  printf '  [l] 跟踪日志\n'
  printf '  [o] 打开 Web 控制台\n'
  printf '  [r] 重启服务\n'
  printf '  [p] 停止服务\n'
  printf '  [b] 重新构建\n'
  printf '  [w] 写出状态报告\n'
  printf '  [q] 退出终端，保留服务运行\n'
  printf '  [x] 停止服务并退出\n'
  printf '\n请选择：'
  read choice

  case "$choice" in
    s|S)
      fq_status || true
      ;;
    m|M)
      "$SCRIPT_DIR/scripts/monitor.command"
      ;;
    l|L)
      printf '按 Ctrl-C 返回菜单。\n'
      fq_tail_log
      ;;
    o|O)
      fq_open_web
      ;;
    r|R)
      fq_restart || true
      ;;
    p|P)
      fq_stop
      ;;
    b|B)
      fq_build || true
      ;;
    w|W)
      fq_write_report || true
      ;;
    q|Q)
      printf '终端即将退出，服务继续运行。PID=%s\n' "$(fq_pid)"
      exit 0
      ;;
    x|X)
      fq_stop
      exit 0
      ;;
    *)
      printf '未知命令：%s\n' "$choice"
      ;;
  esac
done
