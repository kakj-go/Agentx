$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE."
    }
}

function Get-NormalizedText {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return (Get-Content -Raw -LiteralPath $Path).Replace("`r`n", "`n").Replace("`r", "`n")
}

Push-Location $root
try {
    $openSandboxSpecs = @{
        "vendor/opensandbox/specs/sandbox-lifecycle.yml" = "da84de4d80cdad83c47d771135645fbeb8d7477bc8f908cc4b374397010ed6d2"
        "vendor/opensandbox/specs/execd-api.yaml" = "0f03effe1dc5f340d13e39d6e8c815b5bdebb880183db05bea7d592696d5f5e0"
    }
    foreach ($entry in $openSandboxSpecs.GetEnumerator()) {
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root $entry.Key)).Hash.ToLowerInvariant()
        if ($actual -cne $entry.Value) {
            throw "OpenSandbox spec drift detected for $($entry.Key). Expected $($entry.Value), got $actual."
        }
    }
    $oversized = @()
    $sourceFiles = @(rg --files crates services apps/web | Where-Object { $_ -match '\.(rs|ts|tsx|js|jsx|mjs|css)$' })
    foreach ($sourceFile in $sourceFiles) {
        $lineCount = (Get-Content -LiteralPath $sourceFile).Count
        if ($lineCount -gt 2000) {
            $oversized += "$sourceFile ($lineCount lines)"
        }
    }
    if ($oversized.Count -gt 0) {
        throw "Source files exceed the 2000-line limit: $($oversized -join ', ')"
    }
    Invoke-Native "cargo fmt" { cargo fmt --all -- --check }
    Invoke-Native "cargo clippy" { cargo clippy --workspace --all-targets -- -D warnings }
    Invoke-Native "cargo test" { cargo test --workspace }
    $openApiTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-platform-api-$PID.json"
    Invoke-Native "platform OpenAPI generation" { cargo run --quiet -p platform-api -- openapi $openApiTemp }
    if ((Get-NormalizedText $openApiTemp) -cne (Get-NormalizedText "$root/openapi/platform-api.json")) {
        throw "OpenAPI schema drift detected. Run: cargo run -p platform-api -- openapi openapi/platform-api.json"
    }
    Remove-Item -LiteralPath $openApiTemp -Force
    $typeScriptTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-platform-api-$PID.ts"
    Invoke-Native "platform TypeScript generation" { pnpm --filter @agentx/web exec node scripts/generate-api-types.mjs $typeScriptTemp }
    if ((Get-NormalizedText $typeScriptTemp) -cne (Get-NormalizedText "$root/apps/web/src/shared/api/generated.ts")) {
        throw "Generated TypeScript API contract drift detected. Run: pnpm --filter @agentx/web generate:api"
    }
    Remove-Item -LiteralPath $typeScriptTemp -Force
    $gatewayOpenApiTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-trigger-gateway-$PID.json"
    Invoke-Native "gateway OpenAPI generation" { cargo run --quiet -p trigger-gateway -- openapi $gatewayOpenApiTemp }
    if ((Get-NormalizedText $gatewayOpenApiTemp) -cne (Get-NormalizedText "$root/openapi/trigger-gateway.json")) {
        throw "Gateway OpenAPI schema drift detected. Run: cargo run -p trigger-gateway -- openapi openapi/trigger-gateway.json"
    }
    Remove-Item -LiteralPath $gatewayOpenApiTemp -Force
    $gatewayTypeScriptTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-trigger-gateway-$PID.ts"
    Invoke-Native "gateway TypeScript generation" { pnpm --filter @agentx/web exec node scripts/generate-gateway-types.mjs $gatewayTypeScriptTemp }
    if ((Get-NormalizedText $gatewayTypeScriptTemp) -cne (Get-NormalizedText "$root/apps/web/src/shared/api/generated-gateway.ts")) {
        throw "Generated Gateway TypeScript contract drift detected. Run: pnpm --filter @agentx/web generate:gateway"
    }
    Remove-Item -LiteralPath $gatewayTypeScriptTemp -Force
    $nodeOpenApiTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-node-api-$PID.json"
    Invoke-Native "node OpenAPI generation" { cargo run --quiet -p echo-node -- openapi $nodeOpenApiTemp }
    if ((Get-NormalizedText $nodeOpenApiTemp) -cne (Get-NormalizedText "$root/openapi/node-api.json")) {
        throw "Node API OpenAPI drift detected. Run: cargo run -p echo-node -- openapi openapi/node-api.json"
    }
    Remove-Item -LiteralPath $nodeOpenApiTemp -Force
    $nodeSchemaTemp = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-node-schemas-$PID"
    New-Item -ItemType Directory -Path $nodeSchemaTemp | Out-Null
    Invoke-Native "node JSON Schema generation" { cargo run --quiet -p echo-node -- schemas $nodeSchemaTemp }
    $nodeSchemas = @(
        "workflow-definition.schema.json",
        "node-manifest.schema.json",
        "node-action-request.schema.json",
        "node-action-result.schema.json"
    )
    foreach ($schema in $nodeSchemas) {
        $generated = Join-Path $nodeSchemaTemp $schema
        $committed = Join-Path "$root/schemas" $schema
        if ((Get-NormalizedText $generated) -cne (Get-NormalizedText $committed)) {
            throw "Node protocol JSON Schema drift detected for $schema. Run: cargo run -p echo-node -- schemas schemas"
        }
        Remove-Item -LiteralPath $generated -Force
    }
    Remove-Item -LiteralPath $nodeSchemaTemp -Force
    Invoke-Native "web lint" { pnpm lint:web }
    Invoke-Native "web tests" { pnpm --filter @agentx/web test }
    Invoke-Native "web build" { pnpm build:web }
    Invoke-Native "deployment profile tests" { & scripts/deploy-tests.ps1 | Out-Null }
    Invoke-Native "Full Kustomize render" { kubectl kustomize deploy/k8s/stacks/full | Out-Null }
    Invoke-Native "E2E Kustomize render" { kubectl kustomize deploy/k8s/stacks/e2e | Out-Null }
    Invoke-Native "LightRAG Kustomize render" { kubectl kustomize deploy/k8s/addons/lightrag | Out-Null }
    Invoke-Native "Mem0 Kustomize render" { kubectl kustomize deploy/k8s/addons/mem0 | Out-Null }
}
finally {
    Pop-Location
}
