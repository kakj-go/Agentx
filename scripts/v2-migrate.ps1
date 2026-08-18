param(
    [ValidateSet("Control", "Runtime", "Observability")][string]$Target,
    [ValidateSet("Expand", "Contract")][string]$Phase = "Expand",
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = "",
    [string]$OperationId = "",
    [ValidateRange(1, 600)][int]$LockTimeoutSeconds = 60,
    [ValidateRange(30, 3600)][int]$JobTimeoutSeconds = 600,
    [ValidateRange(1, 65535)][int]$CurrentProtocolVersion = 1,
    [ValidateRange(1, 65535)][int]$PreviousProtocolVersion = 1
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$profilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json
$plane = $Target.ToLowerInvariant()
$stage = if ($RunId -match '^07-') { "07" } elseif ($RunId -match '^06-') { "06" } else { "01" }
$suffix = if ($RunId -match '^(?:06|07)-(.+)$') { $Matches[1] } else { $RunId }
$namespacePlane = if ($plane -eq "observability") { "runtime" } else { $plane }
$namespace = if ($suffix) { "agentx-v2-$stage-$namespacePlane-$suffix" } else { [string]$profile.namespaces.$namespacePlane }
$jobName = if ($Target -eq "Control") { "control-migrate" } elseif ($Target -eq "Runtime") { "runtime-migrate" } else { "clickhouse-migrate" }

if ($Phase -eq "Contract") {
    $deploymentPayload = (& kubectl -n $namespace get deployments -o json) -join "`n"
    $replicaSetPayload = (& kubectl -n $namespace get replicasets -o json) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Failed to inspect ReplicaSets for Contract preflight." }
    $revisions = @{}
    foreach ($deployment in @(($deploymentPayload | ConvertFrom-Json).items)) {
        $revisions[[string]$deployment.metadata.name] = [string]$deployment.metadata.annotations.'deployment.kubernetes.io/revision'
    }
    $oldReplicaSets = @((($replicaSetPayload | ConvertFrom-Json).items) | Where-Object {
        $owner = @($_.metadata.ownerReferences | Where-Object { $_.kind -eq "Deployment" } | Select-Object -First 1)
        $owner.Count -eq 1 -and [int]$_.spec.replicas -gt 0 -and [string]$_.metadata.annotations.'deployment.kubernetes.io/revision' -ne $revisions[[string]$owner[0].name]
    })
    if ($oldReplicaSets.Count -gt 0) { throw "Contract migration refused while old ReplicaSets still have replicas." }
    $releasePayload = (& kubectl -n $namespace get configmap "agentx-v2-release-state-$plane" -o json 2>$null) -join "`n"
    if ($LASTEXITCODE -ne 0 -or -not $releasePayload) { throw "Contract migration requires an observed release state." }
    $release = ([string](($releasePayload | ConvertFrom-Json).data.release)) | ConvertFrom-Json
    if ([int]$release.protocolVersion -ne $CurrentProtocolVersion) {
        throw "Contract migration refused: release protocol version $($release.protocolVersion) does not match $CurrentProtocolVersion."
    }
    if (@($release.compatibleProtocolVersions | ForEach-Object { [int]$_ }) -notcontains $PreviousProtocolVersion) {
        throw "Contract migration refused: the current release does not accept previous protocol version $PreviousProtocolVersion."
    }
    $receipt = @{
        apiVersion = "v1"; kind = "ConfigMap"
        metadata = @{ name = "agentx-$plane-contract-receipt"; namespace = $namespace; labels = @{ "agentx.io/managed-by" = "agentx-v2-deploy" } }
        data = @{ phase = "contract"; status = "no-op"; reason = "V2-07A has no contract SQL"; currentProtocolVersion = [string]$CurrentProtocolVersion; previousProtocolVersion = [string]$PreviousProtocolVersion; checkedAt = [DateTimeOffset]::UtcNow.ToString("O") }
    } | ConvertTo-Json -Depth 10 -Compress
    $receipt | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to persist Contract receipt." }
    Write-Output (@{ status = "contract-ready"; target = $Target; namespace = $namespace; schemaRollback = $false } | ConvertTo-Json -Compress)
    exit 0
}

$rendered = (& (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Render -Target $Target -ConfigFile $profilePath -RunId $RunId) -join "`n"
$renderSucceeded = $?
if (-not $renderSucceeded) { throw "Failed to render the $Target migration Job." }
$job = ($rendered -split '(?m)^---\s*$' | Where-Object {
    $_ -match '(?m)^kind:\s*Job\s*$' -and ($_ -match "(?m)^\s*name:\s*$([regex]::Escape($jobName))\s*$" -or $_ -match "metadata:\s*\{\s*name:\s*$([regex]::Escape($jobName))[,}]")
} | Select-Object -First 1)
if (-not $job) { throw "Rendered $Target manifest did not contain $jobName." }
$job = $job -replace '(?m)^(\s+restartPolicy:)', "      activeDeadlineSeconds: $JobTimeoutSeconds`n`${1}"
$job = $job -replace '(?m)^(\s+env:)', "`${1}`n        - name: AGENTX_MIGRATION_LOCK_TIMEOUT_SECONDS`n          value: `"$LockTimeoutSeconds`""
$operationSuffix = if ($OperationId) { $OperationId.ToLowerInvariant() -replace '[^a-z0-9-]', '-' } else { [Guid]::NewGuid().ToString("N").Substring(0, 10) }
if ($operationSuffix -notmatch '^[a-z0-9]([-a-z0-9]*[a-z0-9])?$') { throw "OperationId must form a DNS label." }
$operationJobName = "$jobName-$operationSuffix"
if ($operationJobName.Length -gt 63) { throw "OperationId makes the Migration Job name exceed 63 characters." }
$job = [regex]::Replace($job, "(?m)^(\s*name:\s*)$([regex]::Escape($jobName))\s*$", "`${1}$operationJobName", 1)
$job | & kubectl apply -f - | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Failed to apply $operationJobName." }
& kubectl -n $namespace wait --for=condition=complete "job/$operationJobName" "--timeout=$($JobTimeoutSeconds)s" | Out-Null
if ($LASTEXITCODE -ne 0) {
    & kubectl -n $namespace logs "job/$operationJobName" --all-containers=true
    throw "$operationJobName did not complete."
}
Write-Output (@{ status = "expanded"; target = $Target; namespace = $namespace; job = $operationJobName; lockTimeoutSeconds = $LockTimeoutSeconds } | ConvertTo-Json -Compress)
