param(
    [Parameter(Mandatory = $true)][string]$BaseUrl,
    [Parameter(Mandatory = $true)][string]$ApplicationSlug,
    [Parameter(Mandatory = $true)][string]$LargeWorkflowId,
    [Parameter(Mandatory = $true)][string]$EvaluationId,
    [Parameter(Mandatory = $true)][string]$Namespace,
    [int]$ExecutionCount = 100,
    [int]$SseClientCount = 200,
    [int]$RequiredNodeExecutions = 500,
    [int]$RequiredEvaluationCases = 1000,
    [int]$RequiredWorkflowNodes = 200,
    [int]$StabilityMinutes = 120,
    [int]$StabilityProbeSeconds = 60,
    [int]$TimeoutSeconds = 14400,
    [string]$OutputDirectory = "artifacts/m7"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$started = [DateTimeOffset]::UtcNow
$output = Join-Path $root "$OutputDirectory/$runId"
$token = [Environment]::GetEnvironmentVariable("AGENTX_M7_BEARER_TOKEN")
$base = $BaseUrl.TrimEnd('/')
$gateway = "$base/gateway/v1"
$platform = "$base/api/v1"
$failures = [Collections.Generic.List[string]]::new()
$metrics = [ordered]@{
    executionCount = 0
    completedExecutionCount = 0
    nodeExecutionCount = 0
    sseClientCount = 0
    successfulSseClientCount = 0
    evaluationCaseCount = 0
    workflowNodeCount = 0
    stabilityDurationSeconds = 0
    stabilityProbeCount = 0
    projectorP95Seconds = 0.0
    activeQuotaReservations = 0
}

function Invoke-Agentx([string]$Uri, [string]$Method = "GET", $Body = $null, [hashtable]$AdditionalHeaders = @{}) {
    $headers = @{ Authorization = "Bearer $token" }
    foreach ($entry in $AdditionalHeaders.GetEnumerator()) { $headers[$entry.Key] = $entry.Value }
    $parameters = @{ Uri = $Uri; Method = $Method; Headers = $headers; TimeoutSec = 120 }
    if ($null -ne $Body) {
        $parameters.ContentType = "application/json"
        $parameters.Body = $Body | ConvertTo-Json -Depth 30 -Compress
    }
    Invoke-RestMethod @parameters
}

function Wait-Invocation([string]$InvocationId, [int]$DeadlineSeconds = 600) {
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($DeadlineSeconds)
    do {
        $value = Invoke-Agentx "$gateway/invocations/$InvocationId"
        if ($value.status -in @("completed", "failed", "cancelled")) { return $value }
        Start-Sleep -Milliseconds 500
    } while ([DateTimeOffset]::UtcNow -lt $deadline)
    throw "Invocation $InvocationId did not reach a terminal state."
}

function Invoke-MySql([string]$Sql) {
    $result = kubectl -n $Namespace exec statefulset/mysql -- sh -c `
        'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql --batch --skip-column-names -uroot agentx -e "$1"' -- $Sql
    if ($LASTEXITCODE -ne 0) { throw "MySQL evidence query failed." }
    @($result | Where-Object { $_ -ne "" })
}

if (-not $token) { throw "AGENTX_M7_BEARER_TOKEN is required." }
foreach ($value in @($ExecutionCount, $SseClientCount, $RequiredNodeExecutions, $RequiredEvaluationCases, $RequiredWorkflowNodes, $StabilityProbeSeconds, $TimeoutSeconds)) {
    if ($value -lt 1) { throw "Capacity thresholds and timeouts must be positive." }
}
if ($StabilityMinutes -lt 0) { throw "StabilityMinutes cannot be negative." }
if (-not (Get-Command kubectl -ErrorAction SilentlyContinue)) { throw "kubectl is required." }
New-Item -ItemType Directory -Path $output -Force | Out-Null

try {
    $draft = Invoke-Agentx "$platform/workflows/$LargeWorkflowId/draft"
    $metrics.workflowNodeCount = @($draft.definition.nodes).Count
    if ($metrics.workflowNodeCount -lt $RequiredWorkflowNodes) {
        $failures.Add("Workflow has $($metrics.workflowNodeCount) nodes; required $RequiredWorkflowNodes.")
    }

    $work = 1..$ExecutionCount
    $invocations = @($work | ForEach-Object -Parallel {
        $run = $using:runId
        $gatewayUrl = $using:gateway
        $slug = $using:ApplicationSlug
        $headers = @{ Authorization = "Bearer $using:token"; "Idempotency-Key" = "$run-execution-$_" }
        $body = @{ input = @{ source = "m7-capacity"; sequence = $_ } } | ConvertTo-Json -Compress
        try {
            Invoke-RestMethod -Method Post -Uri "$gatewayUrl/applications/$slug/invocations" -Headers $headers -ContentType "application/json" -Body $body -TimeoutSec 120
        }
        catch {
            [pscustomobject]@{ id = $null; status = "request_failed"; error = $_.Exception.Message }
        }
    } -ThrottleLimit ([Math]::Min($ExecutionCount, 100)))
    $invocationIds = @($invocations | Where-Object id | ForEach-Object { [string]$_.id })
    $metrics.executionCount = $invocationIds.Count
    if ($metrics.executionCount -ne $ExecutionCount) {
        $failures.Add("Created $($metrics.executionCount) of $ExecutionCount requested executions.")
    }

    $sseWork = for ($index = 0; $index -lt $SseClientCount; $index++) {
        if ($invocationIds.Count -gt 0) { $invocationIds[$index % $invocationIds.Count] }
    }
    $sseResults = @($sseWork | ForEach-Object -Parallel {
        $gatewayUrl = $using:gateway
        try {
            $response = Invoke-WebRequest -Uri "$gatewayUrl/invocations/$_/events" -Headers @{ Authorization = "Bearer $using:token" } -TimeoutSec 900
            [pscustomobject]@{ success = $response.StatusCode -eq 200; length = $response.RawContentLength }
        }
        catch {
            [pscustomobject]@{ success = $false; length = 0 }
        }
    } -ThrottleLimit ([Math]::Min($SseClientCount, 200)))
    $metrics.sseClientCount = $sseResults.Count
    $metrics.successfulSseClientCount = @($sseResults | Where-Object success).Count
    if ($metrics.successfulSseClientCount -ne $SseClientCount) {
        $failures.Add("Only $($metrics.successfulSseClientCount) of $SseClientCount SSE clients completed successfully.")
    }

    $terminal = @($invocationIds | ForEach-Object -Parallel {
        $gatewayUrl = $using:gateway
        $deadline = [DateTimeOffset]::UtcNow.AddSeconds([Math]::Min($using:TimeoutSeconds, 1800))
        do {
            try {
                $value = Invoke-RestMethod -Uri "$gatewayUrl/invocations/$_" -Headers @{ Authorization = "Bearer $using:token" } -TimeoutSec 30
                if ($value.status -in @("completed", "failed", "cancelled")) { $value; break }
            }
            catch {}
            Start-Sleep -Milliseconds 500
        } while ([DateTimeOffset]::UtcNow -lt $deadline)
    } -ThrottleLimit 50)
    $completed = @($terminal | Where-Object status -eq "completed")
    $metrics.completedExecutionCount = $completed.Count
    if ($metrics.completedExecutionCount -ne $ExecutionCount) {
        $failures.Add("Only $($metrics.completedExecutionCount) of $ExecutionCount executions completed successfully.")
    }

    $executionIds = @($completed | Where-Object executionId | ForEach-Object { [string]$_.executionId })
    $nodeCounts = @($executionIds | ForEach-Object -Parallel {
        $platformUrl = $using:platform
        try {
            $nodes = Invoke-RestMethod -Uri "$platformUrl/executions/$_/nodes" -Headers @{ Authorization = "Bearer $using:token" } -TimeoutSec 60
            @($nodes.items).Count
        }
        catch { 0 }
    } -ThrottleLimit 50)
    $metrics.nodeExecutionCount = [int](($nodeCounts | Measure-Object -Sum).Sum)
    if ($metrics.nodeExecutionCount -lt $RequiredNodeExecutions) {
        $failures.Add("Observed $($metrics.nodeExecutionCount) node executions; required $RequiredNodeExecutions.")
    }

    $report = Invoke-Agentx "$platform/evaluations/$EvaluationId/report"
    if ($report.run.status -notin @("completed", "failed", "cancelled")) {
        Invoke-Agentx "$platform/evaluations/$EvaluationId/start" "POST" | Out-Null
        $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
        do {
            Start-Sleep -Seconds 2
            $report = Invoke-Agentx "$platform/evaluations/$EvaluationId/report"
        } while ($report.run.status -notin @("completed", "failed", "cancelled") -and [DateTimeOffset]::UtcNow -lt $deadline)
    }
    $metrics.evaluationCaseCount = @($report.results).Count
    if ($report.run.status -ne "completed" -or $metrics.evaluationCaseCount -lt $RequiredEvaluationCases) {
        $failures.Add("Evaluation status is '$($report.run.status)' with $($metrics.evaluationCaseCount) cases; required $RequiredEvaluationCases completed cases.")
    }

    $stabilityStarted = [DateTimeOffset]::UtcNow
    $stabilityDeadline = $stabilityStarted.AddMinutes($StabilityMinutes)
    while ([DateTimeOffset]::UtcNow -lt $stabilityDeadline) {
        $probeKey = "$runId-stability-$($metrics.stabilityProbeCount)"
        $probe = Invoke-Agentx "$gateway/applications/$ApplicationSlug/invocations" "POST" `
            @{ input = @{ source = "m7-stability"; sequence = $metrics.stabilityProbeCount } } `
            @{ "Idempotency-Key" = $probeKey }
        $probe = Wait-Invocation ([string]$probe.id)
        $metrics.stabilityProbeCount++
        if ($probe.status -ne "completed") { $failures.Add("Stability probe $($probe.id) ended as $($probe.status).") }
        $remaining = $stabilityDeadline - [DateTimeOffset]::UtcNow
        if ($remaining.TotalSeconds -gt 0) { Start-Sleep -Seconds ([Math]::Min($StabilityProbeSeconds, [Math]::Ceiling($remaining.TotalSeconds))) }
    }
    $metrics.stabilityDurationSeconds = [Math]::Floor(([DateTimeOffset]::UtcNow - $stabilityStarted).TotalSeconds)

    $sqlStart = $started.UtcDateTime.ToString("yyyy-MM-dd HH:mm:ss.ffffff")
    $latencies = @(Invoke-MySql "SELECT TIMESTAMPDIFF(MICROSECOND,o.occurred_at,r.processed_at)/1000000 FROM outbox_events o JOIN projection_receipts r ON r.event_id=o.id AND r.projector_name='m7-business-v1' WHERE o.occurred_at>='$sqlStart' ORDER BY 1;")
    if ($latencies.Count -gt 0) {
        $sorted = @($latencies | ForEach-Object { [double]$_ } | Sort-Object)
        $index = [Math]::Max(0, [Math]::Ceiling($sorted.Count * 0.95) - 1)
        $metrics.projectorP95Seconds = $sorted[$index]
    }
    else {
        $failures.Add("No Projector latency samples were recorded.")
    }
    if ($metrics.projectorP95Seconds -ge 5) { $failures.Add("Projector p95 is $($metrics.projectorP95Seconds)s; required <5s.") }

    $active = @(Invoke-MySql "SELECT COUNT(*) FROM quota_reservations WHERE status='active' AND expires_at>CURRENT_TIMESTAMP(6);")
    $metrics.activeQuotaReservations = [int]$active[0]
    if ($metrics.activeQuotaReservations -ne 0) { $failures.Add("$($metrics.activeQuotaReservations) active Quota Reservations remain.") }
}
catch {
    $failures.Add($_.Exception.Message)
}
finally {
    $evidence = [ordered]@{
        schemaVersion = "agentx.io/m7-capacity-evidence/v1"
        status = $(if ($failures.Count -eq 0) { "passed" } else { "failed" })
        runId = $runId
        startedAt = $started.ToString("O")
        completedAt = [DateTimeOffset]::UtcNow.ToString("O")
        metrics = $metrics
        failures = @($failures)
    }
    $path = Join-Path $output "capacity-evidence.json"
    $evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/m7-capacity-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Capacity evidence is invalid." }
    Write-Output $path
}
if ($failures.Count -gt 0) { throw "M7 capacity verification failed: $($failures -join '; ')" }
