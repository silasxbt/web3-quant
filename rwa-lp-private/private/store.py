"""私有数据：分析预设、仓位台账、收益记录（Firestore，集合前缀 p_）。"""
import os, time, uuid

MEM = os.environ.get("RWALP_MEMSTORE") == "1"  # 本地开发：数据只存在内存里
_mem = {}
_db = None
def db():
    global _db
    if _db is None:
        from google.cloud import firestore
        _db = firestore.Client()
    return _db

def _list(col, **where):
    if MEM: return [dict(v, id=k) for k, v in _mem.get(col, {}).items() if all(v.get(a) == b for a, b in where.items())]
    q = db().collection(col)
    for k, v in where.items(): q = q.where(k, "==", v)
    return [dict(d.to_dict(), id=d.id) for d in q.stream()]

def _put(col, data, did=None):
    if MEM:
        c = _mem.setdefault(col, {})
        if did and did not in c: raise KeyError(did)
        did = did or uuid.uuid4().hex[:20]
        c[did] = {**c.get(did, {}), **data, "updated": time.time()}
        return did
    ref = db().collection(col).document(did) if did else db().collection(col).document()
    if did and not ref.get().exists: raise KeyError(did)
    ref.set(dict(data, updated=time.time()), merge=bool(did))
    return ref.id

def _del(col, did):
    if MEM: return _mem.get(col, {}).pop(did, None) is not None
    ref = db().collection(col).document(did)
    if not ref.get().exists: return False
    ref.delete(); return True

# 预设
def presets(): return sorted(_list("p_presets"), key=lambda p: p.get("updated", 0), reverse=True)
def save_preset(data, pid=None): return _put("p_presets", data, pid)
def delete_preset(pid): return _del("p_presets", pid)

# 仓位与事件
def positions(): return sorted(_list("p_positions"), key=lambda p: p.get("opened", 0), reverse=True)
def get_position(pid):
    if MEM:
        v = _mem.get("p_positions", {}).get(pid)
        return dict(v, id=pid) if v else None
    d = db().collection("p_positions").document(pid).get()
    return dict(d.to_dict(), id=d.id) if d.exists else None
def save_position(data, pid=None): return _put("p_positions", data, pid)
def delete_position(pid):
    for e in events(pid): _del("p_events", e["id"])
    return _del("p_positions", pid)

def events(pid=None):
    xs = _list("p_events", position=pid) if pid else _list("p_events")
    return sorted(xs, key=lambda e: e.get("ts", 0))
def add_event(data): return _put("p_events", data)
def delete_event(eid): return _del("p_events", eid)

# ---- Passkey：唯一凭证 + 一次性 challenge ----
import threading
_lock = threading.Lock()
CRED = ("p_auth", "owner")

def get_cred():
    if MEM: return _mem.get("auth_cred")
    d = db().collection(CRED[0]).document(CRED[1]).get()
    return d.to_dict() if d.exists else None

def create_cred(data):
    """只可能成功一次：Firestore create() 在文档已存在时抛 AlreadyExists（等价 O_CREAT|O_EXCL）。"""
    if MEM:
        with _lock:
            if "auth_cred" in _mem: raise FileExistsError
            _mem["auth_cred"] = dict(data); return
    from google.api_core.exceptions import AlreadyExists
    try: db().collection(CRED[0]).document(CRED[1]).create(data)
    except AlreadyExists: raise FileExistsError

def update_counter(old, new):
    """计数器只能前进：事务内比较旧值，防并发回退。"""
    if MEM:
        with _lock:
            c = _mem["auth_cred"]
            if c["sign_count"] != old: return False
            c["sign_count"] = new; return True
    from google.cloud import firestore
    ref = db().collection(CRED[0]).document(CRED[1])
    @firestore.transactional
    def tx(t):
        snap = ref.get(transaction=t)
        if not snap.exists or snap.to_dict()["sign_count"] != old: return False
        t.update(ref, {"sign_count": new}); return True
    return tx(db().transaction())

def put_challenge(ch, purpose):
    rec = {"purpose": purpose, "ts": time.time()}
    if MEM:
        with _lock: _mem.setdefault("auth_ch", {})[ch] = rec
        return
    db().collection("p_auth_challenges").document(ch).set(rec)

def take_challenge(ch):
    """取出并删除（用后即焚）；不存在返回 None。"""
    if MEM:
        with _lock: return _mem.setdefault("auth_ch", {}).pop(ch, None)
    from google.cloud import firestore
    ref = db().collection("p_auth_challenges").document(ch)
    @firestore.transactional
    def tx(t):
        snap = ref.get(transaction=t)
        if not snap.exists: return None
        t.delete(ref); return snap.to_dict()
    return tx(db().transaction())

# ---- 自选标的（计算模式的快捷按钮） ----
def tickers(): return sorted(_list("p_tickers"), key=lambda t: t.get("order", 0))
def add_ticker(data): return _put("p_tickers", data)
def delete_ticker(tid): return _del("p_tickers", tid)

def get_state_flag(name):
    if MEM: return _mem.get("flags", {}).get(name)
    d = db().collection("p_state").document(name).get()
    return d.exists

def set_state_flag(name):
    if MEM: _mem.setdefault("flags", {})[name] = True; return
    db().collection("p_state").document(name).set({"at": time.time()})
