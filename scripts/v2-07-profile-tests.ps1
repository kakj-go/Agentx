param(
    [switch]$SkipWebSourceBaseline
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$schema = Join-Path $root "deploy/profiles/deployment-profile-v2.schema.json"
$localPath = Join-Path $root "deploy/profiles/v2-full-local.json"
$productionPath = Join-Path $root "deploy/profiles/v2-production.example.json"
$deploy = Join-Path $root "scripts/deploy-v2.ps1"

function Get-DirectorySha256 {
    param([string]$Path)
    $hash = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    Get-ChildItem -LiteralPath $Path -Recurse -File | Sort-Object FullName | ForEach-Object {
        $relative = [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/')
        $hash.AppendData([Text.Encoding]::UTF8.GetBytes($relative))
        $hash.AppendData([byte[]]@(0))
        $hash.AppendData([IO.File]::ReadAllBytes($_.FullName))
    }
    return [Convert]::ToHexString($hash.GetHashAndReset()).ToLowerInvariant()
}

if (-not $SkipWebSourceBaseline) {
    $expectedWebHash = (Get-Content -Raw -LiteralPath (Join-Path $root "deploy/release/v2-07-web-source.sha256")).Trim()
    $actualWebHash = Get-DirectorySha256 (Join-Path $root "apps/web/src")
    if ($actualWebHash -ne $expectedWebHash) { throw "V2-07A must not modify apps/web/src (expected $expectedWebHash, actual $actualWebHash)." }
}

foreach ($path in @($localPath, $productionPath)) {
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Invalid Profile: $path" }
}

$production = (& $deploy -Action Render -Target All -ConfigFile $productionPath) -join "`n"
if (-not $?) { throw "Production render failed." }
foreach ($forbidden in @("kind: StatefulSet", ":dev", ":latest", "http://object-storage", "http://vault", "redis://runtime-redis", "agentx-control-secrets", "agentx-runtime-secrets")) {
    if ($production.Contains($forbidden)) { throw "Production render contains forbidden value: $forbidden" }
}
foreach ($required in @(
    "@sha256:", "pod-security.kubernetes.io/enforce: restricted", "readOnlyRootFilesystem: true", "seccompProfile:",
    "platform-control-external-egress", "runtime-gateway-external-egress", "observability-external-egress",
    "agentx-platform-control-secrets", "agentx-runtime-gateway-secrets", "agentx-observability-app-secrets"
)) {
    if (-not $production.Contains($required)) { throw "Production render is missing: $required" }
}
foreach ($application in @("platform-control", "web-console", "runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability")) {
    if ($production -notmatch "(?ms)^kind: Deployment\s+metadata:.*?name: $application(?:\s|,)") { throw "The compact production Profile is missing application Deployment $application." }
}
foreach ($imageLine in @($production -split "`n" | Where-Object { $_ -match '^\s+image:' })) {
    if ($imageLine -notmatch '@sha256:[0-9a-f]{64}$') { throw "Production image is not digest pinned: $imageLine" }
}

$control = (& $deploy -Action Render -Target Control -ConfigFile $productionPath) -join "`n"
$runtime = (& $deploy -Action Render -Target Runtime -ConfigFile $productionPath) -join "`n"
$observability = (& $deploy -Action Render -Target Observability -ConfigFile $productionPath) -join "`n"
if ($control -match 'namespace: agentx-runtime') { throw "Control Target leaked another plane." }
if ($runtime -match 'namespace: agentx-control') { throw "Runtime Target leaked another plane." }
if ($observability -match 'namespace: agentx-control' -or $observability -notmatch 'namespace: agentx-runtime') { throw "Observability Target must stay isolated inside the Runtime Namespace." }

$profile = Get-Content -Raw -LiteralPath $productionPath | ConvertFrom-Json
$releasePath = [IO.Path]::GetTempFileName()
try {
    $release = [ordered]@{
        schemaVersion = "agentx.io/v2-release/v1"
        version = "v2-07a-static"
        gitCommit = "a" * 40
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        protocolVersion = 1
        compatibleProtocolVersions = @(1)
        images = @($profile.images.digests.PSObject.Properties | ForEach-Object { @{ name = $_.Name; digest = [string]$_.Value } })
    }
    $release | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $releasePath -Encoding utf8NoBOM
    if (-not ((Get-Content -Raw -LiteralPath $releasePath) | Test-Json -SchemaFile (Join-Path $root "deploy/release/v2-release-manifest.schema.json"))) { throw "V2 Release Manifest fixture is invalid." }
    & $deploy -Action Validate -Target All -ConfigFile $productionPath -ReleaseManifest $releasePath | Out-Null
    $release.images = @($release.images | Select-Object -Skip 1)
    $release | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $releasePath -Encoding utf8NoBOM
    $releaseRejected = $false
    try { & $deploy -Action Validate -Target All -ConfigFile $productionPath -ReleaseManifest $releasePath 2>$null | Out-Null } catch { $releaseRejected = $true }
    if (-not $releaseRejected) { throw "Production accepted an incomplete V2 Release Manifest." }
} finally {
    Remove-Item -LiteralPath $releasePath -Force -ErrorAction SilentlyContinue
}
$negativePath = [IO.Path]::GetTempFileName()
try {
    $profile.components.runtimeRedis.url = "redis://runtime-redis.example.internal:6379/"
    $profile | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $negativePath
    $rejected = $false
    try { & $deploy -Action Validate -Target Runtime -ConfigFile $negativePath 2>$null | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw "Production accepted plaintext Redis." }
} finally {
    Remove-Item -LiteralPath $negativePath -Force -ErrorAction SilentlyContinue
}

$invalidAnnotationPath = [IO.Path]::GetTempFileName()
try {
    $profile = Get-Content -Raw -LiteralPath $productionPath | ConvertFrom-Json
    $profile.network.egressGateway.sandboxAccess.serviceAnnotations = [pscustomobject]@{ "example.com/internal" = "true" }
    $profile | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $invalidAnnotationPath
    $rejected = $false
    try { & $deploy -Action Validate -Target Dependencies -ConfigFile $invalidAnnotationPath 2>$null | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw "Production accepted an arbitrary private LoadBalancer annotation." }
} finally {
    Remove-Item -LiteralPath $invalidAnnotationPath -Force -ErrorAction SilentlyContinue
}

$unknownEgress = Get-Content -Raw -LiteralPath $productionPath | ConvertFrom-Json
$unknownEgress.network.externalEgress | Add-Member -NotePropertyName "providerLegacy" -NotePropertyValue ([pscustomobject]@{ cidrs = @("203.0.113.0/24"); ports = @(443) })
if (($unknownEgress | ConvertTo-Json -Depth 30) | Test-Json -SchemaFile $schema -ErrorAction SilentlyContinue) {
    throw "Profile schema accepted an unknown externalEgress target."
}

foreach ($script in @("scripts/deploy-v2.ps1", "scripts/rotate-egress-keys.ps1", "scripts/v2-migrate.ps1", "scripts/v2-backup-restore.ps1", "scripts/v2-07-e2e.ps1")) {
    $path = Join-Path $root $script
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) { throw "$script has a parse error: $($errors[0].Message)" }
}

$rotationSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/rotate-egress-keys.ps1")
foreach ($required in @("publish-overlap", "roll-callers", "remove-previous", "_PREVIOUS", "rollback was attempted", "agentx-egress-key-rotation-lock")) {
    if (-not $rotationSource.Contains($required)) { throw "Egress key rotation is missing phase or recovery contract: $required" }
}

$deploySource = Get-Content -Raw -LiteralPath $deploy
foreach ($statusField in @("release =", "deployments =", "migrations =", "probes =", "autoscaling =", "disruptionBudgets =", "externalDependencies =")) {
    if (-not $deploySource.Contains($statusField)) { throw "V2 Status contract is missing $statusField" }
}
$migrationSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-migrate.ps1")
foreach ($required in @("OperationId", "compatibleProtocolVersions", "PreviousProtocolVersion", "operationJobName")) {
    if (-not $migrationSource.Contains($required)) { throw "V2 Migration contract is missing $required" }
}
if ($migrationSource.Contains('delete job $jobName')) { throw "Concurrent V2 Migration invocations still delete each other." }

$e2eSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-07-e2e.ps1")
foreach ($required in @(
    '-Action Restore', '-Action Verify', '-Action Rollback', 'Assert-StatusContract', 'StartMigrationContention',
    'NetworkPolicyMatrix', 'PodSecurityMatrix', 'RotateKeys', 'StopControlDependencies', 'AssertNoBusinessResidue',
    'production-profile', 'cleanup-contract'
)) {
    if (-not $e2eSource.Contains($required)) { throw "V2-07A E2E is missing required coverage: $required" }
}
$scenarioNames = [regex]::Match($e2eSource, '(?s)scenarios\s*=\s*@\((.*?)\)\s*\n\s*deferred').Groups[1].Value
if ([regex]::Matches($scenarioNames, '"[a-z0-9-]+"').Count -ne 14) { throw "V2-07A summary must contain exactly fourteen scenarios." }

$restoreTargets = @{
    "control-mysql" = "restored-control-db.internal"
    "runtime-mysql" = "restored-runtime-db.internal"
    "control-objects" = "restored-control-objects"
    "runtime-objects" = "restored-runtime-objects"
    "observability-objects" = "restored-observability-objects"
    clickhouse = "https://restored-clickhouse.internal"
} | ConvertTo-Json
if (-not ($restoreTargets | Test-Json -SchemaFile (Join-Path $root "deploy/release/v2-07-restore-targets.schema.json"))) { throw "Restore target fixture is invalid." }
$scenarioReceipt = @{
    schemaVersion = "agentx.io/v2-07a-scenario-receipt/v1"
    scenario = "networkpolicymatrix"
    status = "passed"
    observedAt = [DateTimeOffset]::UtcNow.ToString("O")
    assertions = @("cross-plane database access was denied")
    contentSha256 = "a" * 64
} | ConvertTo-Json
if (-not ($scenarioReceipt | Test-Json -SchemaFile (Join-Path $root "deploy/release/v2-07-scenario-receipt.schema.json"))) { throw "Scenario receipt fixture is invalid." }

$evidenceDirectory = Join-Path ([IO.Path]::GetTempPath()) "agentx-v2-07-backup-$([Guid]::NewGuid().ToString('N'))"
try {
    $manifest = & (Join-Path $root "scripts/v2-backup-restore.ps1") -Action Backup -Target control-mysql -BackupId v207a-static -Adapter (Join-Path $root "scripts/fixtures/v2-backup-provider-fixture.ps1") -ConfigFile $productionPath -EvidenceDirectory $evidenceDirectory
    if (-not ((Get-Content -Raw -LiteralPath $manifest) | Test-Json -SchemaFile (Join-Path $root "deploy/release/backup-manifest.schema.json"))) { throw "Backup Manifest fixture is invalid." }
    $restore = & (Join-Path $root "scripts/v2-backup-restore.ps1") -Action Restore -Target control-mysql -BackupId v207a-static -Adapter (Join-Path $root "scripts/fixtures/v2-backup-provider-fixture.ps1") -ConfigFile $productionPath -RestoreTarget "restored-control-db.internal" -EvidenceDirectory $evidenceDirectory
    $verify = & (Join-Path $root "scripts/v2-backup-restore.ps1") -Action Verify -Target control-mysql -BackupId v207a-static -Adapter (Join-Path $root "scripts/fixtures/v2-backup-provider-fixture.ps1") -ConfigFile $productionPath -RestoreTarget "restored-control-db.internal" -EvidenceDirectory $evidenceDirectory
    foreach ($path in @($restore, $verify)) {
        if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile (Join-Path $root "deploy/release/backup-manifest.schema.json"))) { throw "Restore/Verify Manifest fixture is invalid." }
    }
} finally {
    Remove-Item -LiteralPath $evidenceDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "V2-07A production Profile and operator contract tests passed"
