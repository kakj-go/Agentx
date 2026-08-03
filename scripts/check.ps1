$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    $openApiTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-platform-api-$PID.json"
    cargo run --quiet -p platform-api -- openapi $openApiTemp
    if ((Get-Content -Raw -LiteralPath $openApiTemp) -cne (Get-Content -Raw -LiteralPath "$root/openapi/platform-api.json")) {
        throw "OpenAPI schema drift detected. Run: cargo run -p platform-api -- openapi openapi/platform-api.json"
    }
    Remove-Item -LiteralPath $openApiTemp -Force
    $typeScriptTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-platform-api-$PID.ts"
    pnpm --filter @agentx/web exec node scripts/generate-api-types.mjs $typeScriptTemp
    if ((Get-Content -Raw -LiteralPath $typeScriptTemp) -cne (Get-Content -Raw -LiteralPath "$root/apps/web/src/shared/api/generated.ts")) {
        throw "Generated TypeScript API contract drift detected. Run: pnpm --filter @agentx/web generate:api"
    }
    Remove-Item -LiteralPath $typeScriptTemp -Force
    pnpm lint:web
    pnpm --filter @agentx/web test
    pnpm build:web
    kubectl kustomize deploy/k8s/overlays/local | Out-Null
    kubectl kustomize deploy/k8s/overlays/e2e | Out-Null
    kubectl kustomize deploy/k8s/addons/lightrag | Out-Null
    kubectl kustomize deploy/k8s/addons/mem0 | Out-Null
}
finally {
    Pop-Location
}
