param(
    [ValidateSet("Backup", "Restore", "Verify", "Rebuild")][string]$Action,
    [ValidateSet("control-mysql", "runtime-mysql", "control-objects", "runtime-objects", "observability-objects", "clickhouse", "runtime-redis")][string]$Target,
    [string]$ConfigFile = "deploy/profiles/v2-production.example.json",
    [Parameter(Mandatory = $true)][string]$BackupId,
    [Parameter(Mandatory = $true)][string]$Adapter,
    [string]$RestoreTarget = "",
    [string]$EvidenceDirectory = "artifacts/v2/data-operations",
    [switch]$AllowInPlaceRestore
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$profilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$adapterPath = if ([IO.Path]::IsPathRooted($Adapter)) { $Adapter } else { Join-Path $root $Adapter }
$outputDirectory = if ([IO.Path]::IsPathRooted($EvidenceDirectory)) { $EvidenceDirectory } else { Join-Path $root $EvidenceDirectory }
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json

if ($profile.environment -ne "production") { throw "Backup/restore acceptance uses a production Profile." }
if (-not (Test-Path -LiteralPath $adapterPath -PathType Leaf)) { throw "Provider adapter does not exist: $adapterPath" }
if ($Target -eq "runtime-redis" -and $Action -ne "Rebuild") { throw "Runtime Redis supports Rebuild only and is never backed up or restored." }
if ($Target -ne "runtime-redis" -and $Action -eq "Rebuild") { throw "Rebuild is reserved for Runtime Redis." }
if ($Action -eq "Restore" -and -not $RestoreTarget) { throw "Restore requires -RestoreTarget." }
if ($Action -eq "Restore" -and -not $AllowInPlaceRestore) {
    $authoritative = switch ($Target) {
        "control-mysql" { [string]$profile.components.controlMysql.host }
        "runtime-mysql" { [string]$profile.components.runtimeMysql.host }
        "clickhouse" { [string]$profile.components.clickhouse.url }
        default { "" }
    }
    if ($authoritative -and $RestoreTarget -eq $authoritative) { throw "In-place restore requires -AllowInPlaceRestore." }
}

$started = [DateTimeOffset]::UtcNow
$receiptText = (& $adapterPath -Action $Action -Target $Target -BackupId $BackupId -ConfigFile $profilePath -RestoreTarget $RestoreTarget) -join "`n"
$adapterSucceeded = $?
if (-not $adapterSucceeded) { throw "Provider adapter failed for $Action/$Target." }
$receipt = $receiptText | ConvertFrom-Json
if ($receipt.status -ne "passed") { throw "Provider adapter did not return status=passed." }
$allowedReceiptFields = @("status", "recoveryPointUtc", "objectCount", "contentSha256", "schemaVersionObserved")
$receiptFields = @($receipt.PSObject.Properties.Name)
if (@($receiptFields | Where-Object { $_ -notin $allowedReceiptFields }).Count -gt 0 -or @($allowedReceiptFields | Where-Object { $_ -notin $receiptFields }).Count -gt 0) {
    throw "Provider adapter receipt has missing or unapproved fields."
}
$completed = [DateTimeOffset]::UtcNow
$recoveryPoint = [DateTimeOffset]::Parse([string]$receipt.recoveryPointUtc)
$elapsedMinutes = ($completed - $started).TotalMinutes
$ageMinutes = ($completed - $recoveryPoint).TotalMinutes
if ($ageMinutes -lt -1) { throw "Provider recovery point is unexpectedly in the future." }
$limits = switch -Wildcard ($Target) {
    "*-mysql" { @{ rpo = [int]$profile.backup.mysqlRpoMinutes; rto = [int]$profile.backup.mysqlRtoMinutes } }
    "*-objects" { @{ rpo = [int]$profile.backup.objectRpoMinutes; rto = [int]$profile.backup.objectRtoMinutes } }
    "clickhouse" { @{ rpo = [int]$profile.backup.clickhouseRpoMinutes; rto = [int]$profile.backup.clickhouseRtoMinutes } }
    "runtime-redis" { @{ rpo = 0; rto = [int]$profile.backup.redisRebuildMinutes } }
}
if ($Action -in @("Backup", "Verify") -and $Target -ne "runtime-redis" -and $ageMinutes -gt $limits.rpo) { throw "RPO exceeded: $([Math]::Round($ageMinutes, 2))m > $($limits.rpo)m." }
if ($Action -in @("Restore", "Rebuild") -and $elapsedMinutes -gt $limits.rto) { throw "RTO exceeded: $([Math]::Round($elapsedMinutes, 2))m > $($limits.rto)m." }

$manifest = [ordered]@{
    schemaVersion = "agentx.io/backup-manifest/v1"
    backupId = $BackupId
    target = $Target
    operation = $Action.ToLowerInvariant()
    status = "passed"
    startedAt = $started.ToString("O")
    completedAt = $completed.ToString("O")
    recoveryPointUtc = $recoveryPoint.ToString("O")
    objectCount = [int64]$receipt.objectCount
    contentSha256 = [string]$receipt.contentSha256
    schemaVersionObserved = [string]$receipt.schemaVersionObserved
    restoreTarget = if ($RestoreTarget) { $RestoreTarget } else { $null }
    providerReceipt = $receipt
}
$json = $manifest | ConvertTo-Json -Depth 30
if (-not ($json | Test-Json -SchemaFile (Join-Path $root "deploy/release/backup-manifest.schema.json"))) { throw "Generated backup manifest is invalid." }
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$outputPath = Join-Path $outputDirectory "$BackupId-$Target-$($Action.ToLowerInvariant()).json"
$json | Set-Content -LiteralPath $outputPath -Encoding utf8NoBOM
Write-Output $outputPath
