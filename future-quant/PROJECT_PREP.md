# Future Quant 项目准备文档

## 1. 项目目标

本项目先围绕交易所间 spread 套利机会做行情监控、提醒和后续半自动/自动执行能力。

当前第一版部署假设：

- 运行机器：Mac mini
- 网络：千兆有线网络
- 出口：全局香港节点
- 初始重点：Binance 与 OKX 之间的永续合约 spread 监控
- 交易范围：Binance 与 OKX 的 U 本位永续、币本位永续
- 币种范围：自动扫描两个交易所共同上架的永续合约币种
- 排除机制：支持 symbol 黑名单
- 报警渠道：交互式 Telegram Bot
- 默认建议仓位：100 USDT
- 报警阈值：最小净 spread 4%
- 本地界面：Mac mini 本地 Web 控制台
- 初始模式：改为第一版全自动，但默认先在 Binance testnet + OKX Demo Trading 中验证；生产盘需要显式开启

项目核心目标不是做一个通用量化平台，而是做一个足够稳定、可观测、可扩展的套利执行工具。

## 2. 当前部署边界

第一阶段明确适配：

```text
Mac mini
-> 有线网络
-> 全局香港节点
-> Binance / OKX API
-> Telegram 或其他提醒渠道
```

这个部署方式适合：

- spread 机会监控
- 盘口深度观察
- Telegram/飞书/企业微信提醒
- 人工确认或半自动按钮执行
- 小仓位实验
- 策略参数验证

这个部署方式暂不适合作为最终低延迟自动交易主节点，原因是：

- 大陆到香港节点再到交易所 API 存在额外链路
- 延迟和抖动不可完全控制
- 极端行情下，代理、DNS、TLS、WebSocket 都可能出现不稳定
- 双边交易中，一边成交另一边失败的风险会被网络抖动放大

因此第一阶段要把它设计成“可升级架构”：Mac mini 先跑起来，后续可以迁移到香港/东京/新加坡 VPS。

## 3. 第一阶段范围

第一阶段改为全自动测试盘优先，不直接默认开启生产盘真实交易。

必须包含：

- Binance / OKX 行情连接
- U 本位永续与币本位永续 symbol 映射
- Binance / OKX 共同永续合约自动扫描
- symbol 黑名单过滤
- best bid / best ask 实时维护
- 基于可成交价格计算 spread
- 手续费、滑点保护后的净 spread 估算
- 盘口深度检查
- 交互式 Telegram Bot 报警
- macOS 系统通知提醒
- Mac mini 本地 Web 控制台
- Binance testnet 交易接口
- OKX Demo Trading 交易接口
- 自动开仓、自动平仓、仓位对账
- kill switch
- 断线重连
- 延迟统计
- 日志记录
- 配置文件管理

默认暂不包含：

- 生产盘自动大仓位开仓
- 生产盘自动大仓位平仓
- 第一版默认不接入生产盘交易私钥
- 多策略回测系统
- 网格、趋势、做市等非 spread 策略
- 复杂前端后台

## 4. 推荐技术栈

主程序建议使用 Rust。

原因：

- 长时间运行稳定性更好
- 异步 WebSocket 并发更适合
- 类型系统能降低交易逻辑错误
- 部署时可以是单个二进制文件
- 后续迁移 VPS 更方便

辅助工具可以使用 Python：

- 数据分析
- 参数回测
- 历史日志分析
- 报表生成
- 临时诊断脚本

推荐技术组件：

```text
Rust async runtime: tokio
WebSocket: tokio-tungstenite
HTTP: reqwest
JSON: serde
日志: tracing
配置: toml + serde
错误处理: anyhow + thiserror
精确数值: rust_decimal 或整数 tick/lot
本地存储: append-only log / SQLite
```

## 5. 第一版模块设计

建议先按轻脚手架组织，不上重型框架。

```text
future-quant/
  PROJECT_PREP.md
  README.md
  config/
    local.example.toml
  crates/
    app/
    core/
    exchange/
    strategy/
    alert/
    web/
    storage/
```

模块职责：

```text
app:
  启动入口，加载配置，组合各模块

core:
  Symbol、ContractType、Price、Quantity、OrderBook、Spread、Fee 等核心类型

exchange:
  交易所通用 trait

okx / binance:
  交易所行情与账户接口实现

strategy:
  spread 计算、机会识别、阈值判断

alert:
  交互式 Telegram Bot 提醒、手动刷新、定频更新、macOS 系统通知

web:
  Mac mini 本地 Web 控制台

storage:
  日志、行情快照、机会记录、延迟统计
```

## 6. Spread 计算原则

不能用最新价计算机会，必须用盘口可成交价格。

例如 OKX 合约相对 Binance 合约溢价：

```text
OKX bid = 1.060
Binance ask = 1.000

open_spread = OKX bid / Binance ask - 1
方向 = short OKX perp + long Binance perp
```

第一版只处理永续合约之间的 spread：

```text
USDT-margined perp vs USDT-margined perp
Coin-margined perp vs Coin-margined perp
```

原则上先不跨保证金币种比较，例如不直接比较 USDT 本位永续与币本位永续，除非后续明确加入统一换算逻辑。

所有机会都需要扣除：

- 开仓手续费
- 平仓手续费
- 预估滑点
- funding 影响
- 盘口深度不足带来的折价

报警时要展示：

```text
币种
机会类型
交易方向
当前 spread
扣费后 spread
两边 best bid / best ask
目标仓位
盘口深度
建议最大仓位
建议开仓条件
建议平仓条件
```

## 7. 报警样例

```text
[SPREAD ALERT]

Symbol: RAVE
Type: perp-perp

Direction:
Short OKX RAVE-USDT-SWAP
Long Binance RAVEUSDT

Prices:
OKX bid: 1.0600
Binance ask: 1.0000

Spread:
Gross: +6.00%
Estimated net: +5.35%

Depth:
OKX bid depth within 0.5%: 3,200 USDT
Binance ask depth within 0.5%: 4,800 USDT

Suggested notional:
100 USDT

Risk:
Max slippage: 0.50%
Stop alert if spread > 9.00%
Close alert if spread < 1.50%
```

## 8. 交互式 Telegram Bot 设计

第一版 Telegram Bot 不只做单向推送，而是围绕每个机会创建一个可交互消息。

触发规则：

```text
Estimated net spread >= 4.00%
```

首次推送包含：

- symbol
- 合约类型：U 本位永续或币本位永续
- 方向：short 哪个交易所，long 哪个交易所
- gross spread
- estimated net spread
- 两边 best bid / best ask
- 两边盘口深度
- 建议仓位：默认 100 USDT
- funding 信息
- 数据延迟
- 本地 Web 控制台链接

按钮设计：

```text
[Refresh Now]
[Auto 5s]
[Auto 10s]
[Auto 20s]
[Stop Updates]
[Open Web]
[Mute Symbol]
```

交互规则：

- 首次触发时立即推送一次
- 用户点击 `Refresh Now` 时，机器人即时刷新该机会
- 用户点击 `Auto 5s` / `Auto 10s` / `Auto 20s` 后，机器人按对应频率更新同一机会
- 用户点击 `Stop Updates` 后停止该机会的定频更新
- 用户点击 `Mute Symbol` 后将该 symbol 加入临时静默列表，避免短时间重复打扰
- 如果机会已经失效，刷新消息时明确标记为 `EXPIRED`

平仓提醒：

- 当一个已推送机会的 spread 回归到配置的 close threshold 时，推送一次平仓提醒
- 同一个机会只推送一次平仓提醒
- 平仓提醒不自动重复，除非用户手动刷新

## 9. 本地 Web 控制台

Mac mini 第一版需要提供本地 Web 控制台，作为 Telegram 的补充。

第一版 Web 控制台包含：

- 当前活跃机会列表
- 每个机会的 spread、方向、深度、更新时间
- Binance / OKX WebSocket 连接状态
- REST /time 延迟
- WebSocket 数据延迟
- 黑名单查看与编辑
- Telegram Bot 状态
- 最近报警记录
- 最近错误日志
- 当前配置只读展示

第一版 Web 控制台不包含：

- 真实下单按钮
- API Key 输入
- 账户余额展示
- 仓位展示

建议默认监听：

```text
127.0.0.1:8787
```

如果后续需要从局域网访问，可以显式配置监听地址。

## 10. 人工执行流程

第一阶段收到报警后，由人执行：

1. 打开两个交易所页面
2. 确认机器人显示的 spread 仍然存在
3. 确认盘口深度能覆盖计划仓位
4. 确认方向：贵的一边做空，便宜的一边做多
5. 使用低杠杆开双边仓
6. 保持仓位数量尽量一致
7. 等待机器人提醒 spread 回归
8. 同步平双边仓
9. 核对实际利润、手续费和滑点

## 11. 后续半自动执行流程

第二阶段可以加入 Telegram 按钮或本地控制台按钮。

按钮示例：

```text
[Open 100U]
[Open 300U]
[Ignore]
[Close Position]
[Emergency Close]
```

点击开仓后，程序必须重新检查：

- 当前 spread 是否仍然超过阈值
- 盘口深度是否仍然足够
- 两边账户余额是否足够
- 当前是否已有同 symbol 仓位
- 下单价格是否超过滑点限制

然后才允许双边并发下单。

## 12. 风控要求

第一阶段即使只做提醒，也要提前设计风控字段。

基础风控：

- symbol 黑名单，第一批偏向排除主流高深度、低机会币
- 单币最大建议仓位
- 单轮最大名义仓位
- 最大允许滑点
- 按盘口深度偏谨慎地自动降低建议仓位
- 最小净 spread
- 最大 spread 扩大提醒
- 最长持仓时间提醒
- funding 异常提醒
- 资金结算费率及周期变动提醒
- 恶意限购、限仓、只减仓等交易限制提醒
- 暂停交易、暂停划转、指数/标记价格异常等其他异常提醒
- WebSocket 延迟过高提醒
- 交易所断线提醒

后续自动执行风控：

- 一边成交另一边失败后的自动补偿
- 部分成交处理
- 撤单失败处理
- 仓位不一致报警
- 账户余额不足报警
- API Key 权限异常报警
- 交易所只减仓/暂停交易报警

## 13. 仓位降档与异常风险提醒

第一版建议仓位默认为 100 USDT，但报警时需要根据盘口深度偏谨慎地自动降低建议仓位。

建议规则：

```text
target_notional = 100 USDT
depth_window = 0.5%
min_side_depth = min(expensive_side_depth, cheap_side_depth)

if min_side_depth >= 500 USDT:
  suggested_notional = 100 USDT
elif min_side_depth >= 300 USDT:
  suggested_notional = 50 USDT
elif min_side_depth >= 150 USDT:
  suggested_notional = 25 USDT
else:
  suggested_notional = 0 USDT
  alert_level = "WATCH_ONLY"
```

额外保守折扣：

```text
suggested_notional <= min_side_depth * 0.20
```

也就是说，即使盘口显示 500 USDT 深度，也最多建议 100 USDT；盘口只有 300 USDT 深度时，最多建议 50 USDT；盘口低于 150 USDT 时只提醒观察，不建议开仓。

第一版 funding 异常时不直接禁止提醒，而是在 Telegram 和 Web 控制台中标记风险。

需要提醒的 funding/结算风险：

- funding rate 明显异常，例如绝对值超过配置阈值
- 两个交易所 funding rate 方向相反或差异过大
- funding 结算周期从常规周期调整为更短或不规则周期
- 距离下一次 funding 结算时间过近
- 交易所公告调整资金费率上限、下限或计算方式

需要提醒的交易限制风险：

- 限购、限卖、限仓、只减仓
- 新增下单数量上限或名义价值上限
- 杠杆上限临时降低
- 保证金率、维持保证金率临时调整
- 指数价格或标记价格异常
- 暂停开仓、暂停平仓、暂停交易
- 暂停充值、提现、划转，导致后续资金调度受限
- API 返回交易状态未知、订单状态延迟、交易所系统维护

Telegram 风险展示示例：

```text
Risk flags:
- Funding spread abnormal: OKX +0.80%, Binance -0.15%
- Funding settlement in 12 min
- Binance order limit updated

Suggested action:
Watch only or reduce size
```

## 14. 网络与延迟监控

Mac mini + 香港节点必须持续记录这些指标：

```text
REST /time latency
WebSocket event time -> local receive time
WebSocket reconnect count
DNS resolution time
TLS handshake time
order book stale duration
Telegram push latency
```

第一阶段建议先运行 24-72 小时，只看监控质量：

- 有没有断线
- 有没有漏推
- 延迟是否稳定
- 高波动时数据是否明显滞后
- Telegram 推送是否及时
- 机会是否有足够持续时间给人执行

## 15. 配置草案

示例配置：

```toml
[runtime]
mode = "monitor"
environment = "mac_mini_hk_proxy"

[network]
proxy_mode = "system_global"
latency_warn_ms = 500
latency_critical_ms = 1000

[exchanges.binance]
enabled = true
market_types = ["usdm_perp", "coinm_perp"]
auto_discover = true

[exchanges.okx]
enabled = true
market_types = ["usdt_swap", "coin_swap"]
auto_discover = true

[symbols]
mode = "auto_common"
blacklist = [
  "BTC",
  "ETH",
  "BNB",
  "SOL",
  "XRP",
  "DOGE",
  "ADA",
  "AVAX",
  "LINK",
  "TRX",
  "LTC",
  "BCH",
  "DOT",
  "MATIC",
]

[strategy.spread]
enabled = true
min_gross_spread_bps = 450
max_gross_spread_bps = 3000
min_estimated_net_spread_bps = 400
max_slippage_bps = 50
default_notional_usdt = 100
close_spread_bps = 150
close_alert_once = true

[strategy.position_sizing]
enabled = true
depth_window_bps = 50
target_notional_usdt = 100
full_size_min_depth_usdt = 500
half_size_min_depth_usdt = 300
quarter_size_min_depth_usdt = 150
max_depth_usage_ratio = 0.20
watch_only_below_depth_usdt = 150

[risk.funding]
warn_only = true
abnormal_abs_rate_bps = 50
abnormal_diff_bps = 50
near_settlement_minutes = 30
warn_on_period_change = true
warn_on_cap_floor_change = true

[risk.exchange_restrictions]
warn_on_order_limit_change = true
warn_on_position_limit_change = true
warn_on_reduce_only = true
warn_on_open_disabled = true
warn_on_trade_suspended = true
warn_on_transfer_suspended = true
warn_on_margin_rate_change = true
warn_on_mark_price_abnormal = true

[storage]
log_retention_days = 30
daily_report_enabled = true

[alert.telegram]
enabled = true
chat_id = ""
bot_token_env = "TELEGRAM_BOT_TOKEN"
interactive = true
manual_refresh_enabled = true
auto_update_intervals_sec = [5, 10, 20]
close_alert_once = true

[alert.macos]
enabled = true
sound = false

[web]
enabled = true
listen_addr = "127.0.0.1:8787"

[system]
auto_start_on_boot = false
```

## 16. 交易权限预留接口

第一版只做行情和提醒，不默认接入交易权限。但工程结构需要提前给交易执行留下清晰接口，避免后续重写。

后续交易权限需要的位置：

```text
exchange:
  ExchangeMarketData trait
  ExchangeTrading trait

binance:
  BinanceMarketDataClient
  BinanceTradingClient

okx:
  OkxMarketDataClient
  OkxTradingClient

execution:
  ExecutionEngine
  OrderPlanner
  HedgeCoordinator
  PositionReconciler

risk:
  PreTradeRiskCheck
  PostTradeRiskCheck
  EmergencyStop
```

交易接口草案：

```rust
trait ExchangeTrading {
    async fn account_snapshot(&self) -> Result<AccountSnapshot>;
    async fn positions(&self) -> Result<Vec<Position>>;
    async fn place_order(&self, order: NewOrder) -> Result<OrderAck>;
    async fn cancel_order(&self, order_id: OrderId) -> Result<CancelAck>;
    async fn open_orders(&self, symbol: &ExchangeSymbol) -> Result<Vec<Order>>;
}
```

双边执行接口草案：

```rust
struct SpreadOpenRequest {
    symbol: UnifiedSymbol,
    long_leg: TradeLeg,
    short_leg: TradeLeg,
    notional_usdt: Decimal,
    max_slippage_bps: i32,
}

struct SpreadCloseRequest {
    position_id: PositionId,
    max_slippage_bps: i32,
}
```

未来接交易权限时，必须加入：

- API Key 环境变量读取
- API Key 权限检查
- IP 白名单检查
- 只读/交易权限分离
- 下单前二次行情校验
- 双边并发下单
- 部分成交对冲
- 仓位对账
- 紧急平仓
- 本地 kill switch

第一版配置里可以只保留占位：

```toml
[trading]
enabled = false
allow_real_orders = false
planned_for_phase_2 = true
one_click_open_enabled = true
require_second_confirmation = false

[trading.binance]
api_key_env = "BINANCE_API_KEY"
api_secret_env = "BINANCE_API_SECRET"

[trading.okx]
api_key_env = "OKX_API_KEY"
api_secret_env = "OKX_API_SECRET"
passphrase_env = "OKX_API_PASSPHRASE"
```

第二版执行决策：

- 做 Telegram 一键开仓
- 一键开仓不做二次确认
- 允许程序从环境变量读取 API Key
- API Key 不写入配置文件，不写入日志
- 真实下单功能必须由 `trading.enabled = true` 和 `allow_real_orders = true` 同时打开

## 17. 已确认决策

- 第一版只做 Binance + OKX
- 第一版只做永续合约，不做现货
- 永续范围包括 U 本位永续和币本位永续
- 自动扫描 Binance 与 OKX 共同上架的永续合约币种
- 支持黑名单机制
- 第一批黑名单接受：BTC、ETH、BNB、SOL、XRP、DOGE、ADA、AVAX、LINK、TRX、LTC、BCH、DOT、MATIC
- 报警渠道先用交互式 Telegram Bot
- 不需要声音提醒
- 需要 macOS 系统通知提醒
- 默认建议仓位按 100 USDT 计算
- 最小净 spread 报警阈值为 4%
- 最大可接受滑点为 0.5%
- 按盘口深度偏谨慎地自动降低建议仓位
- funding 异常时继续提醒机会，但必须标记相应风险
- 资金结算费率及周期调整需要风险提醒
- 恶意限购、限仓、只减仓等交易限制需要风险提醒
- 暂停交易、划转受限、标记价格异常等其他异常需要风险提醒
- 首次触发后支持 Telegram 手动刷新
- 支持 Telegram 定频更新：5 秒、10 秒、20 秒
- spread 回归时推送一次平仓提醒
- 第一版需要 Mac mini 本地 Web 控制台
- Mac mini 不需要开机自动启动
- 日志保留 30 天
- 允许生成每日机会统计报告
- 第二版做 Telegram 一键开仓
- 一键开仓不做二次确认
- 后续允许程序读取 API Key 环境变量
- 第一版先做行情和提醒
- 第一版文档和代码中预留未来交易权限需要的位置和接口

## 18. 当前无阻塞决策

当前第一版工程骨架已经可以开始搭建。后续可以在实现过程中继续微调：

- 黑名单增删
- 每日统计报告格式
- Web 控制台展示字段
- 第二版一键开仓按钮文案
- 第二版真实下单开关位置

## 19. 建议下一步

建议按这个顺序推进：

1. 初始化 Rust workspace
2. 写配置与日志基础设施
3. 接 Binance 与 OKX 公共行情
4. 实现共同永续合约扫描、symbol 映射、黑名单过滤
5. 接 Telegram 报警
6. 接 macOS 系统通知
7. 搭建本地 Web 控制台
8. 连续跑 24-72 小时收集延迟和机会数据
9. 根据实际数据决定是否进入一键执行阶段
