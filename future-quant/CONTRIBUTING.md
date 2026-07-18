# 格式与开发规范

## 基本原则

- 默认只在测试盘或只读环境开发和验证。
- 不提交任何真实 API key、Telegram token、账户信息或私密配置。
- 涉及下单、撤单、仓位、资金费率和风控的改动必须保守处理。
- 不把“测试盘通过”等同于“实盘安全”。

## 代码格式

Rust 代码使用 `rustfmt`：

```bash
cargo fmt --all
```

提交前至少运行：

```bash
cargo check
cargo test
cargo build
```

Shell 脚本需要通过 zsh 语法检查：

```bash
zsh -n FutureQuant.command scripts/*.sh scripts/*.command
```

## 命名规范

- Rust 模块、函数、变量使用 `snake_case`。
- Rust 类型、结构体、枚举使用 `PascalCase`。
- 配置字段使用 `snake_case`。
- 用户可见文案默认使用中文。
- 内部协议、交易所字段、API 字段保持英文或交易所原始命名。

## 配置规范

- `config/local.example.toml` 是可提交模板。
- `config/local.toml` 是本机配置，必须保持 ignored。
- `config/env.example` 是环境变量模板。
- `config/env.local` 是本机密钥和 token 配置，必须保持 ignored。

## 提交信息

提交信息建议使用：

```text
type(scope): summary
```

常用类型：

- `feat`：新增功能
- `fix`：修复缺陷
- `docs`：文档更新
- `refactor`：重构
- `test`：测试相关
- `chore`：工具、配置、依赖或启动链路

示例：

```text
feat(web): add chinese module explanations
docs: add disclaimer and versioning policy
chore: add macos launch command workflow
```

## 风控改动要求

以下改动必须在说明中写清楚行为变化和验证结果：

- spread 阈值
- 滑点估算
- 仓位降档规则
- funding 风险提醒
- 交易权限开关
- kill switch
- 自动下单、撤单、平仓、补偿逻辑

## 上传前检查清单

- 没有提交 `config/local.toml`
- 没有提交 `config/env.local`
- 没有提交 `data/`
- 没有提交 `logs/`
- 没有提交 `target/`
- 没有真实 token、key、secret
- README、免责声明、版本规范和变更日志已同步
