"""私有版：在公开版 rwalp 之上加分析预设、仓位台账、收益记录。整站只认一把 Passkey（见 passkey.py）。"""
import os, time, pathlib
from fastapi import FastAPI, HTTPException, Request, Body
from fastapi.responses import HTMLResponse, JSONResponse
from pydantic import BaseModel, Field
import app.main as public
from app import analytics
from app.chain import cached, call, words
from . import store, ledger, onchain, passkey

HERE = pathlib.Path(__file__).parent
AUTH_HTML = (HERE / "static/auth.html").read_text()
app = FastAPI(title="rwalp private", docs_url=None, redoc_url=None, openapi_url=None)
# 仅本地开发：必须同时开启内存存储才生效，线上（Firestore）永远不会跳过登录
DEV_NOAUTH = os.environ.get("RWALP_DEV_NOAUTH") == "1" and store.MEM
PUBLIC_PATHS = {"/health", "/favicon.svg", "/favicon.ico", "/setup",
                "/auth/register/options", "/auth/register", "/auth/login/options", "/auth/login"}
NO_STORE = {"Cache-Control": "no-store", "X-Frame-Options": "DENY", "Referrer-Policy": "no-referrer"}

def auth_page(mode, status=200):
    return HTMLResponse(AUTH_HTML.replace("{{MODE}}", mode), status, headers=NO_STORE)

@app.middleware("http")
async def passkey_gate(request: Request, call_next):
    path = request.url.path
    if DEV_NOAUTH or path in PUBLIC_PATHS:
        resp = await call_next(request)
    elif passkey.session_ok(request.cookies.get(passkey.COOKIE)):
        resp = await call_next(request)
    elif path == "/":
        resp = auth_page("login" if store.get_cred() else "setup")
    else:
        resp = JSONResponse({"detail": "未登录"}, 401)
    for k, v in NO_STORE.items(): resp.headers.setdefault(k, v)
    return resp

def _auth(fn):
    try: return fn()
    except passkey.AuthError as e: raise HTTPException(e.status, str(e))

@app.get("/setup")
def setup_page():
    if store.get_cred(): raise HTTPException(403, "已注册")
    return auth_page("setup")

@app.post("/auth/register/options")
def reg_options(body: dict = Body(default={})): return _auth(lambda: passkey.register_options(body.get("code")))

@app.post("/auth/register")
def reg(body: dict = Body(...)):
    cid = _auth(lambda: passkey.register(body.get("code"), body))
    return _session_response(cid)

@app.post("/auth/login/options")
def login_options(): return _auth(passkey.login_options)

@app.post("/auth/login")
def login(body: dict = Body(...)):
    tok = _auth(lambda: passkey.login(body))
    r = JSONResponse({"ok": True}); _set_cookie(r, tok); return r

@app.post("/auth/logout")
def logout():
    r = JSONResponse({"ok": True}); r.delete_cookie(passkey.COOKIE, path="/"); return r

def _set_cookie(r, tok):
    r.set_cookie(passkey.COOKIE, tok, max_age=passkey.SESSION_TTL, path="/", httponly=True, secure=True, samesite="strict")

def _session_response(cid):
    tok = _auth(lambda: passkey.session_token(cid))
    r = JSONResponse({"ok": True}); _set_cookie(r, tok); return r

@app.get("/", response_class=HTMLResponse)
def index():
    html = public.INDEX.replace("{{BOT}}", os.environ.get("TG_BOT_USERNAME", ""))
    html = html.replace("<title>rwalp</title>", "<title>rwalp · 私有</title>")
    extra = f"<style>{(HERE / 'static/private.css').read_text()}</style><script>{(HERE / 'static/private.js').read_text()}</script>"
    return html.replace("</body>", extra + "</body>")

# ---------- 预设 ----------
class Preset(BaseModel):
    name: str = Field(min_length=1, max_length=40)
    pool: str = Field(pattern="^0x[0-9a-fA-F]{40}$")
    lo: float | None = Field(None, gt=0)
    hi: float | None = Field(None, gt=0)
    capital: float = Field(1000, gt=0, le=1e8)
    ranges: str = Field("-3:3,-4:6,-5:7", max_length=80)
    shift: float = Field(0.5, gt=0, le=2)
    days: int = Field(7, ge=1, le=30)
    market_hours: bool = True
    hedge: str = Field("none", pattern="^(none|perp|broker)$")

@app.get("/p/presets")
def list_presets(): return store.presets()

@app.post("/p/presets")
def create_preset(p: Preset): return {"id": store.save_preset(p.model_dump())}

@app.put("/p/presets/{pid}")
def update_preset(pid: str, p: Preset):
    try: return {"id": store.save_preset(p.model_dump(), pid)}
    except KeyError: raise HTTPException(404, "预设不存在")

@app.delete("/p/presets/{pid}")
def delete_preset(pid: str):
    if not store.delete_preset(pid): raise HTTPException(404, "预设不存在")
    return {"ok": True}

# ---------- 仓位台账 ----------
class Position(BaseModel):
    name: str = Field(min_length=1, max_length=40)
    pool: str = Field(pattern="^0x[0-9a-fA-F]{40}$")
    lo: float = Field(gt=0)
    hi: float = Field(gt=0)
    token_id: int | None = Field(None, ge=0)
    opened: float | None = None
    note: str = Field("", max_length=500)

class PositionPatch(BaseModel):
    name: str | None = Field(None, min_length=1, max_length=40)
    lo: float | None = Field(None, gt=0)
    hi: float | None = Field(None, gt=0)
    token_id: int | None = Field(None, ge=0)
    note: str | None = Field(None, max_length=500)
    status: str | None = Field(None, pattern="^(open|closed)$")

class Event(BaseModel):
    fees_before: float = Field(0, ge=0)
    type: str = Field(pattern="^(deposit|withdraw|collect|cost|snapshot)$")
    ts: float | None = None
    value: float = Field(0, ge=0)
    stock: float = Field(0, ge=0)
    stable: float = Field(0, ge=0)
    fees: float = Field(0, ge=0)
    cost: float = Field(0, ge=0)
    price: float | None = Field(None, gt=0)
    note: str = Field("", max_length=300)

def light_info(pool):
    """只读链上：池子静态信息（缓存一天）+ 当前价格。比 analytics.pool_info 快，不调用 GeckoTerminal。"""
    pool = pool.lower()
    s = cached(("static", pool), 86400, lambda: analytics._static(pool))
    P = (words(call(pool, "slot0()"))[0] / 2**96) ** 2 * 10 ** (s["d0"] - s["d1"])
    s.setdefault("quote_sym", analytics.QUOTES.get(s["stable"])); s.setdefault("quote_kind", "bnb" if s["stable"] == analytics.WBNB else "usd")
    return dict(s, price=P if s["stable_is_1"] else 1 / P, qusd=analytics.quote_usd(s["stable"]))

def _live(pos):
    """读池子价格；有 NFT 编号时读链上实时价值和未领取手续费。"""
    out = {"price": None, "nft": None, "error": None}
    try:
        i = light_info(pos["pool"]); out["price"] = i["price"]; out["qusd"] = i["qusd"]; out["quote_sym"] = i["quote_sym"]
        if pos.get("token_id"): out["nft"] = onchain.read(i, pos["token_id"])
    except Exception as e:
        out["error"] = str(e)[:160]
    return out

def _view(pos, evs):
    live = _live(pos) if pos.get("status") != "closed" else {"price": None, "nft": None, "error": None}
    nft = live.get("nft")
    if nft:  # 链上实时数据视为最新快照
        evs = evs + [{"type": "snapshot", "ts": time.time(), "value": nft["value"], "fees": nft["fees"],
                      "value_q": nft["value_q"], "fees_q": nft["fees_q"], "qusd": nft["qusd"]}]
    q = live.get("qusd") or pos.get("qusd") or 1.0
    s = ledger.summarize(pos, evs, live.get("price"), q, "usd")
    sym = live.get("quote_sym") or pos.get("quote_sym") or "USDT"
    sq = ledger.summarize(pos, evs, live.get("price"), q, "q") if sym == "BNB" else None
    p = live.get("price")
    in_range = (pos["lo"] <= p <= pos["hi"]) if p else None
    attr = None
    if sq and sq.get("attr") and s.get("pnl") is not None:   # 股票/BNB 仓位：先按 BNB 本位归因再折成 U，剩下的就是 BNB 自身涨跌
        attr = {k: v * q for k, v in sq["attr"].items()}
        attr["quote"] = s["pnl"] - sq["pnl"] * q
    elif s.get("attr"):
        attr = dict(s["attr"])
    return dict(pos, live=live, summary=s, summary_q=sq, attribution=attr, quote_sym=sym, qusd=q, in_range=in_range,
                events=len(evs) - (1 if nft else 0))

@app.get("/p/positions")
def list_positions():
    evs = store.events(); by = {}
    for e in evs: by.setdefault(e.get("position"), []).append(e)
    from concurrent.futures import ThreadPoolExecutor
    ps = store.positions()
    with ThreadPoolExecutor(8) as ex:
        return list(ex.map(lambda p: _view(p, by.get(p["id"], [])), ps))

@app.post("/p/positions")
def create_position(p: Position):
    if sum(x.get("status") != "closed" for x in store.positions()) >= MAX_OPEN:
        raise HTTPException(400, f"最多同时记录 {MAX_OPEN} 个仓位，先平仓或删除一个")
    d = p.model_dump(); d["opened"] = d["opened"] or time.time(); d["status"] = "open"
    d["pool"] = d["pool"].lower(); d["lo"], d["hi"] = sorted((d["lo"], d["hi"]))
    try: d["quote_sym"] = light_info(d["pool"])["quote_sym"]
    except Exception: pass
    return {"id": store.save_position(d)}

@app.patch("/p/positions/{pid}")
def patch_position(pid: str, p: PositionPatch):
    d = p.model_dump(exclude_unset=True)
    if d.get("status") == "closed": d["closed_at"] = time.time()
    try: store.save_position(d, pid)
    except KeyError: raise HTTPException(404, "仓位不存在")
    return {"ok": True}

@app.delete("/p/positions/{pid}")
def delete_position(pid: str):
    if not store.delete_position(pid): raise HTTPException(404, "仓位不存在")
    return {"ok": True}

@app.get("/p/positions/{pid}/events")
def list_events(pid: str): return store.events(pid)

@app.post("/p/positions/{pid}/events")
def add_event(pid: str, e: Event):
    pos = store.get_position(pid)
    if not pos: raise HTTPException(404, "仓位不存在")
    d = e.model_dump(); d["position"] = pid; d["ts"] = d["ts"] or time.time()
    live = _live(pos); q = live.get("qusd") or 1.0
    d["qusd"] = q
    if d["type"] in ("deposit", "withdraw"):
        if not d["value"]:   # 按数量算：stable 是计价币数量（股票/BNB 池即 BNB）
            if not d["price"]: d["price"] = live["price"]
            if not d["price"]: raise HTTPException(400, "取不到价格，请手动填写总价值")
            d["value_q"] = d["stock"] * d["price"] + d["stable"]; d["value"] = d["value_q"] * q
        else:                # 手填总价值按美元
            d["value_q"] = d["value"] / q
    for k in ("fees", "cost", "fees_before"):
        d[k + "_q"] = d.get(k, 0) / q
    if d["type"] == "snapshot": d["value_q"] = d["value"] / q
    return {"id": store.add_event(d)}

@app.post("/p/positions/{pid}/sync")
def sync_snapshot(pid: str):
    """按 NFT 编号把链上实时价值和未领取手续费记成一条快照。"""
    pos = store.get_position(pid)
    if not pos or not pos.get("token_id"): raise HTTPException(400, "这个仓位没有填 NFT 编号")
    live = _live(pos)
    if not live.get("nft"): raise HTTPException(502, live.get("error") or "读取链上仓位失败")
    n = live["nft"]
    hist, err = _history(light_info(pos["pool"]), pos["token_id"])   # 顺便补上导入之后的链上加仓、减仓、领取
    n = dict(n, history_added=_apply_history(pid, pos, hist)["added"] if hist else None, history_error=err)
    store.add_event({"position": pid, "type": "snapshot", "ts": time.time(), "value": n["value"], "fees": n["fees"],
                     "value_q": n["value_q"], "fees_q": n["fees_q"], "qusd": n["qusd"],
                     "stock": n["stock"], "stable": n["stable"], "price": n["price"], "note": "链上同步"})
    return n

@app.delete("/p/events/{eid}")
def delete_event(eid: str):
    if not store.delete_event(eid): raise HTTPException(404, "记录不存在")
    return {"ok": True}

MAX_OPEN = 10

class ImportReq(BaseModel):
    token_ids: list[int] = Field(min_length=1, max_length=10)

def _ticker(info):
    sym = info["symbol0"] if info["stable_is_1"] else info["symbol1"]
    return sym.upper()

def _pick(tid, owners):
    """同号 NFT 在两个 DEX 上都存在时：只留有流动性、且是股票/稳定币池的；还有多个就优先已导入仓位的同一钱包。"""
    cands = []
    for factory, pool, liq, owner in onchain.locate(tid):
        try: info = light_info(pool)
        except Exception: continue
        cands.append((info, liq, owner))
    if not cands: raise ValueError("读取池子失败")
    good = [c for c in cands if c[0]["stable"] in analytics.QUOTES and c[0]["stock"] not in analytics.QUOTES]
    if not good: raise ValueError("只支持股票/USDT 或 股票/USDC 的池子")
    live = [c for c in good if c[1]] or good
    mine = [c for c in live if c[2] in owners] or live
    if len(mine) > 1: raise ValueError("这个编号在 PancakeSwap 和 Uniswap 上都有有效仓位，无法判断是哪个，请先导入同钱包的其他仓位")
    return mine[0][0]

def _import_one(tid, existing, owners):
    if tid in existing: return {"token_id": tid, "ok": False, "error": "已经导入过"}
    try:
        info = _pick(tid, owners)
        n = onchain.read(info, tid)
    except Exception as e:
        return {"token_id": tid, "ok": False, "error": str(e)[:160]}
    if not n["liquidity"]: return {"token_id": tid, "ok": False, "error": "仓位流动性为 0（已撤池）"}
    hist, err = _history(info, tid)
    now = time.time()
    nm = f"{_ticker(info)}{'/BNB' if info['quote_kind'] == 'bnb' else ''} #{tid}"
    pid = store.save_position({"name": nm, "pool": info["pool"], "lo": n["lo"], "hi": n["hi"], "quote_sym": info["quote_sym"],
                               "token_id": tid, "owner": n["owner"], "opened": hist["opened"] if hist else now, "status": "open",
                               "note": "快捷导入" if hist else f"快捷导入（未取到开仓历史：{err}）"})
    _write_history(pid, hist, n, now)
    return {"token_id": tid, "ok": True, "id": pid, "name": nm, "lo": n["lo"], "hi": n["hi"],
            "value": n["value"], "fees": n["fees"], "value_q": n["value_q"], "quote_sym": n["quote_sym"], "in_range": n["in_range"]}

def _history(info, tid):
    try: return onchain.history(info, tid), None
    except Exception as e: return None, str(e)[:80]

def _write_history(pid, hist, n, now):
    """全周期口径：按链上开仓/加仓/减仓/领取写台账；取不到历史时退回「导入时持仓为本金、扣掉已有手续费」。"""
    if hist:
        for e in hist["events"]: store.add_event(dict(e, position=pid))
    else:
        store.add_event({"position": pid, "type": "deposit", "ts": now, "stock": n["stock"], "stable": n["stable"],
                         "value": n["value"], "value_q": n["value_q"], "qusd": n["qusd"], "price": n["price"], "fees": 0, "cost": 0,
                         "fees_before": n["fees"], "fees_before_q": n["fees_q"],
                         "note": "快捷导入：按导入时链上持仓作为本金基准"})

def _chain_event(e):
    return e.get("block") is not None or str(e.get("note", "")).startswith("快捷导入")

def _apply_history(pid, pos, hist):
    """用链上历史替换台账里链上来源的记录（开仓、加仓、减仓、领取），手动记的保留。"""
    old = [e for e in store.events(pid) if e["type"] in ("deposit", "withdraw", "collect") and _chain_event(e)]
    have = {(e.get("block"), e["type"]) for e in old}
    new = [e for e in hist["events"] if (e.get("block"), e["type"]) not in have]
    if len(old) != len(hist["events"]) or new:
        for e in old: store.delete_event(e["id"])
        _write_history(pid, hist, None, time.time())
    store.save_position({"opened": hist["opened"], "note": (pos.get("note") or "").split("（未取到")[0]}, pid)
    return {"opened": hist["opened"], "events": len(hist["events"]), "added": len(new)}

@app.post("/p/positions/{pid}/rebuild")
def rebuild(pid: str):
    """从链上重建这个仓位的全周期台账（替换掉链上可得的记录，保留手动记的成本和备注类记录）。"""
    pos = store.get_position(pid)
    if not pos or not pos.get("token_id"): raise HTTPException(400, "这个仓位没有 NFT 编号")
    info = light_info(pos["pool"])
    hist, err = _history(info, pos["token_id"])
    if not hist: raise HTTPException(502, f"读取开仓历史失败：{err}")
    return _apply_history(pid, pos, hist)

@app.post("/p/import")
def quick_import(req: ImportReq):
    ids = list(dict.fromkeys(req.token_ids))
    ps = store.positions()
    existing = {p.get("token_id") for p in ps}
    active = sum(p.get("status") != "closed" for p in ps)
    new = [i for i in ids if i not in existing]
    if active + len(new) > MAX_OPEN:
        raise HTTPException(400, f"最多同时记录 {MAX_OPEN} 个仓位，当前已有 {active} 个，还能导入 {max(MAX_OPEN - active, 0)} 个")
    from concurrent.futures import ThreadPoolExecutor
    with ThreadPoolExecutor(5) as ex:
        owners = {p.get("owner") for p in ps if p.get("owner")}
        return list(ex.map(lambda t: _import_one(t, existing, owners), ids))

# ---------- 自选标的 ----------
MAX_TICKERS = 10
DEFAULT_TICKERS = {
    "QQQB": "0xfc4e77248b76fefc27c4cac7151a2ee5b5cc590e", "AAPLB": "0xe9b9998b2ec5430d2246c7f1f8d9f298c97d7365",
    "GOOGLB": "0x89001d846f7ca36ee089f73eefc25657e1798144", "NVDAB": "0x8fb4243b553ac29ba088acf00b9b7da24bd6690c",
    "BABAB": "0xfd95cb1391999006eb91797a7c62acfe88b20292", "PDDB": "0xd0c2a4dc7c581db2660286660b703cf6883f29ba",
    "MSTRB": "0x692081209619735f25700557078ab084d3e5d007", "MSFTB": "0x5018b018ceb7645c927c5cf246786f89ebcbe7ea",
    "METAB": "0xc2151a561e928d16576d75ea88544543ac63d80b"}

def _seed_tickers():
    ts = store.tickers()
    if ts or store.get_state_flag("tickers_seeded"): return ts
    for n, (sym, pool) in enumerate(DEFAULT_TICKERS.items()):
        store.add_ticker({"symbol": sym, "pool": pool, "order": n})
    store.set_state_flag("tickers_seeded")
    return store.tickers()

class TickerReq(BaseModel):
    pool: str = Field(pattern="^0x[0-9a-fA-F]{40}$")
    symbol: str | None = Field(None, max_length=12)

@app.get("/p/tickers")
def list_tickers(): return _seed_tickers()

@app.post("/p/tickers")
def add_ticker(t: TickerReq):
    ts = _seed_tickers(); pool = t.pool.lower()
    if any(x["pool"] == pool for x in ts): raise HTTPException(400, "这个池子已经在列表里")
    if len(ts) >= MAX_TICKERS: raise HTTPException(400, f"最多 {MAX_TICKERS} 个，先删掉一个")
    try: info = light_info(pool)
    except Exception: raise HTTPException(400, "读不到这个池子：请确认是 BSC 上 PancakeSwap / Uniswap V3 官方池的地址")
    if not info.get("dex"): raise HTTPException(400, "不是 PancakeSwap / Uniswap V3 官方 factory 创建的池子")
    if info["stable"] not in analytics.QUOTES or info["stock"] in analytics.QUOTES:
        raise HTTPException(400, "只支持股票/USDT、股票/USDC 或 股票/BNB 的池子")
    sym = (t.symbol or _ticker(info)).strip().upper()[:12]
    tid = store.add_ticker({"symbol": sym, "pool": pool, "order": max([x.get("order", 0) for x in ts] + [-1]) + 1,
                            "dex": info["dex"], "fee": info["fee"], "bstocks": info["bstocks"]})
    return {"id": tid, "symbol": sym, "pool": pool, "dex": info["dex"], "fee": info["fee"], "bstocks": info["bstocks"]}

@app.delete("/p/tickers/{tid}")
def remove_ticker(tid: str):
    _seed_tickers()
    if not store.delete_ticker(tid): raise HTTPException(404, "不存在")
    return {"ok": True}

@app.get("/p/nft")
def nft(pool: str, token_id: int):
    try: return onchain.read(light_info(pool), token_id)
    except ValueError as e: raise HTTPException(400, str(e))

# 其余路由（分析、回测、深度、favicon 等）全部复用公开版
app.mount("/", public.app)
