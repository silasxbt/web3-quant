#!/bin/sh
# 把 OAuth 客户端配置给 rwalp-private 的 IAP（密钥只在本机终端输入，不进聊天、不进 git）
set -e
printf "OAuth 客户端 ID: "; read CID
printf "OAuth 客户端密钥（输入不显示）: "; stty -echo; read CSECRET; stty echo; echo
F=$(mktemp); trap 'rm -f "$F"' EXIT
printf 'access_settings:\n  oauth_settings:\n    client_id: "%s"\n    client_secret: "%s"\n' "$CID" "$CSECRET" > "$F"
gcloud iap settings set "$F" --project=tothemoon-477410 --resource-type=cloud-run --region=asia-east1 --service=rwalp-private --account huinkling@gmail.com
echo "完成。重定向 URI 请确认已添加：https://iap.googleapis.com/v1/oauth/clientIds/$CID:handleRedirect"
