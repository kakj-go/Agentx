$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    pnpm lint:web
    pnpm build:web
    kubectl kustomize deploy/k8s/overlays/local | Out-Null
}
finally {
    Pop-Location
}
