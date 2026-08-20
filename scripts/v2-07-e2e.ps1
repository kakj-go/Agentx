param(
    [string]$RunId = (Get-Date -Format "yyyyMMddHHmmss"),
    [Parameter(Mandatory = $true)][string]$ProductionProfileTemplate,
    [Parameter(Mandatory = $true)][string]$ExternalFixtureManifest,
    [Parameter(Mandatory = $true)][string]$BackupAdapter,
    [Parameter(Mandatory = $true)][string]$ScenarioAdapter,
    [Parameter(Mandatory = $true)][string]$RestoreTargetsFile,
    [Parameter(Mandatory = $true)][string]$ReleaseManifest,
    [Parameter(Mandatory = $true)][string]$PreviousReleaseManifest,
    [string]$ArtifactRoot = "artifacts/v2"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$safeRunId = ($RunId.ToLowerInvariant() -replace '[^a-z0-9-]', '-').Trim('-')
if (-not $safeRunId) { throw "RunId must contain a DNS-label character." }
if ($safeRunId.Length -gt 24) { $safeRunId = $safeRunId.Substring(0, 24).TrimEnd('-') }
$namespaces = [ordered]@{
    control = "agentx-e2e-07-control-$safeRunId"
    runtime = "agentx-e2e-07-runtime-$safeRunId"
    observability = "agentx-e2e-07-runtime-$safeRunId"
    dependencies = "agentx-e2e-07-deps-$safeRunId"
}
$artifactDirectory = Join-Path (if ([IO.Path]::IsPathRooted($ArtifactRoot)) { $ArtifactRoot } else { Join-Path $root $ArtifactRoot }) "$RunId/v2-07/07a"
$profilePath = Join-Path $artifactDirectory "production-profile.json"
$contextPath = Join-Path $artifactDirectory "scenario-context.json"
$timeline = [Collections.Generic.List[string]]::new()
$developmentReplicas = [Collections.Generic.List[object]]::new()
$testMigrationJobs = [Collections.Generic.List[object]]::new()
$summary = $null
$runPassed = $false

function Resolve-InputPath {
    param([string]$Path)
    $resolved = if ([IO.Path]::IsPathRooted($Path)) { $Path } else { Join-Path $root $Path }
    if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) { throw "Required V2-07A input does not exist: $resolved" }
    return (Resolve-Path -LiteralPath $resolved).Path
}

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Record-DevelopmentReplicas {
    foreach ($namespace in @("agentx-control", "agentx-runtime")) {
        $json = (& kubectl -n $namespace get deployment -o json 2>$null) -join "`n"
        if ($LASTEXITCODE -ne 0 -or -not $json) { continue }
        foreach ($deployment in @((($json | ConvertFrom-Json).items))) {
            $developmentReplicas.Add(@{ namespace = $namespace; name = [string]$deployment.metadata.name; replicas = [int]$deployment.spec.replicas })
            Invoke-Kubectl -Arguments @("-n", $namespace, "scale", "deployment/$($deployment.metadata.name)", "--replicas=0")
        }
    }
}

function Restore-DevelopmentReplicas {
    foreach ($entry in $developmentReplicas) {
        & kubectl -n $entry.namespace scale "deployment/$($entry.name)" "--replicas=$($entry.replicas)" 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Failed to restore $($entry.namespace)/$($entry.name)." }
    }
}

function Get-PodUids {
    param([string]$Namespace)
    $json = (& kubectl -n $Namespace get pods -o json) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Failed to read Pods from $Namespace." }
    return @((($json | ConvertFrom-Json).items) | ForEach-Object { [string]$_.metadata.uid } | Sort-Object)
}

function Invoke-Scenario {
    param([string]$Action)
    $receiptText = (& $script:scenarioAdapterPath -Action $Action -ContextFile $contextPath) -join "`n"
    if (-not $?) { throw "Scenario Adapter failed: $Action" }
    if (-not ($receiptText | Test-Json -SchemaFile (Join-Path $root "deploy/release/v2-07-scenario-receipt.schema.json"))) {
        throw "Scenario Adapter returned an invalid receipt: $Action"
    }
    $receipt = $receiptText | ConvertFrom-Json
    if ($receipt.scenario -ne ($Action.ToLowerInvariant() -replace '[^a-z0-9-]', '-')) {
        throw "Scenario Adapter receipt does not identify $Action."
    }
    $receiptPath = Join-Path $artifactDirectory "scenario-$($receipt.scenario).json"
    $receiptText | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
    $timeline.Add("$(Get-Date -Format o) scenario $Action passed")
    return $receipt
}

function Assert-StatusContract {
    param([string]$Target)
    $statusText = (& (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Status -Target $Target -ConfigFile $profilePath) -join "`n"
    if (-not $?) { throw "$Target Status failed." }
    $status = $statusText | ConvertFrom-Json
    if ($status.status -ne "observed" -or @($status.planes).Count -ne 1) { throw "$Target Status has an invalid envelope." }
    $plane = @($status.planes)[0]
    foreach ($property in @("release", "deployments", "migrations", "probes", "autoscaling", "disruptionBudgets", "externalDependencies")) {
        if (-not $plane.PSObject.Properties[$property]) { throw "$Target Status is missing $property." }
    }
    if (-not $plane.release -or @($plane.deployments).Count -eq 0 -or @($plane.externalDependencies).Count -eq 0) {
        throw "$Target Status is incomplete."
    }
    $statusText | Set-Content -LiteralPath (Join-Path $artifactDirectory "status-$($Target.ToLowerInvariant()).json") -Encoding utf8NoBOM
}

function Invoke-BackupRestoreMatrix {
    param($RestoreTargets)
    foreach ($target in @("control-mysql", "runtime-mysql", "clickhouse", "control-objects", "runtime-objects", "observability-objects")) {
        $backupId = "$safeRunId-$target"
        $restoreTarget = [string]$RestoreTargets.PSObject.Properties[$target].Value
        & (Join-Path $PSScriptRoot "v2-backup-restore.ps1") -Action Backup -Target $target -BackupId $backupId -Adapter $script:backupAdapterPath -ConfigFile $profilePath -EvidenceDirectory $artifactDirectory | Out-Null
        if (-not $?) { throw "Backup failed for $target." }
        & (Join-Path $PSScriptRoot "v2-backup-restore.ps1") -Action Restore -Target $target -BackupId $backupId -Adapter $script:backupAdapterPath -ConfigFile $profilePath -RestoreTarget $restoreTarget -EvidenceDirectory $artifactDirectory | Out-Null
        if (-not $?) { throw "Restore failed for $target." }
        & (Join-Path $PSScriptRoot "v2-backup-restore.ps1") -Action Verify -Target $target -BackupId $backupId -Adapter $script:backupAdapterPath -ConfigFile $profilePath -RestoreTarget $restoreTarget -EvidenceDirectory $artifactDirectory | Out-Null
        if (-not $?) { throw "Restored data verification failed for $target." }
    }
}

function Invoke-ParallelMigrationTargets {
    $jobs = @()
    foreach ($target in @("Control", "Runtime", "Observability")) {
        $targetCode = if ($target -eq "Control") { "c" } elseif ($target -eq "Runtime") { "r" } else { "o" }
        $operationId = "p-$targetCode-$safeRunId"
        $jobs += Start-Job -ScriptBlock {
            param($Script, $Target, $Profile, $OperationId)
            & $Script -Target $Target -Phase Expand -ConfigFile $Profile -OperationId $OperationId -LockTimeoutSeconds 60
        } -ArgumentList (Join-Path $PSScriptRoot "v2-migrate.ps1"), $target, $profilePath, $operationId
        $testMigrationJobs.Add(@{ namespace = [string]$namespaces[$target.ToLowerInvariant()]; name = "$(if ($target -eq 'Observability') { 'clickhouse' } else { $target.ToLowerInvariant() })-migrate-$operationId" })
    }
    Wait-Job -Job $jobs -Timeout 660 | Out-Null
    foreach ($job in $jobs) {
        if ($job.State -ne "Completed") { throw "Parallel migration job $($job.Id) did not complete." }
        Receive-Job -Job $job -ErrorAction Stop | Out-Null
        Remove-Job -Job $job -Force
    }
}

function Assert-NoSensitiveArtifacts {
    foreach ($file in Get-ChildItem -LiteralPath $artifactDirectory -File) {
        $text = Get-Content -Raw -LiteralPath $file.FullName
        if ($text -match '(?i)-----BEGIN (?:RSA |EC )?PRIVATE KEY-----|"(?:password|accessToken|refreshToken|apiKey|secretValue)"\s*:') {
            throw "Sensitive material was written to $($file.Name)."
        }
    }
}

$profileTemplatePath = Resolve-InputPath $ProductionProfileTemplate
$externalFixturePath = Resolve-InputPath $ExternalFixtureManifest
$script:backupAdapterPath = Resolve-InputPath $BackupAdapter
$script:scenarioAdapterPath = Resolve-InputPath $ScenarioAdapter
$restoreTargetsPath = Resolve-InputPath $RestoreTargetsFile
$releaseManifestPath = Resolve-InputPath $ReleaseManifest
$previousReleaseManifestPath = Resolve-InputPath $PreviousReleaseManifest
$restoreTargetsText = Get-Content -Raw -LiteralPath $restoreTargetsPath
if (-not ($restoreTargetsText | Test-Json -SchemaFile (Join-Path $root "deploy/release/v2-07-restore-targets.schema.json"))) { throw "RestoreTargetsFile is invalid." }
$restoreTargets = $restoreTargetsText | ConvertFrom-Json

New-Item -ItemType Directory -Path $artifactDirectory -Force | Out-Null
try {
    Record-DevelopmentReplicas
    foreach ($plane in @("control", "runtime", "dependencies")) {
        $namespace = [string]$namespaces[$plane]
        $labels = @{ "agentx.io/plane" = $plane }
        if ($plane -eq "dependencies") { $labels["agentx.io/ingress"] = "allowed" }
        $manifest = @{ apiVersion = "v1"; kind = "Namespace"; metadata = @{ name = $namespace; labels = $labels } } | ConvertTo-Json -Depth 8 -Compress
        $manifest | & kubectl apply -f - | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Failed to create $namespace." }
    }

    $profileText = Get-Content -Raw -LiteralPath $profileTemplatePath
    $profileText = $profileText.Replace("__CONTROL_NAMESPACE__", $namespaces.control).Replace("__RUNTIME_NAMESPACE__", $namespaces.runtime).Replace("__OBSERVABILITY_NAMESPACE__", $namespaces.observability).Replace("__DEPS_NAMESPACE__", $namespaces.dependencies)
    $profile = $profileText | ConvertFrom-Json
    $profile.namespaces.control = $namespaces.control
    $profile.namespaces.runtime = $namespaces.runtime
    $profile.namespaces.dependencies = $namespaces.dependencies
    $profile.ingress.className = "agentx-nginx-07-$safeRunId"
    $profile | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $profilePath -Encoding utf8NoBOM

    $context = [ordered]@{
        schemaVersion = "agentx.io/v2-07a-scenario-context/v1"
        runId = $RunId
        namespaces = $namespaces
        profilePath = $profilePath
        releaseManifest = $releaseManifestPath
        previousReleaseManifest = $previousReleaseManifestPath
        restoreTargets = $restoreTargets
        artifactDirectory = $artifactDirectory
    }
    $context | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $contextPath -Encoding utf8NoBOM

    $fixture = (Get-Content -Raw -LiteralPath $externalFixturePath).Replace("__CONTROL_NAMESPACE__", $namespaces.control).Replace("__RUNTIME_NAMESPACE__", $namespaces.runtime).Replace("__OBSERVABILITY_NAMESPACE__", $namespaces.observability).Replace("__DEPS_NAMESPACE__", $namespaces.dependencies)
    $fixture | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "External TLS fixture deployment failed." }
    Invoke-Scenario "Preflight" | Out-Null

    $render = (& (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Render -Target All -ConfigFile $profilePath) -join "`n"
    if ($render -match 'kind: StatefulSet|image:\s+[^\r\n]+:(?:dev|latest)') { throw "Production render contains bundled infrastructure or mutable images." }
    foreach ($target in @("Dependencies", "Control", "Runtime", "Observability")) {
        & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Validate -Target $target -ConfigFile $profilePath | Out-Null
        & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Install -Target $target -ConfigFile $profilePath -ReleaseManifest $previousReleaseManifestPath | Out-Null
        if (-not $?) { throw "$target installation failed." }
        Assert-StatusContract $target
        & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Doctor -Target $target -ConfigFile $profilePath | Out-Null
    }
    Invoke-Scenario "SeedRuntime" | Out-Null

    Invoke-Scenario "StartContinuity" | Out-Null
    $runtimeBefore = @(Get-PodUids $namespaces.runtime)
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Upgrade -Target Control -ConfigFile $profilePath -ReleaseManifest $releaseManifestPath | Out-Null
    $runtimeAfter = @(Get-PodUids $namespaces.runtime)
    if (($runtimeBefore -join ',') -ne ($runtimeAfter -join ',')) { throw "Control upgrade restarted Runtime Pods." }
    Invoke-Scenario "AssertContinuity" | Out-Null

    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Upgrade -Target Runtime -ConfigFile $profilePath -ReleaseManifest $releaseManifestPath | Out-Null
    & (Join-Path $PSScriptRoot "v2-migrate.ps1") -Target Runtime -Phase Contract -ConfigFile $profilePath | Out-Null
    Invoke-Scenario "AssertRuntimeUpgrade" | Out-Null

    $badImage = "$($profile.images.registry)/runtime-gateway@sha256:$('f' * 64)"
    & kubectl -n $namespaces.runtime set image deployment/runtime-gateway "runtime-gateway=$badImage" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to inject the controlled rollout failure." }
    & kubectl -n $namespaces.runtime rollout status deployment/runtime-gateway --timeout=20s 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "The controlled invalid-image rollout unexpectedly succeeded." }
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Rollback -Target Runtime -ConfigFile $profilePath -PreviousReleaseManifest $previousReleaseManifestPath | Out-Null
    Invoke-Scenario "AssertRollbackInvariant" | Out-Null

    Invoke-Scenario "StartMigrationContention" | Out-Null
    $lockRejected = $false
    try {
        & (Join-Path $PSScriptRoot "v2-migrate.ps1") -Target Control -Phase Expand -ConfigFile $profilePath -OperationId "contender-$safeRunId" -LockTimeoutSeconds 1 -JobTimeoutSeconds 60 | Out-Null
    } catch { $lockRejected = $true }
    finally { Invoke-Scenario "StopMigrationContention" | Out-Null }
    if (-not $lockRejected) { throw "A second same-target Migration acquired the held lock." }
    $testMigrationJobs.Add(@{ namespace = $namespaces.control; name = "control-migrate-contender-$safeRunId" })
    Invoke-ParallelMigrationTargets
    Invoke-Scenario "AssertMigrationContention" | Out-Null

    Invoke-Scenario "NetworkPolicyMatrix" | Out-Null
    Invoke-Scenario "PodSecurityMatrix" | Out-Null
    Invoke-Scenario "RotateKeys" | Out-Null

    Invoke-BackupRestoreMatrix $restoreTargets
    & (Join-Path $PSScriptRoot "v2-backup-restore.ps1") -Action Rebuild -Target runtime-redis -BackupId "$safeRunId-runtime-redis" -Adapter $script:backupAdapterPath -ConfigFile $profilePath -EvidenceDirectory $artifactDirectory | Out-Null
    Invoke-Scenario "AssertRedisRebuild" | Out-Null
    foreach ($target in @("Control", "Runtime", "Observability")) {
        & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Doctor -Target $target -ConfigFile $profilePath | Out-Null
        if (-not $?) { throw "$target Doctor failed after the recovery matrix." }
    }

    $controlReplicas = @{}
    foreach ($deployment in @("platform-control", "web-console")) {
        $controlReplicas[$deployment] = [int]((& kubectl -n $namespaces.control get deployment $deployment -o jsonpath='{.spec.replicas}') -join '')
        Invoke-Kubectl -Arguments @("-n", $namespaces.control, "scale", "deployment/$deployment", "--replicas=0")
    }
    Invoke-Scenario "StopControlDependencies" | Out-Null
    try { Invoke-Scenario "AssertContinuity" | Out-Null }
    finally {
        Invoke-Scenario "StartControlDependencies" | Out-Null
        foreach ($deployment in $controlReplicas.Keys) { Invoke-Kubectl -Arguments @("-n", $namespaces.control, "scale", "deployment/$deployment", "--replicas=$($controlReplicas[$deployment])") }
    }
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Doctor -Target Control -ConfigFile $profilePath | Out-Null
    Invoke-Scenario "AssertReconciliation" | Out-Null
    Invoke-Scenario "AssertNoBusinessResidue" | Out-Null

    foreach ($entry in $testMigrationJobs) { & kubectl -n $entry.namespace delete job $entry.name --ignore-not-found --wait=true 2>$null | Out-Null }
    Assert-NoSensitiveArtifacts
    $summary = [ordered]@{
        schemaVersion = "agentx.io/v2-07a-evidence/v1"
        status = "passed"
        runId = $RunId
        namespaces = $namespaces
        scenarios = @(
            "production-profile", "independent-target-operations", "control-upgrade-runtime-continuity", "runtime-expand-contract",
            "rollback-invariants", "migration-locks", "network-boundaries", "pod-secret-security", "dual-kid-rotation",
            "backup-restore", "redis-rebuild", "control-offline-runtime", "residual-reconciliation", "cleanup-contract"
        )
        deferred = @("V2S-006", "gVisor/Kata RuntimeClass", "role-level isolation", "final Cosign gate")
        timeline = $timeline
        cleanup = $null
    }
    $runPassed = $true
}
finally {
    $cleanupErrors = [Collections.Generic.List[string]]::new()
    if (Test-Path -LiteralPath $contextPath -PathType Leaf) {
        try { Invoke-Scenario "CleanupFixtures" | Out-Null } catch { $cleanupErrors.Add($_.Exception.Message) }
    }
    if (Test-Path -LiteralPath $profilePath -PathType Leaf) {
        try { & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -Target All -ConfigFile $profilePath | Out-Null } catch { $cleanupErrors.Add($_.Exception.Message) }
    }
    try { Restore-DevelopmentReplicas } catch { $cleanupErrors.Add($_.Exception.Message) }
    foreach ($namespace in @($namespaces.control, $namespaces.runtime, $namespaces.dependencies) | Select-Object -Unique) {
        & kubectl delete namespace $namespace --ignore-not-found --wait=true --timeout=300s 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { $cleanupErrors.Add("Failed to delete $namespace") }
    }
    if ($runPassed) {
        $summary.cleanup = @{ status = if ($cleanupErrors.Count -eq 0) { "passed" } else { "failed" }; errors = @($cleanupErrors) }
        $summary.timeline = $timeline
        $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $artifactDirectory "summary.json") -Encoding utf8NoBOM
    }
    if ($cleanupErrors.Count -gt 0) { throw "V2-07A cleanup failed: $($cleanupErrors -join '; ')" }
}
