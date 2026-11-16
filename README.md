# web3-quant — 加密信号扫描器套件 / Crypto Signal Scanner Suite

一组独立运行的市场/链上信号扫描器，命中条件即推送 Telegram。全部走公开 API，零成本、无需付费 Key（s1 可选接 LLM）。

同一仓库下还包含：

- `future-quant/`：跨交易所永续合约价差监控与闪电审计项目。
- `dda/`：BSC / Solana DEX 跨链套利与交易所—DEX 偏离研究系统（Node.js / TypeScript，只读采集、回放与模拟），见 [dda/README.md](dda/README.md)。克隆时加 `--recurse-submodules` 以获取其参考仓库。
- `rwa-lp/`：BSC 上 bStocks 代币化美股 V3 LP 做市研究、风控监控与在线服务（rwalp.silasxbt.com，GCP Cloud Run），见 [rwa-lp/README.md](rwa-lp/README.md)。
- `rwa-lp-private/`：rwa-lp 的 Passkey 保护私有站（lp.silasxbt.com）：分析预设、NFT 仓位台账、全周期收益与归因，见 [rwa-lp-private/README.md](rwa-lp-private/README.md)。

这些项目统一归在 `web3-quant` 仓库里。

> ⚠️ 仅供研究学习，非投资建议。涉及高风险/高方差的加密市场，自负盈亏。
> 全部脚本已逐个审查并修复加固（详见各脚本说明与「已知限制」）。

## 脚本一览

| 文件 | 名称 | 监控/动作 | 数据源 | 运行模式 | 依赖 |
|---|---|---|---|---|---|
| `oi_funding_scanner.py` | 费率转负+OI涨 | 费率由正转负 + OI 放大 | 币安合约 API | 定时(cron, ~5min) | requests |
| `s1_binance_alpha_monitor.py` | Binance Alpha 监控 | Alpha 上新公告→评级→上线跟踪 | 币安公告/聚合 + CoinGecko | 常驻(async) | httpx, sqlite3, (可选 Anthropic) |
| `s3_accumulation_radar.py` | 热度做多雷达 | 热度 + 负费率 + OI 暴涨 三维共振 | 币安合约 + CoinGecko + 币安广场 | 定时(cron) | requests |
| `s5_on_chain_narrative_radar.py` | 叙事/链上雷达 | 叙事分类 + 动量(连涨)新币 | **DexScreener** + pump.fun | 常驻(30s 循环) | requests, sqlite3 |
| `s6_futures_alpha_autonomous_trading_v1.py` | 期货自动交易(模拟盘) | 费率/暴涨暴跌信号→开仓→止盈止损 | 币安合约 + Fear&Greed | 定时(cron) | requests |
| `s7_vitalik_sell_radar.py` | Vitalik 抛售雷达 | WebSocket 实时监听 Vitalik 卖出 | 以太坊 WS RPC + DexScreener | 常驻(WebSocket) | aiohttp, websockets |

「定时」= 跑一次即退出，用 cron 周期触发；「常驻」= 进程持续运行，用 `nohup`/`systemd` 守护。

## 各脚本说明

### `oi_funding_scanner.py` — 费率转负 + OI 涨
扫全市场永续，检测费率刚从正转负（空头涌入）+ OI 持续放大（建仓）。快照跨运行对比 + 24h 去重。
- 门槛：OI 总涨幅 ≥ 8%、转负前 ≥ 2 个结算周期为正、24h 成交额 ≥ 5M。
- 加固：核心请求重试退避、状态文件原子写、去重容错、fail-closed 快照。

### `s1_binance_alpha_monitor.py` — Binance Alpha 监控
异步三任务：公告监听(30s) → 数据聚合+规则评级(15s) → 上线后跟踪(120s)。S/A/B/C 评级，可选接 Anthropic 抽取叙事（未配置则规则降级）。SQLite 持久化 `data/alpha.db`。
- 加固：修复时区 bug；从公告正文解析**真实挂牌时间**（`(UTC)`/`(UTC+8)`），解析不到则跳过监控不误报；上线后用**币安实时价**取价（CoinGecko 对新币常无价）。

### `s3_accumulation_radar.py` — 热度做多雷达
"热度先行 + 负费率燃料 + OI 暴涨"三维共振做多。热度三源：CoinGecko Trending + 成交量放量(≥2.5x) + **币安广场讨论增长**(`queryByHashtag` 采样 Top 候选币 + 历史基线判 rapidRiser)。
- 加固：修复原缺失 `square_heat` 依赖致崩；接入 `MIN_OI_DELTA_PCT`/`MIN_OI_USD` 配置；历史原子写。单轮约 2–3 分钟（逐币拉 K + OI 扫描）。

### `s5_on_chain_narrative_radar.py` — 叙事/链上雷达
关键词分类（马斯克/川普 ★★★、币安/CZ ★★★、名人热点 ★★、新叙事 ★★）+ **动量优先**：连涨 3 轮且 >5% 才推；安全检查(rugcheck/goplus 拦蜜罐/可增发)。状态存 `~/crypto-trading/`。
- 加固：**数据源从被 Cloudflare 封的 GMGN 切换到 DexScreener**（boosts/profiles 发现 + 批量行情，覆盖 eth/bsc/base/sol）；修 `init_db` 目录缺失崩溃；TG 配置修复。

### `s6_futures_alpha_autonomous_trading_v1.py` — 期货自动交易（默认模拟盘，可选实盘）
四策略（极端深负费率做多 / 极端正费率做空 / 暴跌反弹 / 暴涨回落）+ BTC/恐惧贪婪/OI/量能 多因子 ≥3/7 门槛；每轮先结算 SL/TP 再开仓；账本原子写；**日亏熔断**。
- **默认模拟盘**：记录虚拟仓位到 `trades.json`，不下真单、不用 Key。
- **实盘模式（可选，默认关闭）**：设 `S6_LIVE=1` 启用；**默认接 Binance 测试网**（主网需另设 `BINANCE_TESTNET=0`）。市价开仓 + 交易所原生 `STOP_MARKET`/`TAKE_PROFIT_MARKET` 兜底止损止盈（脚本宕机止损仍在交易所侧生效），`settle_positions` 对账交易所成交。实盘客户端见 `binance_futures.py`。
  - 环境变量：`BINANCE_API_KEY` / `BINANCE_API_SECRET`（建议仅开合约交易、禁提现、绑 IP）、`MAX_DAILY_LOSS_USD`（默认 20，今日已实现亏损达上限即停开新仓）。
  - ⚠️ 四策略**未经实盘验证**，务必先在**测试网**充分跑通；实盘风险自负。

### `s7_vitalik_sell_radar.py` — Vitalik 抛售雷达
WebSocket 订阅 Vitalik 主钱包 `0xd8dA…6045` 的 ERC-20 `Transfer`，亚秒级发现转出，按去向分类（DEX/CEX/LP Pool），DexScreener 查价后推送。多 RPC 故障转移 + 退避重连 + 去重。
- 加固：修复 Python 3.9 下联合注解(`X | None`)导入崩溃（`from __future__ import annotations`）。

## 安装与运行

```bash
make install      # = pip install -r requirements.txt
make oi           # 跑一次性脚本(oi/s3/s6)
make s7           # 前台跑常驻脚本(s1/s5/s7)，Ctrl-C 退出
```

- **定时类**(oi/s3/s6，跑完退出)：用 cron，模板见 `deploy/cron.example`。
- **常驻类**(s1/s5/s7，持续运行)：systemd 模板 `deploy/web3@.service`
  （`sudo systemctl enable --now web3@s7_vitalik_sell_radar`），或 `make s7 &` / nohup。

### Telegram 配置（已统一）
所有脚本统一经 `tg_config.py` 的 `load_tg()` 读取，**任意来源/变量名都可**：
- 来源(优先级)：环境变量 → 脚本目录 `.env` → `.env.oi` → `~/.env`
- 变量名：`TG_BOT_TOKEN`（或 `TELEGRAM_BOT_TOKEN`）/ `TG_CHAT_ID`（或 `TELEGRAM_CHAT_ID`，支持 `@channel`）

`.env*`、`*.db`、运行时状态/快照(`trades.json`、`*_history.json`、`fr_snapshot.json` 等)均已 `.gitignore`，不入库。

## 目录结构

- `./`：现有 Python 版信号扫描器
- `./future-quant/`：Rust 版永续合约价差监控与审计
- 两者共享同一个 Git 仓库与远端

## 已知限制
- **s5 数据源特性**：DexScreener 发现的是 boosted/新建档币（非全量新币 firehose）；`holders`/聪明钱字段无（置 0）；FLAP 形态已停用（原依赖 GMGN）。
- **s6 默认模拟盘**：`S6_LIVE=1` 可开实盘（默认走 Binance 测试网，原生止损单兜底）；策略未经实盘验证，先测试网、风险自负。
- **非官方接口依赖**：s1/s3 用币安 `bapi`（公告/广场/市值）、s7 用免费公共 RPC——均可能被改版或限流，已做降级/容错但非长期保证。
- 部分脚本单轮请求较多（s3 ~2–3min），适合较长定时间隔。
