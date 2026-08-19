param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [switch]$BuildImages,
    [switch]$SkipLocalGates,
    [switch]$SkipControlUi,
    [ValidateRange(30, 120)][int]$StabilityMinutes = 30,
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OpenSandboxApiKey = "agentx-local-opensandbox-key"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$safeRunId = ($RunId.ToLowerInvariant() -replace '[^a-z0-9-]', '-').Trim('-')
if (-not $safeRunId) { throw "RunId must contain a DNS-label character." }
if ($safeRunId.Length -gt 24) { $safeRunId = $safeRunId.Substring(0, 24).TrimEnd('-') }
$namespaces = [ordered]@{
    control = "agentx-v2-08-control-$safeRunId"
    runtime = "agentx-v2-08-runtime-$safeRunId"
    observability = "agentx-v2-08-runtime-$safeRunId"
    dependencies = "agentx-v2-08-deps-$safeRunId"
}
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-08/08a"
$profilePath = Join-Path $artifactDirectory "v2-full-local.json"
$summaryPath = Join-Path $artifactDirectory "summary.json"
$runtimeContextPath = Join-Path ([IO.Path]::GetTempPath()) "agentx-v2-08-$safeRunId-context.json"
$timeline = [Collections.Generic.List[string]]::new()
$scenarios = [Collections.Generic.List[object]]::new()
$developmentReplicas = [Collections.Generic.List[object]]::new()
$forwards = [Collections.Generic.List[Diagnostics.Process]]::new()
$openSandboxProcess = $null
$openSandboxOwned = $false
$completed = $false
$failureMessage = $null
$summary = $null
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null

function Add-Timeline([string]$Message) { $timeline.Add("$([DateTimeOffset]::UtcNow.ToString('O')) $Message") }
function Complete-Scenario([string]$Name, [string[]]$Evidence) {
    $scenarios.Add([ordered]@{ name = $Name; status = "passed"; evidence = $Evidence })
    Add-Timeline "$Name passed"
}
function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "V2-08A requires $Name." }
}
function Invoke-Kubectl([string[]]$Arguments) {
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $output = & kubectl @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) {
            $safe = (($output -join "`n") -replace '(?i)(password|token|authorization|api-key)=[^\s]+', '$1=<redacted>')
            throw "kubectl $($Arguments -join ' ') failed: $safe"
        }
        return @($output)
    }
    finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
}
function Wait-Deployment([string]$Namespace, [string]$Name, [int]$Seconds = 600) {
    Invoke-Kubectl @("-n", $Namespace, "rollout", "status", "deployment/$Name", "--timeout=${Seconds}s") | Out-Null
}
function Wait-StatefulSet([string]$Namespace, [string]$Name, [int]$Seconds = 600) {
    Invoke-Kubectl @("-n", $Namespace, "rollout", "status", "statefulset/$Name", "--timeout=${Seconds}s") | Out-Null
}
function Wait-TcpPort([int]$Port, [int]$Seconds = 60) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $client = [Net.Sockets.TcpClient]::new()
        try { $client.Connect("127.0.0.1", $Port); return } catch { Start-Sleep -Milliseconds 250 } finally { $client.Dispose() }
    } while ((Get-Date) -lt $deadline)
    throw "Port $Port did not become ready."
}
function Start-Forward([string]$Namespace, [string]$Resource, [int]$LocalPort, [int]$RemotePort) {
    $stdout = Join-Path $artifactDirectory "port-forward-$LocalPort.log"
    $stderr = Join-Path $artifactDirectory "port-forward-$LocalPort.stderr.log"
    $process = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", $Resource, "${LocalPort}:${RemotePort}") `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $forwards.Add($process)
    Wait-TcpPort $LocalPort
}
function Stop-Forwards {
    foreach ($process in $forwards) {
        if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    $forwards.Clear()
}
function Save-RedactedText([string]$Path, [object[]]$Content) {
    $text = ($Content -join "`n")
    $text = $text -replace '(?i)Bearer\s+[A-Za-z0-9._~-]+', 'Bearer <redacted>'
    $text = $text -replace '(?i)(password|token|authorization|api[-_]?key|secret)(\s*[:=]\s*)[^\s,;]+', '$1$2<redacted>'
    $text = $text -replace '(?i)(MYSQL_PWD|[A-Z0-9_]*_PASSWORD)(=)[^\s"'';,]+', '$1$2<redacted>'
    $text = $text -replace '(?i)(mysql(?:\.exe)?\s+[^\r\n]*?\s)-p(?:assword=)?[^\s"'';,]+', '$1-p<redacted>'
    $text | Set-Content -LiteralPath $Path -Encoding utf8NoBOM
}
function Capture-FailureEvidence {
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        # Evidence collection is best-effort: an initializing or terminating Pod
        # commonly makes one kubectl subcommand fail and must not hide the actual
        # product/deployment failure or prevent the remaining snapshots.
        $PSNativeCommandUseErrorActionPreference = $false
        $directory = Join-Path $artifactDirectory "failure"
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        foreach ($plane in @("control", "runtime", "dependencies")) {
            $namespace = [string]$namespaces[$plane]
            if (-not (& kubectl get namespace $namespace --ignore-not-found -o name 2>$null)) { continue }
            Save-RedactedText (Join-Path $directory "$plane-workloads.log") @(& kubectl -n $namespace get pods,deployments,statefulsets,jobs -o wide 2>&1)
            Save-RedactedText (Join-Path $directory "$plane-events.log") @(& kubectl -n $namespace get events --sort-by=.lastTimestamp 2>&1)
            $pods = @(& kubectl -n $namespace get pods -o name 2>$null)
            foreach ($pod in $pods) {
                $podName = ($pod -replace '^pod/', '')
                Save-RedactedText (Join-Path $directory "$plane-$podName.log") @(& kubectl -n $namespace logs $pod --all-containers=true --prefix=true --tail=1000 2>&1)
            }
        }
        $runtimeSnapshots = [Collections.Generic.List[string]]::new()
        $runtimeQueries = [ordered]@{
            executions = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(tenant_id),BIN_TO_UUID(workflow_id),BIN_TO_UUID(application_id),status,state_version,error_json,created_at FROM workflow_executions ORDER BY created_at DESC LIMIT 20;"
            users = "SELECT BIN_TO_UUID(user_id),token_version,status,tenant_query_enabled,admission_epoch FROM runtime_user_admission ORDER BY tenant_id,user_id LIMIT 20;"
            workflows = "SELECT BIN_TO_UUID(user_id),BIN_TO_UUID(workflow_id),grant_version,status,admission_epoch FROM runtime_user_workflow_grants ORDER BY tenant_id,user_id,workflow_id LIMIT 20;"
            commands = "SELECT status,COUNT(*) FROM runtime_commands GROUP BY status;"
            attempts = "SELECT status,COUNT(*) FROM node_attempts GROUP BY status;"
            recent_nodes = "SELECT BIN_TO_UUID(n.execution_id),BIN_TO_UUID(n.id),n.node_key,n.capability,n.status,n.started_at,n.ended_at FROM node_executions n JOIN workflow_executions e ON e.id=n.execution_id ORDER BY e.created_at DESC,n.created_at DESC LIMIT 40;"
            recent_attempts = "SELECT BIN_TO_UUID(a.execution_id),BIN_TO_UUID(a.node_execution_id),BIN_TO_UUID(a.id),a.capability,a.status,a.fencing_token,a.locked_until,a.heartbeat_at,a.started_at,a.ended_at,a.error_code FROM node_attempts a JOIN workflow_executions e ON e.id=a.execution_id ORDER BY e.created_at DESC,a.created_at DESC LIMIT 40;"
            recent_leases = "SELECT BIN_TO_UUID(a.execution_id),BIN_TO_UUID(l.node_attempt_id),BIN_TO_UUID(l.worker_id),l.fencing_token,l.heartbeat_at,l.expires_at,l.released_at FROM worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id JOIN workflow_executions e ON e.id=a.execution_id ORDER BY e.created_at DESC,l.acquired_at DESC LIMIT 40;"
            recent_receipts = "SELECT BIN_TO_UUID(a.execution_id),BIN_TO_UUID(r.attempt_id),r.status,r.created_at FROM worker_result_receipts r JOIN node_attempts a ON a.id=r.attempt_id JOIN workflow_executions e ON e.id=a.execution_id ORDER BY e.created_at DESC,r.created_at DESC LIMIT 40;"
        }
        foreach ($query in $runtimeQueries.GetEnumerator()) {
            $runtimeSnapshots.Add("## $($query.Key)")
            try { $runtimeSnapshots.AddRange([string[]]@(Invoke-RuntimeSql $query.Value)) }
            catch { $runtimeSnapshots.Add("ERROR: $($_.Exception.Message)") }
        }
        Save-RedactedText (Join-Path $directory "runtime-state.tsv") $runtimeSnapshots
        try {
            Save-RedactedText (Join-Path $directory "control-outbox.tsv") @(Invoke-ControlSql "SELECT event_type,aggregate_type,aggregate_id,status,attempt_count,last_error,occurred_at FROM outbox WHERE aggregate_type IN ('runtime_user_admission','workflow_admission') ORDER BY occurred_at DESC LIMIT 40;")
        } catch { Save-RedactedText (Join-Path $directory "control-outbox-error.log") @($_.Exception.Message) }
    }
    finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
}
function Record-And-StopDevelopment {
    foreach ($namespace in @("agentx", "agentx-v2-control", "agentx-v2-runtime")) {
        $json = (& kubectl -n $namespace get deployment -o json 2>$null) -join "`n"
        if ($LASTEXITCODE -ne 0 -or -not $json) { continue }
        foreach ($deployment in @(($json | ConvertFrom-Json).items)) {
            $developmentReplicas.Add(@{ namespace = $namespace; name = [string]$deployment.metadata.name; replicas = [int]$deployment.spec.replicas })
            Invoke-Kubectl @("-n", $namespace, "scale", "deployment/$($deployment.metadata.name)", "--replicas=0") | Out-Null
        }
    }
}
function Restore-Development {
    foreach ($entry in $developmentReplicas) {
        & kubectl -n $entry.namespace scale "deployment/$($entry.name)" "--replicas=$($entry.replicas)" 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Failed to restore $($entry.namespace)/$($entry.name)." }
    }
}
function Remove-OwnedMetricsClusterResources {
    $resources = @(
        @{ apiService = "v1beta1.external.metrics.k8s.io"; clusterRole = "agentx-v2-prometheus-adapter"; clusterRoleBindings = @("agentx-v2-prometheus-adapter", "agentx-v2-prometheus-adapter-auth-delegator"); roleBinding = "agentx-v2-prometheus-adapter-auth-reader" },
        @{ apiService = "v1beta1.metrics.k8s.io"; clusterRole = "agentx-v2-metrics-server"; clusterRoleBindings = @("agentx-v2-metrics-server", "agentx-v2-metrics-server-auth-delegator"); roleBinding = "agentx-v2-metrics-server-auth-reader" }
    )
    foreach ($resource in $resources) {
        $raw = (& kubectl get apiservice $resource.apiService --ignore-not-found -o json 2>$null) -join "`n"
        if (-not $raw) { continue }
        $owner = (($raw | ConvertFrom-Json).metadata.annotations.'agentx.io/metrics-owner')
        if ($owner -ne $namespaces.dependencies) { continue }
        Invoke-Kubectl @("delete", "apiservice", $resource.apiService, "--ignore-not-found") | Out-Null
        Invoke-Kubectl @("delete", "clusterrole", $resource.clusterRole, "--ignore-not-found") | Out-Null
        Invoke-Kubectl (@("delete", "clusterrolebinding") + $resource.clusterRoleBindings + @("--ignore-not-found")) | Out-Null
        Invoke-Kubectl @("-n", "kube-system", "delete", "rolebinding", $resource.roleBinding, "--ignore-not-found") | Out-Null
    }
}
function Assert-OpenSandboxReady {
    $health = Invoke-RestMethod -TimeoutSec 5 -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/health"
    if ([string]$health.status -ne "healthy") { throw "OpenSandbox health response is not healthy." }
    $headers = @{ "OPEN-SANDBOX-API-KEY" = $OpenSandboxApiKey }
    $items = Invoke-RestMethod -TimeoutSec 10 -Headers $headers `
        -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/v1/sandboxes?pageSize=100"
    if ($null -eq $items.items) { throw "OpenSandbox lifecycle response has no items array." }
    $sandboxId = $null
    try {
        $probe = Invoke-RestMethod -TimeoutSec 60 -Method Post -Headers ($headers + @{ "Idempotency-Key" = "v2-08-preflight-$RunId" }) `
            -ContentType "application/json" -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/v1/sandboxes" -Body (@{
                image = @{ uri = "opensandbox/code-interpreter@sha256:64cd01f03f54ba347d1a1310dcbc18ac5cb17d01714e23b4ea4b840fbb0d6623" }
                timeout = 90
                resourceLimits = @{ cpu = "250m"; memory = "268435456"; "ephemeral-storage" = "536870912"; pids = "128" }
                entrypoint = @("tail", "-f", "/dev/null")
                metadata = @{ agentxPreflight = $RunId }
                networkPolicy = @{ defaultAction = "deny"; egress = @() }
                secureAccess = $false
            } | ConvertTo-Json -Depth 10 -Compress)
        $sandboxId = [string]$(if ($probe.id) { $probe.id } else { $probe.sandboxId })
        if ([string]::IsNullOrWhiteSpace($sandboxId)) { throw "OpenSandbox create response has no sandbox id." }
        if ([string]$probe.status.state -ne "Running") { throw "OpenSandbox preflight Sandbox is not running." }
    }
    finally {
        if (-not [string]::IsNullOrWhiteSpace($sandboxId)) {
            Invoke-RestMethod -TimeoutSec 30 -Method Delete -Headers $headers `
                -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/v1/sandboxes/$sandboxId" | Out-Null
        }
    }
}
function Start-OpenSandbox {
    try { Assert-OpenSandboxReady; Add-Timeline "Using existing OpenSandbox"; return } catch { }
    $uri = [Uri]$OpenSandboxEndpoint
    if ($uri.Host -notin @("127.0.0.1", "localhost", "::1") -or $uri.Port -ne 18080) { throw "Configured OpenSandbox is unavailable." }
    $executable = Join-Path $root ".local/opensandbox-venv/Scripts/opensandbox-server.exe"
    $config = Join-Path $root ".local/opensandbox.toml"
    if (-not (Test-Path -LiteralPath $executable) -or -not (Test-Path -LiteralPath $config)) { throw "Run scripts/opensandbox-contract.ps1 before V2-08A." }
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $uri.Port -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)"
        if ($null -eq $process -or [string]$process.CommandLine -notlike "*$config*") {
            throw "OpenSandbox port $($uri.Port) is occupied by a process not managed by this workspace."
        }
        Stop-Process -Id $listener.OwningProcess -Force
    }
    if ($listeners.Count -gt 0) { Start-Sleep -Milliseconds 500 }
    $script:openSandboxProcess = Start-Process $executable -ArgumentList @("--config", $config) -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $artifactDirectory "opensandbox.stdout.log") `
        -RedirectStandardError (Join-Path $artifactDirectory "opensandbox.stderr.log")
    $script:openSandboxOwned = $true
    $deadline = (Get-Date).AddMinutes(2)
    do {
        if ($openSandboxProcess.HasExited) { throw "OpenSandbox exited during startup." }
        try { Assert-OpenSandboxReady; Add-Timeline "OpenSandbox started"; return } catch { Start-Sleep -Seconds 1 }
    } while ((Get-Date) -lt $deadline)
    throw "OpenSandbox did not become ready."
}
function Install-OpenSandboxEgress {
    $hostName = ([Uri]$profile.components.sandbox.endpoint).Host
    $resolved = Invoke-Kubectl @("-n", $namespaces.runtime, "exec", "deployment/sandbox-manager", "--", "getent", "ahostsv4", $hostName)
    $addresses = @($resolved | ForEach-Object { ($_ -split '\s+', 2)[0] } | Where-Object { $_ -match '^\d+\.\d+\.\d+\.\d+$' } | Sort-Object -Unique)
    if ($addresses.Count -eq 0) { throw "Could not resolve OpenSandbox host $hostName from Runtime." }
    $blocks = ($addresses | ForEach-Object { "        - ipBlock: { cidr: $_/32 }" }) -join "`n"
    @"
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: v2-08-opensandbox-egress, namespace: $($namespaces.runtime) }
spec:
  podSelector: { matchLabels: { app.kubernetes.io/name: sandbox-manager } }
  policyTypes: [Egress]
  egress:
    - to:
$blocks
      ports: [{ protocol: TCP, port: 1024, endPort: 65535 }]
"@ | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "OpenSandbox NetworkPolicy failed." }
}
function Invoke-Playwright([string]$Suite, [string[]]$Tests) {
    $env:AGENTX_E2E_STAGE = "v2-08"
    $env:AGENTX_E2E_RUN_ID = $RunId
    $env:AGENTX_E2E_SUITE = $Suite
    $env:AGENTX_E2E_BASE_URL = "http://127.0.0.1:18081"
    $env:AGENTX_E2E_RUNTIME_URL = "http://127.0.0.1:18082"
    & pnpm --filter @agentx/e2e exec playwright test @Tests
    if ($LASTEXITCODE -ne 0) { throw "Playwright suite $Suite failed." }
    $junit = Join-Path $root "apps/e2e/test-results/v2-08/$RunId/$Suite/junit.xml"
    if (-not (Test-Path -LiteralPath $junit)) { throw "Playwright suite $Suite produced no JUnit evidence." }
    [xml]$report = Get-Content -Raw -LiteralPath $junit
    $failures = [int]$report.testsuites.failures + [int]$report.testsuites.errors
    $skipped = [int]$report.testsuites.skipped
    if ($failures -ne 0 -or $skipped -ne 0) { throw "Playwright suite $Suite has failures/errors/skips." }
    Copy-Item -LiteralPath $junit -Destination (Join-Path $artifactDirectory "$Suite-junit.xml") -Force
    Complete-Scenario "playwright-$Suite" @("$Suite-junit.xml")
}
function Invoke-Gateway([string]$Method, [string]$Path, [string]$ApiKey, [string]$IdempotencyKey = "", $Body = $null) {
    $headers = @{ Authorization = "Bearer $ApiKey" }
    if ($IdempotencyKey) { $headers["Idempotency-Key"] = $IdempotencyKey }
    $parameters = @{ Uri = "http://127.0.0.1:18082/gateway/v1$Path"; Method = $Method; Headers = $headers; SkipHttpErrorCheck = $true }
    if ($null -ne $Body) { $parameters.ContentType = "application/json"; $parameters.Body = ($Body | ConvertTo-Json -Depth 12 -Compress) }
    return Invoke-WebRequest @parameters
}
function Wait-Invocation([string]$Id, [string]$ApiKey, [int]$Seconds = 180) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $response = Invoke-Gateway "GET" "/invocations/$Id" $ApiKey
        if ([int]$response.StatusCode -eq 200) {
            $value = $response.Content | ConvertFrom-Json
            if ($value.status -in @("completed", "failed", "cancelled")) { return $value }
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Invocation $Id did not reach terminal state."
}
function Start-TestInvocation([string]$Key, [string]$Message = "v2-08") {
    $response = Invoke-Gateway "POST" "/applications/$($runtimeContext.applicationSlug)/invocations" $runtimeContext.apiKey $Key `
        @{ input = @{ message = $Message }; responseMode = "async" }
    if ([int]$response.StatusCode -ne 202) { throw "Invocation was rejected: $($response.StatusCode) $($response.Content)" }
    return $response.Content | ConvertFrom-Json
}
function Get-RuntimeSecret([string]$Key) {
    $encoded = (Invoke-Kubectl @("-n", $namespaces.runtime, "get", "secret", "agentx-runtime-secrets", "-o", "jsonpath={.data.$Key}")) -join ""
    return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
}
function Invoke-RuntimeSql([string]$Sql) {
    $password = Get-RuntimeSecret "AGENTX_RUNTIME_MYSQL_PASSWORD"
    return Invoke-Kubectl @("-n", $namespaces.runtime, "exec", "statefulset/runtime-mysql", "--", "env", "MYSQL_PWD=$password", "mysql", "-N", "-B", "-uruntime_app", "agentx_runtime", "-e", $Sql)
}
function Invoke-ControlSql([string]$Sql) {
    $encoded = (Invoke-Kubectl @("-n", $namespaces.control, "get", "secret", "agentx-control-secrets", "-o", "jsonpath={.data.AGENTX_CONTROL_MYSQL_PASSWORD}")) -join ""
    $password = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
    return Invoke-Kubectl @("-n", $namespaces.control, "exec", "statefulset/control-mysql", "--", "env", "MYSQL_PWD=$password", "mysql", "-N", "-B", "-ucontrol_app", "agentx_control", "-e", $Sql)
}
function Assert-V2MigrationHistory {
    $control = ((Invoke-ControlSql "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE success=1;") -join "").Trim()
    $runtime = ((Invoke-RuntimeSql "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE success=1;") -join "").Trim()
    if ($control -ne "1,2,3,4,5,6,7") {
        throw "Control Migration history is incomplete ($control); rebuild agentx-migrate before V2-08A."
    }
    if ($runtime -ne "1,2,3,4,5,6,7,8") {
        throw "Runtime Migration history is incomplete ($runtime); rebuild agentx-migrate before V2-08A."
    }
    $observability = (Invoke-Kubectl @(
        "-n", $namespaces.observability, "exec", "statefulset/clickhouse", "--", "sh", "-ec",
        'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --database "$CLICKHOUSE_DB" --query "SELECT count()*100+sum(version) FROM observability_schema_migrations"'
    ) -join "").Trim()
    if ($observability -ne "306") {
        throw "Observability Migration history is incomplete ($observability); rebuild agentx-migrate before V2-08A."
    }
    Complete-Scenario "migration-history-complete" @("control=1..7", "runtime=1..8", "observability=1..3")
}
function Invoke-FailureMatrix {
    foreach ($deployment in @("platform-control", "web-console")) { Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/$deployment", "--replicas=0") | Out-Null }
    try {
        $value = Wait-Invocation (Start-TestInvocation "v2-08-control-offline-$safeRunId" "control-offline").id $runtimeContext.apiKey
        if ($value.status -ne "completed") { throw "Runtime failed while Control was offline." }
    }
    finally {
        foreach ($deployment in @("platform-control", "web-console")) { Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/$deployment", "--replicas=2") | Out-Null; Wait-Deployment $namespaces.control $deployment }
    }
    Complete-Scenario "control-offline-runtime" @("timeline.json")

    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=0") | Out-Null
    try {
        Start-Sleep -Seconds 5
        $response = Invoke-Gateway "POST" "/applications/$($runtimeContext.applicationSlug)/invocations" $runtimeContext.apiKey "v2-08-mysql-down-$safeRunId" @{ input = @{ message = "mysql-down" }; responseMode = "async" }
        if ([int]$response.StatusCode -ne 503) { throw "Runtime MySQL outage returned $($response.StatusCode), expected 503." }
    }
    finally { Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=1") | Out-Null; Wait-StatefulSet $namespaces.runtime "runtime-mysql" }
    Complete-Scenario "runtime-mysql-fail-closed" @("timeline.json")

    Invoke-Kubectl @("-n", $namespaces.observability, "scale", "statefulset/clickhouse", "--replicas=0") | Out-Null
    try {
        $value = Wait-Invocation (Start-TestInvocation "v2-08-clickhouse-down-$safeRunId" "clickhouse-down").id $runtimeContext.apiKey
        if ($value.status -ne "completed") { throw "ClickHouse outage changed Runtime terminal state." }
    }
    finally { Invoke-Kubectl @("-n", $namespaces.observability, "scale", "statefulset/clickhouse", "--replicas=1") | Out-Null; Wait-StatefulSet $namespaces.observability "clickhouse" }
    Complete-Scenario "clickhouse-independent-terminal" @("timeline.json")

    Invoke-Kubectl @("-n", $namespaces.runtime, "exec", "statefulset/runtime-redis", "--", "sh", "-c", 'redis-cli -a "$REDIS_PASSWORD" FLUSHALL') | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "delete", "pod/runtime-redis-0", "--wait=true") | Out-Null
    Wait-StatefulSet $namespaces.runtime "runtime-redis"
    $value = Wait-Invocation (Start-TestInvocation "v2-08-redis-rebuild-$safeRunId" "redis-rebuild").id $runtimeContext.apiKey
    if ($value.status -ne "completed") { throw "Runtime Redis rebuild did not converge." }
    Complete-Scenario "runtime-redis-empty-rebuild" @("timeline.json")
}
function Invoke-PerformanceRegression {
    $before = @{}
    $pods = (Invoke-Kubectl @("-n", $namespaces.runtime, "get", "pods", "-o", "json") | Out-String | ConvertFrom-Json).items
    foreach ($pod in $pods) { $before[[string]$pod.metadata.uid] = [int]$pod.status.containerStatuses[0].restartCount }
    $latencies = [Collections.Generic.List[double]]::new()
    $executionIds = [Collections.Generic.List[string]]::new()
    $jobs = 1..50 | ForEach-Object {
        $index = $_
        Start-ThreadJob -ScriptBlock {
            param($Slug, $ApiKey, $Index, $Run)
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $response = Invoke-WebRequest "http://127.0.0.1:18082/gateway/v1/applications/$Slug/invocations" -Method Post `
                -Headers @{ Authorization = "Bearer $ApiKey"; "Idempotency-Key" = "v2-08-perf-$Run-$Index" } `
                -ContentType "application/json" -Body (@{ input = @{ message = "perf-$Index" }; responseMode = "async" } | ConvertTo-Json -Compress)
            $watch.Stop()
            [pscustomobject]@{ status = [int]$response.StatusCode; latencyMs = $watch.Elapsed.TotalMilliseconds; value = $response.Content }
        } -ArgumentList $runtimeContext.applicationSlug, $runtimeContext.apiKey, $index, $safeRunId
    }
    $results = $jobs | Receive-Job -Wait -AutoRemoveJob
    foreach ($result in $results) {
        if ($result.status -ne 202) { throw "Concurrent invocation returned $($result.status)." }
        $latencies.Add([double]$result.latencyMs)
        $value = $result.value | ConvertFrom-Json
        if ($value.executionId) { $executionIds.Add([string]$value.executionId) }
        if ((Wait-Invocation $value.id $runtimeContext.apiKey).status -ne "completed") { throw "Concurrent invocation failed." }
    }
    if ($executionIds.Count -ne 50) { throw "Concurrent run did not return 50 Execution IDs." }
    $ids = ($executionIds | ForEach-Object { "UUID_TO_BIN('$_')" }) -join ','
    $attempts = [int]((Invoke-RuntimeSql "SELECT COUNT(*) FROM node_attempts WHERE execution_id IN ($ids);") -join "")
    if ($attempts -lt 200) { throw "Expected at least 200 Attempts, observed $attempts." }
    $sorted = @($latencies | Sort-Object)
    $p95 = $sorted[[Math]::Min($sorted.Count - 1, [Math]::Ceiling($sorted.Count * 0.95) - 1)]
    if ($p95 -gt 2000) { throw "Local invocation acceptance p95 $p95 ms exceeds 2000 ms." }

    $sseInvocation = Start-TestInvocation "v2-08-sse-$safeRunId" "sse"
    $sseJobs = 1..100 | ForEach-Object {
        Start-ThreadJob -ScriptBlock {
            param($Id, $ApiKey)
            $response = Invoke-WebRequest "http://127.0.0.1:18082/gateway/v1/invocations/$Id/events" -Headers @{ Authorization = "Bearer $ApiKey" } -SkipHttpErrorCheck
            [int]$response.StatusCode
        } -ArgumentList $sseInvocation.id, $runtimeContext.apiKey
    }
    $sseStatuses = $sseJobs | Receive-Job -Wait -AutoRemoveJob
    if (@($sseStatuses | Where-Object { $_ -ne 200 }).Count -ne 0) { throw "One or more SSE connections failed." }
    Wait-Invocation $sseInvocation.id $runtimeContext.apiKey | Out-Null

    $deadline = (Get-Date).AddMinutes($StabilityMinutes)
    $iteration = 0
    while ((Get-Date) -lt $deadline) {
        $iteration++
        $value = Wait-Invocation (Start-TestInvocation "v2-08-stability-$safeRunId-$iteration" "stability").id $runtimeContext.apiKey
        if ($value.status -ne "completed") { throw "Stability invocation $iteration failed." }
        Start-Sleep -Seconds 4
    }
    $residual = (Invoke-RuntimeSql "SELECT (SELECT COUNT(*) FROM runtime_commands WHERE status IN ('pending','processing'))+(SELECT COUNT(*) FROM execution_outbox WHERE status IN ('pending','processing'))+(SELECT COUNT(*) FROM quota_reservations WHERE status='active')+(SELECT COUNT(*) FROM runtime_retention_holds WHERE released_at IS NULL AND (expires_at IS NULL OR expires_at>UTC_TIMESTAMP(6)));") -join ""
    if ([int]$residual -ne 0) { throw "Runtime has $residual active command/outbox/reservation/hold rows after regression." }
    $after = (Invoke-Kubectl @("-n", $namespaces.runtime, "get", "pods", "-o", "json") | Out-String | ConvertFrom-Json).items
    foreach ($pod in $after) {
        $uid = [string]$pod.metadata.uid
        if (-not $before.ContainsKey($uid) -or [int]$pod.status.containerStatuses[0].restartCount -ne $before[$uid]) { throw "Runtime Pod restarted during performance regression." }
    }
    [ordered]@{ concurrentExecutions = 50; attempts = $attempts; sseConnections = 100; stabilityMinutes = $StabilityMinutes; invocationAcceptanceP95Ms = [Math]::Round($p95, 2) } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $artifactDirectory "performance.json") -Encoding utf8NoBOM
    Complete-Scenario "local-performance-regression" @("performance.json")
}

foreach ($command in @("kubectl", "docker", "cargo", "pnpm", "Start-ThreadJob")) { Require-Command $command }
try {
    if (-not $SkipLocalGates) {
        & (Join-Path $PSScriptRoot "v2-08-api-disposition.ps1") -OutputPath (Join-Path $artifactDirectory "api-disposition.json") -FailOnMigrationRequired
        if ($LASTEXITCODE -ne 0) { throw "Platform API disposition gate failed." }
        cargo test -p platform-control
        if ($LASTEXITCODE -ne 0) { throw "Platform Control API-first tests failed." }
        cargo test -p agentx-v2-runtime
        if ($LASTEXITCODE -ne 0) { throw "Runtime tests failed." }
        cargo run --quiet -p agentx-boundary-check -- check .
        if ($LASTEXITCODE -ne 0) { throw "V2 boundary gate failed." }
    }
    Start-OpenSandbox
    Record-And-StopDevelopment
    $sourceProfilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
    $script:profile = Get-Content -Raw -LiteralPath $sourceProfilePath | ConvertFrom-Json
    $profile.environment = "local"
    $profile.namespaces.control = $namespaces.control
    $profile.namespaces.runtime = $namespaces.runtime
    $profile.namespaces.dependencies = $namespaces.dependencies
    $profile.ingress.controlHost = "control-$safeRunId.agentx.localhost"
    $profile.ingress.runtimeHost = "runtime-$safeRunId.agentx.localhost"
    $profile.components.sandbox.endpoint = ([UriBuilder]$OpenSandboxEndpoint).Uri.AbsoluteUri.Replace("127.0.0.1", "host.docker.internal").TrimEnd('/')
    foreach ($service in $profile.services.PSObject.Properties) { $service.Value.replicas = 2 }
    $profile | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $profilePath -Encoding utf8NoBOM

    $render = (& (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Render -Target All -ConfigFile $profilePath) -join "`n"
    $render | Set-Content -LiteralPath (Join-Path $artifactDirectory "render.yaml") -Encoding utf8NoBOM
    if ($render -match '(?i)platform-api|trigger-gateway|trace-writer|workflow-coordinator|agentx-runtime-rpc|/runtime/v1') { throw "V2-08 render still contains a V1 runtime entry." }
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Install -Target All -ConfigFile $profilePath -RunId "08-$safeRunId" -RecreateV2Data -BuildImages:$BuildImages
    if ($LASTEXITCODE -ne 0) { throw "V2-08 local deployment failed." }
    Assert-V2MigrationHistory
    if ($BuildImages) {
        & (Join-Path $PSScriptRoot "build-images.ps1") -Tag ([string]$profile.images.tag) -Namespace $namespaces.dependencies -Services @("echo-mcp", "echo-node", "lightrag") -SkipWeb
        if ($LASTEXITCODE -ne 0) { throw "V2-08 Provider image build failed." }
    }
    Invoke-Kubectl @("-n", $namespaces.dependencies, "apply", "-k", (Join-Path $root "deploy/k8s/v2/e2e/runtime-providers")) | Out-Null
    foreach ($entry in @(@("echo-mcp", "echo-mcp"), @("echo-node", "echo-node"), @("lightrag", "lightrag"))) {
        Invoke-Kubectl @("-n", $namespaces.dependencies, "set", "image", "deployment/$($entry[0])", "$($entry[0])=$($profile.images.registry)/$($entry[1]):$($profile.images.tag)") | Out-Null
    }
    foreach ($name in @("echo-mcp", "echo-node", "lightrag", "mem0-postgres", "mem0")) { Wait-Deployment $namespaces.dependencies $name }
    Install-OpenSandboxEgress
    # Route the public Runtime URL through the managed Ingress so the browser
    # exercises the production CORS boundary. Chromium maps only this ephemeral
    # run host to the loopback port-forward; no machine-wide hosts entry is used.
    $runtimeBrowserUrl = "http://$($profile.ingress.runtimeHost):18083"
    Invoke-Kubectl @("-n", $namespaces.control, "set", "env", "deployment/web-console", "AGENTX_RUNTIME_PUBLIC_BASE_URL=$runtimeBrowserUrl") | Out-Null
    foreach ($entry in @(
        @($namespaces.control, "platform-control"), @($namespaces.control, "web-console"),
        @($namespaces.runtime, "runtime-gateway"), @($namespaces.runtime, "workflow-runtime"),
        @($namespaces.runtime, "workflow-worker"), @($namespaces.runtime, "sandbox-manager"),
        @($namespaces.observability, "observability")
    )) { Wait-Deployment $entry[0] $entry[1] }
    $workloads = Invoke-Kubectl @("get", "deployment", "-A", "-l", "app.kubernetes.io/part-of=agentx", "-o", "json") | Out-String | ConvertFrom-Json
    $v1 = @($workloads.items | Where-Object { $_.metadata.namespace -in $namespaces.Values -and $_.metadata.name -match 'platform-api|trigger-gateway|trace-writer|workflow-coordinator' })
    if ($v1.Count -ne 0) { throw "Temporary V2-08 Namespaces contain V1 Deployments." }
    Complete-Scenario "empty-v2-three-plane-deployment" @("render.yaml")

    Start-Forward $namespaces.control "service/web-console" 18081 8080
    Start-Forward $namespaces.runtime "service/runtime-gateway-public" 18082 8080
    Start-Forward $namespaces.dependencies "service/$($profile.ingress.className)-08-$safeRunId-controller" 18083 80
    $corsOrigin = "http://127.0.0.1:18081"
    $corsProbe = Invoke-WebRequest -NoProxy -SkipHttpErrorCheck -Method Options `
        -Uri "http://127.0.0.1:18083/gateway/v1/applications/e2e-cors-probe/sessions" `
        -Headers @{ Host = [string]$profile.ingress.runtimeHost; Origin = $corsOrigin; "Access-Control-Request-Method" = "POST"; "Access-Control-Request-Headers" = "authorization,content-type,idempotency-key" }
    if ([string]$corsProbe.Headers["Access-Control-Allow-Origin"] -ne $corsOrigin) {
        throw "RunId ingress CORS preflight did not allow $corsOrigin."
    }
    Complete-Scenario "runid-browser-cors" @("runtime-config=$runtimeBrowserUrl", "allow-origin=$corsOrigin", "ingress-host=$($profile.ingress.runtimeHost)")
    $env:AGENTX_E2E_HOST_RESOLVER_RULES = "MAP $($profile.ingress.runtimeHost) 127.0.0.1"
    $env:AGENTX_E2E_ECHO_BASE_URL = "http://echo-mcp.$($namespaces.dependencies).svc.cluster.local:8090"
    $env:AGENTX_E2E_REMOTE_NODE_ENDPOINT = "http://echo-node.$($namespaces.dependencies).svc.cluster.local:8080"
    $env:AGENTX_E2E_LIGHTRAG_BASE_URL = "http://lightrag.$($namespaces.dependencies).svc.cluster.local:9621"
    $env:AGENTX_E2E_MEM0_BASE_URL = "http://mem0.$($namespaces.dependencies).svc.cluster.local:8000"
    $env:AGENTX_V2_08_CONTEXT_OUTPUT = $runtimeContextPath
    Invoke-Playwright "api-first" @("tests/v2-08-api-first.spec.ts")
    $script:runtimeContext = Get-Content -Raw -LiteralPath $runtimeContextPath | ConvertFrom-Json
    Invoke-Playwright "workflow4" @("tests/workflow4-closure.spec.ts", "--retries=1")
    if (-not $SkipControlUi) {
        Invoke-Playwright "control-ui" @("tests/m2.1-control-plane.spec.ts", "tests/resource-grant-requests.spec.ts")
    }
    $productTests = if ($SkipControlUi) {
        @("tests/m6-workflow-studio.spec.ts", "--grep", "M6 Studio creates")
    } else {
        @("tests/m6-workflow-studio.spec.ts", "tests/m7-business-closure.spec.ts", "tests/safe-deletion.spec.ts")
    }
    Invoke-Playwright "product-closure" $productTests
    Invoke-FailureMatrix
    Invoke-PerformanceRegression
    foreach ($target in @("Control", "Runtime", "Observability")) {
        & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Doctor -Target $target -ConfigFile $profilePath | Set-Content -LiteralPath (Join-Path $artifactDirectory "doctor-$($target.ToLowerInvariant()).log")
        if ($LASTEXITCODE -ne 0) { throw "$target Doctor failed." }
    }
    $summary = [ordered]@{
        schemaVersion = "agentx.io/v2-08a-evidence/v1"; status = "passed"; runId = $RunId
        namespaces = $namespaces; scenarios = $scenarios; timeline = $timeline
        deferred = @("V2S-006", "production TLS/PITR/RPO/RTO", "gVisor/Kata", "role-level isolation", "Cosign/attestation", "attack matrix")
        cleanup = $null
    }
    $completed = $true
}
catch {
    $failureMessage = $_.Exception.Message
    throw
}
finally {
    $errors = [Collections.Generic.List[string]]::new()
    Stop-Forwards
    if (-not $completed) {
        try { Capture-FailureEvidence } catch { $errors.Add("Failure evidence: $($_.Exception.Message)") }
    }
    Remove-Item -LiteralPath $runtimeContextPath -Force -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_V2_08_CONTEXT_OUTPUT -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_RUNTIME_URL -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_HOST_RESOLVER_RULES -ErrorAction SilentlyContinue
    try { Restore-Development } catch { $errors.Add($_.Exception.Message) }
    try { Remove-OwnedMetricsClusterResources } catch { $errors.Add($_.Exception.Message) }
    if (Test-Path -LiteralPath $profilePath -PathType Leaf) {
        try { & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -Target All -ConfigFile $profilePath -RunId "08-$safeRunId" | Out-Null } catch { $errors.Add("V2 uninstall: $($_.Exception.Message)") }
    }
    foreach ($namespace in @($namespaces.control, $namespaces.runtime, $namespaces.dependencies) | Select-Object -Unique) {
        & kubectl delete namespace $namespace --ignore-not-found --wait=false 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { $errors.Add("Failed to start deletion of $namespace") }
    }
    foreach ($namespace in @($namespaces.control, $namespaces.runtime, $namespaces.dependencies) | Select-Object -Unique) {
        & kubectl wait --for=delete "namespace/$namespace" --timeout=300s 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { $errors.Add("Timed out deleting $namespace") }
    }
    if ($openSandboxOwned -and $openSandboxProcess -and -not $openSandboxProcess.HasExited) { Stop-Process -Id $openSandboxProcess.Id -Force -ErrorAction SilentlyContinue }
    if ($completed) {
        $summary.cleanup = @{ status = if ($errors.Count -eq 0) { "passed" } else { "failed" }; errors = @($errors) }
        $summary.timeline = $timeline
        $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $summaryPath -Encoding utf8NoBOM
    } else {
        [ordered]@{
            schemaVersion = "agentx.io/v2-08a-evidence/v1"; status = "failed"; runId = $RunId
            namespaces = $namespaces; scenarios = $scenarios; timeline = $timeline
            failure = $failureMessage; cleanup = @{ status = if ($errors.Count -eq 0) { "passed" } else { "failed" }; errors = @($errors) }
        } | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $summaryPath -Encoding utf8NoBOM
    }
    if ($errors.Count -gt 0) { throw "V2-08A cleanup failed: $($errors -join '; ')" }
}
