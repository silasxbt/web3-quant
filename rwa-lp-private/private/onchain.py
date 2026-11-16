"""按 V3 仓位 NFT 编号直接读链上：区间、当前价值、未领取手续费。只读，不需要私钥。"""
import math, os
from app.chain import call, rpc, sel, words, addr, pad, cached
from app.analytics import STABLES

NPM = {"0x0bfbcf9fa4f9c56b0f40a671ad40e0805a091865": "0x46a15b0b27311cedf172ab29e4f4766fbe7f4364",   # PancakeSwap V3
       "0xdb1d10011ad0ff90774d0c6bb92e5c5c8b4461f7": "0x7b8a01b39d58278b5de7e48c8449c9f4f5170613"}   # Uniswap V3 (BSC)
U128 = 2**128 - 1

def _int24(v):
    v &= 2**24 - 1
    return v - 2**24 if v >= 2**23 else v

def read(pool_info, token_id):
    """pool_info 来自 analytics.pool_info；返回该 NFT 仓位的实时状态（美元计）。"""
    npm = NPM.get(pool_info["factory"])
    if not npm: raise ValueError("该池子不是 PancakeSwap / Uniswap V3 官方池")
    def f():
        tid = format(int(token_id), "064x")
        try: w = words(call(npm, "positions(uint256)", tid))
        except RuntimeError: raise ValueError("NFT 编号不存在或仓位已销毁")
        t0, t1, fee = addr(hex(w[2])), addr(hex(w[3])), w[4]
        if (t0, t1, fee) != (pool_info["token0"], pool_info["token1"], pool_info["fee"]):
            raise ValueError("这个 NFT 不属于当前池子")
        tl, tu, L = _int24(w[5]), _int24(w[6]), w[7]
        owner = addr(call(npm, "ownerOf(uint256)", tid))
        data = sel("collect((uint256,address,uint128,uint128))") + tid + pad(owner) + format(U128, "064x") * 2
        owed = words(rpc("eth_call", [{"from": owner, "to": npm, "data": data}, "latest"]))
        sp = words(call(pool_info["pool"], "slot0()"))[0] / 2**96
        sa, sb = 1.0001 ** (tl / 2), 1.0001 ** (tu / 2)
        spc = min(max(sp, sa), sb)
        a0 = L * (sb - spc) / (spc * sb) / 10 ** pool_info["d0"]
        a1 = L * (spc - sa) / 10 ** pool_info["d1"]
        f0, f1 = owed[0] / 10 ** pool_info["d0"], owed[1] / 10 ** pool_info["d1"]
        p = pool_info["price"]
        if pool_info["stable_is_1"]:
            stock, stable, fs, fst = a0, a1, f0, f1
            dd = 10 ** (pool_info["d0"] - pool_info["d1"]); lo, hi = 1.0001 ** tl * dd, 1.0001 ** tu * dd
        else:
            stock, stable, fs, fst = a1, a0, f1, f0
            dd = 10 ** (pool_info["d0"] - pool_info["d1"]); lo, hi = 1 / (1.0001 ** tu * dd), 1 / (1.0001 ** tl * dd)
        q = pool_info.get("qusd", 1.0)
        vq, fq = stock * p + stable, fs * p + fst
        return dict(token_id=int(token_id), owner=owner, liquidity=L, lo=lo, hi=hi, in_range=lo <= p <= hi,
                    stock=stock, stable=stable, value=vq * q, fees=fq * q, value_q=vq, fees_q=fq, qusd=q,
                    quote_sym=pool_info.get("quote_sym", "USD"), price=p)
    return cached(("nft", pool_info["pool"], int(token_id)), 30, f)

def locate(token_id):
    """同一个编号在 PancakeSwap 和 Uniswap 上可能各有一个仓位，全部返回：[(factory, 池子, 流动性, 持有人)]。"""
    from app.chain import addr as _a
    tid = format(int(token_id), "064x")
    out = []
    for factory, npm in NPM.items():
        try: w = words(call(npm, "positions(uint256)", tid))
        except RuntimeError: continue
        t0, t1 = _a(format(w[2], "040x")), _a(format(w[3], "040x"))
        pool = _a(call(factory, "getPool(address,address,uint24)", pad(t0) + pad(t1) + format(w[4], "064x")))
        owner = _a(call(npm, "ownerOf(uint256)", tid))
        out.append((factory, pool, w[7], owner))
    if not out: raise ValueError("在 PancakeSwap 和 Uniswap 上都找不到这个 NFT（编号不存在或已销毁）")
    return out

# ---------- 全周期历史：从 NFT 铸造开始的加仓 / 减仓 / 领取 ----------
import time as _time, requests as _rq
from app.chain import k as _k
ARCHIVE = [u for u in os.environ.get("BSC_ARCHIVE_RPC", "").split(",") if u] + [
    "https://bsc-mainnet.nodereal.io/v1/64a9df0874fb4a93b9d0a3849de012d3",
    "https://bsc-mainnet.public.blastapi.io", "https://bnb.api.onfinality.io/public"]
EV = {"inc": "0x" + _k("IncreaseLiquidity(uint256,uint128,uint256,uint256)"),
      "dec": "0x" + _k("DecreaseLiquidity(uint256,uint128,uint256,uint256)"),
      "col": "0x" + _k("Collect(uint256,address,uint256,uint256)")}

def arpc(method, params):
    err = None
    for attempt in range(3):
        for u in ARCHIVE:
            try:
                r = _rq.post(u, json={"jsonrpc": "2.0", "id": 1, "method": method, "params": params}, timeout=20).json()
                if "result" in r: return r["result"]
                e = str(r.get("error"))
                if "revert" in e.lower(): raise LookupError(e)
                err = e
            except LookupError: raise
            except Exception as ex: err = str(ex)
        _time.sleep(1 + attempt)
    raise RuntimeError(f"归档节点不可用：{err[:120] if err else ''}")

def _state(npm, tid, block):
    try: w = words(arpc("eth_call", [{"to": npm, "data": sel("positions(uint256)") + tid}, hex(block)]))
    except LookupError: return None
    return tuple(w[5:12])

def history(pool_info, token_id, lookback_days=60):
    """返回该 NFT 从铸造至今的链上事件，已换算成台账事件（deposit / withdraw / collect）。"""
    npm = NPM[pool_info["factory"]]; tid = format(int(token_id), "064x"); topic = "0x" + tid
    head = int(arpc("eth_blockNumber", []), 16)
    lo, hi = max(head - int(lookback_days * 86400 / 0.45), 1), head
    if _state(npm, tid, lo) is not None: raise ValueError(f"仓位早于 {lookback_days} 天前开仓，超出查询范围")
    while hi - lo > 1:                              # 二分找铸造区块
        m = (lo + hi) // 2
        if _state(npm, tid, m) is None: lo = m
        else: hi = m
    mint = hi
    blocks = set()
    def scan(a, b):                                 # 在 [a, b] 里找状态变化的区块
        logs = arpc("eth_getLogs", [{"address": npm, "fromBlock": hex(a), "toBlock": hex(b), "topics": [list(EV.values()), topic]}])
        for l in logs: blocks.add(int(l["blockNumber"], 16))
        return logs
    def bisect(a, sa, b, sb, out):
        if sa == sb: return
        if b - a <= 2000: out.extend(scan(a + 1, b)); return
        m = (a + b) // 2; sm = _state(npm, tid, m)
        bisect(a, sa, m, sm, out); bisect(m, sm, b, sb, out)
    logs = scan(mint, mint)
    bisect(mint, _state(npm, tid, mint), head, _state(npm, tid, head), logs)
    logs.sort(key=lambda l: (int(l["blockNumber"], 16), int(l["logIndex"], 16)))
    ts_cache, px_cache = {}, {}
    def ts(b):
        if b not in ts_cache: ts_cache[b] = int(arpc("eth_getBlockByNumber", [hex(b), False])["timestamp"], 16)
        return ts_cache[b]
    def price(b):
        if b not in px_cache:
            P = (words(arpc("eth_call", [{"to": pool_info["pool"], "data": sel("slot0()")}, hex(b)]))[0] / 2**96) ** 2 * 10 ** (pool_info["d0"] - pool_info["d1"])
            px_cache[b] = P if pool_info["stable_is_1"] else 1 / P
        return px_cache[b]
    from app.analytics import CHAINLINK_BNB
    bnb_q = pool_info.get("quote_kind") == "bnb"
    q_cache = {}
    def qusd(b):            # 该区块时计价币的美元价（BNB 用 Chainlink 历史值）
        if not bnb_q: return 1.0
        if b not in q_cache: q_cache[b] = int(arpc("eth_call", [{"to": CHAINLINK_BNB, "data": sel("latestAnswer()")}, hex(b)]), 16) / 1e8
        return q_cache[b]
    d0, d1 = 10 ** pool_info["d0"], 10 ** pool_info["d1"]
    split = lambda a0, a1: (a0 / d0, a1 / d1) if pool_info["stable_is_1"] else (a1 / d1, a0 / d0)  # (股票, 稳定币)
    owed_stock = owed_stable = 0.0                  # 减仓后还没领走的本金
    events = []
    for l in logs:
        b = int(l["blockNumber"], 16); data = l["data"][2:]; kind = next(n for n, t in EV.items() if t == l["topics"][0])
        p = price(b); t = ts(b); q = qusd(b)
        if kind in ("inc", "dec"):
            st, sb = split(int(data[64:128], 16), int(data[128:192], 16))
            if kind == "inc":
                events.append({"type": "deposit", "ts": t, "stock": st, "stable": sb, "value_q": st * p + sb, "value": (st * p + sb) * q,
                               "qusd": q, "price": p, "block": b, "note": "链上加仓" if events else "链上开仓"})
            else:
                owed_stock += st; owed_stable += sb
                events.append({"type": "withdraw", "ts": t, "stock": st, "stable": sb, "value_q": st * p + sb, "value": (st * p + sb) * q,
                               "qusd": q, "price": p, "block": b, "note": "链上减仓"})
        else:
            st, sb = split(int(data[64:128], 16), int(data[128:192], 16))
            fs, fb = max(st - owed_stock, 0), max(sb - owed_stable, 0)
            owed_stock, owed_stable = max(owed_stock - st, 0), max(owed_stable - sb, 0)
            fee = fs * p + fb
            if fee > 1e-9: events.append({"type": "collect", "ts": t, "fees_q": fee, "fees": fee * q, "qusd": q, "price": p, "block": b, "note": "链上领取手续费"})
    if not events or events[0]["type"] != "deposit": raise ValueError("没有找到开仓记录")
    return {"mint_block": mint, "opened": events[0]["ts"], "events": events}
