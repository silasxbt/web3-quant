# web3-quant 一键运行
# 用法: make install / make s7 / make help
PY ?= python3

.PHONY: help install oi s1 s3 s5 s6 s7

help: ; @echo "目标: install | oi s3 s6 (定时类) | s1 s5 s7 (常驻类)" && echo "示例: make install; make s7"
install: ; $(PY) -m pip install -r requirements.txt

# 一次性脚本(跑完退出，配合 cron，见 deploy/cron.example)
oi: ; $(PY) oi_funding_scanner.py
s3: ; $(PY) s3_accumulation_radar.py
s6: ; $(PY) s6_futures_alpha_autonomous_trading_v1.py

# 常驻脚本(持续运行，部署见 deploy/web3@.service)
s1: ; $(PY) s1_binance_alpha_monitor.py
s5: ; $(PY) s5_on_chain_narrative_radar.py
s7: ; $(PY) s7_vitalik_sell_radar.py
