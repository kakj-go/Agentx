param(
    [switch]$KeepNamespace,
    [switch]$KeepDevelopmentRunning,
    [switch]$SkipBuild,
    [switch]$Headed,
    [int]$Port = 18081,
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OpenSandboxApiKey = "agentx-local-opensandbox-key"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$namespace = "agentx-e2e"
$e2eRunId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$env:AGENTX_E2E_STAGE = "kubernetes"
$env:AGENTX_E2E_RUN_ID = $e2eRunId
$results = Join-Path $root "apps/e2e/test-results/kubernetes/$e2eRunId"
$forwardOut = Join-Path $results "port-forward.out.log"
$forwardError = Join-Path $results "port-forward.err.log"
$addonEvidence = Join-Path $results "m5-addons-contract.json"
$forward = $null
$developmentReplicas = @()
$e2eRedisScaledDown = $false
$e2eClickHouseScaledDown = $false
$sandboxCleanupError = $null
$openSandboxBefore = @()
$openSandboxHeaders = @{ "OPEN-SANDBOX-API-KEY" = $OpenSandboxApiKey }
$deploymentProfile = Join-Path ([IO.Path]::GetTempPath()) "agentx-e2e-deployment-$PID.json"
$clusterOpenSandbox = [UriBuilder]$OpenSandboxEndpoint
if ($clusterOpenSandbox.Host -in @("127.0.0.1", "localhost", "::1")) { $clusterOpenSandbox.Host = "host.docker.internal" }

function Get-OpenSandboxIds {
    $response = Invoke-RestMethod -TimeoutSec 10 -Headers $openSandboxHeaders -Uri "$OpenSandboxEndpoint/v1/sandboxes?pageSize=100"
    if ($null -eq $response.items) {
        throw "OpenSandbox Lifecycle response did not contain an items array."
    }
    return @($response.items | ForEach-Object { [string]$_.id })
}

function Assert-OpenSandboxReady {
    $health = Invoke-RestMethod -TimeoutSec 5 -Uri "$OpenSandboxEndpoint/health"
    if ([string]$health.status -ne "healthy") {
        throw "OpenSandbox health response was not healthy at $OpenSandboxEndpoint."
    }
    [void](Get-OpenSandboxIds)
}

function Clear-M5Sandboxes {
    $current = @(Get-OpenSandboxIds)
    $created = @($current | Where-Object { $openSandboxBefore -notcontains $_ })
    foreach ($sandboxId in $created) {
        Invoke-RestMethod -Method Delete -TimeoutSec 15 -Headers $openSandboxHeaders -Uri "$OpenSandboxEndpoint/v1/sandboxes/$sandboxId" -ErrorAction SilentlyContinue | Out-Null
    }
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        $remaining = @(Get-OpenSandboxIds | Where-Object { $openSandboxBefore -notcontains $_ })
        if ($remaining.Count -eq 0) {
            "created=$($created.Count)`nremaining=0" | Out-File -LiteralPath (Join-Path $results "m5-sandbox-cleanup.txt") -Encoding utf8
            return
        }
        Start-Sleep -Seconds 1
    }
    throw "M5 E2E left OpenSandbox instances after cleanup: $($remaining -join ', ')"
}

function Save-KubernetesLogs([string]$Resource, [string]$Destination) {
    try {
        kubectl -n $namespace logs $Resource --all-containers --tail=500 2>&1 | Out-File -LiteralPath $Destination -Encoding utf8
    }
    catch {
        $_ | Out-File -LiteralPath $Destination -Encoding utf8
    }
}

function Wait-TcpPort([int]$TargetPort) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $client.Connect("127.0.0.1", $TargetPort)
            return
        }
        catch {
            Start-Sleep -Seconds 1
        }
        finally {
            $client.Dispose()
        }
    }
    throw "Timed out waiting for local port $TargetPort."
}

function Invoke-Playwright([string]$Suite, [string[]]$Tests) {
    $arguments = @("--filter", "@agentx/e2e", "exec", "playwright", "test")
    if ($Headed) {
        $arguments += "--headed"
    }
    $arguments += $Tests
    $env:AGENTX_E2E_SUITE = $Suite
    try {
        & pnpm @arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Playwright failed for suite ${Suite}: $($Tests -join ', ')"
        }
    }
    finally {
        Remove-Item Env:AGENTX_E2E_SUITE -ErrorAction SilentlyContinue
    }
}

function Invoke-MySqlEvidenceQuery([string]$Query) {
    $value = @()
    $nativeErrorPreference = $PSNativeCommandUseErrorActionPreference
    $commandErrorPreference = $ErrorActionPreference
    $PSNativeCommandUseErrorActionPreference = $false
    $ErrorActionPreference = "Continue"
    try {
        for ($attempt = 0; $attempt -lt 5; $attempt++) {
            $value = $Query | kubectl -n $namespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"' 2>&1
            if ($LASTEXITCODE -eq 0) {
                return [string]($value | Select-Object -Last 1)
            }
            Start-Sleep -Seconds 1
        }
    }
    finally {
        $PSNativeCommandUseErrorActionPreference = $nativeErrorPreference
        $ErrorActionPreference = $commandErrorPreference
    }
    throw "MySQL evidence query failed after 5 attempts: $Query`n$($value -join [Environment]::NewLine)"
}

function Get-MySqlScalar([string]$Query) {
    return [long](Invoke-MySqlEvidenceQuery $Query)
}

function Assert-MySqlScalar([string]$Query, [long]$Expected, [string]$Evidence) {
    $actual = Get-MySqlScalar $Query
    if ($actual -ne $Expected) {
        throw "$Evidence. Expected $Expected, got $actual."
    }
}

function Wait-MySqlScalar([string]$Query, [long]$Expected, [string]$Evidence) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ((Get-MySqlScalar $Query) -eq $Expected) {
            return
        }
        Start-Sleep -Seconds 1
    }
    throw "$Evidence did not reach $Expected before timeout."
}

function Get-MySqlValue([string]$Query) {
    return Invoke-MySqlEvidenceQuery $Query
}

function Wait-MySqlValue([string]$Query, [string[]]$Expected, [string]$Evidence, [int]$Attempts = 120) {
    for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
        $actual = Get-MySqlValue $Query
        if ($Expected -contains $actual) {
            return $actual
        }
        Start-Sleep -Seconds 1
    }
    throw "$Evidence did not reach one of [$($Expected -join ', ')] before timeout. Last value: $actual"
}

function Suspend-DevelopmentNamespace {
    if ($KeepDevelopmentRunning -or -not (kubectl get namespace agentx --ignore-not-found -o name)) {
        return
    }
    $workloads = kubectl -n agentx get deployments,statefulsets -o json | ConvertFrom-Json
    foreach ($item in $workloads.items) {
        $replicas = if ($null -eq $item.spec.replicas) { 1 } else { [int]$item.spec.replicas }
        $kind = if ($item.kind -eq "Deployment") { "deployment" } else { "statefulset" }
        $script:developmentReplicas += [pscustomobject]@{ kind = $kind; name = $item.metadata.name; replicas = $replicas }
        kubectl -n agentx scale "$kind/$($item.metadata.name)" --replicas=0
    }
    $developmentReplicas | ConvertTo-Json | Out-File -LiteralPath (Join-Path $results "agentx-original-replicas.json") -Encoding utf8
}

function Restore-DevelopmentNamespace {
    if (-not (kubectl get namespace agentx --ignore-not-found -o name)) {
        return
    }
    foreach ($workload in $developmentReplicas) {
        $replicas = [int]$workload.replicas
        kubectl -n agentx scale "$($workload.kind)/$($workload.name)" --replicas=$replicas
    }
}

function Start-M4Execution([string]$WorkflowName, [string]$IdempotencyKey) {
    $escapedName = $WorkflowName.Replace("'", "''")
    $versionId = Get-MySqlValue "SELECT BIN_TO_UUID(v.id) FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE w.name='$escapedName' ORDER BY v.version_number DESC LIMIT 1"
    if (-not $versionId) {
        throw "Workflow version not found for $WorkflowName"
    }
    $headers = @{ Authorization = "Bearer $script:accessToken" }
    $body = @{ input = @{}; idempotencyKey = $IdempotencyKey } | ConvertTo-Json -Compress
    $response = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$Port/api/v1/workflow-versions/$versionId/executions" -Headers $headers -ContentType "application/json" -Body $body
    return [string]$response.executionId
}

function Wait-RemoteLease([string]$ExecutionId) {
    Wait-MySqlScalar "SELECT COUNT(*) FROM worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id JOIN node_executions n ON n.id=a.node_execution_id WHERE a.execution_id=UUID_TO_BIN('$ExecutionId') AND n.node_type='remote_action' AND l.released_at IS NULL" 1 "Remote node lease for $ExecutionId"
}

function Invoke-M4FaultSuite {
    $loginBody = @{ username = "admin"; password = "agentx-e2e-admin-password" } | ConvertTo-Json -Compress
    $login = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$Port/api/v1/auth/login" -ContentType "application/json" -Body $loginBody
    $script:accessToken = [string]$login.accessToken

    $workerExecution = Start-M4Execution "M4 Fault Fixture" "m4-fault-worker"
    Wait-RemoteLease $workerExecution
    $workerPod = kubectl -n $namespace get pod -l app.kubernetes.io/name=workflow-worker -o jsonpath='{.items[0].metadata.name}'
    kubectl -n $namespace delete pod $workerPod --grace-period=0 --force --wait=false
    kubectl -n $namespace rollout status deployment/workflow-worker --timeout=180s
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$workerExecution')" @("succeeded") "Worker crash recovery" 150 | Out-Null
    Assert-MySqlScalar "SELECT COUNT(*) FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id WHERE a.execution_id=UUID_TO_BIN('$workerExecution') AND n.node_type='remote_action' AND a.status='succeeded'" 1 "Worker crash produced multiple effective completions"
    Assert-MySqlScalar "SELECT COUNT(*) FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id WHERE a.execution_id=UUID_TO_BIN('$workerExecution') AND n.node_type='remote_action' AND a.status='failed' AND a.error_code='LEASE_EXPIRED'" 1 "Worker crash did not expire exactly one lease"

    $coordinatorExecution = Start-M4Execution "M4 Fault Fixture" "m4-fault-coordinator"
    Wait-RemoteLease $coordinatorExecution
    $coordinatorPod = kubectl -n $namespace get pod -l app.kubernetes.io/name=workflow-coordinator -o jsonpath='{.items[0].metadata.name}'
    kubectl -n $namespace delete pod $coordinatorPod --grace-period=0 --force --wait=false
    kubectl -n $namespace rollout status deployment/workflow-coordinator --timeout=180s
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$coordinatorExecution')" @("succeeded") "Coordinator restart recovery" 120 | Out-Null

    kubectl -n $namespace scale statefulset/redis --replicas=0
    $script:e2eRedisScaledDown = $true
    kubectl -n $namespace wait --for=delete pod/redis-0 --timeout=120s
    $redisExecution = Start-M4Execution "M4 Runtime Fixture" "m4-fault-redis"
    Wait-MySqlValue "SELECT status FROM execution_outbox WHERE execution_id=UUID_TO_BIN('$redisExecution') ORDER BY created_at LIMIT 1" @("pending", "failed") "Outbox retained while Redis was unavailable" 30 | Out-Null
    kubectl -n $namespace scale statefulset/redis --replicas=1
    kubectl -n $namespace rollout status statefulset/redis --timeout=240s
    $script:e2eRedisScaledDown = $false
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$redisExecution')" @("succeeded") "Redis outbox recovery" 120 | Out-Null

    kubectl -n $namespace scale statefulset/clickhouse --replicas=0
    $script:e2eClickHouseScaledDown = $true
    kubectl -n $namespace wait --for=delete pod/clickhouse-0 --timeout=120s
    $clickHouseExecution = Start-M4Execution "M4 Runtime Fixture" "m4-fault-clickhouse"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$clickHouseExecution')" @("succeeded") "ClickHouse outage must not roll back execution" 120 | Out-Null
    kubectl -n $namespace scale statefulset/clickhouse --replicas=1
    kubectl -n $namespace rollout status statefulset/clickhouse --timeout=300s
    $script:e2eClickHouseScaledDown = $false
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'" 0 "Trace delivery after ClickHouse recovery"

    Assert-MySqlScalar "SELECT COUNT(*) FROM execution_outbox WHERE status<>'published'" 0 "Runtime outbox final delivery"
    Assert-MySqlScalar "SELECT COUNT(*) FROM worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id JOIN workflow_executions e ON e.id=a.execution_id WHERE e.status IN ('succeeded','failed','cancelled','timed_out') AND l.released_at IS NULL" 0 "Terminal executions retain worker leases"
    Assert-MySqlScalar "SELECT COUNT(*) FROM worker_leases l JOIN node_executions n ON n.id=l.node_execution_id JOIN workflow_executions e ON e.id=n.execution_id WHERE e.status IN ('waiting','waiting_approval') AND l.released_at IS NULL" 0 "Waiting executions retain worker leases"
    Assert-MySqlScalar "SELECT COUNT(*) FROM workflow_executions child JOIN workflow_executions parent ON parent.id=child.parent_execution_id WHERE child.trigger_type='fork' AND (parent.parent_execution_id IS NOT NULL OR parent.fork_checkpoint_id IS NOT NULL)" 0 "Fork mutated its source execution"
    Assert-MySqlScalar "SELECT COUNT(*) FROM wait_subscriptions w JOIN workflow_executions e ON e.id=w.execution_id WHERE e.status='cancelled' AND w.status<>'cancelled'" 0 "Cancelled wait remained resumable"

    @(
        "workerExecution=$workerExecution",
        "coordinatorExecution=$coordinatorExecution",
        "redisExecution=$redisExecution",
        "clickHouseExecution=$clickHouseExecution",
        "runtimeOutboxPending=$(Get-MySqlValue "SELECT COUNT(*) FROM execution_outbox WHERE status<>'published'")",
        "traceOutboxPending=$(Get-MySqlValue "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'")",
        "activeTerminalLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id JOIN workflow_executions e ON e.id=a.execution_id WHERE e.status IN ('succeeded','failed','cancelled','timed_out') AND l.released_at IS NULL")"
    ) | Out-File -LiteralPath (Join-Path $results "m4-database-evidence.txt") -Encoding utf8
}

function Invoke-M5SandboxFaultSuite {
    $headers = @{ Authorization = "Bearer $script:accessToken" }
    $memoryExecution = Start-M4Execution "M5 Memory Limit Fixture" "m5-memory-limit"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$memoryExecution')" @("failed") "M5 Sandbox memory limit" 120 | Out-Null
    Wait-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$memoryExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1" @("SANDBOX_COMMAND_FAILED", "SANDBOX_STREAM_INCOMPLETE") "M5 Sandbox memory limit error" 120 | Out-Null
    Assert-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$memoryExecution') AND status<>'terminated'" 0 "M5 memory limit did not terminate its Sandbox"

    $ttlExecution = Start-M4Execution "M5 Natural TTL Fixture" "m5-natural-ttl"
    Wait-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$ttlExecution') AND status='running'" 1 "M5 natural TTL Sandbox did not start"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$ttlExecution')" @("failed") "M5 natural Sandbox TTL" 150 | Out-Null
    Wait-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$ttlExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1" @("SANDBOX_TTL_EXPIRED") "M5 natural Sandbox TTL error" 30 | Out-Null
    Assert-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$ttlExecution') AND status<>'terminated'" 0 "M5 natural TTL did not terminate its Sandbox"

    $cancelExecution = Start-M4Execution "M5 Cancellation Fixture" "m5-cancellation"
    Wait-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$cancelExecution') AND status='running'" 1 "M5 cancellation Sandbox did not start"
    $quotaExecution = Start-M4Execution "M5 Manager Restart Fixture" "m5-concurrency-limit"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$quotaExecution')" @("failed") "M5 Sandbox tenant concurrency" 120 | Out-Null
    Wait-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$quotaExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1" @("SANDBOX_TENANT_CONCURRENCY_EXCEEDED") "M5 Sandbox tenant concurrency error" 120 | Out-Null
    Assert-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$quotaExecution')" 0 "M5 rejected concurrency created a Sandbox Lease"
    Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$Port/api/v1/executions/$cancelExecution/cancel" -Headers $headers | Out-Null
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$cancelExecution')" @("cancelled") "M5 cancellation" 90 | Out-Null
    Wait-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$cancelExecution') AND status<>'terminated'" 0 "M5 cancellation did not terminate its Sandbox"

    $restartExecution = Start-M4Execution "M5 Manager Restart Fixture" "m5-manager-restart"
    Wait-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$restartExecution') AND status='running'" 1 "M5 manager restart Sandbox did not start"
    $managerPod = kubectl -n $namespace get pod -l app.kubernetes.io/name=sandbox-manager -o jsonpath='{.items[0].metadata.name}'
    kubectl -n $namespace delete pod $managerPod --grace-period=0 --force --wait=false
    kubectl -n $namespace rollout status deployment/sandbox-manager --timeout=180s
    Invoke-MySqlEvidenceQuery "UPDATE sandbox_leases SET expires_at=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE execution_id=UUID_TO_BIN('$restartExecution') AND status<>'terminated'" | Out-Null
    Wait-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$restartExecution') AND status<>'terminated'" 0 "M5 reaper did not terminate the Sandbox after Manager restart"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$restartExecution')" @("failed", "cancelled", "timed_out") "M5 manager restart Execution" 120 | Out-Null

    kubectl -n $namespace scale statefulset/clickhouse --replicas=0
    $script:e2eClickHouseScaledDown = $true
    kubectl -n $namespace wait --for=delete pod/clickhouse-0 --timeout=120s
    $m5TraceExecution = Start-M4Execution "M5 Agent MCP Fixture" "m5-fault-clickhouse"
    Wait-MySqlValue "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$m5TraceExecution')" @("succeeded") "M5 Agent must commit while ClickHouse is unavailable" 120 | Out-Null
    Wait-MySqlScalar "SELECT COUNT(*) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$m5TraceExecution')" 3 "M5 Agent Ledger was not committed during ClickHouse outage"
    Assert-MySqlScalar "SELECT COUNT(*) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$m5TraceExecution') AND call_kind='model' AND status='succeeded'" 2 "M5 Agent model calls were not settled during ClickHouse outage"
    Assert-MySqlScalar "SELECT COUNT(*) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$m5TraceExecution') AND status IN ('reserved','sent')" 0 "M5 Agent Ledger retained non-terminal calls during ClickHouse outage"
    $m5TraceOutbox = Get-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE execution_id=UUID_TO_BIN('$m5TraceExecution')"
    if ($m5TraceOutbox -lt 1) {
        throw "M5 Agent did not retain Trace Outbox records while ClickHouse was unavailable."
    }
    kubectl -n $namespace scale statefulset/clickhouse --replicas=1
    kubectl -n $namespace rollout status statefulset/clickhouse --timeout=300s
    $script:e2eClickHouseScaledDown = $false
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE execution_id=UUID_TO_BIN('$m5TraceExecution') AND status<>'delivered'" 0 "M5 Trace delivery after ClickHouse recovery"

    @(
        "memoryExecution=$memoryExecution",
        "memoryError=$(Get-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$memoryExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1")",
        "memoryActiveLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$memoryExecution') AND status<>'terminated'")",
        "ttlExecution=$ttlExecution",
        "ttlError=$(Get-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$ttlExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1")",
        "ttlActiveLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$ttlExecution') AND status<>'terminated'")",
        "cancelExecution=$cancelExecution",
        "cancelActiveLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$cancelExecution') AND status<>'terminated'")",
        "quotaExecution=$quotaExecution",
        "quotaError=$(Get-MySqlValue "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$quotaExecution') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1")",
        "quotaSandboxLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$quotaExecution')")",
        "restartExecution=$restartExecution",
        "restartActiveLeases=$(Get-MySqlValue "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$restartExecution') AND status<>'terminated'")",
        "clickHouseExecution=$m5TraceExecution",
        "clickHouseRuntimeCalls=$(Get-MySqlValue "SELECT COUNT(*) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$m5TraceExecution')")",
        "clickHousePendingRuntimeCalls=$(Get-MySqlValue "SELECT COUNT(*) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$m5TraceExecution') AND status IN ('reserved','sent')")",
        "clickHouseTraceOutbox=$(Get-MySqlValue "SELECT COUNT(*) FROM trace_delivery_outbox WHERE execution_id=UUID_TO_BIN('$m5TraceExecution') AND status<>'delivered'")",
        "unrevokedCredentialHandles=$(Get-MySqlValue "SELECT COUNT(*) FROM node_invocation_handles WHERE sandbox_lease_id IS NOT NULL AND revoked_at IS NULL")"
    ) | Out-File -LiteralPath (Join-Path $results "m5-database-evidence.txt") -Encoding utf8
}

New-Item -ItemType Directory -Force -Path $results | Out-Null
Assert-OpenSandboxReady
$openSandboxBefore = @(Get-OpenSandboxIds)
if (-not $SkipBuild) {
    & "$PSScriptRoot/build-images.ps1" -Namespace $namespace -Services @("echo-node", "m3-fixture", "m4-fixture", "m5-fixture") -SkipWeb
}

try {
    Remove-Item -LiteralPath $addonEvidence -Force -ErrorAction SilentlyContinue
    & "$PSScriptRoot/opensandbox-contract.ps1" -OpenSandboxEndpoint $OpenSandboxEndpoint -OpenSandboxApiKey $OpenSandboxApiKey -ResultsPath (Join-Path $results "m5-opensandbox-go-oracle.json") -SkipRustTests | Out-Null
    Suspend-DevelopmentNamespace
    $existing = kubectl get namespace $namespace --ignore-not-found -o name
    if ($existing) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
    $profile = Get-Content "$root/deploy/profiles/full-local.json" -Raw | ConvertFrom-Json -Depth 30
    $profile.namespace = $namespace
    $profile.ingress.host = "agentx-e2e.localhost"
    $profile.components.sandbox.mode = "remote"
    $profile.components.sandbox.endpoint = $clusterOpenSandbox.Uri.AbsoluteUri.TrimEnd('/')
    $profile.components.sandbox.allowedHosts = @($clusterOpenSandbox.Host)
    if ($SkipBuild) { $profile.images.mode = "registry"; $profile.images.registry = "agentx" }
    [IO.File]::WriteAllText($deploymentProfile, ($profile | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
    $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY = $OpenSandboxApiKey
    & "$PSScriptRoot/deploy.ps1" -Action Install -ConfigFile $deploymentProfile -NonInteractive
    $coreSecret = kubectl -n $namespace get secret $profile.secrets.name -o json | ConvertFrom-Json
    $encodedWaitSigningSecret = [string]$coreSecret.data.AGENTX_JWT_SIGNING_SECRET
    if (-not $encodedWaitSigningSecret) {
        throw "Secret $namespace/$($profile.secrets.name) does not contain AGENTX_JWT_SIGNING_SECRET."
    }
    $env:AGENTX_E2E_WAIT_SIGNING_SECRET = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encodedWaitSigningSecret))
    kubectl -n $namespace apply -k "$root/deploy/k8s/fixtures/echo-node"
    $config = kubectl -n $namespace get configmap agentx-config -o json | ConvertFrom-Json
    $config.data | Add-Member -Force -NotePropertyName AGENTX_REMOTE_NODE_ENDPOINT -NotePropertyValue "http://echo-node:8080"
    $config.data | Add-Member -Force -NotePropertyName AGENTX_M5_SANDBOX_IMAGE -NotePropertyValue "opensandbox/code-interpreter@sha256:133a3c1720dd52291a019740c2987e7164ea6de79e23d8198798e58950ae2e6e"
    $config.data | Add-Member -Force -NotePropertyName AGENTX_M5_BROWSER_IMAGE -NotePropertyValue "opensandbox/playwright@sha256:09709684c785db3107fc3357e7af5b921f5d5a60e75071601122a473d344b475"
    $config.data | Add-Member -Force -NotePropertyName AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT -NotePropertyValue "1"
    $config | ConvertTo-Json -Depth 30 | kubectl apply -f - | Out-Null
    kubectl -n $namespace rollout restart deployment/platform-api deployment/workflow-worker deployment/sandbox-manager
    foreach ($deployment in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web", "echo-mcp", "echo-node")) {
        kubectl -n $namespace rollout status "deployment/$deployment" --timeout=300s
    }
    foreach ($deployment in @("lightrag", "mem0-postgres", "mem0")) {
        kubectl -n $namespace rollout status "deployment/$deployment" --timeout=600s
    }
    & "$PSScriptRoot/m5-addons-contract.ps1" -Namespace $namespace -ResultsPath $addonEvidence -PreserveFixtureData | Out-Null

    $forward = Start-Process kubectl -ArgumentList @("-n", $namespace, "port-forward", "service/web", "${Port}:80") -PassThru -WindowStyle Hidden -RedirectStandardOutput $forwardOut -RedirectStandardError $forwardError
    Wait-TcpPort $Port
    $env:AGENTX_E2E_BASE_URL = "http://127.0.0.1:$Port"
    Invoke-Playwright -Suite "m2-m3-control-plane" -Tests @("tests/m2.1-control-plane.spec.ts", "tests/m3-control-plane.spec.ts")
    Assert-MySqlScalar "SELECT COUNT(*) FROM application_invocations i JOIN applications a ON a.id=i.application_id WHERE a.slug='m3-e2e'" 0 "Runtime-unavailable invocation created a fake Invocation"
    Assert-MySqlScalar "SELECT COUNT(*) FROM application_messages m JOIN application_sessions s ON s.id=m.session_id JOIN applications a ON a.id=s.application_id WHERE a.slug='m3-e2e'" 0 "Runtime-unavailable message created a fake Message"
    Assert-MySqlScalar "SELECT COUNT(*) FROM evaluation_case_results r JOIN evaluation_runs e ON e.id=r.evaluation_run_id WHERE e.name='M3 Runtime Boundary'" 0 "Runtime-unavailable evaluation created fake Case Results"
    kubectl -n $namespace delete job/m3-fixture --ignore-not-found --wait=true
    kubectl apply -f "$root/deploy/k8s/stacks/e2e/m3-fixture-job.yaml"
    kubectl -n $namespace wait --for=condition=complete job/m3-fixture --timeout=180s
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'" 0 "Trace delivery outbox before UI verification"
    Invoke-Playwright -Suite "m3-observability" -Tests @("tests/m3-observability.spec.ts")
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'" 0 "Trace delivery outbox"
    kubectl -n $namespace delete job/m4-fixture --ignore-not-found --wait=true
    kubectl apply -f "$root/deploy/k8s/stacks/e2e/m4-fixture-job.yaml"
    kubectl -n $namespace wait --for=condition=complete job/m4-fixture --timeout=180s
    Invoke-Playwright -Suite "m4-runtime-recovery" -Tests @("tests/m4-runtime.spec.ts", "tests/m4-recovery.spec.ts")
    Invoke-M4FaultSuite
    kubectl -n $namespace delete job/m5-fixture --ignore-not-found --wait=true
    kubectl apply -f "$root/deploy/k8s/stacks/e2e/m5-fixture-job.yaml"
    kubectl -n $namespace wait --for=condition=complete job/m5-fixture --timeout=180s
    Invoke-Playwright -Suite "m5-agent-sandbox" -Tests @("tests/m5-agent-sandbox.spec.ts")
    Invoke-Playwright -Suite "m6-workflow-studio" -Tests @("tests/m6-workflow-studio.spec.ts")
    Invoke-M5SandboxFaultSuite
    Assert-MySqlScalar "SELECT COUNT(*) FROM sandbox_leases WHERE status<>'terminated'" 0 "M5 terminal Sandbox leases"
    Assert-MySqlScalar "SELECT COUNT(*) FROM node_invocation_handles WHERE sandbox_lease_id IS NOT NULL AND revoked_at IS NULL" 0 "M5 Sandbox Credential Handles were not revoked"
}
finally {
    if ($forward -and -not $forward.HasExited) {
        Stop-Process -Id $forward.Id -Force
    }
    Remove-Item Env:AGENTX_E2E_BASE_URL -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_WAIT_SIGNING_SECRET -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_STAGE -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_RUN_ID -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_E2E_SUITE -ErrorAction SilentlyContinue
    if (-not $KeepNamespace -and (Test-Path -LiteralPath $deploymentProfile)) {
        try {
            & "$PSScriptRoot/deploy.ps1" -Action Uninstall -ConfigFile $deploymentProfile -Target ingress -NonInteractive | Out-Null
        }
        catch {
            if (-not $sandboxCleanupError) {
                $sandboxCleanupError = $_
            }
        }
    }
    Remove-Item -LiteralPath $deploymentProfile -Force -ErrorAction SilentlyContinue
    try {
        Clear-M5Sandboxes
    }
    catch {
        $sandboxCleanupError = $_
    }
    $namespaceExists = kubectl get namespace $namespace --ignore-not-found -o name
    if ($namespaceExists) {
        if (Test-Path -LiteralPath $addonEvidence) {
            try {
                & "$PSScriptRoot/m5-addons-contract.ps1" -Namespace $namespace -ResultsPath $addonEvidence -CleanupOnly | Out-Null
            }
            catch {
                if (-not $sandboxCleanupError) {
                    $sandboxCleanupError = $_
                }
            }
        }
        if ($e2eRedisScaledDown) {
            kubectl -n $namespace scale statefulset/redis --replicas=1
        }
        if ($e2eClickHouseScaledDown) {
            kubectl -n $namespace scale statefulset/clickhouse --replicas=1
        }
        kubectl -n $namespace get all,pvc -o wide | Out-File -LiteralPath (Join-Path $results "resources.txt") -Encoding utf8
        kubectl -n $namespace get events --sort-by=.lastTimestamp | Out-File -LiteralPath (Join-Path $results "events.txt") -Encoding utf8
        if (kubectl -n $namespace get deployment/platform-api --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/platform-api" (Join-Path $results "platform-api.log")
        }
        if (kubectl -n $namespace get deployment/echo-mcp --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/echo-mcp" (Join-Path $results "echo-mcp.log")
        }
        if (kubectl -n $namespace get deployment/trigger-gateway --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/trigger-gateway" (Join-Path $results "trigger-gateway.log")
        }
        if (kubectl -n $namespace get deployment/trace-writer --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/trace-writer" (Join-Path $results "trace-writer.log")
        }
        if (kubectl -n $namespace get deployment/workflow-coordinator --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/workflow-coordinator" (Join-Path $results "workflow-coordinator.log")
        }
        if (kubectl -n $namespace get deployment/workflow-worker --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/workflow-worker" (Join-Path $results "workflow-worker.log")
        }
        if (kubectl -n $namespace get deployment/sandbox-manager --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/sandbox-manager" (Join-Path $results "sandbox-manager.log")
        }
        if (kubectl -n $namespace get deployment/echo-node --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/echo-node" (Join-Path $results "echo-node.log")
        }
        if (kubectl -n $namespace get deployment/lightrag --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/lightrag" (Join-Path $results "lightrag.log")
        }
        if (kubectl -n $namespace get deployment/mem0 --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/mem0" (Join-Path $results "mem0.log")
        }
        if (kubectl -n $namespace get job/m3-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m3-fixture" (Join-Path $results "m3-fixture.log")
        }
        if (kubectl -n $namespace get job/m4-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m4-fixture" (Join-Path $results "m4-fixture.log")
        }
        if (kubectl -n $namespace get job/m5-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m5-fixture" (Join-Path $results "m5-fixture.log")
        }
    }
    if ($namespaceExists -and -not $KeepNamespace) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
    Restore-DevelopmentNamespace
    if ($sandboxCleanupError) {
        throw $sandboxCleanupError
    }
}
