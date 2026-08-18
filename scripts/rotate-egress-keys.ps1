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

function Get-DeploymentKeyId {
    param([string]$Deployment)
    $deploymentObject = (& kubectl -n $runtimeNamespace get deployment $Deployment -o json | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or -not $deploymentObject) { throw "Deployment $runtimeNamespace/$Deployment is unavailable." }
    $entry = @((@($deploymentObject.spec.template.spec.containers)[0].env) | Where-Object name -eq "AGENTX_EGRESS_JWT_KEY_ID")
    if ($entry.Count -ne 1 -or -not $entry[0].value) { throw "Deployment $Deployment has no literal AGENTX_EGRESS_JWT_KEY_ID." }
    return [string]$entry[0].value
}

function Set-DeploymentKeyId {
    param([string]$Deployment, [string]$KeyId)
    Invoke-Kubectl -n $runtimeNamespace set env "deployment/$Deployment" "AGENTX_EGRESS_JWT_KEY_ID=$KeyId" | Out-Null
    Invoke-Kubectl -n $runtimeNamespace rollout status "deployment/$Deployment" --timeout=300s | Out-Null
}

function Restart-Gateway {
    Invoke-Kubectl -n $dependenciesNamespace rollout restart deployment/agentx-egress-gateway | Out-Null
    Invoke-Kubectl -n $dependenciesNamespace rollout status deployment/agentx-egress-gateway --timeout=300s | Out-Null
}

$gatewaySecret = Get-SecretName "egressGateway" "agentx-egress-gateway-secrets"
$roles = @(
    [ordered]@{ name = "runtime-gateway"; deployment = "runtime-gateway"; secret = Get-SecretName "runtimeGateway" ([string]$profile.secrets.runtime); secretKey = "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM"; privateProperty = "runtimeGatewayEgressPrivateKeyPem"; publicProperty = "runtimeGatewayEgressPublicKeyPem" },
    [ordered]@{ name = "workflow-runtime"; deployment = "workflow-runtime"; secret = Get-SecretName "workflowRuntime" ([string]$profile.secrets.runtime); secretKey = "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM"; privateProperty = "workflowRuntimeEgressPrivateKeyPem"; publicProperty = "workflowRuntimeEgressPublicKeyPem" },
    [ordered]@{ name = "workflow-worker"; deployment = "workflow-worker"; secret = Get-SecretName "workflowWorker" ([string]$profile.secrets.runtime); secretKey = "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM"; privateProperty = "workflowWorkerEgressPrivateKeyPem"; publicProperty = "workflowWorkerEgressPublicKeyPem" },
    [ordered]@{ name = "sandbox"; deployment = "sandbox-manager"; secret = Get-SecretName "sandboxManager" ([string]$profile.secrets.runtime); secretKey = "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM"; privateProperty = "sandboxEgressPrivateKeyPem"; publicProperty = "sandboxEgressPublicKeyPem" }
)
$gatewayData = Get-SecretData $dependenciesNamespace $gatewaySecret
if (-not $gatewayData.ContainsKey("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON")) { throw "Gateway public-key Secret entry is missing." }
$originalPublicJson = [Text.Encoding]::UTF8.GetString($gatewayData.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON)
$originalPublicKeys = $originalPublicJson | ConvertFrom-Json -AsHashtable
foreach ($role in $roles) {
    $role["oldKid"] = Get-DeploymentKeyId $role.deployment
    if (-not $originalPublicKeys.ContainsKey($role.oldKid)) { throw "Gateway does not trust active key $($role.oldKid)." }
    $secretData = Get-SecretData $runtimeNamespace $role.secret
    if (-not $secretData.ContainsKey($role.secretKey)) { throw "Secret $runtimeNamespace/$($role.secret) lacks $($role.secretKey)." }
    $role["oldPrivate"] = $secretData[$role.secretKey]
}

if ($Action -eq "Plan") {
    Write-Output (@{
        status = "ready"
        action = "Rotate"
        runtimeNamespace = $runtimeNamespace
        dependenciesNamespace = $dependenciesNamespace
        gatewaySecret = $gatewaySecret
        callers = @($roles | ForEach-Object { @{ deployment = $_.deployment; secret = $_.secret; activeKid = $_.oldKid } })
        phases = @("publish-overlap", "restart-gateway", "roll-callers", "remove-previous")
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
            "$($role.secretKey)_PREVIOUS" = $role.oldPrivate
        }
        Set-DeploymentKeyId $role.deployment $role.newKid
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
    Write-Output (@{ status = "rotated"; rotationId = $rotationId; activeKids = @($roles.newKid) } | ConvertTo-Json -Depth 5 -Compress)
} catch {
    $rotationError = $_
    if ($overlapPublished) {
        foreach ($role in $roles) {
            try {
                Set-SecretValues $runtimeNamespace $role.secret @{ $role.secretKey = $role.oldPrivate } @("$($role.secretKey)_PREVIOUS")
                Set-DeploymentKeyId $role.deployment $role.oldKid
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
