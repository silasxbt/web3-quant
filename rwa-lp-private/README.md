# rwa-lp-private

> 本项目位于 web3-quant 仓库的 `rwa-lp-private/` 目录（2026-11-16 由原私有仓库 silasxbt/rwa-lp-private 连同历史并入，按所有者要求公开）。

rwalp 的私有站：在开源版（本仓库 `rwa-lp/`，以 `vendor/rwa-lp` 子模块固定版本）之上，加分析预设、仓位台账、收益记录。

- 网址：https://lp.silasxbt.com （Cloud Run `rwalp-private`）
- 登录：整站只认第一次注册的那一把 Passkey（`private/passkey.py`），没有账号也没有密码。注册需要一次性设置码（Secret Manager `rwalp-private-setup-code`），注册成功后注册入口永久关闭。
- 数据：Firestore 集合 `p_presets`、`p_positions`、`p_events`；凭证在 `p_auth/owner`
- 仓位填了 NFT 编号时，直接读链上实时价值和未领取手续费（只读）

```bash
git submodule update --init --remote vendor/rwa-lp   # 同步开源版（在 rwa-lp-private/ 目录下执行）
# 本地运行（数据只存内存）
RWALP_MEMSTORE=1 PYTHONPATH=vendor/rwa-lp/service:. uv run --python 3.12 --with-requirements vendor/rwa-lp/service/requirements.txt --with cryptography uvicorn private.main:app --port 8081
# 测试矩阵
RWALP_MEMSTORE=1 PYTHONPATH=vendor/rwa-lp/service:. uv run --python 3.12 --with-requirements vendor/rwa-lp/service/requirements.txt --with cryptography --with httpx --with pytest pytest -q tests
# 部署
gcloud run deploy rwalp-private --source . --region asia-east1 --allow-unauthenticated --max-instances 2 \
  --set-env-vars RP_ID=lp.silasxbt.com \
  --set-secrets SETUP_CODE=rwalp-private-setup-code:latest,SESSION_SECRET=rwalp-private-session-secret:latest
```

## 注册

```bash
./scripts/open_setup.sh   # 在浏览器打开一次性注册链接，设置码不会打印出来
```

## 丢失所有设备时重置

网页上没有任何找回入口。用有 GCP 权限的账号删除凭证文档，所有会话在 30 秒内失效，首页回到「创建 Passkey」：

```bash
curl -X DELETE -H "Authorization: Bearer $(gcloud auth print-access-token --account <有 Firestore 权限的 GCP 账号>)" \
  "https://firestore.googleapis.com/v1/projects/tothemoon-477410/databases/(default)/documents/p_auth/owner"
./scripts/open_setup.sh
```

如果怀疑设置码泄露，先给 `rwalp-private-setup-code` 加一个新版本，再重新部署。
