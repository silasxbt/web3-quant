"""Binance USDⓈ-M 合约签名下单客户端 — 仅 s6 实盘模式使用。
默认 testnet；无新依赖(requests)。所有方法失败返回 (status, body)，status!=200 即失败。
"""
import hashlib
import hmac
import math
import time
from urllib.parse import urlencode

import requests

MAINNET = "https://fapi.binance.com"
TESTNET = "https://testnet.binancefuture.com"


def _decimals(step_str: str) -> int:
    s = step_str.rstrip("0")
    return len(s.split(".")[1]) if "." in s else 0


class BinanceFutures:
    def __init__(self, key: str, secret: str, testnet: bool = True):
        self.key, self.secret = key, secret
        self.base = TESTNET if testnet else MAINNET
        self._filters = {}

    def _signed(self, method: str, path: str, params: dict = None):
        p = dict(params or {})
        p["timestamp"] = int(time.time() * 1000)
        p["recvWindow"] = 5000
        qs = urlencode(p)
        sig = hmac.new(self.secret.encode(), qs.encode(), hashlib.sha256).hexdigest()
        try:
            r = requests.request(
                method, f"{self.base}{path}?{qs}&signature={sig}",
                headers={"X-MBX-APIKEY": self.key}, timeout=10,
            )
            return r.status_code, (r.json() if r.text else {})
        except Exception as e:
            return 0, {"error": str(e)}

    def _filt(self, symbol):
        if not self._filters:
            try:
                info = requests.get(f"{self.base}/fapi/v1/exchangeInfo", timeout=10).json()
                for s in info.get("symbols", []):
                    step, tick = "1", "0.00000001"
                    for f in s.get("filters", []):
                        if f["filterType"] == "LOT_SIZE":
                            step = f["stepSize"]
                        elif f["filterType"] == "PRICE_FILTER":
                            tick = f["tickSize"]
                    self._filters[s["symbol"]] = (step, tick)
            except Exception:
                pass
        return self._filters.get(symbol, ("1", "0.00000001"))

    def round_qty(self, symbol, qty):
        step = self._filt(symbol)[0]
        return round(math.floor(qty / float(step)) * float(step), _decimals(step))

    def round_price(self, symbol, price):
        tick = self._filt(symbol)[1]
        return round(math.floor(price / float(tick)) * float(tick), _decimals(tick))

    def available_usdt(self):
        st, data = self._signed("GET", "/fapi/v2/balance")
        if st == 200 and isinstance(data, list):
            for b in data:
                if b.get("asset") == "USDT":
                    return float(b.get("availableBalance", 0))
        return None

    def set_leverage(self, symbol, lev):
        return self._signed("POST", "/fapi/v1/leverage", {"symbol": symbol, "leverage": lev})

    def market(self, symbol, side, qty):
        return self._signed("POST", "/fapi/v1/order",
                            {"symbol": symbol, "side": side, "type": "MARKET", "quantity": qty})

    def stop_close(self, symbol, side, kind, stop_price):
        """kind: STOP_MARKET(止损) / TAKE_PROFIT_MARKET(止盈)；closePosition 平整仓(交易所侧兜底)"""
        return self._signed("POST", "/fapi/v1/order",
                            {"symbol": symbol, "side": side, "type": kind,
                             "stopPrice": stop_price, "closePosition": "true",
                             "workingType": "MARK_PRICE"})

    def position_amt(self, symbol):
        st, data = self._signed("GET", "/fapi/v2/positionRisk", {"symbol": symbol})
        if st == 200 and isinstance(data, list) and data:
            try:
                return float(data[0].get("positionAmt", 0))
            except Exception:
                return 0.0
        return 0.0

    def open_order_ids(self, symbol):
        st, data = self._signed("GET", "/fapi/v1/openOrders", {"symbol": symbol})
        return {str(o.get("orderId")) for o in data} if st == 200 and isinstance(data, list) else set()

    def cancel_all(self, symbol):
        return self._signed("DELETE", "/fapi/v1/allOpenOrders", {"symbol": symbol})
