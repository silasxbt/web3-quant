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
PUBLIC_PATHS = {"/health", "/favicon.svg", "/favicon.ico", "/setup",
                "/auth/register/options", "/auth/register", "/auth/login/options", "/auth/login"}
NO_STORE = {"Cache-Control": "no-store", "X-Frame-Options": "DENY", "Referrer-Policy": "no-referrer"}

def auth_page(mode, status=200):
    return HTMLResponse(AUTH_HTML.replace("{{MODE}}", mode), status, headers=NO_STORE)

@app.middleware("http")
async def passkey_gate(request: Request, call_next):
    path = request.url.path
    if path in PUBLIC_PATHS:
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
    return dict(s, price=P if s["stable_is_1"] else 1 / P)

def _live(pos):
    """读池子价格；有 NFT 编号时读链上实时价值和未领取手续费。"""
    out = {"price": None, "nft": None, "error": None}
    try:
        i = light_info(pos["pool"]); out["price"] = i["price"]
        if pos.get("token_id"): out["nft"] = onchain.read(i, pos["token_id"])
    except Exception as e:
        out["error"] = str(e)[:160]
    return out

def _view(pos, evs):
    live = _live(pos) if pos.get("status") != "closed" else {"price": None, "nft": None, "error": None}
    nft = live.get("nft")
    if nft:  # 链上实时数据视为最新快照
        evs = evs + [{"type": "snapshot", "ts": time.time(), "value": nft["value"], "fees": nft["fees"]}]
    s = ledger.summarize(pos, evs, live.get("price"))
    p = live.get("price")
    in_range = (pos["lo"] <= p <= pos["hi"]) if p else None
    return dict(pos, live=live, summary=s, in_range=in_range, events=len(evs) - (1 if nft else 0))

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
    if d["type"] in ("deposit", "withdraw") and not d["value"]:
        if not d["price"]: d["price"] = _live(pos)["price"]
        if not d["price"]: raise HTTPException(400, "取不到价格，请手动填写总价值")
        d["value"] = d["stock"] * d["price"] + d["stable"]
    return {"id": store.add_event(d)}

@app.post("/p/positions/{pid}/sync")
def sync_snapshot(pid: str):
    """按 NFT 编号把链上实时价值和未领取手续费记成一条快照。"""
    pos = store.get_position(pid)
    if not pos or not pos.get("token_id"): raise HTTPException(400, "这个仓位没有填 NFT 编号")
    live = _live(pos)
    if not live.get("nft"): raise HTTPException(502, live.get("error") or "读取链上仓位失败")
    n = live["nft"]
    store.add_event({"position": pid, "type": "snapshot", "ts": time.time(), "value": n["value"], "fees": n["fees"],
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
    good = [c for c in cands if c[0]["stable"] in analytics.STABLES]
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
    now = time.time()
    pid = store.save_position({"name": f"{_ticker(info)} #{tid}", "pool": info["pool"], "lo": n["lo"], "hi": n["hi"],
                               "token_id": tid, "owner": n["owner"], "opened": now, "status": "open", "note": "快捷导入"})
    store.add_event({"position": pid, "type": "deposit", "ts": now, "stock": n["stock"], "stable": n["stable"],
                     "value": n["value"], "price": n["price"], "fees": 0, "cost": 0,
                     "note": "快捷导入：按导入时链上持仓作为本金基准，可删掉后补记真实投入"})
    return {"token_id": tid, "ok": True, "id": pid, "name": f"{_ticker(info)} #{tid}", "lo": n["lo"], "hi": n["hi"],
            "value": n["value"], "fees": n["fees"], "in_range": n["in_range"]}

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

@app.get("/p/nft")
def nft(pool: str, token_id: int):
    try: return onchain.read(light_info(pool), token_id)
    except ValueError as e: raise HTTPException(400, str(e))

# 其余路由（分析、回测、深度、favicon 等）全部复用公开版
app.mount("/", public.app)
