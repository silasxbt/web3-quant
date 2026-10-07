"""单 Passkey 极端登录：无账号、无密码，整站只信任第一次注册的那一把 Passkey（WebAuthn）。

- 注册需要一次性设置码（环境变量 SETUP_CODE），注册成功后注册入口永久关闭。
- 凭证存 Firestore p_auth/owner，用 create() 保证只写成功一次；重置 = 用 gcloud 删除这个文档。
- 会话是 HMAC 签名的 cookie（Cloud Run 会缩容到 0，内存会话每次冷启动都会丢），绑定凭证 ID，凭证被删即全部失效。
"""
import os, json, time, hmac, base64, hashlib, secrets, struct
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec, rsa, padding
from cryptography.exceptions import InvalidSignature
from . import store

RP_ID = os.environ.get("RP_ID", "lp.silasxbt.com")
ORIGIN = os.environ.get("ORIGIN", f"https://{RP_ID}")
SETUP_CODE = os.environ.get("SETUP_CODE", "")
SESSION_SECRET = os.environ.get("SESSION_SECRET", "").encode()
SESSION_TTL = 30 * 86400
CHALLENGE_TTL = 300
COOKIE = "rwalp_session"

class AuthError(Exception):
    def __init__(self, status, msg): super().__init__(msg); self.status = status

def b64e(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()
def b64d(s): return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))

# ---------- 最小 CBOR 解码器 ----------
def cbor(data, i=0):
    ib = data[i]; mt, ai = ib >> 5, ib & 31; i += 1
    if ai < 24: val = ai
    elif ai == 24: val = data[i]; i += 1
    elif ai == 25: val = struct.unpack(">H", data[i:i + 2])[0]; i += 2
    elif ai == 26: val = struct.unpack(">I", data[i:i + 4])[0]; i += 4
    elif ai == 27: val = struct.unpack(">Q", data[i:i + 8])[0]; i += 8
    else: raise ValueError("不支持的 CBOR 长度")
    if mt == 0: return val, i
    if mt == 1: return -1 - val, i
    if mt == 2: return bytes(data[i:i + val]), i + val
    if mt == 3: return data[i:i + val].decode(), i + val
    if mt == 4:
        out = []
        for _ in range(val): v, i = cbor(data, i); out.append(v)
        return out, i
    if mt == 5:
        out = {}
        for _ in range(val): k, i = cbor(data, i); v, i = cbor(data, i); out[k] = v
        return out, i
    if mt == 7: return {20: False, 21: True, 22: None}.get(val, val), i
    raise ValueError("不支持的 CBOR 类型")

# ---------- challenge ----------
def new_challenge(purpose):
    ch = b64e(secrets.token_bytes(32))
    store.put_challenge(ch, purpose)
    return ch

def check_client_data(raw, typ, purpose):
    try: cd = json.loads(raw)
    except ValueError: raise AuthError(400, "clientDataJSON 无效")
    if cd.get("type") != typ: raise AuthError(400, "类型不匹配")
    if cd.get("origin") != ORIGIN: raise AuthError(400, "来源不匹配")
    rec = store.take_challenge(cd.get("challenge", ""))
    if not rec or rec["purpose"] != purpose or time.time() - rec["ts"] > CHALLENGE_TTL:
        raise AuthError(400, "challenge 无效或已过期")

def check_auth_data(ad, need_at=False):
    if len(ad) < 37 or ad[:32] != hashlib.sha256(RP_ID.encode()).digest(): raise AuthError(400, "RP ID 不匹配")
    flags = ad[32]
    if not (flags & 0x01 and flags & 0x04): raise AuthError(400, "必须完成生物识别验证")
    if need_at and not flags & 0x40: raise AuthError(400, "缺少凭证数据")
    return flags, struct.unpack(">I", ad[33:37])[0]

def code_ok(code):
    return bool(SETUP_CODE) and hmac.compare_digest((code or "").encode(), SETUP_CODE.encode())

# ---------- 注册 ----------
def register_options(code):
    if store.get_cred(): raise AuthError(403, "已注册")
    if not code_ok(code): raise AuthError(403, "设置码错误")
    return {"challenge": new_challenge("register"), "rp": {"id": RP_ID, "name": "rwalp"},
            "user": {"id": b64e(b"owner"), "name": "owner", "displayName": "rwalp"},
            "pubKeyCredParams": [{"type": "public-key", "alg": -7}, {"type": "public-key", "alg": -257}],
            "authenticatorSelection": {"residentKey": "required", "requireResidentKey": True, "userVerification": "required"},
            "attestation": "none", "timeout": CHALLENGE_TTL * 1000}

def register(code, body):
    if store.get_cred(): raise AuthError(403, "已注册")
    if not code_ok(code): raise AuthError(403, "设置码错误")
    try:
        r = body["response"]; cdj = b64d(r["clientDataJSON"]); att, _ = cbor(b64d(r["attestationObject"]))
    except (KeyError, ValueError, TypeError, IndexError): raise AuthError(400, "注册数据格式错误")
    check_client_data(cdj, "webauthn.create", "register")
    ad = att.get("authData") if isinstance(att, dict) else None
    if not isinstance(ad, bytes): raise AuthError(400, "缺少 authData")
    _, count = check_auth_data(ad, need_at=True)
    try:
        n = struct.unpack(">H", ad[53:55])[0]; cid = ad[55:55 + n]; key, _ = cbor(ad, 55 + n)
    except (struct.error, ValueError, IndexError): raise AuthError(400, "凭证数据无效")
    if key.get(3) not in (-7, -257): raise AuthError(400, "不支持的算法")
    _pubkey(key)  # 解析失败即拒绝
    try:
        store.create_cred({"cred_id": b64e(cid), "cose": b64e(cbor_key_bytes(ad, 55 + n)), "sign_count": count, "created": time.time()})
    except FileExistsError:
        raise AuthError(403, "已注册")
    return b64e(cid)

def cbor_key_bytes(ad, start):
    _, end = cbor(ad, start)
    return ad[start:end]

def _pubkey(key):
    try:
        if key[3] == -7:
            if key.get(1) != 2 or key.get(-1) != 1: raise ValueError
            return ec.EllipticCurvePublicNumbers(int.from_bytes(key[-2], "big"), int.from_bytes(key[-3], "big"), ec.SECP256R1()).public_key()
        if key[3] == -257:
            return rsa.RSAPublicNumbers(int.from_bytes(key[-2], "big"), int.from_bytes(key[-1], "big")).public_key()
    except (KeyError, ValueError, TypeError): pass
    raise AuthError(400, "公钥无效")

# ---------- 登录 ----------
def login_options():
    c = store.get_cred()
    if not c: raise AuthError(403, "尚未注册")
    return {"challenge": new_challenge("login"), "rpId": RP_ID, "userVerification": "required", "timeout": CHALLENGE_TTL * 1000,
            "allowCredentials": [{"type": "public-key", "id": c["cred_id"]}]}

def login(body):
    c = store.get_cred()
    if not c: raise AuthError(403, "尚未注册")
    if body.get("id") != c["cred_id"]: raise AuthError(403, "不是本站的 Passkey")
    try:
        r = body["response"]; cdj = b64d(r["clientDataJSON"]); ad = b64d(r["authenticatorData"]); sig = b64d(r["signature"])
    except (KeyError, ValueError, TypeError): raise AuthError(400, "登录数据格式错误")
    check_client_data(cdj, "webauthn.get", "login")
    _, count = check_auth_data(ad)
    key, _ = cbor(b64d(c["cose"])); pk = _pubkey(key)
    data = ad + hashlib.sha256(cdj).digest()
    try:
        if key[3] == -7: pk.verify(sig, data, ec.ECDSA(hashes.SHA256()))
        else: pk.verify(sig, data, padding.PKCS1v15(), hashes.SHA256())
    except InvalidSignature:
        raise AuthError(400, "签名无效")
    old = c["sign_count"]
    if count or old:
        if count <= old or not store.update_counter(old, count): raise AuthError(400, "签名计数器回退，疑似克隆")
    return session_token(c["cred_id"])

# ---------- 会话 ----------
_cc = {"t": 0, "v": None}
def _cred_cached():
    """会话校验每个请求都要用，缓存 30 秒（凭证被删后最多 30 秒内全部会话失效）。"""
    if time.time() - _cc["t"] > 30: _cc.update(t=time.time(), v=store.get_cred())
    return _cc["v"]

def _mac(payload): return hmac.new(SESSION_SECRET, payload.encode(), hashlib.sha256).hexdigest()

def session_token(cid):
    if len(SESSION_SECRET) < 32: raise AuthError(500, "SESSION_SECRET 未配置")
    p = b64e(json.dumps({"exp": int(time.time()) + SESSION_TTL, "cid": hashlib.sha256(cid.encode()).hexdigest()[:32],
                         "n": secrets.token_hex(8)}).encode())
    return f"{p}.{_mac(p)}"

def session_ok(tok):
    if not tok or "." not in tok or len(SESSION_SECRET) < 32: return False
    p, m = tok.rsplit(".", 1)
    if not hmac.compare_digest(m, _mac(p)): return False
    try: d = json.loads(b64d(p))
    except ValueError: return False
    c = _cred_cached()
    return bool(c) and d.get("exp", 0) > time.time() and hmac.compare_digest(d.get("cid", ""), hashlib.sha256(c["cred_id"].encode()).hexdigest()[:32])
