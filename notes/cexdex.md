# DDA-012 交易所—DEX 偏离研究

产品授权：DECISIONS.md DDA-012。只读：币安公开行情与公开充提网络列表、链上 DEX 报价；无账户、无 API key、不下单、不划转。

## 资产与池登记

```sh
pnpm exec tsx src/cli.ts cexdex-discover [config/cexdex-registry.json]
```

只纳入币安同时支持 BSC 与 Solana 充提、两边充提均开启、存在 `<币>USDT` 现货交易对的币。合约地址取自币安网络列表，精度从链上读取；交易所单位是链上倍数的币（如 1000CAT）先排除。每条链在 PancakeSwap V3（USDT / USDC / WBNB 计价，四档费率）与 Raydium CLMM（USDT / USDC / SOL 计价）中找池，用 100 美元双向报价核验，按往返成本保留至多两个。登记表是可审阅的 JSON，提交前人工检查；删改币或池后直接用于采集，不会在采集中自动换池。

## 采集与报告

```sh
pnpm exec tsx src/cli.ts cexdex-collect 30            # 数据默认在 data/cexdex
pnpm exec tsx src/cli.ts cexdex-report <会话> exports/cexdex.md
```

- DEX：沿用 DDA-008 的 BSC 单次 Multicall 与 Solana 快照报价，同一时效门禁；规模 100 / 1000 美元，同一币所有池使用同一基础数量（L-003）。
- 交易所：一条公开 WebSocket（`<symbol>@depth20@100ms`）订阅全部币与 BNB / SOL / USDC 换算对；开始时用 REST 深度补齐安静的盘口，之后只接受更新的 lastUpdateId。币安只在前 20 档变化时推送，连接一秒内有任何消息即视为盘口为当前；否则快照带最后消息时间，过不了 2 秒的两侧时间门禁。每条链 DEX 报价完成时立即拍该链用到的盘口，使用的 20 档原样写入原始记录。
- 收益：方向 A（DEX 买、交易所卖）= 交易所卖出所得 × (1 − taker) − DEX 买入成本 × (1 + 换算费 + 额外滑点) − gas；方向 B 对称。非 USDT 计价池按同周期币安中间价换算并计一次 taker；gas 按币安 BNB / SOL 中间价计。
- 统计：每池 × 规模 × 方向的为正占比、分位数、机会段（连续为正的周期）、段长、下一周期仍为正的比例（约 5 秒延迟可否吃到）、每段首次观测合计与每小时值；再平衡按会话开始时记录的公开提币费逐笔计（最保守）。另给中间价偏离分布。

## 限制

5 秒采样（13 个 Solana 池顺序快照时实测约 8–9 秒）会漏掉更短的机会；报价不是成交，没有排队、对手方、被抢概率；交易所盘口时间是本机接收时间。交易所充提可能暂停、限额或改费，对手方风险不在数字里。结论只用于决定是否继续研究，不构成执行授权。
