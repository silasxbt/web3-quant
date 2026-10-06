# rwa-lp-private

rwalp 的私有版：在开源版 [silasxbt/rwa-lp](https://github.com/silasxbt/rwa-lp)（`vendor/rwa-lp` 子模块）之上，加分析预设、仓位台账、收益记录。

- 网址：https://lp.silasxbt.com （Cloud Run `rwalp-private`，IAP 只放行 `ALLOWED_EMAIL`）
- 数据：Firestore 集合 `p_presets`、`p_positions`、`p_events`
- 仓位填了 NFT 编号时，直接读链上实时价值和未领取手续费（只读）

```bash
git submodule update --remote vendor/rwa-lp          # 同步开源版
# 本地运行（数据只存内存）
RWALP_MEMSTORE=1 PYTHONPATH=vendor/rwa-lp/service:. uv run --python 3.12 --with-requirements vendor/rwa-lp/service/requirements.txt uvicorn private.main:app --port 8081
# 部署
gcloud run deploy rwalp-private --source . --region asia-east1 --no-allow-unauthenticated --iap --set-env-vars ALLOWED_EMAIL=1@silasxbt.com
```
