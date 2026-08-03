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

New-Item -ItemType Directory -Force -Path $results | Out-Null
if (-not $SkipBuild) {
    & "$PSScriptRoot/build-images.ps1" -Services @("platform-api", "echo-mcp")
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
    kubectl -n $namespace rollout status statefulset/minio --timeout=420s
    foreach ($deployment in @("platform-api", "web", "echo-mcp")) {
        kubectl -n $namespace rollout status "deployment/$deployment" --timeout=300s
    }

    $forward = Start-Process kubectl -ArgumentList @("-n", $namespace, "port-forward", "service/web", "${Port}:8080") -PassThru -WindowStyle Hidden -RedirectStandardOutput $forwardOut -RedirectStandardError $forwardError
    Wait-TcpPort $Port
    $env:AGENTX_E2E_BASE_URL = "http://127.0.0.1:$Port"
    $command = if ($Headed) { "test:headed" } else { "test" }
    pnpm --filter @agentx/e2e $command
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
            kubectl -n $namespace logs deployment/platform-api --all-containers --tail=500 | Out-File -LiteralPath (Join-Path $results "platform-api.log") -Encoding utf8
        }
        if (kubectl -n $namespace get deployment/echo-mcp --ignore-not-found -o name) {
            kubectl -n $namespace logs deployment/echo-mcp --all-containers --tail=500 | Out-File -LiteralPath (Join-Path $results "echo-mcp.log") -Encoding utf8
        }
    }
    if ($namespaceExists -and -not $KeepNamespace) {
        kubectl delete namespace $namespace --wait=true --timeout=300s
    }
}
