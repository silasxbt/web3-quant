"""按 V3 仓位 NFT 编号直接读链上：区间、当前价值、未领取手续费。只读，不需要私钥。"""
import math
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
        return dict(token_id=int(token_id), owner=owner, liquidity=L, lo=lo, hi=hi, in_range=lo <= p <= hi,
                    stock=stock, stable=stable, value=stock * p + stable, fees=fs * p + fst, price=p)
    return cached(("nft", pool_info["pool"], int(token_id)), 30, f)

def locate(token_id):
    """在 PancakeSwap / Uniswap 两个仓位管理合约里找这个 NFT，返回 (factory, 池子地址)。"""
    from app.chain import addr as _a
    tid = format(int(token_id), "064x")
    for factory, npm in NPM.items():
        try: w = words(call(npm, "positions(uint256)", tid))
        except RuntimeError: continue
        t0, t1 = _a(format(w[2], "040x")), _a(format(w[3], "040x"))
        pool = _a(call(factory, "getPool(address,address,uint24)", pad(t0) + pad(t1) + format(w[4], "064x")))
        return factory, pool
    raise ValueError("在 PancakeSwap 和 Uniswap 上都找不到这个 NFT（编号不存在或已销毁）")
