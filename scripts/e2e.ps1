param(
    [switch]$KeepNamespace,
    [switch]$KeepDevelopmentRunning,
    [switch]$SkipBuild,
    [switch]$Headed,
    [int]$Port = 18081
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$namespace = "agentx-e2e"
$results = Join-Path $root "apps/e2e/test-results/kubernetes"
$forwardOut = Join-Path $results "port-forward.out.log"
$forwardError = Join-Path $results "port-forward.err.log"
$forward = $null
$developmentReplicas = @()
$e2eRedisScaledDown = $false
$e2eClickHouseScaledDown = $false

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

function Get-MySqlScalar([string]$Query) {
    $value = kubectl -n $namespace exec statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE" -e "$1"' agentx-e2e-query $Query
    if ($LASTEXITCODE -ne 0) {
        throw "MySQL evidence query failed: $Query"
    }
    return [long]($value | Select-Object -Last 1)
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
    $value = kubectl -n $namespace exec statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE" -e "$1"' agentx-e2e-query $Query
    if ($LASTEXITCODE -ne 0) {
        throw "MySQL evidence query failed: $Query"
    }
    return [string]($value | Select-Object -Last 1)
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

New-Item -ItemType Directory -Force -Path $results | Out-Null
if (-not $SkipBuild) {
    & "$PSScriptRoot/build-images.ps1" -Services @("platform-api", "echo-mcp", "echo-node", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer", "m3-fixture", "m4-fixture")
}

try {
    Suspend-DevelopmentNamespace
    $existing = kubectl get namespace $namespace --ignore-not-found -o name
    if ($existing) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
    kubectl apply -k "$root/deploy/k8s/overlays/e2e"
    kubectl -n $namespace rollout status statefulset/mysql --timeout=420s
    kubectl -n $namespace wait --for=condition=complete job/platform-api-migrate --timeout=300s
    kubectl -n $namespace rollout status statefulset/redis --timeout=420s
    kubectl -n $namespace rollout status statefulset/clickhouse --timeout=420s
    kubectl -n $namespace wait --for=condition=complete job/trace-writer-migrate --timeout=300s
    kubectl -n $namespace rollout status statefulset/minio --timeout=420s
    kubectl -n $namespace wait --for=condition=complete job/minio-bucket-init --timeout=300s
    foreach ($deployment in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer", "web", "echo-mcp", "echo-node")) {
        kubectl -n $namespace rollout status "deployment/$deployment" --timeout=300s
    }

    $forward = Start-Process kubectl -ArgumentList @("-n", $namespace, "port-forward", "service/web", "${Port}:8080") -PassThru -WindowStyle Hidden -RedirectStandardOutput $forwardOut -RedirectStandardError $forwardError
    Wait-TcpPort $Port
    $env:AGENTX_E2E_BASE_URL = "http://127.0.0.1:$Port"
    $headedArgument = if ($Headed) { @("--headed") } else { @() }
    pnpm --filter @agentx/e2e exec playwright test @headedArgument tests/m2.1-control-plane.spec.ts tests/m3-control-plane.spec.ts
    Assert-MySqlScalar "SELECT COUNT(*) FROM application_invocations i JOIN applications a ON a.id=i.application_id WHERE a.slug='m3-e2e'" 0 "Runtime-unavailable invocation created a fake Invocation"
    Assert-MySqlScalar "SELECT COUNT(*) FROM application_messages m JOIN application_sessions s ON s.id=m.session_id JOIN applications a ON a.id=s.application_id WHERE a.slug='m3-e2e'" 0 "Runtime-unavailable message created a fake Message"
    Assert-MySqlScalar "SELECT COUNT(*) FROM evaluation_case_results r JOIN evaluation_runs e ON e.id=r.evaluation_run_id WHERE e.name='M3 Runtime Boundary'" 0 "Runtime-unavailable evaluation created fake Case Results"
    kubectl -n $namespace delete job/m3-fixture --ignore-not-found --wait=true
    kubectl apply -f "$root/deploy/k8s/overlays/e2e/m3-fixture-job.yaml"
    kubectl -n $namespace wait --for=condition=complete job/m3-fixture --timeout=180s
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'" 0 "Trace delivery outbox before UI verification"
    pnpm --filter @agentx/e2e exec playwright test @headedArgument tests/m3-observability.spec.ts
    Wait-MySqlScalar "SELECT COUNT(*) FROM trace_delivery_outbox WHERE status<>'delivered'" 0 "Trace delivery outbox"
    kubectl -n $namespace delete job/m4-fixture --ignore-not-found --wait=true
    kubectl apply -f "$root/deploy/k8s/overlays/e2e/m4-fixture-job.yaml"
    kubectl -n $namespace wait --for=condition=complete job/m4-fixture --timeout=180s
    pnpm --filter @agentx/e2e exec playwright test @headedArgument tests/m4-runtime.spec.ts tests/m4-recovery.spec.ts
    Invoke-M4FaultSuite
}
finally {
    if ($forward -and -not $forward.HasExited) {
        Stop-Process -Id $forward.Id -Force
    }
    Remove-Item Env:AGENTX_E2E_BASE_URL -ErrorAction SilentlyContinue
    $namespaceExists = kubectl get namespace $namespace --ignore-not-found -o name
    if ($namespaceExists) {
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
        if (kubectl -n $namespace get deployment/echo-node --ignore-not-found -o name) {
            Save-KubernetesLogs "deployment/echo-node" (Join-Path $results "echo-node.log")
        }
        if (kubectl -n $namespace get job/m3-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m3-fixture" (Join-Path $results "m3-fixture.log")
        }
        if (kubectl -n $namespace get job/m4-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m4-fixture" (Join-Path $results "m4-fixture.log")
        }
    }
    if ($namespaceExists -and -not $KeepNamespace) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
    Restore-DevelopmentNamespace
}
