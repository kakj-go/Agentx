param(
    [Parameter(Mandatory = $true)][string]$BaseUrl,
    [Parameter(Mandatory = $true)][string]$ApplicationSlug,
    [string]$BearerToken,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string]$StopPath,
    [int]$DurationSeconds = 900,
    [int]$IntervalSeconds = 2,
    [switch]$SkipCertificateCheck
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$base = "$($BaseUrl.TrimEnd('/'))/gateway/v1"
$BearerToken = if ($BearerToken) { $BearerToken } else { $env:AGENTX_M7_PROBE_TOKEN }
if (-not $BearerToken) { throw "BearerToken or AGENTX_M7_PROBE_TOKEN is required." }
$headers = @{ Authorization = "Bearer $BearerToken" }
$items = [Collections.Generic.List[object]]::new()
$deadline = [DateTimeOffset]::UtcNow.AddSeconds($DurationSeconds)

function Invoke-Agentx([string]$Uri, [string]$Method = "GET", $Body = $null, [hashtable]$Extra = @{}) {
    $requestHeaders = @{} + $headers
    foreach ($entry in $Extra.GetEnumerator()) { $requestHeaders[$entry.Key] = $entry.Value }
    $parameters = @{ Uri = $Uri; Method = $Method; Headers = $requestHeaders; TimeoutSec = 90 }
    if ($SkipCertificateCheck) { $parameters.SkipCertificateCheck = $true }
    if ($null -ne $Body) {
        $parameters.ContentType = "application/json"
        $parameters.Body = $Body | ConvertTo-Json -Depth 20 -Compress
    }
    Invoke-RestMethod @parameters
}

function Read-EventCursor([string]$InvocationId) {
    try {
        $parameters = @{ Uri = "$base/invocations/$InvocationId/events?after=0"; Headers = $headers; TimeoutSec = 30 }
        if ($SkipCertificateCheck) { $parameters.SkipCertificateCheck = $true }
        $response = Invoke-WebRequest @parameters
        $cursor = 0L
        foreach ($line in ($response.Content -split "`r?`n")) {
            if ($line -match '^id:\s*(\d+)$') { $cursor = [Math]::Max($cursor, [long]$Matches[1]) }
        }
        return $cursor
    } catch { return 0L }
}

while ([DateTimeOffset]::UtcNow -lt $deadline -and (-not $StopPath -or -not (Test-Path -LiteralPath $StopPath))) {
    $key = "m7-upgrade-probe-$([Guid]::NewGuid().ToString('N'))"
    $started = [DateTimeOffset]::UtcNow
    try {
        $created = Invoke-Agentx "$base/applications/$ApplicationSlug/invocations" "POST" @{ input = @{ source = "m7-upgrade-probe"; startedAt = $started.ToString("O") } } @{ "Idempotency-Key" = $key }
        if (-not $created.id) { throw "Invocation response did not contain an id." }
        $invocation = $null
        $terminalDeadline = [DateTimeOffset]::UtcNow.AddSeconds(180)
        do {
            Start-Sleep -Milliseconds 500
            $invocation = Invoke-Agentx "$base/invocations/$($created.id)"
        } while ($invocation.status -notin @("completed", "failed", "cancelled") -and [DateTimeOffset]::UtcNow -lt $terminalDeadline)
        if ($invocation.status -notin @("completed", "failed", "cancelled")) { throw "Invocation did not reach a terminal state." }
        if ($invocation.status -ne "completed") { throw "Invocation ended as $($invocation.status)." }
        $replay = Invoke-Agentx "$base/applications/$ApplicationSlug/invocations" "POST" @{ input = @{ source = "m7-upgrade-probe"; startedAt = $started.ToString("O") } } @{ "Idempotency-Key" = $key }
        if ([string]$replay.id -ne [string]$created.id) { throw "Idempotency replay created a different Invocation." }
        $items.Add([ordered]@{ key = $key; invocationId = [string]$created.id; executionId = [string]$invocation.executionId; status = [string]$invocation.status; eventCursor = Read-EventCursor ([string]$created.id); completedAt = [DateTimeOffset]::UtcNow.ToString("O") })
    } catch {
        $items.Add([ordered]@{ key = $key; status = "failed"; error = $_.Exception.Message; occurredAt = [DateTimeOffset]::UtcNow.ToString("O") })
    }
    $remaining = $deadline - [DateTimeOffset]::UtcNow
    if ($remaining.TotalSeconds -gt 0 -and (-not $StopPath -or -not (Test-Path -LiteralPath $StopPath))) {
        Start-Sleep -Seconds ([Math]::Min($IntervalSeconds, [Math]::Ceiling($remaining.TotalSeconds)))
    }
}

$items | ForEach-Object { $_ | ConvertTo-Json -Depth 20 -Compress } | Set-Content -LiteralPath $OutputPath -Encoding utf8NoBOM
$failed = @($items | Where-Object status -eq "failed")
if ($items.Count -eq 0 -or $failed.Count -gt 0) { throw "Invocation probe recorded $($failed.Count) failures across $($items.Count) probes." }
