"""统一的 Telegram 配置加载。

兼容多来源（优先级：环境变量 > 脚本目录 .env > 脚本目录 .env.oi > ~/.env）
与多变量名（TG_BOT_TOKEN / TELEGRAM_BOT_TOKEN，TG_CHAT_ID / TELEGRAM_CHAT_ID）。
"""
import os
from pathlib import Path

_DIR = Path(__file__).resolve().parent


def _load_files() -> dict:
    env = {}
    for p in (_DIR / ".env", _DIR / ".env.oi", Path.home() / ".env"):
        try:
            if p.exists():
                for line in p.read_text().splitlines():
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        k, v = line.split("=", 1)
                        env.setdefault(k.strip(), v.strip().strip('"').strip("'"))
        except Exception:
            pass
    return env


def load_tg():
    """返回 (token, chat_id)。chat_id 为 int，或 @channel 字符串，取不到为空串。"""
    files = _load_files()

    def get(*keys):
        for k in keys:
            v = os.environ.get(k) or files.get(k)
            if v:
                return v
        return ""

    token = get("TG_BOT_TOKEN", "TELEGRAM_BOT_TOKEN")
    raw = get("TG_CHAT_ID", "TELEGRAM_CHAT_ID")
    try:
        chat_id = int(raw) if raw else ""
    except (TypeError, ValueError):
        chat_id = raw  # 支持 @channel
    return token, chat_id
