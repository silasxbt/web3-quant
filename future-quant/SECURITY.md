# 安全说明

## 密钥管理

- 不要把真实 API key、secret、passphrase、Telegram token 提交到仓库。
- 本机密钥只应放在 `config/env.local`，该文件已被 `.gitignore` 忽略。
- OpenAI 兼容模型 API key 也应只放在 `config/env.local`，例如 `OPENAI_API_KEY`。
- 交易所 API key 应优先使用测试盘权限或只读权限。
- 生产盘 key 必须单独创建，最小化权限，并绑定 IP 或其他交易所支持的限制条件。

## 交易权限

当前版本没有生产盘真实交易执行能力。后续接入交易权限时，必须满足：

- 默认禁止真实订单。
- 生产盘真实订单必须显式开启。
- 每次下单前检查 kill switch。
- 对双边成交、单边成交、撤单失败、仓位不一致和接口超时做状态记录。

## 模型审计数据

闪电审计会把机会信息、交易所 symbol、价格、价差、盘口深度和风险标记发送给配置的 OpenAI 兼容模型服务。启用该功能前应确认：

- 选择的模型服务商和 base URL 是可信的。
- 不在提示词中写入交易所 API key、账户信息或真实仓位信息。
- 不把模型审计结论当成自动交易许可。
- 模型审计失败、超时或证据不足时，应默认人工复核。

## 漏洞与风险反馈

如果发现密钥泄露、错误下单、风控绕过、仓位对账异常或其他安全问题，应立即：

1. 停止本地服务。
2. 撤销相关 API key。
3. 导出日志和配置快照。
4. 在修复前保持生产盘交易权限关闭。

## 上传前安全检查

```bash
git status --short --ignored
git check-ignore -v config/local.toml config/env.local data logs target
rg -n --hidden --glob '!target/**' --glob '!data/**' --glob '!logs/**' --glob '!config/local.toml' --glob '!config/env.local' 'api[_-]?secret|api[_-]?key|TELEGRAM_BOT_TOKEN|OKX_API|BINANCE_API|passphrase|token\s*=|secret\s*=|sk-[A-Za-z0-9]|ghp_[A-Za-z0-9]' .
```
