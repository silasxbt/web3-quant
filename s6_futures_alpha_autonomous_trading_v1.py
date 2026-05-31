#!/usr/bin/env python3
"""
市场扫描器 - 每分钟运行
纯Python零AI成本，发现异常信号自动开仓
"""

import json
import os
import sys
import time
import requests
from datetime import datetime, timezone, timedelta

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_FILE = os.path.join(SCRIPT_DIR, "trades.json")
SCANNER_STATE = os.path.join(SCRIPT_DIR, "scanner_state.json")
SCANNER_LOG = os.path.join(SCRIPT_DIR, "scanner.log")
INITIAL_BALANCE = 100.0
TZ_UTC8 = timezone(timedelta(hours=8))

# === 配置 ===
MAX_OPEN_POSITIONS = 3       # 最多同时持仓
POSITION_PCT = 30            # 每笔仓位占比%
LEVERAGE = 3                 # 杠杆
COOLDOWN_HOURS = 4           # 同一币种冷却时间
MIN_VOLUME_M = 10            # 最小24h成交额(百万U)

# === 实盘开关 (默认关闭=模拟盘；不显式设 S6_LIVE=1 则行为完全不变) ===
S6_LIVE = os.getenv("S6_LIVE", "0") == "1"
BINANCE_TESTNET = os.getenv("BINANCE_TESTNET", "1") != "0"   # 默认测试网
try:
    MAX_DAILY_LOSS_USD = float(os.getenv("MAX_DAILY_LOSS_USD", "20"))
except ValueError:
    MAX_DAILY_LOSS_USD = 20.0
_EX = None

def get_exchange():
    global _EX
    if _EX is None:
        from binance_futures import BinanceFutures
        _EX = BinanceFutures(os.getenv("BINANCE_API_KEY", ""),
                             os.getenv("BINANCE_API_SECRET", ""), testnet=BINANCE_TESTNET)
    return _EX

def today_realized_pnl(data):
    """今日(UTC+8)已实现盈亏合计，用于日亏熔断"""
    today = datetime.now(TZ_UTC8).strftime("%Y-%m-%d")
    return sum((t.get("pnl_usd") or 0) for t in data["trades"]
               if t["status"] == "closed" and (t.get("exit_time") or "")[:10] == today)

# === 通用工具 ===
def _atomic_write(path, text):
    """原子写入，避免进程中断损坏账本/状态文件"""
    tmp = f"{path}.tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(text)
    os.replace(tmp, path)

def _get_json(url, params=None, timeout=10, retries=3):
    """带退避重试的 GET JSON；429/超时/异常重试，全部失败返回 None"""
    for i in range(retries):
        try:
            r = requests.get(url, params=params, timeout=timeout)
            if r.status_code == 429:
                time.sleep(2 * (i + 1))
                continue
            r.raise_for_status()
            return r.json()
        except Exception:
            if i < retries - 1:
                time.sleep(1)
    return None


# === TG推送 ===
def load_tg_config():
    """Load TG config from environment variables or .env file"""
    env = {}
    # Try .env in script directory, then current directory
    for env_path in [
        os.path.join(SCRIPT_DIR, ".env"),
        os.path.join(os.getcwd(), ".env"),
    ]:
        if os.path.exists(env_path):
            with open(env_path) as f:
                for line in f:
                    line = line.strip()
                    if '=' in line and not line.startswith('#'):
                        k, v = line.split('=', 1)
                        env[k] = v.strip().strip('"').strip("'")
            break
    # OS environment variables override file
    for key in ['TG_BOT_TOKEN', 'TELEGRAM_BOT_TOKEN', 'TG_CHAT_ID']:
        val = os.environ.get(key)
        if val:
            env[key] = val
    return env

def send_tg(text):
    try:
        from tg_config import load_tg
        token, chat_id = load_tg()
        if not token:
            return
        if not chat_id:
            return
        url = f"https://api.telegram.org/bot{token}/sendMessage"
        requests.post(url, json={
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        }, timeout=10)
    except Exception:
        pass


# === 数据加载 ===
def load_trades():
    if os.path.exists(DATA_FILE):
        with open(DATA_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    return {"initial_balance": INITIAL_BALANCE, "trades": []}

def save_trades(data):
    _atomic_write(DATA_FILE, json.dumps(data, ensure_ascii=False, indent=2))

def load_state():
    if os.path.exists(SCANNER_STATE):
        with open(SCANNER_STATE, "r") as f:
            return json.load(f)
    return {"last_opens": {}, "signals_seen": {}}

def save_state(state):
    _atomic_write(SCANNER_STATE, json.dumps(state, ensure_ascii=False, indent=2))

def get_balance(data):
    balance = data.get("initial_balance", INITIAL_BALANCE)
    for t in data["trades"]:
        if t["status"] == "closed" and t["pnl_usd"] is not None:
            balance += t["pnl_usd"]
    return balance

def next_id(data):
    if not data["trades"]:
        return "001"
    max_id = max(int(t["id"]) for t in data["trades"])
    return f"{max_id + 1:03d}"

def now_str():
    return datetime.now(TZ_UTC8).strftime("%Y-%m-%dT%H:%M:%S")

def log(msg):
    ts = datetime.now(TZ_UTC8).strftime("%m-%d %H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line)
    with open(SCANNER_LOG, "a") as f:
        f.write(line + "\n")


# === 币安API ===
def get_all_tickers():
    return _get_json("https://fapi.binance.com/fapi/v1/ticker/24hr")

def get_funding_rates():
    """获取所有币种最新费率"""
    arr = _get_json("https://fapi.binance.com/fapi/v1/premiumIndex")
    return {item['symbol']: float(item['lastFundingRate']) * 100 for item in arr} if arr else {}

def get_funding_history(symbol, limit=8):
    url = f"https://fapi.binance.com/fapi/v1/fundingRate?symbol={symbol}&limit={limit}"
    resp = requests.get(url, timeout=10)
    return [float(item['fundingRate']) * 100 for item in resp.json()]

def get_open_interest(symbol):
    url = f"https://fapi.binance.com/fapi/v1/openInterest?symbol={symbol}"
    resp = requests.get(url, timeout=10)
    data = resp.json()
    return float(data['openInterest'])

def get_klines(symbol, interval="4h", limit=6):
    url = f"https://fapi.binance.com/fapi/v1/klines?symbol={symbol}&interval={interval}&limit={limit}"
    resp = requests.get(url, timeout=10)
    return resp.json()


# === 信号检测 ===

def detect_extreme_negative_funding(symbol, funding_rate, funding_rates_map):
    """
    策略1: 费率极端深负 → 做多(逼空)
    条件: 当前费率<-0.08% 且 连续多期为负
    """
    if funding_rate >= -0.08:
        return None
    
    try:
        history = get_funding_history(symbol, 8)
        neg_count = sum(1 for r in history if r < -0.03)
        if neg_count < 4:
            return None
        
        avg_rate = sum(history) / len(history)
        
        # 费率越极端，信号越强
        strength = "S" if avg_rate < -0.15 else "A" if avg_rate < -0.10 else "B"
        
        return {
            "type": "extreme_neg_funding",
            "direction": "long",
            "strength": strength,
            "reason": f"费率极端深负 avg:{avg_rate:.4f}% 连续{neg_count}/8期为负 逼空概率高",
            "sl_pct": 0.08,   # 止损8%
            "tp_pct": 0.12,   # 止盈12%
        }
    except Exception:
        return None


def detect_extreme_positive_funding(symbol, funding_rate, funding_rates_map):
    """
    策略2: 费率极端正 → 做空(多头拥挤)
    条件: 当前费率>0.10% 且 连续多期高正
    """
    if funding_rate <= 0.10:
        return None
    
    try:
        history = get_funding_history(symbol, 8)
        pos_count = sum(1 for r in history if r > 0.05)
        if pos_count < 4:
            return None
        
        avg_rate = sum(history) / len(history)
        strength = "S" if avg_rate > 0.20 else "A" if avg_rate > 0.12 else "B"
        
        return {
            "type": "extreme_pos_funding",
            "direction": "short",
            "strength": strength,
            "reason": f"费率极端正 avg:{avg_rate:.4f}% 连续{pos_count}/8期高正 多头过度拥挤",
            "sl_pct": 0.10,
            "tp_pct": 0.15,
        }
    except Exception:
        return None


def detect_crash_bounce(ticker):
    """
    策略3: 暴跌后反弹(超跌反弹)
    条件: 24h跌>25% 但最近1h K线企稳/反弹
    """
    change_pct = float(ticker['priceChangePercent'])
    if change_pct >= -25:
        return None
    
    symbol = ticker['symbol']
    try:
        klines = get_klines(symbol, "1h", 6)
        # 最近2根K线
        recent_closes = [float(k[4]) for k in klines[-3:]]
        # 企稳: 最近K线收盘 >= 前一根
        if len(recent_closes) >= 2 and recent_closes[-1] >= recent_closes[-2]:
            return {
                "type": "crash_bounce",
                "direction": "long",
                "strength": "B",  # 风险较高给B级
                "reason": f"24h暴跌{change_pct:.1f}%后企稳 超跌反弹",
                "sl_pct": 0.10,
                "tp_pct": 0.15,
            }
    except Exception:
        pass
    return None


def detect_pump_short(ticker):
    """
    策略4: 暴涨后做空(ATH回落)
    条件: 24h涨>40% — 根据生命周期模型，暴涨后回调概率>85%
    需要确认已经开始回落(不在最高点做空)
    """
    change_pct = float(ticker['priceChangePercent'])
    if change_pct <= 40:
        return None
    
    symbol = ticker['symbol']
    try:
        klines = get_klines(symbol, "1h", 6)
        highs = [float(k[2]) for k in klines]
        closes = [float(k[4]) for k in klines]
        current = closes[-1]
        peak = max(highs)
        
        # 从最高点回落超过10%才做空
        pullback = (peak - current) / peak * 100
        if pullback < 10:
            return None
        
        strength = "A" if change_pct > 80 else "B"
        
        return {
            "type": "pump_short",
            "direction": "short",
            "strength": strength,
            "reason": f"24h暴涨{change_pct:.1f}%后回落{pullback:.1f}% 历史回调概率>85%",
            "sl_pct": 0.15,   # 暴涨币波动大，止损宽一些
            "tp_pct": 0.20,
        }
    except Exception:
        pass
    return None


# === 综合环境检查 ===
def check_environment(symbol, signal):
    """
    开仓前综合检查：不是单一信号触发就开，要多维度对齐
    返回 (pass/fail, analysis_dict, adjusted_strength)
    """
    analysis = {
        "btc_env": "",
        "sentiment": "",
        "oi_check": "",
        "volume_check": "",
        "verdict": ""
    }
    score = 0  # 综合得分，>=3才开仓
    ticker = None  # 供 OI/量能两段共用，避免前段失败时未定义
    
    try:
        # 1. BTC环境 — 做多需要BTC不在暴跌，做空需要BTC不在暴涨
        btc_url = "https://fapi.binance.com/fapi/v1/ticker/24hr?symbol=BTCUSDT"
        btc = requests.get(btc_url, timeout=5).json()
        btc_chg = float(btc['priceChangePercent'])
        
        if signal["direction"] == "long":
            if btc_chg > -2:
                score += 1
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 环境正常 +1"
            elif btc_chg < -5:
                score -= 1
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 暴跌中做多危险 -1"
            else:
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 偏弱 0"
        else:  # short
            if btc_chg < 2:
                score += 1
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 环境正常 +1"
            elif btc_chg > 5:
                score -= 1
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 暴涨中做空危险 -1"
            else:
                analysis["btc_env"] = f"BTC {btc_chg:+.1f}% 偏强 0"
        
        # 2. 市场情绪(Fear & Greed)
        try:
            fng = requests.get("https://api.alternative.me/fng/", timeout=5).json()
            fng_val = int(fng['data'][0]['value'])
            if signal["direction"] == "long":
                if fng_val <= 25:
                    score += 1
                    analysis["sentiment"] = f"FGI={fng_val}极度恐惧 逆向做多 +1"
                elif fng_val >= 75:
                    score -= 1
                    analysis["sentiment"] = f"FGI={fng_val}极度贪婪 做多风险 -1"
                else:
                    analysis["sentiment"] = f"FGI={fng_val}中性 0"
            else:
                if fng_val >= 75:
                    score += 1
                    analysis["sentiment"] = f"FGI={fng_val}极度贪婪 逆向做空 +1"
                elif fng_val <= 25:
                    score -= 1
                    analysis["sentiment"] = f"FGI={fng_val}极度恐惧 做空风险 -1"
                else:
                    analysis["sentiment"] = f"FGI={fng_val}中性 0"
        except Exception:
            analysis["sentiment"] = "FGI获取失败 0"
        
        # 3. OI变化 — 看该币OI是否支持方向
        try:
            oi = get_open_interest(symbol)
            ticker = requests.get(f"https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={symbol}", timeout=5).json()
            price = float(ticker['lastPrice'])
            oi_usd = oi * price
            
            if oi_usd > 5_000_000:  # OI > 5M说明有关注度
                score += 1
                analysis["oi_check"] = f"OI={oi_usd/1e6:.1f}M 有关注度 +1"
            else:
                analysis["oi_check"] = f"OI={oi_usd/1e6:.1f}M 关注度低 0"
        except Exception:
            analysis["oi_check"] = "OI获取失败 0"
        
        # 4. 成交量 — 量能是否活跃
        try:
            vol = float(ticker['quoteVolume']) if ticker else 0
            if not ticker:
                analysis["volume_check"] = "量能获取失败 0"
            elif vol > 50_000_000:
                score += 1
                analysis["volume_check"] = f"24h量={vol/1e6:.0f}M 活跃 +1"
            elif vol > 20_000_000:
                analysis["volume_check"] = f"24h量={vol/1e6:.0f}M 一般 0"
            else:
                score -= 1
                analysis["volume_check"] = f"24h量={vol/1e6:.0f}M 冷清 -1"
        except Exception:
            analysis["volume_check"] = "量能获取失败 0"
        
        # 5. 信号本身的强度加分
        if signal["strength"] == "S":
            score += 2
        elif signal["strength"] == "A":
            score += 1
        
        # 综合判定: >=3通过
        analysis["verdict"] = f"综合得分:{score}/7"
        
        if score >= 3:
            return True, analysis, signal["strength"]
        else:
            return False, analysis, signal["strength"]
            
    except Exception as e:
        analysis["verdict"] = f"检查异常:{e} 保守不开"
        return False, analysis, signal["strength"]


# === 开仓执行 ===
def execute_open(data, state, symbol, price, signal):
    """执行虚拟开仓 — 先过综合环境检查"""
    
    # 综合环境检查
    passed, env_analysis, strength = check_environment(symbol, signal)
    env_summary = " | ".join(v for v in env_analysis.values() if v)
    
    if not passed:
        log(f"综合检查未通过 {symbol}: {env_summary}")
        return
    
    log(f"综合检查通过 {symbol}: {env_summary}")
    
    balance = get_balance(data)
    position_usd = balance * POSITION_PCT / 100
    
    if signal["direction"] == "long":
        sl = round(price * (1 - signal["sl_pct"]), 6)
        tp = round(price * (1 + signal["tp_pct"]), 6)
    else:
        sl = round(price * (1 + signal["sl_pct"]), 6)
        tp = round(price * (1 - signal["tp_pct"]), 6)

    # 实盘下单(默认关闭): 市价开仓 + 交易所原生 STOP_MARKET/TAKE_PROFIT_MARKET 兜底止损止盈
    live_info = {}
    if S6_LIVE:
        ex = get_exchange()
        bal = ex.available_usdt()
        if bal is not None:
            position_usd = bal * POSITION_PCT / 100  # 用真实可用余额定仓
        qty = ex.round_qty(symbol, position_usd * LEVERAGE / price)
        if qty <= 0:
            log(f"[实盘] {symbol} 数量过小，跳过")
            return
        side = "BUY" if signal["direction"] == "long" else "SELL"
        close_side = "SELL" if signal["direction"] == "long" else "BUY"
        ex.set_leverage(symbol, LEVERAGE)
        st, resp = ex.market(symbol, side, qty)
        if st != 200:
            log(f"[实盘] 开仓失败 {symbol}: {resp}")
            return
        st_sl, r_sl = ex.stop_close(symbol, close_side, "STOP_MARKET", ex.round_price(symbol, sl))
        st_tp, r_tp = ex.stop_close(symbol, close_side, "TAKE_PROFIT_MARKET", ex.round_price(symbol, tp))
        if st_sl != 200:
            log(f"[实盘] ⚠️ {symbol} 止损单失败: {r_sl}")
        if st_tp != 200:
            log(f"[实盘] ⚠️ {symbol} 止盈单失败: {r_tp}")
        live_info = {
            "live": True, "qty": qty,
            "sl_order_id": str(r_sl.get("orderId", "")) if st_sl == 200 else "",
            "tp_order_id": str(r_tp.get("orderId", "")) if st_tp == 200 else "",
        }

    trade = {
        "id": next_id(data),
        "symbol": symbol,
        "direction": signal["direction"],
        "leverage": LEVERAGE,
        "position_pct": POSITION_PCT,
        "position_usd": round(position_usd, 4),
        "notional_usd": round(position_usd * LEVERAGE, 4),
        "entry_price": price,
        "stop_loss": sl,
        "take_profit": tp,
        "entry_time": now_str(),
        "exit_price": None,
        "exit_time": None,
        "exit_reason": None,
        "pnl_pct": None,
        "pnl_usd": None,
        "status": "open",
        "pre_analysis": {
            "btc_env": env_analysis.get("btc_env", ""),
            "sentiment": env_analysis.get("sentiment", ""),
            "oi": env_analysis.get("oi_check", ""),
            "volume": env_analysis.get("volume_check", ""),
            "key_reason": f"[{signal['strength']}级] {signal['reason']}",
            "risk": f"综合得分:{env_analysis.get('verdict','')} 策略:{signal['type']}"
        },
        "post_review": None
    }
    trade.update(live_info)

    data["trades"].append(trade)
    save_trades(data)
    
    # 记录冷却
    state["last_opens"][symbol] = now_str()
    save_state(state)
    
    direction_cn = "做多" if signal["direction"] == "long" else "做空"
    
    msg = f"""```
[扫描开仓{'·实盘' if S6_LIVE else '·模拟'}] #{trade['id']}
币种: {symbol}
方向: {direction_cn} {LEVERAGE}x
入场: {price}
止损: {sl}
止盈: {tp}
仓位: {position_usd:.2f}U
信号: [{signal['strength']}] {signal['reason']}
时间: {trade['entry_time']}
```"""
    
    log(f"开仓 #{trade['id']} {symbol} {direction_cn} @ {price} | {signal['reason']}")
    send_tg(msg)
    print(msg)


# === 换仓逻辑 ===
def swap_weakest(data, state, open_positions, new_signal, tickers):
    """满仓时遇到S级信号，平掉浮亏最大的持仓，开新仓"""
    ticker_map = {t['symbol']: float(t['lastPrice']) for t in tickers}
    
    # 计算每个持仓的浮盈%
    worst_trade = None
    worst_pnl = float('inf')
    
    for t in open_positions:
        price = ticker_map.get(t["symbol"])
        if price is None:
            continue
        if t["direction"] == "long":
            pnl_pct = (price - t["entry_price"]) / t["entry_price"] * 100
        else:
            pnl_pct = (t["entry_price"] - price) / t["entry_price"] * 100
        
        if pnl_pct < worst_pnl:
            worst_pnl = pnl_pct
            worst_trade = t
            worst_price = price
    
    if worst_trade is None:
        return
    
    # 只换掉亏损的仓位，盈利的不动
    if worst_pnl > 0:
        log(f"满仓但所有持仓盈利，不换仓 | 新信号: {new_signal['symbol']}")
        return
    
    # 平掉最弱的
    if worst_trade["direction"] == "long":
        pnl_pct_lev = (worst_price - worst_trade["entry_price"]) / worst_trade["entry_price"] * 100 * worst_trade["leverage"]
    else:
        pnl_pct_lev = (worst_trade["entry_price"] - worst_price) / worst_trade["entry_price"] * 100 * worst_trade["leverage"]
    
    pos_usd = worst_trade.get("position_usd", worst_trade.get("position_pct", 30))
    pnl_usd = round(pnl_pct_lev / 100 * pos_usd, 4)
    
    worst_trade["exit_price"] = worst_price
    worst_trade["exit_time"] = now_str()
    worst_trade["exit_reason"] = f"换仓→{new_signal['symbol']}"
    worst_trade["pnl_pct"] = round(pnl_pct_lev, 2)
    worst_trade["pnl_usd"] = pnl_usd
    worst_trade["status"] = "closed"
    save_trades(data)
    
    direction_cn = "多" if worst_trade["direction"] == "long" else "空"
    msg = f"""```
[换仓平仓] #{worst_trade['id']}
平掉: {worst_trade['symbol']} {direction_cn}
入场: {worst_trade['entry_price']}
出场: {worst_price}
盈亏: {pnl_pct_lev:+.2f}% ({pnl_usd:+.2f}U)
原因: S级信号{new_signal['symbol']}更强
```"""
    log(f"换仓平 #{worst_trade['id']} {worst_trade['symbol']} {pnl_usd:+.2f}U → 开 {new_signal['symbol']}")
    send_tg(msg)
    
    # 开新仓
    execute_open(data, state, new_signal["symbol"], new_signal["price"], new_signal)


# === 持仓结算(止盈/止损) ===
def _paper_trigger(t, price):
    """模拟盘: 现价是否触及 SL/TP，返回 'stop_loss'/'take_profit'/None"""
    if t["direction"] == "long":
        if price <= t["stop_loss"]:
            return "stop_loss"
        if price >= t["take_profit"]:
            return "take_profit"
    else:
        if price >= t["stop_loss"]:
            return "stop_loss"
        if price <= t["take_profit"]:
            return "take_profit"
    return None


def _reconcile_live(t, price_map):
    """实盘: 查交易所持仓是否已被原生 SL/TP 平掉，返回 (reason, exit_price) 或 (None, None)"""
    ex = get_exchange()
    if abs(ex.position_amt(t["symbol"])) > 1e-9:
        return None, None  # 仍持仓
    open_ids = ex.open_order_ids(t["symbol"])
    sl_id, tp_id = t.get("sl_order_id"), t.get("tp_order_id")
    if tp_id and tp_id not in open_ids and (not sl_id or sl_id in open_ids):
        reason = "take_profit"
    elif sl_id and sl_id not in open_ids and (not tp_id or tp_id in open_ids):
        reason = "stop_loss"
    else:
        # 兜底: 用现价相对开盘价方向推断
        up = price_map.get(t["symbol"], t["entry_price"]) >= t["entry_price"]
        long = t["direction"] == "long"
        reason = "take_profit" if (up == long) else "stop_loss"
    ex.cancel_all(t["symbol"])  # 撤掉残留的另一张单
    return reason, (t["stop_loss"] if reason == "stop_loss" else t["take_profit"])


def settle_positions(data, tickers):
    """每轮先结算持仓: 模拟盘按现价判 SL/TP；实盘按交易所原生单成交情况对账。"""
    price_map = {t['symbol']: float(t['lastPrice']) for t in tickers}
    closed = 0
    for t in data["trades"]:
        if t["status"] != "open":
            continue
        if t.get("live") and S6_LIVE:
            reason, exit_price = _reconcile_live(t, price_map)
            if reason is None:
                continue
        else:
            price = price_map.get(t["symbol"])
            if price is None:
                continue
            reason = _paper_trigger(t, price)
            if reason is None:
                continue
            exit_price = t["stop_loss"] if reason == "stop_loss" else t["take_profit"]
        if t["direction"] == "long":
            pnl_pct = (exit_price - t["entry_price"]) / t["entry_price"] * 100 * t["leverage"]
        else:
            pnl_pct = (t["entry_price"] - exit_price) / t["entry_price"] * 100 * t["leverage"]
        pos_usd = t.get("position_usd", 0) or 0
        t["exit_price"] = exit_price
        t["exit_time"] = now_str()
        t["exit_reason"] = reason
        t["pnl_pct"] = round(pnl_pct, 2)
        t["pnl_usd"] = round(pnl_pct / 100 * pos_usd, 4)
        t["status"] = "closed"
        closed += 1
        direction_cn = "多" if t["direction"] == "long" else "空"
        tag = "✅止盈" if reason == "take_profit" else "🛑止损"
        msg = f"""```
[{tag}] #{t['id']} {t['symbol']} {direction_cn}
入场: {t['entry_price']}  出场: {exit_price}
盈亏: {pnl_pct:+.2f}% ({t['pnl_usd']:+.2f}U)
```"""
        log(f"平仓 #{t['id']} {t['symbol']} {reason} {t['pnl_usd']:+.2f}U")
        send_tg(msg)
    if closed:
        save_trades(data)
    return closed


# === 主扫描逻辑 ===
def scan():
    data = load_trades()
    state = load_state()
    now = datetime.now(TZ_UTC8)

    # 获取市场数据(先拉，结算与开仓都要用)
    tickers = get_all_tickers()
    funding_rates = get_funding_rates()
    if not tickers:
        log("行情拉取失败, 跳过本轮")
        return

    # 先结算已有持仓的止盈/止损(每轮必做，平仓后可能腾出仓位)
    settle_positions(data, tickers)

    # 结算后再统计持仓
    open_positions = [t for t in data["trades"] if t["status"] == "open"]
    open_symbols = set(t["symbol"] for t in open_positions)

    if len(open_positions) >= MAX_OPEN_POSITIONS:
        return

    # 日亏熔断: 今日已实现亏损达上限则停止开新仓
    if today_realized_pnl(data) <= -MAX_DAILY_LOSS_USD:
        log(f"今日已实现亏损达上限 ${MAX_DAILY_LOSS_USD:.0f}，停止开新仓")
        return
    
    # 过滤USDT合约 + 最小成交量
    exclude = {"BTCUSDT", "ETHUSDT", "USDCUSDT", "FDUSDUSDT", "BTCDOMUSDT", "BTCSTUSDT"}
    candidates = [t for t in tickers 
                  if t['symbol'].endswith('USDT') 
                  and t['symbol'] not in exclude
                  and float(t['quoteVolume']) > MIN_VOLUME_M * 1e6]
    
    signals = []
    
    for ticker in candidates:
        symbol = ticker['symbol']
        
        # 跳过已持仓的币
        if symbol in open_symbols:
            continue
        
        # 冷却检查
        last_open = state.get("last_opens", {}).get(symbol)
        if last_open:
            try:
                last_dt = datetime.fromisoformat(last_open)
                if last_dt.tzinfo is None:
                    last_dt = last_dt.replace(tzinfo=TZ_UTC8)
                if (now - last_dt).total_seconds() < COOLDOWN_HOURS * 3600:
                    continue
            except Exception:
                pass
        
        fr = funding_rates.get(symbol, 0)
        
        # 运行所有策略检测
        for detect_fn in [
            lambda s, f, m: detect_extreme_negative_funding(s, f, m),
            lambda s, f, m: detect_extreme_positive_funding(s, f, m),
            lambda s, f, m: detect_crash_bounce(ticker),
            lambda s, f, m: detect_pump_short(ticker),
        ]:
            try:
                signal = detect_fn(symbol, fr, funding_rates)
                if signal:
                    signal["symbol"] = symbol
                    signal["price"] = float(ticker['lastPrice'])
                    signal["volume_m"] = float(ticker['quoteVolume']) / 1e6
                    signals.append(signal)
            except Exception:
                continue
    
    if not signals:
        return
    
    # 按信号强度排序 S>A>B
    strength_order = {"S": 0, "A": 1, "B": 2}
    signals.sort(key=lambda x: strength_order.get(x["strength"], 3))
    
    # 只取最强的信号开仓(一次最多开1笔)
    best = signals[0]
    
    # B级信号跳过，只开S和A级
    if best["strength"] == "B":
        log(f"B级信号跳过: {best['symbol']} {best['reason']}")
        return
    
    slots = MAX_OPEN_POSITIONS - len(open_positions)
    if slots > 0:
        execute_open(data, state, best["symbol"], best["price"], best)
    elif best["strength"] == "S" and not S6_LIVE:
        # 满仓但遇到S级信号 → 换掉最弱的持仓 (仅模拟盘；实盘换仓涉及交易所平仓+撤单，暂不自动换)
        swap_weakest(data, state, open_positions, best, tickers)


if __name__ == "__main__":
    scan()
