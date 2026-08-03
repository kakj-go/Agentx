param(
    [switch]$KeepNamespace,
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

New-Item -ItemType Directory -Force -Path $results | Out-Null
if (-not $SkipBuild) {
    & "$PSScriptRoot/build-images.ps1" -Services @("platform-api", "echo-mcp", "trigger-gateway", "trace-writer", "m3-fixture")
}

try {
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
    foreach ($deployment in @("platform-api", "trigger-gateway", "trace-writer", "web", "echo-mcp")) {
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
}
finally {
    if ($forward -and -not $forward.HasExited) {
        Stop-Process -Id $forward.Id -Force
    }
    Remove-Item Env:AGENTX_E2E_BASE_URL -ErrorAction SilentlyContinue
    $namespaceExists = kubectl get namespace $namespace --ignore-not-found -o name
    if ($namespaceExists) {
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
        if (kubectl -n $namespace get job/m3-fixture --ignore-not-found -o name) {
            Save-KubernetesLogs "job/m3-fixture" (Join-Path $results "m3-fixture.log")
        }
    }
    if ($namespaceExists -and -not $KeepNamespace) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
}
