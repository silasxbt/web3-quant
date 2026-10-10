"""仓位台账汇总：投入、提取、手续费、成本、当前价值、实际年化、相对持币。

事件类型（金额都以美元计，stock 为股票数量）：
  deposit   投入：value = 投入总价值，stock/stable = 投入的股票数量和稳定币数量（用于算持币基准）
  withdraw  提取：value = 取出的总价值，stock/stable 同上（全部撤池即平仓）
  collect   领取手续费：fees = 美元价值
  cost      成本：cost = gas、swap 费等
  snapshot  记录当前状态：value = 仓位当前价值，fees = 未领取手续费

投入事件可带 fees_before：开始记账时仓位里已经存在的未领取手续费（快捷导入时自动记录）。
这部分是记账前赚的，后面领取或快照时会被一起算进来，所以要扣掉，不计入收益和年化。

两个年化（都按资金加权平均本金、从首笔投入到现在的时长折算成年）：
  实际年化   = (当前价值 + 已提取 + 记账后手续费 − 投入 − 成本)  → 含标的价格波动（含无常损失）和手续费
  手续费年化 = 记账后手续费                                       → 只算手续费
"""
import time

def summarize(pos, evs, price=None, qusd=1.0, unit="usd"):
    """unit="usd" 按美元；unit="q" 按计价币（股票/BNB 池即 BNB 本位）。price 是计价币价格，qusd 是计价币当前美元价。
    事件里 value/fees/cost/fees_before 是美元，*_q 是计价币；老记录没有 *_q 时（稳定币池）两者相同。"""
    g = (lambda e, k: e.get(k + "_q", e.get(k, 0) / (e.get("qusd") or 1))) if unit == "q" else (lambda e, k: e.get(k, 0))
    dep = wd = fees_c = cost = fees_before = 0.0
    stock = stable = 0.0      # 持币基准：投入的股票和稳定币，扣掉提取的部分
    last_snap = None
    t0 = min([pos.get("opened") or time.time()] + [e["ts"] for e in evs if e["type"] == "deposit"])
    cap_time = 0.0; cap = 0.0; tprev = t0  # 资金加权时间，用于年化
    for e in sorted(evs, key=lambda e: e["ts"]):
        cap_time += cap * max(e["ts"] - tprev, 0); tprev = e["ts"]
        t = e["type"]
        if t == "deposit":
            dep += g(e, "value"); cap += g(e, "value"); fees_before += g(e, "fees_before")
            stock += e.get("stock", 0); stable += e.get("stable", 0)
        elif t == "withdraw":
            wd += g(e, "value"); cap = max(cap - g(e, "value"), 0)
            stock -= e.get("stock", 0); stable -= e.get("stable", 0)
        elif t == "collect": fees_c += g(e, "fees")
        elif t == "cost": cost += g(e, "cost")
        elif t == "snapshot": last_snap = e
    closed = pos.get("status") == "closed"
    now = pos.get("closed_at") if closed and pos.get("closed_at") else time.time()
    cap_time += cap * max(now - tprev, 0)
    value = 0.0 if closed else (g(last_snap, "value") if last_snap and last_snap.get("value") is not None else None)
    unclaimed = 0.0 if closed else (g(last_snap, "fees") if last_snap else 0)
    fees = max(fees_c + (unclaimed or 0) - fees_before, 0)
    out = dict(deposited=dep, withdrawn=wd, fees_collected=fees_c, fees_unclaimed=unclaimed or 0, fees_before=fees_before, fees=fees, cost=cost,
               value=value, snapshot_at=(last_snap or {}).get("ts"), days=max((now - t0) / 86400, 0))
    avg_cap = cap_time / max(now - t0, 1)
    if value is not None:
        pnl = value + wd + fees - dep - cost
        ok = avg_cap and out["days"] >= 1  # 不足 1 天年化会被放大到没有意义
        out.update(pnl=pnl, apr=pnl / avg_cap * 365 / out["days"] if ok else None,
                   fee_apr=fees / avg_cap * 365 / out["days"] if ok else None)
        if price and not closed and (stock or stable):
            hodl = (stock * price + stable) * (qusd if unit == "usd" else 1)
            out.update(hodl_value=hodl, vs_hodl=value + fees - hodl)
    return out
