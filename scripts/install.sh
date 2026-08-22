#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
config_file="${AGENTX_CONFIG_FILE:-${1:-deploy/profiles/v2-dockerhub-beta.json}}"
target="${AGENTX_TARGET:-${2:-All}}"

if ! command -v pwsh >/dev/null 2>&1; then
    echo "未找到 PowerShell 7（pwsh），请先安装后再运行此脚本。" >&2
    exit 1
fi

arguments=(
    -NoProfile
    -File "$script_dir/install.ps1"
    -ConfigFile "$config_file"
    -Target "$target"
)

if [[ -n "${AGENTX_RUN_ID:-}" ]]; then
    arguments+=(-RunId "$AGENTX_RUN_ID")
fi

exec pwsh "${arguments[@]}"
