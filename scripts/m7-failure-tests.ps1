param(
    [Parameter(Mandatory = $true)][string]$E2ERunDirectory,
    [string]$Namespace = "agentx",
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OutputDirectory = "artifacts/m7"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "$OutputDirectory/$runId/failure"
$assertions = [Collections.Generic.List[object]]::new()
$failures = [Collections.Generic.List[string]]::new()
$minioScaledDown = $false

function Resolve-RequiredFile([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Label does not exist: $Path" }
    (Resolve-Path -LiteralPath $Path).Path
}

function Read-KeyValues([string]$Path) {
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $Path) {
        $parts = $line -split '=', 2
        if ($parts.Count -eq 2) { $values[$parts[0]] = $parts[1] }
    }
    $values
}

function Assert-Uuid([string]$Value, [string]$Label) {
    $parsed = [Guid]::Empty
    if (-not [Guid]::TryParse($Value, [ref]$parsed) -or $parsed -eq [Guid]::Empty) {
        throw "$Label is not a non-empty UUID."
    }
}

function Add-Assertion([string]$Name, [scriptblock]$Check) {
    $detailPath = Join-Path $output "$Name.txt"
    try {
        $detail = & $Check
        if (-not $detail) { $detail = "$Name passed" }
        @($detail) | Set-Content -LiteralPath $detailPath -Encoding utf8NoBOM
        $assertions.Add([ordered]@{ name = $Name; status = "passed"; evidence = [IO.Path]::GetRelativePath($root, $detailPath).Replace('\', '/') })
    }
    catch {
        $message = $_.Exception.Message
        $message | Set-Content -LiteralPath $detailPath -Encoding utf8NoBOM
        $assertions.Add([ordered]@{ name = $Name; status = "failed"; evidence = [IO.Path]::GetRelativePath($root, $detailPath).Replace('\', '/') })
        $failures.Add("${Name}: $message")
    }
}

function Invoke-Probe([string]$Name, [int]$TimeoutSeconds) {
    kubectl -n $Namespace delete pod $Name --ignore-not-found --wait=true | Out-Null
    kubectl -n $Namespace run $Name --image=busybox:1.37 --restart=Never --command -- sh -c "wget -T $TimeoutSeconds -qO- http://minio:9000/minio/health/live" | Out-Null
    for ($attempt = 0; $attempt -lt ($TimeoutSeconds + 15); $attempt++) {
        $phase = kubectl -n $Namespace get pod $Name -o jsonpath='{.status.phase}' 2>$null
        if ($phase -in @("Succeeded", "Failed")) { return $phase }
        Start-Sleep -Seconds 1
    }
    "TimedOut"
}

if (-not (Get-Command kubectl -ErrorAction SilentlyContinue)) { throw "kubectl is required." }
$e2e = (Resolve-Path -LiteralPath $E2ERunDirectory -ErrorAction Stop).Path
New-Item -ItemType Directory -Path $output -Force | Out-Null
$m4 = Read-KeyValues (Resolve-RequiredFile (Join-Path $e2e "m4-database-evidence.txt") "M4 database evidence")
$m5 = Read-KeyValues (Resolve-RequiredFile (Join-Path $e2e "m5-database-evidence.txt") "M5 database evidence")

try {
    Add-Assertion "coordinator_restart" {
        Assert-Uuid $m4.coordinatorExecution "Coordinator recovery Execution"
        if ($m4.activeTerminalLeases -ne "0") { throw "Coordinator recovery left terminal leases." }
        "execution=$($m4.coordinatorExecution)`nactiveTerminalLeases=$($m4.activeTerminalLeases)"
    }
    Add-Assertion "worker_kill" {
        Assert-Uuid $m4.workerExecution "Worker recovery Execution"
        if ($m4.activeTerminalLeases -ne "0") { throw "Worker recovery left terminal leases." }
        "execution=$($m4.workerExecution)`nactiveTerminalLeases=$($m4.activeTerminalLeases)"
    }
    Add-Assertion "redis_outage" {
        Assert-Uuid $m4.redisExecution "Redis recovery Execution"
        if ($m4.runtimeOutboxPending -ne "0") { throw "Runtime Outbox did not converge after Redis recovery." }
        "execution=$($m4.redisExecution)`nruntimeOutboxPending=$($m4.runtimeOutboxPending)"
    }
    Add-Assertion "clickhouse_outage" {
        Assert-Uuid $m4.clickHouseExecution "ClickHouse recovery Execution"
        if ($m4.traceOutboxPending -ne "0" -or $m5.clickHouseTraceOutbox -ne "0") { throw "Trace Outbox did not converge after ClickHouse recovery." }
        "m4Execution=$($m4.clickHouseExecution)`nm5Execution=$($m5.clickHouseExecution)`ntraceOutboxPending=0"
    }
    Add-Assertion "minio_latency" {
        $pod = (kubectl -n $Namespace get pod -l app.kubernetes.io/name=minio -o jsonpath='{.items[0].metadata.name}').Trim()
        if (-not $pod) { throw "MinIO Pod was not found." }
        # MinIO is PID 1 in its container and ignores the default stop signal.
        # Removing its StatefulSet endpoint gives the probe a real dependency outage
        # while keeping the recovery path deterministic in Kubernetes.
        kubectl -n $Namespace scale statefulset/minio --replicas=0 | Out-Null
        kubectl -n $Namespace wait --for=delete "pod/$pod" --timeout=120s | Out-Null
        $script:minioScaledDown = $true
        $failedPhase = Invoke-Probe "m7-minio-latency-$PID" 2
        if ($failedPhase -ne "Failed") { throw "MinIO latency probe did not time out; phase was $failedPhase." }
        kubectl -n $Namespace scale statefulset/minio --replicas=1 | Out-Null
        kubectl -n $Namespace rollout status statefulset/minio --timeout=180s | Out-Null
        $script:minioScaledDown = $false
        $healthyPhase = Invoke-Probe "m7-minio-recovery-$PID" 10
        if ($healthyPhase -ne "Succeeded") { throw "MinIO did not recover; phase was $healthyPhase." }
        "pod=$pod`nlatencyProbe=$failedPhase`nrecoveryProbe=$healthyPhase"
    }
    Add-Assertion "opensandbox_timeout" {
        if ($m5.ttlError -ne "SANDBOX_TTL_EXPIRED" -or $m5.ttlActiveLeases -ne "0") {
            throw "Sandbox TTL did not terminate cleanly."
        }
        $health = Invoke-RestMethod -TimeoutSec 5 -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/health"
        if ($health.status -ne "healthy") { throw "OpenSandbox is not healthy after timeout recovery." }
        "execution=$($m5.ttlExecution)`nerror=$($m5.ttlError)`nactiveLeases=$($m5.ttlActiveLeases)`nhealth=$($health.status)"
    }
    Add-Assertion "opensandbox_residual" {
        $oracle = Get-Content -Raw -LiteralPath (Resolve-RequiredFile (Join-Path $e2e "m5-opensandbox-go-oracle.json") "OpenSandbox oracle evidence") | ConvertFrom-Json -Depth 20
        $cleanup = Read-KeyValues (Resolve-RequiredFile (Join-Path $e2e "m5-sandbox-cleanup.txt") "OpenSandbox cleanup evidence")
        if (-not $oracle.oracle.terminated -or $cleanup.remaining -ne "0" -or $m5.unrevokedCredentialHandles -ne "0") {
            throw "OpenSandbox residual resources or Credential Handles remain."
        }
        "sandboxId=$($oracle.oracle.sandboxId)`nterminated=$($oracle.oracle.terminated)`nremaining=$($cleanup.remaining)`nunrevokedCredentialHandles=$($m5.unrevokedCredentialHandles)"
    }
    Add-Assertion "vault_outage" {
        $transcript = & cargo test -p agentx-infrastructure credential::tests::external_provider_failure_does_not_fall_back_to_local_ciphertext -- --exact 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Vault fail-closed test failed: $($transcript -join [Environment]::NewLine)" }
        $transcript
    }
    Add-Assertion "sse_reconnect" {
        $junit = Resolve-RequiredFile (Join-Path $e2e "m7-business-closure/junit.xml") "M7 JUnit"
        [xml]$document = Get-Content -Raw -LiteralPath $junit
        $suites = @($document.SelectNodes('//testsuite[not(testsuite)]'))
        $tests = ($suites | Measure-Object -Property tests -Sum).Sum
        $failuresCount = ($suites | Measure-Object -Property failures -Sum).Sum
        if ([int]$tests -lt 1 -or [int]$failuresCount -ne 0) { throw "M7 SSE reconnect scenario did not pass." }
        "junit=$junit`ntests=$tests`nfailures=$failuresCount"
    }
    Add-Assertion "projector_duplicate_out_of_order" {
        $m7 = Read-KeyValues (Resolve-RequiredFile (Join-Path $e2e "m7-database-evidence.txt") "M7 database evidence")
        if ($m7.runtimeEventsWithoutProjection -ne "0") { throw "Runtime Events remain without Projector receipts." }
        $transcript = & cargo test -p agentx-infrastructure runtime_projector::tests -- --nocapture 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Projector idempotency tests failed: $($transcript -join [Environment]::NewLine)" }
        @($transcript) + "runtimeEventsWithoutProjection=$($m7.runtimeEventsWithoutProjection)"
    }
}
finally {
    if ($minioScaledDown) {
        kubectl -n $Namespace scale statefulset/minio --replicas=1 2>$null | Out-Null
        kubectl -n $Namespace rollout status statefulset/minio --timeout=180s 2>$null | Out-Null
    }
    kubectl -n $Namespace delete pod "m7-minio-latency-$PID" "m7-minio-recovery-$PID" --ignore-not-found --wait=true 2>$null | Out-Null
    $evidence = [ordered]@{
        schemaVersion = "agentx.io/m7-operational-evidence/v1"
        evidenceType = "failure"
        status = $(if ($failures.Count -eq 0) { "passed" } else { "failed" })
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @($assertions)
    }
    $path = Join-Path $output "failure-evidence.json"
    $evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/m7-operational-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Failure evidence is invalid." }
    Write-Output $path
}
if ($failures.Count -gt 0) { throw "M7 failure matrix failed: $($failures -join '; ')" }
