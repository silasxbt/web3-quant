#!/bin/sh
# 在浏览器打开一次性注册链接（设置码只在本机读取，不打印、不进聊天）
CODE=$(gcloud secrets versions access latest --secret rwalp-private-setup-code --project tothemoon-477410 --account huinkling@gmail.com) || exit 1
open "https://lp.silasxbt.com/?setup=$CODE"
