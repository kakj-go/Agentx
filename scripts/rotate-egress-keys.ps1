param(
    [ValidateSet("Plan", "Rotate")]
    [string]$Action = "Plan",
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$configPath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $configPath | ConvertFrom-Json
if ($profile.apiVersion -ne "agentx.io/deployment/v2alpha3") { throw "Egress key rotation requires a deployment/v2alpha3 Profile." }
$runtimeNamespace = [string]$profile.namespaces.runtime
$dependenciesNamespace = [string]$profile.namespaces.dependencies

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Get-SecretName {
    param([string]$Workload, [string]$Fallback)
    if ($profile.secrets.workloads -and $profile.secrets.workloads.PSObject.Properties[$Workload]) {
        return [string]$profile.secrets.workloads.PSObject.Properties[$Workload].Value
    }
    return $Fallback
}

function Get-SecretData {
    param([string]$Namespace, [string]$Name)
    $secret = (& kubectl -n $Namespace get secret $Name -o json | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or -not $secret) { throw "Secret $Namespace/$Name is unavailable." }
    $result = @{}
    foreach ($property in @($secret.data.PSObject.Properties)) {
        $result[$property.Name] = [Convert]::FromBase64String([string]$property.Value)
    }
    return $result
}

function Set-SecretValues {
    param([string]$Namespace, [string]$Name, [hashtable]$Values, [string[]]$Remove = @())
    $data = @{}
    foreach ($entry in $Values.GetEnumerator()) {
        $bytes = if ($entry.Value -is [byte[]]) { $entry.Value } else { [Text.Encoding]::UTF8.GetBytes([string]$entry.Value) }
        $data[$entry.Key] = [Convert]::ToBase64String($bytes)
    }
    foreach ($key in $Remove) { $data[$key] = $null }
    $patch = @{ data = $data } | ConvertTo-Json -Depth 6 -Compress
    Invoke-Kubectl @("-n", $Namespace, "patch", "secret", $Name, "--type", "merge", "-p", $patch) | Out-Null
}

function Restart-Caller {
    param([string]$Deployment)
    Invoke-Kubectl -n $runtimeNamespace rollout restart "deployment/$Deployment" | Out-Null
    Invoke-Kubectl -n $runtimeNamespace rollout status "deployment/$Deployment" --timeout=300s | Out-Null
}

function Restart-Gateway {
    Invoke-Kubectl -n $dependenciesNamespace rollout restart deployment/agentx-egress-gateway | Out-Null
    Invoke-Kubectl -n $dependenciesNamespace rollout status deployment/agentx-egress-gateway --timeout=300s | Out-Null
}

$gatewaySecret = Get-SecretName "egressGateway" "agentx-egress-gateway-secrets"
$canonicalSecret = [string]$profile.secrets.dependencies
if ([string]::IsNullOrWhiteSpace($canonicalSecret)) { throw "Profile secrets.dependencies must name agentx-dependencies-secrets." }
$roles = @(
    [ordered]@{ name = "runtime-gateway"; deployment = "runtime-gateway"; secret = Get-SecretName "runtimeGateway" ([string]$profile.secrets.runtime); secretKey = "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM"; kidSecretKey = "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID"; privateProperty = "runtimeGatewayEgressPrivateKeyPem"; publicProperty = "runtimeGatewayEgressPublicKeyPem" },
    [ordered]@{ name = "workflow-runtime"; deployment = "workflow-runtime"; secret = Get-SecretName "workflowRuntime" ([string]$profile.secrets.runtime); secretKey = "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM"; kidSecretKey = "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID"; privateProperty = "workflowRuntimeEgressPrivateKeyPem"; publicProperty = "workflowRuntimeEgressPublicKeyPem" },
    [ordered]@{ name = "workflow-worker"; deployment = "workflow-worker"; secret = Get-SecretName "workflowWorker" ([string]$profile.secrets.runtime); secretKey = "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM"; kidSecretKey = "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID"; privateProperty = "workflowWorkerEgressPrivateKeyPem"; publicProperty = "workflowWorkerEgressPublicKeyPem" },
    [ordered]@{ name = "sandbox"; deployment = "sandbox-manager"; secret = Get-SecretName "sandboxManager" ([string]$profile.secrets.runtime); secretKey = "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM"; kidSecretKey = "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID"; privateProperty = "sandboxEgressPrivateKeyPem"; publicProperty = "sandboxEgressPublicKeyPem" }
)
$canonicalData = Get-SecretData $dependenciesNamespace $canonicalSecret
$gatewayData = Get-SecretData $dependenciesNamespace $gatewaySecret
if (-not $canonicalData.ContainsKey("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON")) { throw "Canonical Egress public-key entry is missing." }
$originalPublicJson = [Text.Encoding]::UTF8.GetString($canonicalData.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON)
if (-not $gatewayData.ContainsKey("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON") -or [Text.Encoding]::UTF8.GetString($gatewayData.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON) -ne $originalPublicJson) { throw "Gateway public keys differ from $dependenciesNamespace/$canonicalSecret; run SyncSecrets before rotation." }
$originalPublicKeys = $originalPublicJson | ConvertFrom-Json -AsHashtable
foreach ($role in $roles) {
    if (-not $canonicalData.ContainsKey($role.kidSecretKey) -or -not $canonicalData.ContainsKey($role.secretKey)) { throw "Canonical Secret lacks $($role.kidSecretKey) or $($role.secretKey)." }
    $role["oldKid"] = [Text.Encoding]::UTF8.GetString($canonicalData[$role.kidSecretKey])
    if (-not $originalPublicKeys.ContainsKey($role.oldKid)) { throw "Gateway does not trust active key $($role.oldKid)." }
    $secretData = Get-SecretData $runtimeNamespace $role.secret
    if (-not $secretData.ContainsKey($role.secretKey) -or -not $secretData.ContainsKey($role.kidSecretKey)) { throw "Secret $runtimeNamespace/$($role.secret) lacks canonical Egress key material." }
    $role["oldPrivate"] = $secretData[$role.secretKey]
    if ([Convert]::ToBase64String($role.oldPrivate) -ne [Convert]::ToBase64String($canonicalData[$role.secretKey]) -or [Text.Encoding]::UTF8.GetString($secretData[$role.kidSecretKey]) -ne $role.oldKid) { throw "Caller $($role.deployment) differs from $dependenciesNamespace/$canonicalSecret; run SyncSecrets before rotation." }
}

if ($Action -eq "Plan") {
    Write-Output (@{
        status = "ready"
        action = "Rotate"
        runtimeNamespace = $runtimeNamespace
        dependenciesNamespace = $dependenciesNamespace
        gatewaySecret = $gatewaySecret
        callers = @($roles | ForEach-Object { @{ deployment = $_.deployment; secret = $_.secret; activeKid = $_.oldKid } })
        phases = @("publish-overlap", "restart-gateway", "roll-callers", "remove-previous", "commit-canonical")
    } | ConvertTo-Json -Depth 8)
    exit 0
}

$rotationId = "egress-$([DateTimeOffset]::UtcNow.ToString('yyyyMMddHHmmss'))-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
$lockName = "agentx-egress-key-rotation-lock"
& kubectl -n $dependenciesNamespace create configmap $lockName --from-literal="rotationId=$rotationId" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Another Egress key rotation is active in $dependenciesNamespace." }
$overlapPublished = $false
try {
    $material = ((& cargo run --quiet -p agentx-v2-ops --bin agentx-keygen) -join "`n") | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or -not $material) { throw "Failed to generate Egress signing keys." }
    $overlap = @{}
    foreach ($entry in $originalPublicKeys.GetEnumerator()) { $overlap[$entry.Key] = [string]$entry.Value }
    foreach ($role in $roles) {
        $role["newKid"] = "$($role.name)-$rotationId"
        $role["newPrivate"] = [Text.Encoding]::UTF8.GetBytes([string]$material.PSObject.Properties[$role.privateProperty].Value)
        $overlap[$role.newKid] = [string]$material.PSObject.Properties[$role.publicProperty].Value
    }
    Set-SecretValues $dependenciesNamespace $gatewaySecret @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = ($overlap | ConvertTo-Json -Compress) }
    $overlapPublished = $true
    Restart-Gateway

    foreach ($role in $roles) {
        Set-SecretValues $runtimeNamespace $role.secret @{
            $role.secretKey = $role.newPrivate
            $role.kidSecretKey = $role.newKid
            "$($role.secretKey)_PREVIOUS" = $role.oldPrivate
        }
        Restart-Caller $role.deployment
    }

    $finalKeys = @{}
    foreach ($entry in $overlap.GetEnumerator()) {
        if (@($roles.oldKid) -notcontains $entry.Key) { $finalKeys[$entry.Key] = $entry.Value }
    }
    Set-SecretValues $dependenciesNamespace $gatewaySecret @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = ($finalKeys | ConvertTo-Json -Compress) }
    Restart-Gateway
    foreach ($role in $roles) {
        Set-SecretValues $runtimeNamespace $role.secret @{} @("$($role.secretKey)_PREVIOUS")
    }
    $canonicalValues = @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = ($finalKeys | ConvertTo-Json -Compress) }
    foreach ($role in $roles) {
        $canonicalValues[$role.secretKey] = $role.newPrivate
        $canonicalValues[$role.kidSecretKey] = $role.newKid
    }
    Set-SecretValues $dependenciesNamespace $canonicalSecret $canonicalValues
    Write-Output (@{ status = "rotated"; rotationId = $rotationId; activeKids = @($roles.newKid) } | ConvertTo-Json -Depth 5 -Compress)
} catch {
    $rotationError = $_
    if ($overlapPublished) {
        foreach ($role in $roles) {
            try {
                Set-SecretValues $runtimeNamespace $role.secret @{ $role.secretKey = $role.oldPrivate; $role.kidSecretKey = $role.oldKid } @("$($role.secretKey)_PREVIOUS")
                Restart-Caller $role.deployment
            } catch { Write-Warning "Rollback failed for $($role.deployment): $($_.Exception.Message)" }
        }
        try {
            Set-SecretValues $dependenciesNamespace $gatewaySecret @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = $originalPublicJson }
            Restart-Gateway
        } catch { Write-Warning "Gateway key rollback failed: $($_.Exception.Message)" }
    }
    throw "Egress key rotation $rotationId failed and rollback was attempted: $($rotationError.Exception.Message)"
} finally {
    & kubectl -n $dependenciesNamespace delete configmap $lockName --ignore-not-found | Out-Null
}
