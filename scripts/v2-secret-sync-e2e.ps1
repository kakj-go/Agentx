param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = "sync-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$safeRunId = $RunId.ToLowerInvariant() -replace '[^a-z0-9-]', '-'
$safeRunId = $safeRunId.Trim('-')
if (-not $safeRunId -or $safeRunId.Length -gt 32) { throw "RunId must resolve to 1-32 DNS-safe characters." }
$namespaces = @{
    control = "agentx-e2e-01-control-$safeRunId"
    runtime = "agentx-e2e-01-runtime-$safeRunId"
    dependencies = "agentx-e2e-01-deps-$safeRunId"
}

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Set-TestSecret {
    param([string]$Namespace, [string]$Name, [hashtable]$Values)
    $data = @{}
    foreach ($entry in $Values.GetEnumerator()) { $data[$entry.Key] = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$entry.Value)) }
    $manifest = @{ apiVersion = "v1"; kind = "Secret"; metadata = @{ name = $Name; namespace = $Namespace }; type = "Opaque"; data = $data } | ConvertTo-Json -Depth 8 -Compress
    $manifest | kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to apply Secret $Namespace/$Name." }
}

function Get-SecretValue {
    param([string]$Namespace, [string]$Name, [string]$Key)
    $payload = (& kubectl -n $Namespace get secret $Name -o json | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or -not $payload.data.PSObject.Properties[$Key]) { throw "Secret $Namespace/$Name is missing $Key." }
    return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String([string]$payload.data.PSObject.Properties[$Key].Value))
}

function Assert-Mirror {
    param([hashtable]$Canonical, [string]$CanonicalKey, [string]$Namespace, [string]$Secret, [string]$MirrorKey)
    if ([string]$Canonical[$CanonicalKey] -ne (Get-SecretValue $Namespace $Secret $MirrorKey)) { throw "Mirror mismatch for $CanonicalKey -> $Namespace/${Secret}:$MirrorKey." }
}

$canonicalKeys = @(
    "CONTROL_VAULT_TOKEN", "RUNTIME_VAULT_TOKEN", "OBSERVABILITY_REDIS_PASSWORD",
    "AGENTX_CONTROL_PUBLISHER_JWT_KID", "AGENTX_CONTROL_PROJECTOR_JWT_KID", "AGENTX_CONTROL_BFF_JWT_KID",
    "AGENTX_CONTROL_BUNDLE_KEY_ID", "AGENTX_CONTROL_WORK_PACKAGE_KEY_ID", "AGENTX_CONTROL_USER_JWT_KID",
    "AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM",
    "AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM",
    "AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON", "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON",
    "AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON",
    "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM",
    "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM",
    "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID",
    "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID", "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID",
    "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON", "AGENTX_EGRESS_TLS_CERTIFICATE_PEM", "AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM"
)
$canonical = @{}
foreach ($key in $canonicalKeys) { $canonical[$key] = "e2e-$safeRunId-$key" }

try {
    foreach ($namespace in $namespaces.Values) { Invoke-Kubectl create namespace $namespace | Out-Null }
    Set-TestSecret $namespaces.dependencies "agentx-dependencies-secrets" $canonical
    Set-TestSecret $namespaces.control "agentx-control-secrets" @{ LOCAL_CONTROL_VALUE = "preserve-control" }
    Set-TestSecret $namespaces.runtime "agentx-runtime-secrets" @{ AGENTX_RUNTIME_REDIS_PASSWORD = "runtime-redis-$safeRunId"; LOCAL_RUNTIME_VALUE = "preserve-runtime" }
    Set-TestSecret $namespaces.runtime "agentx-observability-secrets" @{ LOCAL_OBSERVABILITY_VALUE = "preserve-observability" }

    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action SyncSecrets -Target All -ConfigFile $ConfigFile -RunId $safeRunId | Out-Null
    Assert-Mirror $canonical "CONTROL_VAULT_TOKEN" $namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_VAULT_TOKEN"
    Assert-Mirror $canonical "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON" $namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_USER_JWT_PUBLIC_KEYS_JSON"
    Assert-Mirror $canonical "RUNTIME_VAULT_TOKEN" $namespaces.runtime "agentx-runtime-secrets" "AGENTX_RUNTIME_VAULT_TOKEN"
    Assert-Mirror $canonical "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID" $namespaces.runtime "agentx-runtime-secrets" "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID"
    Assert-Mirror $canonical "OBSERVABILITY_REDIS_PASSWORD" $namespaces.runtime "agentx-observability-secrets" "AGENTX_OBSERVABILITY_REDIS_PASSWORD"
    Assert-Mirror $canonical "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON" $namespaces.dependencies "agentx-egress-gateway-secrets" "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"
    if ((Get-SecretValue $namespaces.runtime "agentx-runtime-secrets" "LOCAL_RUNTIME_VALUE") -ne "preserve-runtime") { throw "SyncSecrets removed a domain-local value." }

    $canonical.CONTROL_VAULT_TOKEN = "e2e-$safeRunId-control-token-updated"
    Set-TestSecret $namespaces.dependencies "agentx-dependencies-secrets" $canonical
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action SyncSecrets -Target All -ConfigFile $ConfigFile -RunId $safeRunId | Out-Null
    Assert-Mirror $canonical "CONTROL_VAULT_TOKEN" $namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_VAULT_TOKEN"

    Write-Output (@{ status = "passed"; runId = $safeRunId; source = "$($namespaces.dependencies)/agentx-dependencies-secrets"; namespaces = $namespaces } | ConvertTo-Json -Depth 5 -Compress)
} finally {
    foreach ($namespace in $namespaces.Values) { & kubectl delete namespace $namespace --ignore-not-found --wait=true --timeout=120s | Out-Null }
}
