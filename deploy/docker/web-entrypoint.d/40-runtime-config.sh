#!/bin/sh
set -eu

runtime_base_url=${AGENTX_RUNTIME_PUBLIC_BASE_URL:-}
if [ -z "$runtime_base_url" ]; then
  exit 0
fi
escaped=$(printf '%s' "$runtime_base_url" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf 'window.__AGENTX_RUNTIME__={runtimeBaseUrl:"%s"};\n' "$escaped" \
  > /usr/share/nginx/html/runtime-config.js
