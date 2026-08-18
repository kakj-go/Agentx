param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [ValidateSet("03", "04", "05", "06")]
    [string]$Stage = "03",
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$KeepOnFailure,
    [switch]$KeepOnSuccess,
    [string]$ContextOutputPath
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$deploy = Join-Path $PSScriptRoot "deploy-v2.ps1"
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-$Stage"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$profilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json
$e2eProfilePath = Join-Path $artifactDirectory "v2-$Stage-profile.json"
$profile.ingress.controlHost = "control-$RunId.agentx.localhost"
$profile.ingress.runtimeHost = "runtime-$RunId.agentx.localhost"
$profile | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $e2eProfilePath
$profilePath = $e2eProfilePath
$namespaces = @{
    control = "agentx-v2-$Stage-control-$RunId"
    runtime = "agentx-v2-$Stage-runtime-$RunId"
    observability = "agentx-v2-$Stage-runtime-$RunId"
    dependencies = "agentx-v2-$Stage-deps-$RunId"
}
$timeline = [Collections.Generic.List[string]]::new()
$developmentReplicas = @()
$objectStorageAdminReady = $false
$succeeded = $false
$faultProxyForward = $null
$gatewayPodForwards = [Collections.Generic.List[System.Diagnostics.Process]]::new()

function Invoke-Kubectl([string[]]$Arguments) {
    $PSNativeCommandUseErrorActionPreference = $false
    $output = & kubectl @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        $command = ($Arguments -join ' ') -replace '(?i)\b(MYSQL_PWD|VAULT_TOKEN|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>' -replace '(?i)(Authorization:\s*Bearer\s+)[^\s]+', '$1<redacted>'
        $details = ($output -join [Environment]::NewLine) -replace '(?i)\b(MYSQL_PWD|VAULT_TOKEN|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>' -replace '(?i)(Authorization:\s*Bearer\s+)[^\s]+', '$1<redacted>'
        throw "kubectl $command failed: $details"
    }
    return @($output)
}

function Get-DevelopmentReplicas {
    $replicas = @()
    foreach ($kind in @("deployment", "statefulset")) {
        $json = & kubectl -n agentx get $kind -o json 2>$null
        if ($LASTEXITCODE -ne 0) { continue }
        foreach ($item in (($json -join "`n") | ConvertFrom-Json).items) {
            $replicas += [pscustomobject]@{ kind = $kind; name = [string]$item.metadata.name; replicas = [int]$item.spec.replicas }
        }
    }
    return $replicas
}

function Set-DevelopmentReplicas([array]$Replicas, [bool]$Stop) {
    foreach ($item in $Replicas) {
        $target = if ($Stop) { 0 } else { $item.replicas }
        Invoke-Kubectl @("-n", "agentx", "scale", "$($item.kind)/$($item.name)", "--replicas=$target") | Out-Null
    }
}

function Start-PortForward([string]$Namespace, [string]$Service, [int]$LocalPort, [int]$RemotePort) {
    $stdout = Join-Path $artifactDirectory "$Service-port-forward.stdout.log"
    $stderr = Join-Path $artifactDirectory "$Service-port-forward.stderr.log"
    $process = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", "service/$Service", "${LocalPort}:${RemotePort}") -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $deadline = (Get-Date).AddSeconds(30)
    do {
        if ($process.HasExited) { throw "Port-forward for $Service exited." }
        try { Invoke-WebRequest "http://127.0.0.1:$LocalPort/health/live" -UseBasicParsing -TimeoutSec 2 | Out-Null; return $process } catch { Start-Sleep -Milliseconds 500 }
    } while ((Get-Date) -lt $deadline)
    throw "Port-forward for $Service did not become ready."
}

function Start-PodPortForward([string]$Namespace, [string]$Pod, [int]$RemotePort, [string]$LogName) {
    $stdout = Join-Path $artifactDirectory "$LogName.stdout.log"
    $stderr = Join-Path $artifactDirectory "$LogName.stderr.log"
    $process = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", "pod/$Pod", ":$RemotePort") -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $gatewayPodForwards.Add($process)
    $deadline = (Get-Date).AddSeconds(30)
    do {
        if ($process.HasExited) {
            $details = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw } else { "no stderr" }
            throw "Port-forward for Pod $Pod exited before becoming ready: $details"
        }
        if (Test-Path -LiteralPath $stdout) {
            $forwardOutput = Get-Content -LiteralPath $stdout -Raw
            if (-not [string]::IsNullOrWhiteSpace($forwardOutput)) {
                $match = [regex]::Match($forwardOutput, 'Forwarding from (?:127\.0\.0\.1|\[::1\]):(?<port>\d+)')
                if ($match.Success) {
                    $localPort = [int]$match.Groups['port'].Value
                    try {
                        Invoke-WebRequest "http://127.0.0.1:$localPort/health/live" -UseBasicParsing -TimeoutSec 2 | Out-Null
                        return $localPort
                    }
                    catch {
                        Start-Sleep -Milliseconds 250
                    }
                }
            }
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    $details = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw } else { "no stderr" }
    throw "Port-forward for Pod $Pod did not become ready: $details"
}

function Invoke-SseCurl([string]$Url, [string]$Token, [string]$LastEventId = "") {
    $arguments = @("-fsS", "-N", "--max-time", "15", "-H", "Authorization: Bearer $Token")
    if (-not [string]::IsNullOrWhiteSpace($LastEventId)) { $arguments += @("-H", "Last-Event-ID: $LastEventId") }
    $arguments += $Url
    $previousNativeErrorPreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $events = (& curl.exe @arguments 2>&1) -join "`n"
        $exitCode = $LASTEXITCODE
    }
    finally {
        $PSNativeCommandUseErrorActionPreference = $previousNativeErrorPreference
    }
    if ($exitCode -ne 0) { throw "SSE curl failed with exit code $exitCode for $Url`: $events" }
    return $events
}

function Wait-PublishAttempt([string]$ControlUrl, [hashtable]$Headers, [string]$ApplicationId, [string]$AttemptId) {
    $deadline = (Get-Date).AddMinutes(3)
    do {
        $attempt = Invoke-RestMethod "$ControlUrl/api/v1/applications/$ApplicationId/publish-attempts/$AttemptId" -Headers $Headers
        if ($attempt.state -eq "active") { return $attempt }
        if ($attempt.state -eq "rejected") { throw "Publish rejected: $($attempt.errorCode) $($attempt.errorMessage)" }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    throw "Publish Attempt $AttemptId timed out."
}

function Wait-RejectedPublishAttempt([string]$ControlUrl, [hashtable]$Headers, [string]$ApplicationId, [string]$AttemptId) {
    $deadline = (Get-Date).AddMinutes(3)
    do {
        $attempt = Invoke-RestMethod "$ControlUrl/api/v1/applications/$ApplicationId/publish-attempts/$AttemptId" -Headers $Headers
        if ($attempt.state -eq "rejected") { return $attempt }
        if ($attempt.state -eq "active") { throw "Publish Attempt $AttemptId unexpectedly became active." }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    throw "Publish Attempt $AttemptId did not reach rejected state."
}

function Wait-ControlOutbox([string]$EventType, [string]$AggregateId) {
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $count = [int](Invoke-MySql "SELECT COUNT(*) FROM outbox WHERE event_type='$EventType' AND aggregate_id='$AggregateId' AND status='published';" -join "")
        if ($count -gt 0) { return }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Outbox event $EventType/$AggregateId did not converge."
}

function Wait-ApplicationHead([string]$BundleId) {
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $head = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(bundle_id) FROM deployment_heads WHERE tenant_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000001') AND application_id=UUID_TO_BIN('018f0000-0000-7000-8000-00000000000a');" -join "")
        if ($head.ToLowerInvariant() -eq $BundleId.ToLowerInvariant()) { return }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Runtime Head did not converge to Bundle $BundleId."
}

function Assert-RuntimeMySqlUnavailable([string]$RuntimeUrl, [string]$ApiKey) {
    $idempotencyKey = "v2-03-$RunId-mysql-down"
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=0") | Out-Null
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $pods = @(Invoke-Kubectl @("-n", $namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=runtime-mysql", "-o", "name"))
        if ($pods.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($pods.Count -ne 0) { throw "Runtime MySQL did not stop for failure verification." }
    try {
        Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $ApiKey"; "Idempotency-Key" = $idempotencyKey } -ContentType application/json -Body (@{ input = @{ message = "must-fail" }; responseMode = "async" } | ConvertTo-Json -Depth 5) | Out-Null
        throw "Runtime MySQL outage returned a false success."
    }
    catch {
        if ($_.Exception.Message -eq "Runtime MySQL outage returned a false success.") { throw }
        $status = [int]$_.Exception.Response.StatusCode
        if ($status -ne 503) { throw "Runtime MySQL outage returned HTTP $status instead of 503." }
    }
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "statefulset/runtime-mysql", "--timeout=300s") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "restart", "deployment/runtime-gateway", "deployment/workflow-runtime", "deployment/workflow-worker") | Out-Null
    foreach ($deployment in @("runtime-gateway", "workflow-runtime", "workflow-worker")) {
        Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "deployment/$deployment", "--timeout=300s") | Out-Null
    }
    $orphanFacts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM application_invocations WHERE idempotency_key='$idempotencyKey'),(SELECT COUNT(*) FROM runtime_commands WHERE idempotency_key='$idempotencyKey'),(SELECT COUNT(*) FROM execution_outbox WHERE execution_id IN (SELECT execution_id FROM application_invocations WHERE idempotency_key='$idempotencyKey'));" -join "`t")
    if ($orphanFacts -ne "0`t0`t0") { throw "Runtime MySQL outage left orphan request facts: $orphanFacts" }
}

function Install-FaultProxy {
    $manifest = @"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: runtime-fault-proxy
  namespace: $($namespaces.runtime)
spec:
  replicas: 1
  selector: { matchLabels: { app.kubernetes.io/name: runtime-fault-proxy } }
  template:
    metadata: { labels: { app.kubernetes.io/name: runtime-fault-proxy, agentx.io/plane: runtime, agentx.io/internal-api: runtime-v1, agentx.io/e2e-fault-proxy: allowed } }
    spec:
      serviceAccountName: runtime-gateway
      automountServiceAccountToken: false
      containers:
        - name: runtime-fault-proxy
          image: agentx/runtime-fault-proxy:$($profile.images.tag)
          imagePullPolicy: $($profile.images.pullPolicy)
          env:
            - { name: AGENTX_FAULT_PROXY_UPSTREAM, value: http://runtime-gateway-public:8080 }
          ports: [{ name: http, containerPort: 8080 }]
          readinessProbe: { httpGet: { path: /health/ready, port: http }, periodSeconds: 2 }
---
apiVersion: v1
kind: Service
metadata: { name: runtime-fault-proxy, namespace: $($namespaces.runtime) }
spec:
  selector: { app.kubernetes.io/name: runtime-fault-proxy }
  ports: [{ name: http, port: 8080, targetPort: http }]
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: runtime-fault-proxy-egress, namespace: $($namespaces.runtime) }
spec:
  podSelector: { matchLabels: { agentx.io/e2e-fault-proxy: allowed } }
  policyTypes: [Egress]
  egress:
    - to: [{ podSelector: { matchLabels: { app.kubernetes.io/name: runtime-gateway } } }]
      ports: [{ protocol: TCP, port: 8080 }]
    - to: [{ namespaceSelector: {}, podSelector: { matchLabels: { k8s-app: kube-dns } } }]
      ports: [{ protocol: UDP, port: 53 }, { protocol: TCP, port: 53 }]
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: runtime-fault-proxy-gateway-ingress, namespace: $($namespaces.runtime) }
spec:
  podSelector: { matchLabels: { app.kubernetes.io/name: runtime-gateway } }
  policyTypes: [Ingress]
  ingress:
    - from: [{ podSelector: { matchLabels: { agentx.io/e2e-fault-proxy: allowed } } }]
      ports: [{ protocol: TCP, port: 8080 }]
"@
    $manifest | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to install the V2-03 fault proxy." }
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "deployment/runtime-fault-proxy", "--timeout=180s") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/platform-control", "--replicas=0") | Out-Null
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $controlPods = @(Invoke-Kubectl @("-n", $namespaces.control, "get", "pods", "-l", "app.kubernetes.io/name=platform-control", "-o", "name"))
        if ($controlPods.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($controlPods.Count -ne 0) { throw "The direct Runtime Publisher Pod did not terminate before fault injection." }
    Invoke-Kubectl @("-n", $namespaces.control, "set", "env", "deployment/platform-control", "AGENTX_RUNTIME_INTERNAL_URL=http://runtime-fault-proxy.$($namespaces.runtime).svc:8080") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/platform-control", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "rollout", "status", "deployment/platform-control", "--timeout=180s") | Out-Null
}

function Invoke-FaultProxy([string]$Action, [string]$Operation) {
    Invoke-RestMethod "http://127.0.0.1:18082/e2e/faults/${Operation}:$Action" -Method Post | Out-Null
}

function Install-TriggerProvider {
    $manifest = @"
apiVersion: apps/v1
kind: Deployment
metadata: { name: echo-node, namespace: $($namespaces.dependencies) }
spec:
  replicas: 1
  selector: { matchLabels: { app.kubernetes.io/name: echo-node } }
  template:
    metadata: { labels: { app.kubernetes.io/name: echo-node, agentx.io/plane: dependencies, agentx.io/runtime-provider: allowed } }
    spec:
      containers:
        - name: echo-node
          image: agentx/echo-node:$($profile.images.tag)
          imagePullPolicy: $($profile.images.pullPolicy)
          ports: [{ name: http, containerPort: 8080 }]
          readinessProbe: { httpGet: { path: /health/ready, port: http }, periodSeconds: 2 }
---
apiVersion: v1
kind: Service
metadata: { name: echo-node, namespace: $($namespaces.dependencies) }
spec:
  selector: { app.kubernetes.io/name: echo-node }
  ports: [{ name: http, port: 8080, targetPort: http }]
"@
    $manifest | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to install the V2-03 Trigger Provider." }
    Invoke-Kubectl @("-n", $namespaces.dependencies, "rollout", "status", "deployment/echo-node", "--timeout=180s") | Out-Null
}

function Invoke-MySql([string]$Sql) {
    $password = Invoke-Kubectl @("-n", $namespaces.control, "get", "secret", "agentx-control-secrets", "-o", "jsonpath={.data.AGENTX_CONTROL_MYSQL_PASSWORD}")
    $password = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String(($password -join "")))
    return Invoke-Kubectl @("-n", $namespaces.control, "exec", "statefulset/control-mysql", "--", "env", "MYSQL_PWD=$password", "mysql", "-N", "-B", "-ucontrol_app", "agentx_control", "-e", $Sql)
}

function Invoke-RuntimeMySql([string]$Sql) {
    $password = Invoke-Kubectl @("-n", $namespaces.runtime, "get", "secret", "agentx-runtime-secrets", "-o", "jsonpath={.data.AGENTX_RUNTIME_MYSQL_PASSWORD}")
    $password = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String(($password -join "")))
    return Invoke-Kubectl @("-n", $namespaces.runtime, "exec", "statefulset/runtime-mysql", "--", "env", "MYSQL_PWD=$password", "mysql", "-N", "-B", "-uruntime_app", "agentx_runtime", "-e", $Sql)
}

function ConvertTo-Base64Url([byte[]]$Bytes) {
    return [Convert]::ToBase64String($Bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

function Get-WebhookSignature([string]$Secret, [string]$Timestamp, [string]$Body) {
    $hmac = [Security.Cryptography.HMACSHA256]::new([Text.Encoding]::UTF8.GetBytes($Secret))
    try {
        return ConvertTo-Base64Url ($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes("$Timestamp.$Body")))
    }
    finally {
        $hmac.Dispose()
    }
}

function Wait-InvocationTerminal([string]$RuntimeUrl, [string]$Token, [string]$InvocationId) {
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $invocation = Invoke-RestMethod "$RuntimeUrl/gateway/v1/invocations/$InvocationId" -Headers @{ Authorization = "Bearer $Token" }
        if ($invocation.status -in @("completed", "failed", "cancelled")) { return $invocation }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "Invocation $InvocationId did not reach a terminal state."
}

function Assert-IdempotencyConflict([scriptblock]$Request) {
    try {
        & $Request | Out-Null
        throw "A conflicting Idempotency-Key was accepted."
    }
    catch {
        if ($_.Exception.Message -eq "A conflicting Idempotency-Key was accepted.") { throw }
        if ([int]$_.Exception.Response.StatusCode -ne 409) { throw "Idempotency conflict returned HTTP $([int]$_.Exception.Response.StatusCode) instead of 409." }
    }
}

function Assert-SessionPolicy([string]$RuntimeUrl, [string]$Token, [string]$Slug, [string]$ExpectedPolicy, [string]$ExpectedBundle, [string]$Key) {
    $body = @{ externalUserId = "v2-03-$ExpectedPolicy"; title = "V2-03 $ExpectedPolicy" } | ConvertTo-Json
    $headers = @{ Authorization = "Bearer $Token"; "Idempotency-Key" = $Key }
    $session = Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/$Slug/sessions" -Method Post -Headers $headers -ContentType application/json -Body $body
    $replay = Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/$Slug/sessions" -Method Post -Headers $headers -ContentType application/json -Body $body
    if ($session.id -ne $replay.id -or $session.versionPolicy -ne $ExpectedPolicy) { throw "Session $ExpectedPolicy did not replay the same authoritative result." }
    if ($ExpectedPolicy -eq "follow_deployment") {
        if ($null -ne $session.bundleId) { throw "follow_deployment Session unexpectedly pinned a Bundle." }
    }
    elseif ($session.bundleId.ToLowerInvariant() -ne $ExpectedBundle.ToLowerInvariant()) {
        throw "Session $ExpectedPolicy pinned an unexpected Bundle $($session.bundleId)."
    }
    return $session
}

function Assert-MessageReplay([string]$RuntimeUrl, [string]$Token, [string]$SessionId) {
    $key = "v2-03-$RunId-message-replay"
    $headers = @{ Authorization = "Bearer $Token"; "Idempotency-Key" = $key }
    $body = @{ parts = @(@{ partType = "text"; content = "agentx-v2"; artifactId = $null }) } | ConvertTo-Json -Depth 6
    Invoke-FaultProxy "arm" "message"
    $dropJob = Start-Job -ScriptBlock {
        param($Url, $Bearer, $IdempotencyKey, $RequestBody, $TargetSessionId)
        try {
            Invoke-RestMethod "$Url/gateway/v1/sessions/$TargetSessionId/messages" -Method Post -Headers @{ Authorization = "Bearer $Bearer"; "Idempotency-Key" = $IdempotencyKey } -ContentType application/json -Body $RequestBody | Out-Null
            return "unexpected-success"
        }
        catch {
            return "response-dropped"
        }
    } -ArgumentList "http://127.0.0.1:18082", $Token, $key, $body, $SessionId
    try {
        $deadline = (Get-Date).AddMinutes(1)
        do {
            $committed = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM application_messages WHERE session_id=UUID_TO_BIN('$SessionId') AND idempotency_key='$key';" -join "")
            if ($committed -eq 1) { break }
            Start-Sleep -Milliseconds 250
        } while ((Get-Date) -lt $deadline)
        if ($committed -ne 1) { throw "Message response-loss request did not commit before timeout." }
        Invoke-FaultProxy "release" "message"
        Wait-Job $dropJob -Timeout 30 | Out-Null
        if ((Receive-Job $dropJob) -ne "response-dropped") { throw "Message fault proxy did not discard the committed response." }
    }
    finally {
        Invoke-FaultProxy "release" "message"
        Remove-Job $dropJob -Force -ErrorAction SilentlyContinue
    }
    $first = Invoke-RestMethod "$RuntimeUrl/gateway/v1/sessions/$SessionId/messages" -Method Post -Headers $headers -ContentType application/json -Body $body
    $second = Invoke-RestMethod "$RuntimeUrl/gateway/v1/sessions/$SessionId/messages" -Method Post -Headers $headers -ContentType application/json -Body $body
    if ($first.id -ne $second.id -or $first.executionId -ne $second.executionId) { throw "Message response-loss replay created a different Invocation or Execution." }
    $facts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM application_messages WHERE session_id=UUID_TO_BIN('$SessionId') AND idempotency_key='$key'),(SELECT COUNT(*) FROM application_invocations WHERE id=UUID_TO_BIN('$($first.id)')),(SELECT COUNT(*) FROM workflow_executions WHERE id=UUID_TO_BIN('$($first.executionId)')),(SELECT COUNT(*) FROM runtime_commands WHERE command_type='start_execution' AND aggregate_id='$($first.executionId)');" -join "`t")
    if ($facts -ne "1`t1`t1`t1") { throw "Message/Invocation/Execution/Command did not commit exactly once: $facts" }
    Assert-IdempotencyConflict { Invoke-RestMethod "$RuntimeUrl/gateway/v1/sessions/$SessionId/messages" -Method Post -Headers $headers -ContentType application/json -Body (@{ parts = @(@{ partType = "text"; content = "different"; artifactId = $null }) } | ConvertTo-Json -Depth 6) }
    return $first
}

function Assert-SseReplayAndRedisRebuild([string]$RuntimeUrl, [string]$Token, [string]$InvocationId) {
    $pods = @(Invoke-Kubectl @("-n", $namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=runtime-gateway", "-o", "name")) |
        ForEach-Object { $_.Trim() -replace '^pod/', '' } |
        Where-Object { $_ }
    if ($pods.Count -lt 2) { throw "V2-03 SSE requires at least two Runtime Gateway Pods." }
    $firstPort = Start-PodPortForward $namespaces.runtime $pods[0] 8080 "sse-pod-1"
    $secondPort = Start-PodPortForward $namespaces.runtime $pods[1] 8080 "sse-pod-2"
    $firstEvents = Invoke-SseCurl "http://127.0.0.1:$firstPort/gateway/v1/invocations/$InvocationId/events" $Token
    if ($firstEvents -notmatch '(?m)^id:\s*1\s*$') { throw "First Gateway Pod did not replay the persisted SSE Cursor." }
    $lastCursor = [int]([regex]::Matches($firstEvents, '(?m)^id:\s*(\d+)\s*$') | Select-Object -Last 1).Groups[1].Value
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-redis", "--replicas=0") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "delete", "pvc", "data-runtime-redis-0", "--wait=true", "--timeout=120s") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "statefulset/runtime-redis", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "statefulset/runtime-redis", "--timeout=180s") | Out-Null
    $secondEvents = Invoke-SseCurl "http://127.0.0.1:$secondPort/gateway/v1/invocations/$InvocationId/events" $Token ([string][Math]::Max(0, $lastCursor - 1))
    if ($secondEvents -notmatch "(?m)^id:\s*$lastCursor\s*$") { throw "Second Gateway Pod did not replay MySQL events after Runtime Redis was rebuilt." }
}

function Assert-VaultDomainPermissions([string]$TenantId, [string]$WebhookId) {
    $controlToken = (Invoke-Kubectl @("-n", $namespaces.control, "get", "secret", "agentx-control-secrets", "-o", "jsonpath={.data.AGENTX_CONTROL_VAULT_TOKEN}")) -join ""
    $runtimeToken = (Invoke-Kubectl @("-n", $namespaces.runtime, "get", "secret", "agentx-runtime-secrets", "-o", "jsonpath={.data.AGENTX_RUNTIME_VAULT_TOKEN}")) -join ""
    $controlToken = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($controlToken))
    $runtimeToken = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($runtimeToken))
    $path = "secret/data/tenants/$TenantId/webhooks/$WebhookId"
    $controlRead = Invoke-Kubectl @("-n", $namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$controlToken' vault read '$path' >/dev/null 2>&1; echo `$?")
    $runtimeWrite = Invoke-Kubectl @("-n", $namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$runtimeToken' vault kv put -mount=secret 'tenants/$TenantId/webhooks/$WebhookId' value=forbidden >/dev/null 2>&1; echo `$?")
    $runtimeRead = Invoke-Kubectl @("-n", $namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$runtimeToken' vault kv get -mount=secret 'tenants/$TenantId/webhooks/$WebhookId' >/dev/null 2>&1; echo `$?")
    if (($controlRead -join "").Trim() -eq "0") { throw "Control Vault writer was able to read Webhook plaintext." }
    if (($runtimeWrite -join "").Trim() -eq "0") { throw "Runtime Vault reader was able to write Webhook plaintext." }
    if (($runtimeRead -join "").Trim() -ne "0") { throw "Runtime Vault reader could not read the versioned Webhook secret." }
}

function Seed-WaitResumeFixture([string]$InvocationId, [string]$ExecutionId) {
    $tokenBytes = New-Object byte[] 32
    [Security.Cryptography.RandomNumberGenerator]::Fill($tokenBytes)
    $token = ConvertTo-Base64Url $tokenBytes
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($token))).ToLowerInvariant()
    $nodeId = "018f0000-0000-7000-8000-000000000061"
    $resumeId = "018f0000-0000-7000-8000-000000000062"
    Invoke-RuntimeMySql "INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES(UUID_TO_BIN('$nodeId'),UUID_TO_BIN('018f0000-0000-7000-8000-000000000001'),UUID_TO_BIN('$ExecutionId'),'wait-e2e','wait_e2e','Wait E2E','wait',1,1,1,1,'waiting','builtin'); INSERT INTO execution_resume_tokens(id,tenant_id,execution_id,node_execution_id,token_hash,resume_kind,status,authentication_mode,expires_at) VALUES(UUID_TO_BIN('$resumeId'),UUID_TO_BIN('018f0000-0000-7000-8000-000000000001'),UUID_TO_BIN('$ExecutionId'),UUID_TO_BIN('$nodeId'),'$hash','webhook','active','none',DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 5 MINUTE));" | Out-Null
    return $token
}

function Assert-ArtifactReplay([string]$RuntimeUrl, [string]$Token) {
    $path = Join-Path $artifactDirectory "artifact-upload.txt"
    Set-Content -LiteralPath $path -NoNewline -Value "agentx-v2-artifact"
    $key = "v2-03-$RunId-artifact"
    $upload = {
        param([string]$Name)
        $responsePath = Join-Path $artifactDirectory "artifact-$Name.json"
        $status = (& curl.exe -sS --output $responsePath --write-out "%{http_code}" --request POST -H "Authorization: Bearer $Token" -H "Idempotency-Key: $key" -F "content=@$path;type=text/plain" "$RuntimeUrl/gateway/v1/artifacts") -join ""
        $body = Get-Content -LiteralPath $responsePath -Raw
        if ($status -ne "201") { throw "Artifact $Name returned HTTP ${status}: $body" }
        return $body | ConvertFrom-Json
    }
    $first = & $upload "first"
    $second = & $upload "replay"
    if ($first.artifactId -ne $second.artifactId -or $first.sha256 -ne $second.sha256) { throw "Artifact response-loss replay created another object." }
    Set-Content -LiteralPath $path -NoNewline -Value "different-artifact"
    $conflictResponse = Join-Path $artifactDirectory "artifact-conflict.json"
    $conflictStatus = (& curl.exe -sS --output $conflictResponse --write-out "%{http_code}" --request POST -H "Authorization: Bearer $Token" -H "Idempotency-Key: $key" -F "content=@$path;type=text/plain" "$RuntimeUrl/gateway/v1/artifacts") -join ""
    if ($conflictStatus -ne "409") { throw "Artifact Idempotency conflict returned HTTP $conflictStatus instead of 409." }
    $temporaryObjects = (Invoke-ObjectStorageAdmin @("ls", "--recursive", "e2e/agentx-runtime/temporary/")) -join ""
    if ($temporaryObjects.Trim()) { throw "Artifact upload left temporary Runtime OSS objects." }
    return $first
}

function Assert-Cancel([string]$RuntimeUrl, [string]$Token) {
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "deployment/workflow-runtime", "deployment/workflow-worker", "--replicas=0") | Out-Null
    $invocation = Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $Token"; "Idempotency-Key" = "v2-03-$RunId-cancel-invocation" } -ContentType application/json -Body (@{ input = @{ message = "cancel" }; responseMode = "async" } | ConvertTo-Json -Depth 5)
    $headers = @{ Authorization = "Bearer $Token"; "Idempotency-Key" = "v2-03-$RunId-cancel-command" }
    $cancel = Invoke-RestMethod "$RuntimeUrl/gateway/v1/invocations/$($invocation.id)/cancel" -Method Post -Headers $headers
    $replay = Invoke-RestMethod "$RuntimeUrl/gateway/v1/invocations/$($invocation.id)/cancel" -Method Post -Headers $headers
    if (!$cancel.accepted -or !$replay.replayed) { throw "Cancel did not replay the unique Runtime Command." }
    $count = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM runtime_commands WHERE tenant_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000001') AND command_type='cancel_execution' AND idempotency_key='v2-03-$RunId-cancel-command';" -join "")
    if ($count -ne 1) { throw "Cancel created $count Runtime Commands." }
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "deployment/workflow-runtime", "--replicas=2") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "deployment/workflow-runtime", "--timeout=180s") | Out-Null
    $cancelled = Wait-InvocationTerminal $RuntimeUrl $Token $invocation.id
    if ($cancelled.status -ne "cancelled") { throw "Cancel lost its race with Execution terminal state: $($cancelled.status)" }
    Invoke-Kubectl @("-n", $namespaces.runtime, "scale", "deployment/workflow-worker", "--replicas=2") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.runtime, "rollout", "status", "deployment/workflow-worker", "--timeout=180s") | Out-Null
    return $cancelled
}

function Initialize-ObjectStorageAdmin {
    if ($script:objectStorageAdminReady) { return }
    $overrides = @{
        spec = @{
            serviceAccountName = "object-storage"
            automountServiceAccountToken = $false
            containers = @(@{
                name = "agentx-object-e2e"
                image = "quay.io/minio/mc:latest"
                imagePullPolicy = "IfNotPresent"
                command = @("sleep", "3600")
                envFrom = @(@{ secretRef = @{ name = "agentx-dependencies-secrets" } })
            })
        }
    } | ConvertTo-Json -Compress -Depth 8
    Invoke-Kubectl @(
        "-n", $namespaces.dependencies,
        "run", "agentx-object-e2e",
        "--restart=Never",
        "--image=quay.io/minio/mc:latest",
        "--labels=agentx.io/plane=dependencies",
        "--overrides=$overrides"
    ) | Out-Null
    Invoke-Kubectl @("-n", $namespaces.dependencies, "wait", "--for=condition=Ready", "pod/agentx-object-e2e", "--timeout=120s") | Out-Null
    $script:objectStorageAdminReady = $true
}

function Invoke-ObjectStorageAdmin([string[]]$Arguments) {
    Initialize-ObjectStorageAdmin
    $kubectlArguments = @(
        "-n", $namespaces.dependencies,
        "exec", "pod/agentx-object-e2e", "--",
        "sh", "-ec",
        'mc alias set e2e http://object-storage:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null; mc "$@"',
        "agentx-object-e2e"
    )
    $kubectlArguments += $Arguments
    return Invoke-Kubectl $kubectlArguments
}

function Seed-NoOpWorkflow {
    $tenant = "018f0000-0000-7000-8000-000000000001"
    $user = "018f0000-0000-7000-8000-000000000002"
    $workflow = "018f0000-0000-7000-8000-000000000003"
    $identity = "018f0000-0000-7000-8000-000000000004"
    $department = "018f0000-0000-7000-8000-000000000005"
    $environment = "018f0000-0000-7000-8000-000000000007"
    $version = "018f0000-0000-7000-8000-000000000008"
    $versionTwo = "018f0000-0000-7000-8000-00000000000b"
    $workflowDeployment = "018f0000-0000-7000-8000-000000000009"
    $workflowDeploymentTwo = "018f0000-0000-7000-8000-00000000000c"
    $application = "018f0000-0000-7000-8000-00000000000a"
    $definition = '{"schemaVersion":"4.0","start":{"inputs":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"contexts":{}},"nodes":[{"id":"pass","key":"pass","type":"no_op","typeVersion":1,"name":"Pass","parameters":{},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],"connections":[{"id":"start-pass","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"pass","targetHandle":"main","order":0},{"id":"pass-end","sourceNodeId":"pass","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}],"end":{"outputs":{"message":{"expression":"${{ outputs.pass.main.current.json.message }}","schema":{"type":"string"},"required":true}}},"settings":{"activationBudget":20,"executionOrder":"deterministic"}}'
    $escaped = $definition.Replace("'", "''")
    $sql = @"
INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(UUID_TO_BIN('$workflow'),UUID_TO_BIN('$tenant'),'V2 No Op','active','private',UUID_TO_BIN('$user'),UUID_TO_BIN('$department')) ON DUPLICATE KEY UPDATE name=VALUES(name);
INSERT INTO workflow_service_identities(id,tenant_id,workflow_id,status,version) VALUES(UUID_TO_BIN('$identity'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),'active',1) ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_environments(id,tenant_id,code,name,is_builtin,status) VALUES(UUID_TO_BIN('$environment'),UUID_TO_BIN('$tenant'),'production','Production',TRUE,'active') ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(UUID_TO_BIN('$version'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),1,1,'4.0','$escaped','sha256:e2e',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE definition_json=VALUES(definition_json);
INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(UUID_TO_BIN('$versionTwo'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),2,2,'4.0','$escaped','sha256:e2e-v2',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE definition_json=VALUES(definition_json);
INSERT INTO workflow_deployments(id,tenant_id,workflow_id,environment_id,workflow_version_id,sequence_number,status,source,created_by) VALUES(UUID_TO_BIN('$workflowDeployment'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),UUID_TO_BIN('$environment'),UUID_TO_BIN('$version'),1,'active','publish',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_deployments(id,tenant_id,workflow_id,environment_id,workflow_version_id,sequence_number,status,source,created_by) VALUES(UUID_TO_BIN('$workflowDeploymentTwo'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),UUID_TO_BIN('$environment'),UUID_TO_BIN('$versionTwo'),2,'active','publish',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_deployment_heads(tenant_id,workflow_id,environment_id,active_deployment_id,version) VALUES(UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),UUID_TO_BIN('$environment'),UUID_TO_BIN('$workflowDeployment'),1) ON DUPLICATE KEY UPDATE active_deployment_id=VALUES(active_deployment_id);
INSERT INTO applications(id,tenant_id,workflow_id,name,slug,visibility,owner_user_id,owner_department_id,status) VALUES(UUID_TO_BIN('$application'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$workflow'),'V2 No Op','v2-no-op','private',UUID_TO_BIN('$user'),UUID_TO_BIN('$department'),'draft') ON DUPLICATE KEY UPDATE status='draft';
"@
    Invoke-MySql $sql | Out-Null
    $triggerWorkflow = "018f0000-0000-7000-8000-000000000021"
    $triggerIdentity = "018f0000-0000-7000-8000-000000000022"
    $triggerVersion = "018f0000-0000-7000-8000-000000000023"
    $triggerDeployment = "018f0000-0000-7000-8000-000000000024"
    $triggerApplication = "018f0000-0000-7000-8000-000000000025"
    $triggerDefinition = '{"schemaVersion":"4.0","start":{"inputs":{"type":"object","additionalProperties":true},"contexts":{}},"nodes":[{"id":"remote-action","key":"remote_action","type":"remote_action","typeVersion":1,"name":"Remote Action","parameters":{"endpoint":"http://echo-node.' + $namespaces.dependencies + '.svc:8080","pollIntervalSeconds":60,"eventId":"v2-03-poll-' + $RunId + '","pollInput":{"message":"v2-03-poll"}},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],"connections":[{"id":"start-remote","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"remote-action","targetHandle":"main","order":0},{"id":"remote-end","sourceNodeId":"remote-action","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}],"end":{"outputs":{}},"settings":{"activationBudget":20,"executionOrder":"deterministic"}}'
    $triggerEscaped = $triggerDefinition.Replace("'", "''")
    $triggerSql = @"
INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(UUID_TO_BIN('$triggerWorkflow'),UUID_TO_BIN('$tenant'),'V2 Trigger','active','private',UUID_TO_BIN('$user'),UUID_TO_BIN('$department')) ON DUPLICATE KEY UPDATE name=VALUES(name);
INSERT INTO workflow_service_identities(id,tenant_id,workflow_id,status,version) VALUES(UUID_TO_BIN('$triggerIdentity'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$triggerWorkflow'),'active',1) ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(UUID_TO_BIN('$triggerVersion'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$triggerWorkflow'),1,1,'4.0','$triggerEscaped','sha256:v2-03-trigger',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE definition_json=VALUES(definition_json);
INSERT INTO workflow_deployments(id,tenant_id,workflow_id,environment_id,workflow_version_id,sequence_number,status,source,created_by) VALUES(UUID_TO_BIN('$triggerDeployment'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$triggerWorkflow'),UUID_TO_BIN('$environment'),UUID_TO_BIN('$triggerVersion'),1,'active','publish',UUID_TO_BIN('$user')) ON DUPLICATE KEY UPDATE status='active';
INSERT INTO workflow_deployment_heads(tenant_id,workflow_id,environment_id,active_deployment_id,version) VALUES(UUID_TO_BIN('$tenant'),UUID_TO_BIN('$triggerWorkflow'),UUID_TO_BIN('$environment'),UUID_TO_BIN('$triggerDeployment'),1) ON DUPLICATE KEY UPDATE active_deployment_id=VALUES(active_deployment_id);
INSERT INTO applications(id,tenant_id,workflow_id,name,slug,visibility,owner_user_id,owner_department_id,status) VALUES(UUID_TO_BIN('$triggerApplication'),UUID_TO_BIN('$tenant'),UUID_TO_BIN('$triggerWorkflow'),'V2 Trigger','v2-trigger','private',UUID_TO_BIN('$user'),UUID_TO_BIN('$department'),'draft') ON DUPLICATE KEY UPDATE status='draft';
"@
    Invoke-MySql $triggerSql | Out-Null
    return @{ application = $application; version = $version; versionTwo = $versionTwo; workflowDeployment = $workflowDeployment; workflowDeploymentTwo = $workflowDeploymentTwo; environment = $environment; triggerApplication = $triggerApplication; triggerVersion = $triggerVersion }
}

try {
    $timeline.Add("$(Get-Date -Format o) V2-03 E2E start")
    $developmentReplicas = @(Get-DevelopmentReplicas)
    $developmentReplicas | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $artifactDirectory "development-replicas.json")
    if ($ScaleDownDevelopment) { Set-DevelopmentReplicas $developmentReplicas $true }

    & $deploy -Action Install -ConfigFile $profilePath -RunId "$Stage-$RunId" -BuildImages:$BuildImages -CleanupOnFailure:(!$KeepOnFailure)
    if ($LASTEXITCODE -ne 0) { throw "V2 deployment failed." }
    if ($BuildImages) {
        & (Join-Path $PSScriptRoot "build-images.ps1") -Tag ([string]$profile.images.tag) -Namespace $namespaces.dependencies -Services @("runtime-fault-proxy", "echo-node")
        if ($LASTEXITCODE -ne 0) { throw "V2-03 fault proxy image build failed." }
    }
    Install-TriggerProvider
    $controlForward = Start-PortForward $namespaces.control "platform-control" 18080 8080
    $runtimeForward = Start-PortForward $namespaces.runtime "runtime-gateway-public" 18081 8080
    $controlUrl = "http://127.0.0.1:18080"
    $runtimeUrl = "http://127.0.0.1:18081"

    $login = Invoke-RestMethod "$controlUrl/api/v1/auth/login" -Method Post -ContentType application/json -Body (@{ username = "agentx-v2-e2e"; password = "agentx-v2-e2e-password" } | ConvertTo-Json)
    $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $fixture = Seed-NoOpWorkflow
    $key = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/api-keys" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 E2E" } | ConvertTo-Json)
    $triggerKey = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/api-keys" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 Trigger E2E" } | ConvertTo-Json)
    $draftWebhook = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/webhooks" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 Draft Webhook" } | ConvertTo-Json)
    if (!$draftWebhook.secret) { throw "Control did not return the one-time Webhook secret." }
    $draftSchedule = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/schedules" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 Draft Schedule"; cronExpression = "*/1 * * * * *"; timezone = "UTC"; input = @{ message = "v2-03-schedule" }; misfirePolicy = "fire_once" } | ConvertTo-Json -Depth 6)
    try {
        Invoke-RestMethod "$runtimeUrl/gateway/v1/webhooks/$($draftWebhook.publicId)" -Method Post -Headers @{ "Idempotency-Key" = "v2-03-$RunId-draft"; "X-Agentx-Timestamp" = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); "X-Agentx-Signature" = "invalid" } -ContentType application/json -Body '{}' | Out-Null
        throw "Unpublished Webhook draft was callable from Runtime."
    } catch {
        if ($_.Exception.Message -eq "Unpublished Webhook draft was callable from Runtime.") { throw }
        if ([int]$_.Exception.Response.StatusCode -ne 404) { throw "Unpublished Webhook returned a non-404 response." }
    }
    $draftBindings = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM trigger_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)');" -join "")
    if ($draftBindings -ne 0) { throw "Trigger draft leaked into Runtime before Deployment." }
    $draftScheduleBindings = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM trigger_bindings WHERE id=UUID_TO_BIN('$($draftSchedule.id)');" -join "")
    if ($draftScheduleBindings -ne 0) { throw "Schedule draft leaked into Runtime before Deployment." }
    Install-FaultProxy
    $faultProxyForward = Start-PortForward $namespaces.runtime "runtime-fault-proxy" 18082 8080
    if ($controlForward -and !$controlForward.HasExited) { Stop-Process -Id $controlForward.Id -Force }
    $controlForward = Start-PortForward $namespaces.control "platform-control" 18080 8080
    Invoke-FaultProxy "arm" "prepare"
    Invoke-FaultProxy "arm" "activate"
    $deployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.version; environmentId = $fixture.environment; sessionVersionPolicy = "pinned" } | ConvertTo-Json)
    if ($deployment.status -eq "active") { throw "Deployment must not be active before Runtime activation." }
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $prepareReceipt = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM publish_receipts WHERE operation='prepare' AND bundle_id IS NOT NULL;" -join "")
        if ($prepareReceipt -gt 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($prepareReceipt -lt 1) { throw "Runtime never persisted the fault-injected Prepare receipt." }
    $routeBeforeActivate = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM application_routes WHERE application_id=UUID_TO_BIN('$($fixture.application)') AND active_bundle_id IS NOT NULL;" -join "")
    if ($routeBeforeActivate -ne 0) { throw "Prepare-only Bundle received a production Route before Activate." }
    Invoke-FaultProxy "release" "prepare"
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $activateReceipt = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM publish_receipts WHERE operation='activate' AND status='accepted';" -join "")
        if ($activateReceipt -gt 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($activateReceipt -lt 1) { throw "Runtime never persisted the fault-injected Activate receipt." }
    $faultState = Invoke-RestMethod "http://127.0.0.1:18082/e2e/faults"
    if ($faultState.activate -ne "held") { throw "Activate response was not held after the Runtime committed its receipt." }
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/platform-control", "--replicas=0") | Out-Null
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $publisherPods = @(Invoke-Kubectl @("-n", $namespaces.control, "get", "pods", "-l", "app.kubernetes.io/name=platform-control", "-o", "name"))
        if ($publisherPods.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($publisherPods.Count -ne 0) { throw "Publisher Pod did not terminate while the committed Activate response was held." }
    $timeline.Add("$(Get-Date -Format o) Runtime Activate committed; Publisher killed before local transition")
    Invoke-FaultProxy "release" "activate"
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/platform-control", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "rollout", "status", "deployment/platform-control", "--timeout=180s") | Out-Null
    if ($controlForward -and !$controlForward.HasExited) { Stop-Process -Id $controlForward.Id -Force }
    $controlForward = Start-PortForward $namespaces.control "platform-control" 18080 8080
    $attempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $deployment.publishAttemptId
    $receiptFacts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM publish_receipts WHERE operation='prepare' AND bundle_id=UUID_TO_BIN('$($attempt.bundleId)')),(SELECT COUNT(*) FROM publish_receipts WHERE operation='activate' AND bundle_id=UUID_TO_BIN('$($attempt.bundleId)')),(SELECT COUNT(*) FROM deployment_heads WHERE application_id=UUID_TO_BIN('$($fixture.application)'));" -join "`t")
    if ($receiptFacts -ne "1`t1`t1") { throw "Response-loss replay created duplicate Runtime facts: $receiptFacts" }
    $timeline.Add("$(Get-Date -Format o) Bundle active $($attempt.bundleId)")
    $triggerDeployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.triggerVersion; environmentId = $fixture.environment; sessionVersionPolicy = "pinned" } | ConvertTo-Json)
    $triggerAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.triggerApplication $triggerDeployment.publishAttemptId
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $triggerFacts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM webhook_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND status='active'),(SELECT COUNT(*) FROM trigger_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND trigger_kind='schedule' AND status='active'),(SELECT COUNT(*) FROM trigger_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND trigger_kind='poll' AND status='active'),(SELECT COUNT(*) FROM trigger_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND trigger_kind='lifecycle' AND status='active');" -join "`t")
        if ($triggerFacts -eq "1`t1`t1`t1") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($triggerFacts -ne "1`t1`t1`t1") { throw "Deployment did not activate Webhook/Schedule/Poll/Lifecycle Runtime Bindings: $triggerFacts" }
    $triggerInvocationDeadline = (Get-Date).AddMinutes(2)
    do {
        $triggerInvocations = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM application_invocations WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND caller_type='schedule'),(SELECT COUNT(*) FROM application_invocations WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND caller_type='poll'),(SELECT COUNT(*) FROM application_invocations WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND caller_type='lifecycle');" -join "`t")
        if (($triggerInvocations -split "`t") -notcontains "0") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $triggerInvocationDeadline)
    if (($triggerInvocations -split "`t") -contains "0") { throw "Runtime Trigger roles did not create Schedule, Poll and Lifecycle Invocations: $triggerInvocations" }
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $webhookBody = '{"message":"v2-03-webhook"}'
    $webhookHeaders = @{ "Idempotency-Key" = "v2-03-$RunId-webhook"; "X-Agentx-Timestamp" = $timestamp; "X-Agentx-Signature" = Get-WebhookSignature $draftWebhook.secret $timestamp $webhookBody }
    $webhookInvocation = Invoke-RestMethod "$runtimeUrl/gateway/v1/webhooks/$($draftWebhook.publicId)" -Method Post -Headers $webhookHeaders -ContentType application/json -Body $webhookBody
    $webhookReplay = Invoke-RestMethod "$runtimeUrl/gateway/v1/webhooks/$($draftWebhook.publicId)" -Method Post -Headers $webhookHeaders -ContentType application/json -Body $webhookBody
    if ($webhookInvocation.id -ne $webhookReplay.id) { throw "Webhook idempotent replay created another Invocation." }
    $badWebhookHeaders = $webhookHeaders.Clone(); $badWebhookHeaders["Idempotency-Key"] = "v2-03-$RunId-webhook-bad"; $badWebhookHeaders["X-Agentx-Signature"] = "invalid"
    try {
        Invoke-RestMethod "$runtimeUrl/gateway/v1/webhooks/$($draftWebhook.publicId)" -Method Post -Headers $badWebhookHeaders -ContentType application/json -Body $webhookBody | Out-Null
        throw "A Webhook with a tampered signature was accepted."
    } catch {
        if ($_.Exception.Message -eq "A Webhook with a tampered signature was accepted.") { throw }
        if ([int]$_.Exception.Response.StatusCode -ne 401) { throw "Tampered Webhook returned a non-401 response." }
    }
    Assert-VaultDomainPermissions "018f0000-0000-7000-8000-000000000001" $draftWebhook.id
    $triggerDuplicateFacts = Invoke-RuntimeMySql "SELECT caller_type,idempotency_key,COUNT(*) FROM application_invocations WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND caller_type IN ('schedule','poll','lifecycle') GROUP BY caller_type,idempotency_key HAVING COUNT(*)>1;"
    if (($triggerDuplicateFacts -join "").Trim()) { throw "Two Trigger Role replicas created duplicate business facts: $($triggerDuplicateFacts -join ';')" }
    $timeline.Add("$(Get-Date -Format o) Trigger draft published; Webhook/Schedule/Poll/Lifecycle active bundle=$($triggerAttempt.bundleId)")
    $disabledSchedule = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/schedules/$($draftSchedule.id)" -Method Patch -Headers $headers -ContentType application/json -Body (@{
        name = $draftSchedule.name
        cronExpression = $draftSchedule.cronExpression
        timezone = $draftSchedule.timezone
        input = $draftSchedule.input
        misfirePolicy = $draftSchedule.misfirePolicy
        status = "disabled"
        version = $draftSchedule.version
    } | ConvertTo-Json -Depth 6)
    if ($disabledSchedule.status -ne "disabled") { throw "Schedule draft was not disabled before the isolation scenarios." }
    $triggerDisableDeployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.triggerApplication)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.triggerVersion; environmentId = $fixture.environment; sessionVersionPolicy = "pinned" } | ConvertTo-Json)
    $triggerDisableAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.triggerApplication $triggerDisableDeployment.publishAttemptId
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $activeSchedules = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM trigger_bindings WHERE application_id=UUID_TO_BIN('$($fixture.triggerApplication)') AND trigger_kind='schedule' AND status='active';" -join "")
        if ($activeSchedules -eq 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($activeSchedules -ne 0) { throw "Disabled Schedule remained active after Deployment." }
    $timeline.Add("$(Get-Date -Format o) High-frequency Schedule disabled by deployed revision bundle=$($triggerDisableAttempt.bundleId)")
    $runtimeObject = (Invoke-RuntimeMySql "SELECT object_key,content_hash,size_bytes,status FROM runtime_objects WHERE tenant_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000001') AND object_id=UUID_TO_BIN('$($fixture.version)');" -join "`t")
    if (($runtimeObject -split "`t").Count -ne 4 -or ($runtimeObject -split "`t")[3] -ne "ready") {
        throw "Bundle object was not copied to Runtime OSS: $runtimeObject"
    }
    Invoke-ObjectStorageAdmin @("rm", "--recursive", "--force", "e2e/agentx-control/control/") | Out-Null
    $runtimeObjectCount = (Invoke-ObjectStorageAdmin @("ls", "--recursive", "e2e/agentx-runtime/runtime/") | Select-String $fixture.version | Measure-Object).Count
    if ($runtimeObjectCount -lt 1) { throw "Runtime OSS lost the copied Bundle object after Control source deletion." }
    $timeline.Add("$(Get-Date -Format o) Control source object deleted; Runtime copy retained")

    Invoke-MySql "UPDATE workflow_deployment_heads SET active_deployment_id=UUID_TO_BIN('$($fixture.workflowDeploymentTwo)'),version=version+1 WHERE tenant_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000001') AND workflow_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000003') AND environment_id=UUID_TO_BIN('$($fixture.environment)');" | Out-Null
    $secondDeployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.versionTwo; environmentId = $fixture.environment; sessionVersionPolicy = "pinned" } | ConvertTo-Json)
    $secondAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $secondDeployment.publishAttemptId
    Wait-ApplicationHead $secondAttempt.bundleId
    $acceptedBeforeRollback = Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($key.secret)"; "Idempotency-Key" = "v2-03-$RunId-in-flight" } -ContentType application/json -Body (@{ input = @{ message = "in-flight-v2" }; responseMode = "async" } | ConvertTo-Json -Depth 5)
    $pinnedSession = Assert-SessionPolicy $runtimeUrl $key.secret "v2-no-op" "pinned" $secondAttempt.bundleId "v2-03-$RunId-session-pinned"
    $rollback = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments/$($deployment.id):rollback" -Method Post -Headers $headers
    $rollbackAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $rollback.id
    Wait-ApplicationHead $attempt.bundleId
    $inFlightBundle = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(bundle_id) FROM workflow_executions WHERE id=UUID_TO_BIN('$($acceptedBeforeRollback.executionId)');" -join "")
    if ($inFlightBundle.ToLowerInvariant() -ne $secondAttempt.bundleId.ToLowerInvariant()) { throw "In-flight Execution changed its pinned Bundle during rollback." }
    $rollbackEpoch = [int](Invoke-RuntimeMySql "SELECT admission_epoch FROM deployment_heads WHERE application_id=UUID_TO_BIN('$($fixture.application)');" -join "")
    $timeline.Add("$(Get-Date -Format o) second Bundle activated and first Bundle rolled back")

    $followDeployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.versionTwo; environmentId = $fixture.environment; sessionVersionPolicy = "follow_deployment" } | ConvertTo-Json)
    $followAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $followDeployment.publishAttemptId
    $followSession = Assert-SessionPolicy $runtimeUrl $key.secret "v2-no-op" "follow_deployment" $followAttempt.bundleId "v2-03-$RunId-session-follow"
    $followInvocation = Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($key.secret)"; "Idempotency-Key" = "v2-03-$RunId-follow-invocation" } -ContentType application/json -Body (@{ input = @{ message = "follow" }; sessionId = $followSession.id; responseMode = "async" } | ConvertTo-Json -Depth 5)
    if ($followInvocation.bundleId.ToLowerInvariant() -ne $followAttempt.bundleId.ToLowerInvariant()) { throw "follow_deployment Session did not use the current Head Bundle." }
    Invoke-MySql "UPDATE workflow_deployment_heads SET active_deployment_id=UUID_TO_BIN('$($fixture.workflowDeployment)'),version=version+1 WHERE tenant_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000001') AND workflow_id=UUID_TO_BIN('018f0000-0000-7000-8000-000000000003') AND environment_id=UUID_TO_BIN('$($fixture.environment)');" | Out-Null
    $manualDeployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.version; environmentId = $fixture.environment; sessionVersionPolicy = "manual_upgrade" } | ConvertTo-Json)
    $manualAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $manualDeployment.publishAttemptId
    $manualSession = Assert-SessionPolicy $runtimeUrl $key.secret "v2-no-op" "manual_upgrade" $manualAttempt.bundleId "v2-03-$RunId-session-manual"
    $upgradedSession = Invoke-RestMethod "$controlUrl/api/v1/sessions/$($manualSession.id)/upgrade" -Method Post -Headers $headers -ContentType application/json -Body (@{ workflowVersionId = $fixture.versionTwo; version = $manualSession.version } | ConvertTo-Json)
    if ($upgradedSession.version -ne ($manualSession.version + 1) -or $upgradedSession.workflowVersionId.ToLowerInvariant() -ne $fixture.versionTwo.ToLowerInvariant()) { throw "manual_upgrade Session did not CAS to Workflow Version 2." }
    $manualBundle = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(bundle_id) FROM application_sessions WHERE id=UUID_TO_BIN('$($manualSession.id)');" -join "")
    if ($manualBundle.ToLowerInvariant() -ne $followAttempt.bundleId.ToLowerInvariant()) { throw "manual_upgrade Session did not switch its pinned Bundle Reference." }
    $messageInvocation = Assert-MessageReplay $runtimeUrl $key.secret $manualSession.id
    $terminalMessageInvocation = Wait-InvocationTerminal $runtimeUrl $key.secret $messageInvocation.id
    Assert-SseReplayAndRedisRebuild $runtimeUrl $key.secret $terminalMessageInvocation.id
    $userInvocation = Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($login.accessToken)"; "Idempotency-Key" = "v2-03-$RunId-user-jwt" } -ContentType application/json -Body (@{ input = @{ message = "browser-jwt" }; responseMode = "async" } | ConvertTo-Json -Depth 5)
    if (!$userInvocation.id) { throw "Browser RS256 user JWT did not invoke the exact Runtime Application grant." }
    $artifact = Assert-ArtifactReplay $runtimeUrl $key.secret
    $cancelled = Assert-Cancel $runtimeUrl $key.secret
    $waitToken = Seed-WaitResumeFixture $cancelled.id $cancelled.executionId
    $resumeHeaders = @{ "Idempotency-Key" = "v2-03-$RunId-resume" }
    $resumeBody = @{ outputPort = "main"; payload = @{ message = "resume" } } | ConvertTo-Json -Depth 5
    $resume = Invoke-RestMethod "$runtimeUrl/gateway/v1/waits/$waitToken/resume" -Method Post -Headers $resumeHeaders -ContentType application/json -Body $resumeBody
    $resumeReplay = Invoke-RestMethod "$runtimeUrl/gateway/v1/waits/$waitToken/resume" -Method Post -Headers $resumeHeaders -ContentType application/json -Body $resumeBody
    if (!$resume.accepted -or !$resumeReplay.replayed) { throw "Wait Resume did not persist and replay one Runtime Command." }
    $resumeCommandCount = [int](Invoke-RuntimeMySql "SELECT COUNT(*) FROM runtime_commands WHERE command_type='resume_wait' AND idempotency_key='v2-03-$RunId-resume';" -join "")
    if ($resumeCommandCount -ne 1) { throw "Wait Resume created $resumeCommandCount Runtime Commands." }
    $timeline.Add("$(Get-Date -Format o) pinned/follow/manual sessions, browser JWT, Artifact, Cancel, Resume, response-loss replay and MySQL-authoritative SSE verified")

    Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/api-keys/$($key.id)/revoke" -Method Post -Headers $headers | Out-Null
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $revokedState = (Invoke-RuntimeMySql "SELECT status,admission_epoch FROM api_key_admission WHERE key_id=UUID_TO_BIN('$($key.id)');" -join "`t")
        if (($revokedState -split "`t")[0] -eq "revoked") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if (($revokedState -split "`t")[0] -ne "revoked") { throw "API Key revoke did not reach Runtime." }
    $revokedEpoch = [int](($revokedState -split "`t")[1])
    if ($revokedEpoch -le $rollbackEpoch) { throw "API Key revoke did not advance Admission Epoch." }
    try {
        Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($key.secret)"; "Idempotency-Key" = "v2-02-$RunId-revoked" } -ContentType application/json -Body (@{ input = @{ message = "must-fail" }; responseMode = "async" } | ConvertTo-Json -Depth 5) | Out-Null
        throw "Revoked API Key unexpectedly created an Invocation."
    } catch {
        if ($_.Exception.Message -eq "Revoked API Key unexpectedly created an Invocation.") { throw }
    }
    $postRevokeRollback = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments/$($secondDeployment.id):rollback" -Method Post -Headers $headers
    $postRevokeAttempt = Wait-RejectedPublishAttempt $controlUrl $headers $fixture.application $postRevokeRollback.id
    $runtimeKey = (Invoke-RuntimeMySql "SELECT status,admission_epoch FROM api_key_admission WHERE key_id=UUID_TO_BIN('$($key.id)');" -join "`t")
    if ($runtimeKey -ne "revoked`t$revokedEpoch") { throw "Rollback restored stale API Key Admission: $runtimeKey" }
    $replacementKey = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/api-keys" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 E2E Offline" } | ConvertTo-Json)
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $replacementState = (Invoke-RuntimeMySql "SELECT status FROM api_key_admission WHERE key_id=UUID_TO_BIN('$($replacementKey.id)');" -join "")
        if ($replacementState -eq "active") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($replacementState -ne "active") { throw "Replacement API Key did not reach Runtime." }

    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/web-console", "deployment/platform-control", "--replicas=0") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "statefulset/control-mysql", "--replicas=0") | Out-Null
    if (!$controlForward.HasExited) { Stop-Process -Id $controlForward.Id -Force }
    $timeline.Add("$(Get-Date -Format o) Control plane stopped")

    $results = @()
    for ($index = 1; $index -le 10; $index++) {
        $body = @{ input = @{ message = "agentx-v2" }; responseMode = "async" } | ConvertTo-Json -Depth 5
        $accepted = Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($replacementKey.secret)"; "Idempotency-Key" = "v2-02-$RunId-$index" } -ContentType application/json -Body $body
        $deadline = (Get-Date).AddMinutes(2)
        do {
            Start-Sleep -Milliseconds 500
            $row = (Invoke-RuntimeMySql "SELECT status,JSON_UNQUOTE(JSON_EXTRACT(output_json,'$.message')),BIN_TO_UUID(bundle_id),admission_epoch FROM workflow_executions WHERE id=UUID_TO_BIN('$($accepted.executionId)');" -join "`t")
            $columns = $row -split "`t"
        } while ($columns[0] -notin @("succeeded", "failed") -and (Get-Date) -lt $deadline)
        if ($columns[0] -ne "succeeded" -or $columns[1] -ne "agentx-v2" -or $columns[2].ToLowerInvariant() -ne $manualAttempt.bundleId.ToLowerInvariant()) {
            throw "Execution $($accepted.executionId) did not reach the required authoritative result: $row"
        }
        $results += @{ invocationId = $accepted.invocationId; executionId = $accepted.executionId; bundleId = $columns[2]; admissionEpoch = $columns[3]; output = $columns[1] }
    }
    $results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $artifactDirectory "offline-executions.json")

    Invoke-Kubectl @("-n", $namespaces.control, "scale", "statefulset/control-mysql", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "rollout", "status", "statefulset/control-mysql", "--timeout=300s") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "scale", "deployment/platform-control", "deployment/web-console", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespaces.control, "rollout", "status", "deployment/platform-control", "--timeout=300s") | Out-Null
    if ($controlForward -and !$controlForward.HasExited) { Stop-Process -Id $controlForward.Id -Force }
    $controlForward = Start-PortForward $namespaces.control "platform-control" 18080 8080
    foreach ($result in $results) {
        if ($Stage -in @("05", "06")) {
            $query = Invoke-RestMethod "$controlUrl/api/v1/executions/$($result.executionId)" -Headers $headers
            $actualExecutionId = $query.id
            $actualBundleId = $query.bundleId
        }
        else {
            $query = Invoke-RestMethod "$controlUrl/api/v1/runtime-query/executions/$($result.executionId)" -Headers $headers
            $actualExecutionId = $query.summary.executionId
            $actualBundleId = $query.summary.bundleId
        }
        if ($actualExecutionId -ne $result.executionId -or $actualBundleId -ne $result.bundleId -or $query.output.message -ne "agentx-v2") {
            throw "Runtime Query returned a non-authoritative result for $($result.executionId)."
        }
    }
    $duplicates = Invoke-MySql "SELECT (SELECT COUNT(*) FROM execution_spec_bundles WHERE id=UUID_TO_BIN('$($attempt.bundleId)')),(SELECT COUNT(*) FROM execution_spec_bundles WHERE id=UUID_TO_BIN('$($secondAttempt.bundleId)')),(SELECT COUNT(*) FROM publish_attempts WHERE id IN (UUID_TO_BIN('$($attempt.id)'),UUID_TO_BIN('$($secondAttempt.id)'),UUID_TO_BIN('$($rollbackAttempt.id)'),UUID_TO_BIN('$($postRevokeAttempt.id)'))),(SELECT COUNT(*) FROM outbox WHERE status IN ('pending','processing','failed'));" -join "`t"
    if ($duplicates -ne "1`t1`t4`t0") { throw "Control recovery did not converge: $duplicates" }
    $timeline.Add("$(Get-Date -Format o) Control recovery converged")
    Assert-RuntimeMySqlUnavailable $runtimeUrl $replacementKey.secret
    $timeline.Add("$(Get-Date -Format o) Runtime MySQL outage returned 503 and left no orphan facts")
    $succeeded = $true
    if (-not [string]::IsNullOrWhiteSpace($ContextOutputPath)) {
        $contextDirectory = Split-Path -Parent $ContextOutputPath
        if ($contextDirectory) { New-Item -ItemType Directory -Force -Path $contextDirectory | Out-Null }
        [ordered]@{
            apiVersion = "agentx.io/v2-e2e-context/v1"
            stage = "v2-$Stage"
            runId = $RunId
            profilePath = $profilePath
            artifactDirectory = $artifactDirectory
            namespaces = $namespaces
        } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ContextOutputPath
    }
}
finally {
    $timeline.Add("$(Get-Date -Format o) cleanup begin success=$succeeded")
    foreach ($process in @($controlForward, $runtimeForward, $faultProxyForward) + @($gatewayPodForwards)) { if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force } }
    foreach ($target in @(
        @{ Namespace = $namespaces.control; Resource = "deployment/platform-control" },
        @{ Namespace = $namespaces.runtime; Resource = "deployment/runtime-gateway" },
        @{ Namespace = $namespaces.runtime; Resource = "deployment/workflow-runtime" },
        @{ Namespace = $namespaces.runtime; Resource = "deployment/workflow-worker" }
    )) {
        $safeName = $target.Resource.Replace('/', '-')
        try {
            $previousNativeErrorPreference = $PSNativeCommandUseErrorActionPreference
            $PSNativeCommandUseErrorActionPreference = $false
            & kubectl -n $target.Namespace logs $target.Resource --all-pods=true --prefix --tail=200 *> (Join-Path $artifactDirectory "$safeName.log")
        }
        finally {
            $PSNativeCommandUseErrorActionPreference = $previousNativeErrorPreference
        }
    }
    if (($succeeded -and !$KeepOnSuccess) -or (!$succeeded -and !$KeepOnFailure)) {
        & $deploy -Action Uninstall -ConfigFile $profilePath -RunId "$Stage-$RunId"
    }
    if ($ScaleDownDevelopment -and $developmentReplicas.Count -gt 0) { Set-DevelopmentReplicas $developmentReplicas $false }
    $timeline.Add("$(Get-Date -Format o) cleanup end")
    $timeline | Set-Content (Join-Path $artifactDirectory "timeline.txt")
}

if (!$succeeded) { throw "V2-03 E2E failed; see $artifactDirectory" }
$retained = if ($KeepOnSuccess) { " Temporary namespaces were retained for the caller." } else { "" }
Write-Output "V2-03 E2E passed. Evidence: $artifactDirectory$retained"
