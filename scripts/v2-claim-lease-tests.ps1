$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$auditPath = Join-Path $root "docs/planv2/contracts/claim-lease-audit.json"
$audit = Get-Content -Raw -LiteralPath $auditPath | ConvertFrom-Json
if ($audit.apiVersion -ne "agentx.io/claim-lease-audit/v1") { throw "Unsupported Claim/Lease audit version." }
if ($audit.defaults.leaseSeconds -ne 30 -or $audit.defaults.heartbeatSeconds -ne 10 -or $audit.defaults.maxBatchSize -ne 100 -or $audit.defaults.clock -ne "UTC_TIMESTAMP(6)") {
    throw "Claim/Lease defaults drifted from V2-06A."
}
$expected = @(
    "control.publisher", "control.admission_outbox", "control.projector", "control.retention",
    "runtime.command", "runtime.execution_outbox", "runtime.event_sequencer", "runtime.trigger",
    "runtime.recovery", "runtime.wait", "runtime.artifact", "runtime.quota_leader",
    "runtime.bundle_gc", "runtime.retention", "runtime.trace_relay", "runtime.worker_attempt",
    "runtime.sandbox_reaper", "observability.trace_consumer"
)
$actual = @($audit.entries.role | Sort-Object -Unique)
if ($actual.Count -ne $expected.Count -or (Compare-Object ($expected | Sort-Object) $actual)) { throw "Claim/Lease Role catalog is incomplete or contains duplicates." }
$controlMigration = Get-Content -Raw -LiteralPath (Join-Path $root "migrations/control/0006_horizontal_scalability.sql")
$runtimeMigration = Get-Content -Raw -LiteralPath (Join-Path $root "migrations/runtime/0006_horizontal_scalability.sql")
foreach ($entry in $audit.entries) {
    if ([int]$entry.batchSize -lt 1 -or [int]$entry.batchSize -gt 100) { throw "$($entry.role) has an invalid Claim batch size." }
    if (-not [bool]$entry.externalIoAfterCommit) { throw "$($entry.role) permits external I/O inside its Claim transaction." }
    $sourcePath = Join-Path $root ([string]$entry.source)
    if (-not (Test-Path -LiteralPath $sourcePath)) { throw "$($entry.role) source does not exist: $($entry.source)" }
    if ($entry.authority -like "control_mysql.*" -and $entry.index -like "idx_v206_*") {
        if (-not $controlMigration.Contains([string]$entry.index)) { throw "$($entry.role) Control index is missing." }
    }
    elseif ($entry.index -like "idx_v206_*") {
        if (-not $runtimeMigration.Contains([string]$entry.index)) { throw "$($entry.role) Runtime index is missing." }
    }
}
$production = @(rg --files services/platform-control services/agentx-v2-runtime services/observability crates/agentx-mysql-lease | Where-Object { $_ -match '\.rs$' -and $_ -notmatch '(^|[\\/])tests?([\\/]|$)|_tests?\.rs$' })
function Test-LiveFencedTerminal([string]$Source) {
    foreach ($line in ($Source -split "`r?`n")) {
        if ($line -match 'UPDATE .*locked_by=NULL' -and $line -match 'locked_by=\?' -and $line -match 'fencing_token=\?' -and $line -notmatch 'locked_until>UTC_TIMESTAMP\(6\)') {
            return $false
        }
    }
    return $true
}
if (-not (Test-LiveFencedTerminal 'UPDATE jobs SET locked_by=NULL WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)')) {
    throw 'The positive live fenced terminal fixture was rejected.'
}
if (Test-LiveFencedTerminal 'UPDATE jobs SET locked_by=NULL WHERE id=? AND locked_by=? AND fencing_token=?') {
    throw 'The negative expired-Lease terminal fixture was accepted.'
}
foreach ($path in $production) {
    $source = (Get-Content -Raw -LiteralPath (Join-Path $root $path)) -replace '(?s)#\[cfg\(test\)\].*$', ''
    if ($source -match '(?s)locked_until.{0,160}OffsetDateTime::now_utc\(\)|OffsetDateTime::now_utc\(\).{0,160}locked_until') {
        throw "Pod-local Lease expiry comparison is forbidden: $path"
    }
    if (-not (Test-LiveFencedTerminal $source)) {
        throw "A fenced terminal write does not require a live database Lease: $path"
    }
}
foreach ($manifest in @(
    "deploy/k8s/v2/control/applications.yaml",
    "deploy/k8s/v2/runtime/applications.yaml",
    "deploy/k8s/v2/observability/applications.yaml"
)) {
    $source = Get-Content -Raw -LiteralPath (Join-Path $root $manifest)
    $backendCount = ([regex]::Matches($source, '(?m)^kind: Deployment\s*$')).Count
    $ownerCount = ([regex]::Matches($source, 'name: AGENTX_INSTANCE_ID')).Count
    if ($manifest -like '*control*') { $backendCount-- }
    if ($ownerCount -ne $backendCount) { throw "$manifest does not inject Pod UID into every backend Deployment." }
}
Write-Output "V2 Claim/Lease audit tests passed"
