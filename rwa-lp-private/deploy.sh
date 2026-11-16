#!/bin/sh
# Deploy lp.silasxbt.com: stage the public service (../rwa-lp/service) and the private app into one build context.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp -R "$here/../rwa-lp/service" "$stage/public"
cp -R "$here/private" "$stage/private"
cp "$here/Dockerfile" "$here/.dockerignore" "$stage/"
find "$stage" -name __pycache__ -type d -prune -exec rm -rf {} +
gcloud run deploy rwalp-private --source "$stage" --region asia-east1 --project tothemoon-477410 --allow-unauthenticated --max-instances 2 \
  --set-env-vars RP_ID=lp.silasxbt.com \
  --set-secrets SETUP_CODE=rwalp-private-setup-code:latest,SESSION_SECRET=rwalp-private-session-secret:latest --quiet "$@"
