"""单 Passkey 登录测试矩阵：用 cryptography 模拟认证器。
运行：RWALP_MEMSTORE=1 PYTHONPATH=vendor/rwa-lp/service:. uv run --python 3.12 --with-requirements vendor/rwa-lp/service/requirements.txt --with cryptography --with httpx --with pytest pytest -q tests
"""
import os, json, hashlib, struct, base64
os.environ.update(RWALP_MEMSTORE="1", RP_ID="lp.silasxbt.com", SETUP_CODE="setup-code-for-tests", SESSION_SECRET="s" * 48)
import pytest
from fastapi.testclient import TestClient
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from private import main, store, passkey

ORIGIN = "https://lp.silasxbt.com"
b64e = passkey.b64e

def cbor_enc(v):
    def head(mt, n):
        if n < 24: return bytes([mt << 5 | n])
        if n < 256: return bytes([mt << 5 | 24, n])
        if n < 65536: return bytes([mt << 5 | 25]) + struct.pack(">H", n)
        return bytes([mt << 5 | 26]) + struct.pack(">I", n)
    if isinstance(v, bool): return bytes([0xf5 if v else 0xf4])
    if isinstance(v, int): return head(0, v) if v >= 0 else head(1, -1 - v)
    if isinstance(v, bytes): return head(2, len(v)) + v
    if isinstance(v, str): b = v.encode(); return head(3, len(b)) + b
    if isinstance(v, dict): return head(5, len(v)) + b"".join(cbor_enc(k) + cbor_enc(x) for k, x in v.items())
    raise TypeError(v)

class Authenticator:
    def __init__(self, cid=None):
        self.key = ec.generate_private_key(ec.SECP256R1()); self.cid = cid or os.urandom(16); self.count = 0
    def cose(self):
        n = self.key.public_key().public_numbers()
        return cbor_enc({1: 2, 3: -7, -1: 1, -2: n.x.to_bytes(32, "big"), -3: n.y.to_bytes(32, "big")})
    def auth_data(self, flags, at=False, rp="lp.silasxbt.com"):
        ad = hashlib.sha256(rp.encode()).digest() + bytes([flags]) + struct.pack(">I", self.count)
        if at: ad += b"\0" * 16 + struct.pack(">H", len(self.cid)) + self.cid + self.cose()
        return ad
    def create(self, challenge, origin=ORIGIN, flags=0x45):
        cd = json.dumps({"type": "webauthn.create", "challenge": challenge, "origin": origin}).encode()
        att = cbor_enc({"fmt": "none", "attStmt": {}, "authData": self.auth_data(flags, at=True)})
        return {"id": b64e(self.cid), "rawId": b64e(self.cid), "type": "public-key",
                "response": {"clientDataJSON": b64e(cd), "attestationObject": b64e(att)}}
    def get(self, challenge, origin=ORIGIN, flags=0x05, cid=None):
        cd = json.dumps({"type": "webauthn.get", "challenge": challenge, "origin": origin}).encode()
        ad = self.auth_data(flags)
        sig = self.key.sign(ad + hashlib.sha256(cd).digest(), ec.ECDSA(hashes.SHA256()))
        return {"id": b64e(cid or self.cid), "rawId": b64e(cid or self.cid), "type": "public-key",
                "response": {"clientDataJSON": b64e(cd), "authenticatorData": b64e(ad), "signature": b64e(sig), "userHandle": None}}

CODE = os.environ["SETUP_CODE"]

@pytest.fixture()
def c():
    store._mem.clear(); passkey._cc.update(t=0, v=None)
    return TestClient(main.app, base_url=ORIGIN)

def reg(c, a, **kw):
    ch = c.post("/auth/register/options", json={"code": CODE}).json()["challenge"]
    return c.post("/auth/register", json={"code": CODE, **a.create(ch, **kw)})

def login(c, a, **kw):
    ch = c.post("/auth/login/options").json()["challenge"]
    return c.post("/auth/login", json=a.get(ch, **kw))

def fresh(c):
    passkey._cc.update(t=0, v=None); c.cookies.clear()

def test_uninitialized_shows_setup(c):
    r = c.get("/"); assert r.status_code == 200 and 'const MODE = "setup"' in r.text and "no-store" in r.headers["cache-control"]

def test_wrong_setup_code(c):
    assert c.post("/auth/register/options", json={"code": "nope"}).status_code == 403
    ch = c.post("/auth/register/options", json={"code": CODE}).json()["challenge"]
    assert c.post("/auth/register", json={"code": "nope", **Authenticator().create(ch)}).status_code == 403

def test_wrong_origin_register(c):
    assert reg(c, Authenticator(), origin="https://evil.example").status_code == 400

def test_register_ok_and_locked(c):
    r = reg(c, Authenticator())
    assert r.status_code == 200
    ck = r.headers["set-cookie"].lower()
    assert "httponly" in ck and "secure" in ck and "samesite=strict" in ck
    assert c.post("/auth/register/options", json={"code": CODE}).status_code == 403
    assert c.get("/setup").status_code == 403
    assert c.post("/auth/register", json={"code": CODE, **Authenticator().create(b64e(os.urandom(32)))}).status_code == 403

def test_logged_out_and_forged_cookie(c):
    reg(c, Authenticator()); fresh(c)
    assert 'const MODE = "login"' in c.get("/").text
    assert c.get("/p/positions").status_code == 401
    c.cookies.set(passkey.COOKIE, "eyJ4IjoxfQ.deadbeef", domain="lp.silasxbt.com")
    assert 'const MODE = "login"' in c.get("/").text and c.get("/p/positions").status_code == 401

def test_login_matrix(c):
    me = Authenticator(); reg(c, me); fresh(c)
    thief = Authenticator(cid=me.cid)
    assert login(c, thief).status_code == 400                       # 其他密钥冒用 credId
    assert login(c, Authenticator()).status_code == 403              # 其他 Passkey
    assert login(c, me, flags=0x01).status_code == 400               # 只有 UP 没有 UV
    assert login(c, me, origin="https://evil.example").status_code == 400
    assert c.post("/auth/login", json=me.get(b64e(os.urandom(32)))).status_code == 400   # 服务端没发过的 challenge
    ch = c.post("/auth/login/options").json()["challenge"]; resp = me.get(ch)
    r = c.post("/auth/login", json=resp); assert r.status_code == 200
    assert c.get("/p/positions").status_code == 200
    home = c.get("/").text; assert "const MODE" not in home and "我的预设" in home            # 已登录看到的是主站
    fresh(c); assert c.post("/auth/login", json=resp).status_code == 400            # 重放

def test_counter(c):
    me = Authenticator(); reg(c, me); fresh(c)
    me.count = 5; assert login(c, me).status_code == 200
    me.count = 6; assert login(c, me).status_code == 200
    me.count = 3; assert login(c, me).status_code == 400

def test_reset_revokes_sessions(c):
    me = Authenticator(); r = reg(c, me)
    assert c.get("/p/positions").status_code == 200
    store._mem.pop("auth_cred"); passkey._cc.update(t=0, v=None)   # 服务器侧重置
    assert c.get("/p/positions").status_code == 401
    assert 'const MODE = "setup"' in c.get("/").text

def test_health_public(c):
    assert c.get("/health").status_code == 200

def test_concurrent_register_only_once(c):
    import threading
    a, b = Authenticator(), Authenticator()
    cha = c.post("/auth/register/options", json={"code": CODE}).json()["challenge"]
    chb = c.post("/auth/register/options", json={"code": CODE}).json()["challenge"]
    res = []
    ts = [threading.Thread(target=lambda x, ch: res.append(c.post("/auth/register", json={"code": CODE, **x.create(ch)}).status_code), args=(x, ch)) for x, ch in ((a, cha), (b, chb))]
    [t.start() for t in ts]; [t.join() for t in ts]
    assert sorted(res) == [200, 403]
