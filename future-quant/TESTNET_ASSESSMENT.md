# Binance / OKX 测试盘评估

## 结论

Binance 与 OKX 都有可用于自动交易开发的测试/模拟交易环境，足够覆盖第一版全自动脚本的大部分工程链路：

- 行情读取
- 合约发现
- API Key 鉴权
- 下单
- 撤单
- 仓位查询
- 账户查询
- 成交/订单状态回报
- WebSocket 私有频道
- 双边执行状态机
- 异常处理演练

但测试盘不能等价覆盖真实盘的交易效果：

- 真实盘口深度
- 真实滑点
- 极端行情下成交概率
- 交易所风控/限仓/只减仓触发概率
- 真实资金费率变化
- 真实 API 拥堵和撮合延迟
- 跨所 spread 的真实出现频率和持续时间

所以第一版如果改成全自动，推荐验收路径是：

```text
1. 测试盘全自动全链路通过
2. 生产盘只读影子运行 24-72 小时
3. 生产盘小额自动灰度
4. 再逐步提高仓位
```

测试盘可以证明“脚本不会乱下单、状态机能跑通、异常能处理”，不能证明“策略一定能在真实盘赚钱”。

## Binance 测试网

### USD-M Futures

官方 USD-M Futures 测试网 REST base URL：

```text
https://demo-fapi.binance.com
```

本机连通性测试：

```text
GET https://demo-fapi.binance.com/fapi/v1/exchangeInfo
HTTP 200
测试网 symbols: 712
```

适配范围：

- U 本位永续合约
- 公共行情
- 账户信息
- 下单/撤单
- 仓位查询
- 用户数据流
- WebSocket 行情

下单接口与生产 USD-M Futures 同路径：

```text
POST /fapi/v1/order
```

### COIN-M Futures

官方 COIN-M Futures 测试网 REST base URL：

```text
https://testnet.binancefuture.com
```

官方 COIN-M Futures 测试网 WebSocket base URL：

```text
wss://dstream.binancefuture.com
```

本机连通性测试：

```text
GET https://testnet.binancefuture.com/dapi/v1/exchangeInfo
HTTP 200
测试网 symbols: 43
```

适配范围：

- 币本位永续合约
- 公共行情
- 账户信息
- 下单/撤单
- 仓位查询
- 用户数据流
- WebSocket 行情

下单接口与生产 COIN-M Futures 同路径：

```text
POST /dapi/v1/order
```

### Binance 测试网限制

Binance 测试网适合测试：

- HMAC 签名
- `timestamp` / `recvWindow`
- order placement
- order cancel
- order query
- position query
- dual-side / hedge mode 参数
- reduce-only / close-position 相关逻辑
- API 错误处理

但需要注意：

- 测试网 symbols 数量与生产环境不完全一致
- 测试网深度和成交行为不代表真实市场
- 测试网资金费率与真实资金费率不可等价使用
- 部分小币种 spread 机会在测试网没有意义
- 测试网流动性可能过低或过假

## OKX Demo Trading

OKX 使用 Demo Trading 机制。官方说明中 Demo Trading REST URL 仍是：

```text
https://openapi.okx.com
```

Demo Trading WebSocket：

```text
Public WebSocket:   wss://wspap.okx.com:8443/ws/v5/public
Private WebSocket:  wss://wspap.okx.com:8443/ws/v5/private
Business WebSocket: wss://wspap.okx.com:8443/ws/v5/business
```

Demo Trading REST 请求需要加 header：

```text
x-simulated-trading: 1
```

本机连通性测试：

```text
GET https://www.okx.com/api/v5/public/instruments?instType=SWAP
Header: x-simulated-trading: 1
HTTP 200
Demo SWAP instruments: 156
```

OKX Demo Trading 支持：

- 公共行情
- 模拟账户
- 模拟下单
- 模拟撤单
- 模拟仓位
- 私有 WebSocket
- Demo API Key
- Demo balance adjustment

OKX 官方同时说明 Demo Trading 不支持部分功能，例如提现、充值、申购/赎回等。

### OKX 测试盘限制

OKX Demo Trading 适合测试：

- `x-simulated-trading: 1`
- REST 签名
- API Key / passphrase 鉴权
- 下单
- 批量下单
- 撤单
- 订单查询
- 仓位查询
- 私有频道登录与回报
- 模拟余额调整

但需要注意：

- Demo Trading 的流动性与真实盘不同
- Demo 成交速度、盘口深度、滑点不代表生产
- Demo instruments 数量可能少于生产
- Demo 中一些风控、限制、结算行为可能与生产不同

## 是否满足全自动脚本的全量测试

结论：基本满足，但要定义“全量测试”为工程链路全量，而不是收益环境全量。

可以覆盖：

- 自动发现测试盘共同合约
- 行情读取
- spread 计算
- 自动开仓
- 自动平仓
- IOC / LIMIT / MARKET 下单策略
- 双边并发下单
- 一边成交、一边失败后的补偿逻辑
- 部分成交处理
- 撤单
- 仓位同步
- 账户余额检查
- API 错误处理
- WebSocket 断线重连
- 私有订单/成交回报
- 本地 kill switch
- Telegram 一键/自动开仓按钮逻辑
- Web 控制台状态展示
- 日志与日报

不能充分覆盖：

- 真实 spread 捕获能力
- 真实成交概率
- 真实滑点
- 真实 funding 成本
- 交易所生产环境限仓/限购策略
- 极端行情下生产 API 拥堵
- 真实资金调度和提现/转账

## 是否满足实战全链路功能

结论：满足“实战功能结构”的演练，不满足“实战市场质量”的验证。

测试盘可以把全自动架构完整跑通：

```text
发现机会
-> 风控预检查
-> 双边下单
-> 订单回报
-> 仓位对账
-> 异常补偿
-> 持仓监控
-> 平仓触发
-> 双边平仓
-> 余额/仓位归零检查
-> 日志/报告
```

但是正式上线前仍必须做生产只读影子运行：

```text
生产行情
-> 真实共同币种
-> 真实盘口深度
-> 真实 spread 频率
-> 真实 funding
-> 真实延迟
-> 仅模拟下单决策，不发真实订单
```

之后再做小额生产自动灰度。

## 第一版改成全自动的建议

第一版可以改成全自动，但必须做成三层环境：

```text
mode = "paper"
  使用生产行情，模拟订单，不触达交易 API

mode = "testnet"
  使用 Binance testnet + OKX demo trading，真实发测试盘订单

mode = "prod"
  使用生产行情和生产交易 API，必须显式开启
```

建议默认配置：

```toml
[runtime]
mode = "testnet"

[trading]
enabled = true
allow_real_orders = false
environment = "testnet"

[risk.execution]
max_notional_usdt = 100
max_unhedged_notional_usdt = 20
max_order_wait_ms = 1500
max_slippage_bps = 50
kill_switch_file = "data/KILL_SWITCH"
```

生产盘必须额外要求：

```toml
[trading]
enabled = true
allow_real_orders = true
environment = "prod"
```

并且程序启动时需要明确打印：

```text
REAL ORDERS ENABLED
ENVIRONMENT: prod
MAX NOTIONAL: ...
```

## 自动执行最小闭环

全自动第一版至少需要这些模块：

- `ExchangeTrading` trait
- Binance USD-M testnet trading client
- Binance COIN-M testnet trading client
- OKX demo trading client
- signed request builder
- account snapshot
- position snapshot
- place order
- cancel order
- query order
- open orders
- position reconciliation
- hedge coordinator
- kill switch
- execution journal

建议下单策略：

```text
1. 发现机会
2. 二次读取两边盘口
3. 检查 spread 仍 >= 4%
4. 检查 suggested_notional > 0
5. 生成 long leg / short leg
6. 双边并发发限价 IOC 或保护限价单
7. 等待成交回报
8. 如果两边都成交，进入持仓监控
9. 如果一边失败，立即撤单/反向平掉已成交腿
10. spread <= close threshold 时自动平仓
11. 平仓后仓位对账
```

## 当前机器连通性观察

本机当前访问测试盘可达，但网络耗时有波动：

```text
Binance USD-M testnet exchangeInfo: HTTP 200, 首次大响应接近 10s 超时边界
Binance COIN-M testnet exchangeInfo: HTTP 200, 约 6s
OKX demo public instruments: HTTP 200, 约 7s
```

这说明 Mac mini + 香港节点可以用于开发和测试盘自动化验证，但如果要上生产全自动，仍建议保留后续迁移到香港/东京/新加坡 VPS 的能力。

## 资料来源

- Binance USD-M Futures General Info: https://developers.binance.com/docs/derivatives/usds-margined-futures/general-info
- Binance COIN-M Futures General Info: https://developers.binance.com/docs/derivatives/coin-margined-futures/general-info
- Binance USD-M New Order: https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/New-Order
- OKX API v5 Docs: https://www.okx.com/docs-v5/en/
