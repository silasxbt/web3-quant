# web3 — 加密信号扫描器套件 / Crypto Signal Scanner Suite

一组独立运行的市场/链上信号扫描器，命中条件即推送 Telegram。全部走公开 API，零成本、无需付费 Key（个别脚本可选接 LLM）。

> 说明：本 README 为**概览版**，逐个脚本的深度审查与质量优化为下一步。

## 脚本一览

| 文件 | 名称 | 监控什么 | 数据源 | 依赖 |
|---|---|---|---|---|
| `oi_funding_scanner.py` | 费率转负+OI涨 | 资金费率由正转负 + OI 放大的合约 | 币安合约 API | requests |
| `s1_binance_alpha_monitor.py` | Binance Alpha 监控 | 币安 Alpha 上新公告，规则评级后推送 | 币安公告/聚合 API | httpx, sqlite3（可选 Anthropic LLM） |
| `s3_accumulation_radar.py` | 热度做多雷达 | 热度 + 负费率 + OI 暴涨 三维共振 | 币安合约 + CoinGecko Trending | requests, 本地 `square_heat` 模块 |
| `s5_on_chain_narrative_radar.py` | 叙事/链上雷达 | 全新叙事、马斯克/川普、币安/CZ 相关新币 | GMGN 新币 + DEXScreener | requests, sqlite3 |
| `s7_vitalik_sell_radar.py` | Vitalik 抛售雷达 | 事件驱动监听 Vitalik 钱包的 ERC-20 转出（卖出） | 以太坊 WebSocket RPC + DexScreener | aiohttp, websockets |

## 各脚本简介

### `oi_funding_scanner.py` — 费率转负 + OI 涨
每 5 分钟扫全市场永续合约，检测费率刚从正转负（空头涌入）且 OI 持续放大（真金白银建仓）的币种。基于快照跨运行对比，含 24h 去重。
- 信号门槛：OI 总涨幅 ≥ 8%、转负前 ≥ 2 个结算周期为正、24h 成交额 ≥ 5M。
- 输出：TG 推送（价格/费率变化/OI 分段/成交额/市值/现货/币安广场热度）。

### `s1_binance_alpha_monitor.py` — Binance Alpha 监控
REST 轮询币安公告，捕捉 Alpha 上新，用规则引擎评级后推送；可选接 Anthropic 抽取叙事（未配置则纯规则降级）。异步架构，SQLite 持久化（`data/alpha.db`）。
- 轮询：公告 30s / 聚合 15s / 上线后监控 120s。

### `s3_accumulation_radar.py` — 热度做多雷达
"热度先行 + 负费率燃料 + OI 暴涨建仓"三维共振做多信号。
- 门槛：成交量放大 ≥ 2.5x、日均成交 ≥ $20M、OI 变化 ≥ 3%、OI ≥ $2M。
- 含 API 重试（3 次）；依赖同目录 `square_heat` 模块取币安广场热度。

### `s5_on_chain_narrative_radar.py` — 叙事/链上雷达
关键词匹配 + 叙事去重（零 AI），三条推送通道：① 全新叙事（全链）② 马斯克/川普相关（ETH+SOL+BSC）③ 币安/CZ 相关（仅 BSC）。带动量追踪（连续上涨轮次）。
- 数据：GMGN 新币 + DEXScreener；历史去重存 `~/crypto-trading/narrative_history.db`。

### `s7_vitalik_sell_radar.py` — Vitalik 抛售雷达
WebSocket 订阅 Vitalik 主钱包 `0xd8dA…6045` 的 ERC-20 `Transfer` 事件，亚秒级发现卖出，按去向分类（DEX Router / CEX / LP），用 DexScreener 查代币与价格后推送。多 RPC 故障转移 + 自动重连。

## 通用配置与运行

```bash
pip install requests httpx aiohttp websockets   # 按需安装对应脚本依赖

python3 oi_funding_scanner.py        # 直接运行
```

Telegram 配置（⚠️ 当前各脚本不统一，见下）：
- `oi_funding_scanner.py` / `s3`：同目录 `.env.oi`（`TG_BOT_TOKEN` / `TG_CHAT_ID`）
- `s5`：`~/.env`（`TELEGRAM_BOT_TOKEN`）+ 环境变量 `TG_CHAT_ID`
- `s1` / `s7`：环境变量或同目录 `.env`（`TG_BOT_TOKEN` / `TG_CHAT_ID`）

定时运行（以 5 分钟为例）：
```bash
crontab -e
*/5 * * * * cd /你的路径/web3 && python3 oi_funding_scanner.py >> scanner.log 2>&1
```

## 已知事项 / 待认真审查（下一步）

- **TG 配置不统一**：`.env.oi` / `~/.env` / 环境变量混用，变量名也不一致（`TG_BOT_TOKEN` vs `TELEGRAM_BOT_TOKEN`），建议统一。
- **`s3` 依赖本地 `square_heat` 模块**，当前目录未见该文件，可能无法直接运行——待确认。
- 各脚本数据路径分散（`./data`、`~/crypto-trading`、同目录快照），错误处理与重试策略不一致。
- 多处使用币安/GMGN 非公开接口，存在被改版/封禁的脆弱性。

> 这些仅为概览扫读所见，详细质量评估与修复在后续逐个脚本进行。
