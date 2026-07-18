# Future Quant

Future Quant 是一个面向 Binance / OKX 永续合约 spread 套利的本地量化执行项目。项目当前以 Mac mini + 有线网络 + 全局香港节点为第一版部署环境，先完成跨交易所行情监控、机会识别、Telegram/macOS 提醒和本地 Web 控制台；后续目标是把第一版升级为测试盘全自动执行，并在生产盘做只读影子运行与小额灰度。

> 重要说明：本项目不是“无风险套利”保证器。它做的是跨交易所 spread / basis 机会监控与执行框架，真实风险包括滑点、单边成交、盘口消失、资金费率变化、交易所限仓/只减仓、API 延迟和网络抖动。
>
> 使用前请阅读 [DISCLAIMER.md](DISCLAIMER.md)。本项目不构成投资建议、交易建议、法律建议或财务顾问服务。

## 版本

当前版本：

```text
0.1.0
```

相关文档：

- [CHANGELOG.md](CHANGELOG.md)：变更日志
- [VERSIONING.md](VERSIONING.md)：版本和发布规范
- [CONTRIBUTING.md](CONTRIBUTING.md)：格式、提交和验证规范
- [DISCLAIMER.md](DISCLAIMER.md)：风险免责声明
- [SECURITY.md](SECURITY.md)：密钥、交易权限和上传前安全检查

## 当前状态

已经实现：

- Rust workspace 与本地运行配置
- Binance / OKX 永续合约公共行情扫描
- U 本位永续与币本位永续共同币种自动发现
- symbol 黑名单机制
- spread 计算与可疑超大价差过滤
- 最小净 spread 阈值：`4%`
- 最大滑点预估：`0.5%`
- 默认建议仓位：`100 USDT`
- 偏谨慎盘口深度降档
- funding 风险提示框架
- 交互式 Telegram Bot 推送与按钮回调
- macOS 系统通知，无声音
- 本地 Web 控制台
- 闪电审计：OpenAI 兼容模型审计 Agent
- 全局中文显示配置
- 每个功能模块的详细中文说明
- 日报生成基础结构
- 测试盘/生产盘配置字段

尚未完成：

- Binance/OKX 签名交易客户端
- 测试盘真实下单/撤单/仓位对账
- 双边自动开仓和平仓执行器
- 单边成交补偿逻辑
- 生产盘小额自动灰度

## 项目目标

第一阶段目标：

```text
行情发现
-> spread 计算
-> 风险标记
-> Telegram / macOS / Web 提醒
-> 测试盘全自动执行准备
```

目标环境：

```text
Mac mini
-> 千兆有线网络
-> 全局香港节点
-> Binance / OKX API
-> Telegram Bot
-> 本地 Web 控制台
```

后续实盘路径：

```text
1. Binance testnet + OKX Demo Trading 全自动跑通
2. 生产盘只读影子运行 24-72 小时
3. 生产盘小额自动灰度
4. 再逐步扩大仓位
```

## 目录结构

```text
future-quant/
  Cargo.toml
  Cargo.lock
  README.md
  PROJECT_PREP.md
  TESTNET_ASSESSMENT.md
  config/
    local.example.toml
    local.toml
  crates/
    app/
      src/
        alert/
        exchange/
        config.rs
        main.rs
        model.rs
        report.rs
        state.rs
        strategy.rs
        web.rs
```

主要文件：

- `PROJECT_PREP.md`：项目准备文档和产品/风控决策
- `TESTNET_ASSESSMENT.md`：Binance 测试网与 OKX Demo Trading 评估
- `config/local.example.toml`：可提交的配置模板
- `config/local.toml`：本机运行配置，已被 `.gitignore` 忽略
- `crates/app/src/main.rs`：应用入口和轮询主循环
- `crates/app/src/audit.rs`：闪电审计、模型刷新、提示词和本地配置保存
- `crates/app/src/strategy.rs`：spread 计算、阈值判断、仓位建议
- `crates/app/src/exchange/`：Binance / OKX 公共行情客户端
- `crates/app/src/alert/`：Telegram 与 macOS 通知
- `crates/app/src/web.rs`：本地 Web 控制台

## 运行方式

### 一键启动

已经安装两个可双击入口：

```text
/Users/silas/Desktop/FutureQuant.command
/Users/silas/Desktop/future-quant/FutureQuant.command
```

双击后会自动完成：

```text
cargo build
-> 启动本地服务
-> 打开 Web 控制台
-> 进入交互式终端菜单
```

交互式终端支持：

```text
[s] 查看状态
[m] 打开 5 秒刷新一次的监控面板
[l] 跟踪日志
[o] 重新打开 Web 控制台
[r] 重启服务
[p] 停止服务
[b] 重新构建
[w] 写出当前状态报告
[q] 关闭终端但保留服务运行
[x] 停止服务并退出
```

日志路径：

```text
/Users/silas/Library/Application Support/FutureQuant/logs/future-quant.log
```

PID 文件：

```text
/Users/silas/Library/Application Support/FutureQuant/data/future-quant.pid
```

报告输出：

```text
/Users/silas/Library/Application Support/FutureQuant/reports/
```

本地运行时目录：

```text
/Users/silas/Library/Application Support/FutureQuant/
```

说明：启动器会把当前构建产物和配置复制到该目录，再通过 macOS LaunchAgent 手动启动服务。不会设置开机自启；只有双击 `.command` 或在菜单里选择重启时才会加载。

### 命令行启动

安装 Rust 后，在项目根目录运行：

```bash
cargo run -p future-quant -- --config config/local.toml
```

如果 `config/local.toml` 不存在，程序会回退读取：

```text
config/local.example.toml
```

本地 Web 控制台默认地址：

```text
http://127.0.0.1:8787
```

当前项目不需要开机自动启动。

## 配置说明

运行模式：

```toml
[runtime]
mode = "testnet"
environment = "mac_mini_hk_proxy"
poll_interval_secs = 5
```

`mode` 规划：

```text
paper:
  使用生产行情，模拟订单，不触达交易 API

testnet:
  使用 Binance testnet + OKX Demo Trading，真实发送测试盘订单

prod:
  使用生产行情和生产交易 API，必须显式开启
```

交易安全开关：

```toml
[trading]
enabled = true
allow_real_orders = false
environment = "testnet"
```

含义：

- `enabled = true`：允许交易模块工作
- `allow_real_orders = false`：禁止生产盘真实订单
- `environment = "testnet"`：测试盘环境

生产盘真实订单必须显式设置：

```toml
[trading]
enabled = true
allow_real_orders = true
environment = "prod"
```

## Spread 策略

核心阈值：

```toml
[strategy.spread]
min_gross_spread_bps = 450
max_gross_spread_bps = 3000
min_estimated_net_spread_bps = 400
max_slippage_bps = 50
round_trip_fee_bps = 20
default_notional_usdt = 100
close_spread_bps = 150
```

解释：

- `min_estimated_net_spread_bps = 400`：净 spread 至少 `4%` 才触发机会
- `max_gross_spread_bps = 3000`：超过 `30%` 的价差先视为可疑，过滤掉，避免同名币或合约单位异常
- `close_spread_bps = 150`：spread 回归到 `1.5%` 时触发平仓提醒
- `round_trip_fee_bps = 20`：开平双边手续费估算
- `max_slippage_bps = 50`：最大滑点预估 `0.5%`

方向判断：

```text
贵的一边做空
便宜的一边做多
```

示例：

```text
OKX bid = 1.060
Binance ask = 1.000

方向 = short OKX / long Binance
```

## 仓位降档

第一版默认建议仓位是 `100 USDT`，但会按盘口深度偏谨慎降档：

```toml
[strategy.position_sizing]
target_notional_usdt = 100
full_size_min_depth_usdt = 500
half_size_min_depth_usdt = 300
quarter_size_min_depth_usdt = 150
max_depth_usage_ratio = 0.20
watch_only_below_depth_usdt = 150
```

规则：

```text
最小侧深度 >= 500U：建议 100U
最小侧深度 >= 300U：建议 50U
最小侧深度 >= 150U：建议 25U
最小侧深度 < 150U：只观察，不建议开仓
```

且任何情况下建议仓位不超过最小侧盘口深度的 `20%`。

## 黑名单

黑名单用于排除主流高深度、机会小或不希望监控的币种。

当前第一批：

```toml
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
```

## Telegram Bot

设置 Bot Token：

```bash
export TELEGRAM_BOT_TOKEN="..."
```

在 `config/local.toml` 中设置 chat id：

```toml
[alert.telegram]
enabled = true
chat_id = "..."
bot_token_env = "TELEGRAM_BOT_TOKEN"
```

支持的交互按钮：

```text
立即刷新
自动 5 秒
自动 10 秒
自动 20 秒
停止更新
打开 Web 控制台
静默币种
```

说明：

- `立即刷新`：即时刷新当前机会
- `自动 5 秒 / 10 秒 / 20 秒`：按固定频率推送该机会更新
- `停止更新`：停止定频更新
- `静默币种`：临时静默某个 symbol
- `打开 Web 控制台`：打开本地 Web 控制台

## macOS 通知

配置：

```toml
[alert.macos]
enabled = true
sound = false
```

当前使用 macOS 系统通知，无声音提醒。

## 本地 Web 控制台

默认监听：

```toml
[web]
enabled = true
listen_addr = "127.0.0.1:8787"
```

```toml
[ui]
language = "zh-CN"
module_explanations_enabled = true
```

访问：

```text
http://127.0.0.1:8787
```

控制台展示：

- Binance / OKX 状态
- 共同永续合约数量
- 最近轮询耗时
- 当前活跃机会
- 最近机会记录
- 黑名单
- 静默 symbol
- 风险标记
- 闪电审计
- 功能模块说明

当前 Web 控制台不提供真实下单按钮。

## 闪电审计

闪电审计用于在机会出现后快速调用一个 OpenAI 兼容模型，对该机会做风控复核。

审计目标：

1. 两个交易所代币标的实际是否等价
2. 该代币是否存在恶意操纵、强控盘、暂停交易、充提异常或其他风险
3. 资金费率是否即将结算，或是否存在资金费率异常

配置：

```toml
[lightning_audit]
enabled = true
config_path = "data/lightning-audit-config.json"
openai_compatible_base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
default_model = "gpt-4.1-mini"
default_prompt_id = "strict_token_equivalence"
request_timeout_secs = 45
temperature = 0.1
max_tokens = 1200
show_process_log = true
```

本机密钥放在：

```bash
OPENAI_API_KEY="..."
```

建议写入 `config/env.local`，该文件不会提交到 Git。

Web 控制台支持：

- 刷新模型列表：调用 OpenAI 兼容 `/v1/models`
- 选择模型：保存到本地配置
- 选择提示词模板：默认提供严格标的等价审计和快速风险分诊
- 修改提示词：支持编辑 system prompt 和 user prompt 模板
- 保存配置：写入运行时目录的 `data/lightning-audit-config.json`
- 对活跃机会执行闪电审计

可见过程：

- 机会快照
- 检索入口 URL
- 当前模型和提示词
- 模型输入
- 模型返回
- 可见依据链接

说明：这里展示的是可见审计过程和依据，不要求模型输出隐藏推理链。检索入口会生成给模型和人类使用的搜索 URL；是否能真实联网取决于所选模型服务商是否支持联网能力。

## 中文显示与模块说明

当前默认语言：

```toml
[ui]
language = "zh-CN"
module_explanations_enabled = true
```

已经中文化的展示面：

- 本地 Web 控制台
- Telegram Bot 消息与交互按钮
- macOS 系统通知标题
- 本地 `.command` 交互式终端
- 状态报告和日报

Web 控制台内置功能模块说明，覆盖：

- 行情扫描
- 价差识别
- 仓位建议
- 风险提醒
- 闪电审计
- Telegram 交互
- macOS 通知
- Web 控制台
- 启动与托管

## 测试盘情况

详见：

```text
TESTNET_ASSESSMENT.md
```

核心结论：

- Binance USD-M Futures testnet REST：`https://demo-fapi.binance.com`
- Binance COIN-M Futures testnet REST：`https://testnet.binancefuture.com`
- Binance COIN-M Futures testnet WebSocket：`wss://dstream.binancefuture.com`
- OKX Demo Trading REST：`https://openapi.okx.com`
- OKX Demo Trading REST header：`x-simulated-trading: 1`
- OKX Demo WebSocket：`wss://wspap.okx.com:8443/ws/v5/...`

测试盘可以覆盖：

- API 签名
- 下单/撤单
- 仓位查询
- 账户查询
- 用户数据流
- 双边执行状态机
- 异常补偿

测试盘不能覆盖：

- 真实盘口深度
- 真实滑点
- 真实成交概率
- 真实 funding 成本
- 极端行情下生产 API 拥堵

## 风控

当前风控设计包括：

- 最小净 spread 阈值
- 最大 gross spread 过滤
- 最大滑点估算
- 盘口深度降档
- 单轮最大名义仓位
- 最大未对冲敞口
- funding 异常提醒
- 资金费率周期变化提醒
- 限仓、限购、只减仓提醒
- 暂停交易/划转提醒
- 标记价格异常提醒
- kill switch 文件

kill switch 配置：

```toml
[trading.safety]
kill_switch_file = "data/KILL_SWITCH"
```

后续交易执行模块应在每次下单前检查该文件是否存在。

## 验证命令

格式化：

```bash
cargo fmt --all
```

编译检查：

```bash
cargo check
```

测试：

```bash
cargo test
```

构建：

```bash
cargo build
```

当前已验证：

```text
cargo check 通过
cargo test 通过
cargo build 通过
Web 控制台返回 200
Binance / OKX 公共行情状态正常
```

## 当前限制

当前版本仍然是“行情和提醒已实现，交易执行待实现”的状态。

主要限制：

- 还没有真实签名下单客户端
- 还没有双边自动执行器
- 还没有仓位对账器
- 还没有单边成交补偿
- 还没有生产盘小额灰度逻辑
- REST 轮询版会受网络波动影响，后续应升级为 WebSocket 盘口

## 下一步

建议开发顺序：

1. 增加交易环境抽象：`paper` / `testnet` / `prod`
2. 实现 Binance USD-M testnet 交易客户端
3. 实现 Binance COIN-M testnet 交易客户端
4. 实现 OKX Demo Trading 交易客户端
5. 实现 `ExchangeTrading` trait
6. 实现双边执行器
7. 实现仓位对账
8. 实现单边成交补偿
9. 在测试盘跑全自动开平仓
10. 生产盘只读影子运行
11. 小额生产盘灰度

## 资料来源

- Binance USD-M Futures Docs: https://developers.binance.com/docs/derivatives/usds-margined-futures/general-info
- Binance COIN-M Futures Docs: https://developers.binance.com/docs/derivatives/coin-margined-futures/general-info
- OKX API v5 Docs: https://www.okx.com/docs-v5/en/
