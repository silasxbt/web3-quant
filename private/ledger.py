"""仓位台账汇总：投入、提取、手续费、成本、当前价值、实际年化、相对持币。

事件类型（金额都以美元计，stock 为股票数量）：
  deposit   投入：value = 投入总价值，stock/stable = 投入的股票数量和稳定币数量（用于算持币基准）
  withdraw  提取：value = 取出的总价值，stock/stable 同上（全部撤池即平仓）
  collect   领取手续费：fees = 美元价值
  cost      成本：cost = gas、swap 费等
  snapshot  记录当前状态：value = 仓位当前价值，fees = 未领取手续费
"""
import time

def summarize(pos, evs, price=None):
    dep = wd = fees_c = cost = 0.0
    stock = stable = 0.0      # 持币基准：投入的股票和稳定币，扣掉提取的部分
    last_snap = None
    t0 = min([pos.get("opened") or time.time()] + [e["ts"] for e in evs if e["type"] == "deposit"])
    cap_time = 0.0; cap = 0.0; tprev = t0  # 资金加权时间，用于年化
    for e in sorted(evs, key=lambda e: e["ts"]):
        cap_time += cap * max(e["ts"] - tprev, 0); tprev = e["ts"]
        t = e["type"]
        if t == "deposit":
            dep += e.get("value", 0); cap += e.get("value", 0)
            stock += e.get("stock", 0); stable += e.get("stable", 0)
        elif t == "withdraw":
            wd += e.get("value", 0); cap = max(cap - e.get("value", 0), 0)
            stock -= e.get("stock", 0); stable -= e.get("stable", 0)
        elif t == "collect": fees_c += e.get("fees", 0)
        elif t == "cost": cost += e.get("cost", 0)
        elif t == "snapshot": last_snap = e
    closed = pos.get("status") == "closed"
    now = pos.get("closed_at") if closed and pos.get("closed_at") else time.time()
    cap_time += cap * max(now - tprev, 0)
    value = 0.0 if closed else (last_snap or {}).get("value")
    unclaimed = 0.0 if closed else (last_snap or {}).get("fees", 0)
    fees = fees_c + (unclaimed or 0)
    out = dict(deposited=dep, withdrawn=wd, fees_collected=fees_c, fees_unclaimed=unclaimed or 0, fees=fees, cost=cost,
               value=value, snapshot_at=(last_snap or {}).get("ts"), days=max((now - t0) / 86400, 0))
    avg_cap = cap_time / max(now - t0, 1)
    if value is not None:
        pnl = value + wd + fees - dep - cost
        out.update(pnl=pnl, apr=pnl / avg_cap * 365 / max(out["days"], 1 / 24) if avg_cap else None,
                   fee_apr=fees / avg_cap * 365 / max(out["days"], 1 / 24) if avg_cap else None)
        if price and not closed and (stock or stable):
            hodl = stock * price + stable
            out.update(hodl_value=hodl, vs_hodl=value + fees - hodl)
    return out
