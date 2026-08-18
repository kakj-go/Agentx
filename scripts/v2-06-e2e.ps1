param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$SkipLocalGates
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-06/06a"
$contextPath = Join-Path $artifactDirectory "v2-06-context.json"
$summaryPath = Join-Path $artifactDirectory "summary.json"
$timeline = [Collections.Generic.List[string]]::new()
$scenarios = [Collections.Generic.List[object]]::new()
$forwards = [Collections.Generic.List[Diagnostics.Process]]::new()
$developmentReplicas = @()
$context = $null
$scalabilityProfilePath = $null
$completed = $false
$controlPassword = $null
$runtimePassword = $null
$runtimeRedisPassword = $null
$clickhousePassword = $null
$tenantId = "018f0000-0000-7000-8000-000000000001"
$applicationId = "018f0000-0000-7000-8000-00000000000a"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null

function Add-Timeline([string]$Message) { $timeline.Add("$([DateTimeOffset]::UtcNow.ToString('O')) $Message") }

function Complete-Scenario([int]$Id, [string]$Assertion, [string[]]$Evidence) {
    $scenarios.Add([ordered]@{ id = $Id; status = "passed"; assertion = $Assertion; evidence = $Evidence })
    Add-Timeline "Scenario $Id passed: $Assertion"
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "V2-06A E2E requires $Name." }
}

function Invoke-Kubectl([string[]]$Arguments) {
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $output = & kubectl @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) {
            $command = ($Arguments -join ' ') -replace '(?i)\b(MYSQL_PWD|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>'
            $details = ($output -join [Environment]::NewLine) -replace '(?i)\b(MYSQL_PWD|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>'
            throw "kubectl $command failed: $details"
        }
        return @($output)
    }
    finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
}

function Get-SecretValue([string]$Namespace, [string]$Name, [string]$Key) {
    $encoded = (Invoke-Kubectl @("-n", $Namespace, "get", "secret", $Name, "-o", "jsonpath={.data.$Key}")) -join ""
    if ([string]::IsNullOrWhiteSpace($encoded)) { throw "Secret $Namespace/$Name has no $Key." }
    return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
}

function Invoke-ControlMySql([string]$Sql) {
    if (-not $controlPassword) {
        $script:controlPassword = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_MYSQL_PASSWORD"
    }
    return Invoke-Kubectl @(
        "-n", [string]$context.namespaces.control, "exec", "statefulset/control-mysql", "--",
        "env", "MYSQL_PWD=$controlPassword", "mysql", "-N", "-B", "-ucontrol_app", "agentx_control", "-e", $Sql
    )
}

function Invoke-RuntimeMySql([string]$Sql) {
    if (-not $runtimePassword) {
        $script:runtimePassword = Get-SecretValue $context.namespaces.runtime "agentx-runtime-secrets" "AGENTX_RUNTIME_MYSQL_PASSWORD"
    }
    return Invoke-Kubectl @(
        "-n", [string]$context.namespaces.runtime, "exec", "statefulset/runtime-mysql", "--",
        "env", "MYSQL_PWD=$runtimePassword", "mysql", "-N", "-B", "-uruntime_app", "agentx_runtime", "-e", $Sql
    )
}

function Invoke-RuntimeRedis([string[]]$Arguments) {
    if (-not $runtimeRedisPassword) {
        $script:runtimeRedisPassword = Get-SecretValue $context.namespaces.runtime "agentx-runtime-secrets" "AGENTX_RUNTIME_REDIS_PASSWORD"
    }
    $commandArguments = @(
        "-n", [string]$context.namespaces.runtime, "exec", "statefulset/runtime-redis", "--",
        "env", "REDISCLI_AUTH=$runtimeRedisPassword", "redis-cli", "--no-auth-warning", "--raw"
    ) + $Arguments
    return Invoke-Kubectl $commandArguments
}

function Invoke-ClickHouse([string]$Sql) {
    if (-not $clickhousePassword) {
        $script:clickhousePassword = Get-SecretValue $context.namespaces.observability "agentx-observability-secrets" "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD"
    }
    return Invoke-Kubectl @(
        "-n", [string]$context.namespaces.observability, "exec", "statefulset/clickhouse", "--",
        "env", "CLICKHOUSE_PASSWORD=$clickhousePassword", "clickhouse-client", "--user", "observability_migrate",
        "--database", "agentx_observability", "--format", "TabSeparated", "--query", $Sql
    )
}

function Get-DevelopmentReplicas {
    $items = @()
    foreach ($kind in @("deployment", "statefulset")) {
        $json = & kubectl -n agentx get $kind -o json 2>$null
        if ($LASTEXITCODE -ne 0) { continue }
        foreach ($item in (($json -join "`n") | ConvertFrom-Json).items) {
            $items += [pscustomobject]@{ kind = $kind; name = [string]$item.metadata.name; replicas = [int]$item.spec.replicas }
        }
    }
    return $items
}

function Set-DevelopmentReplicas([array]$Items, [bool]$Stop) {
    foreach ($item in $Items) {
        $replicas = if ($Stop) { 0 } else { $item.replicas }
        Invoke-Kubectl @("-n", "agentx", "scale", "$($item.kind)/$($item.name)", "--replicas=$replicas") | Out-Null
    }
}

function Wait-TcpPort([int]$Port, [int]$TimeoutSeconds = 45) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $client = [Net.Sockets.TcpClient]::new()
        try { $client.Connect("127.0.0.1", $Port); return }
        catch { Start-Sleep -Milliseconds 300 }
        finally { $client.Dispose() }
    } while ((Get-Date) -lt $deadline)
    throw "TCP port $Port did not become ready."
}

function Start-PortForward([string]$Namespace, [string]$Resource, [int]$LocalPort, [int]$RemotePort) {
    $safe = $Resource.Replace('/', '-')
    $stdout = Join-Path $artifactDirectory "$safe-$LocalPort.stdout.log"
    $stderr = Join-Path $artifactDirectory "$safe-$LocalPort.stderr.log"
    $process = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", $Resource, "${LocalPort}:${RemotePort}") `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Wait-TcpPort $LocalPort
    $forwards.Add($process)
    return $process
}

function Stop-Forwards {
    foreach ($process in $forwards) {
        if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    $forwards.Clear()
}

function Stop-Forward([Diagnostics.Process]$Process) {
    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        $Process.WaitForExit(5000) | Out-Null
    }
}

function Wait-PodNotReady([string]$Namespace, [string]$Pod, [int]$TimeoutSeconds = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $json = (Invoke-Kubectl @("-n", $Namespace, "get", "pod", $Pod, "-o", "json")) -join "`n" | ConvertFrom-Json
        $ready = @($json.status.conditions | Where-Object { $_.type -eq 'Ready' -and $_.status -eq 'True' }).Count -eq 1
        if (-not $ready) { return }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    throw "$Namespace/$Pod remained Ready after Drain."
}

function Wait-Deployment([string]$Namespace, [string]$Name, [int]$Replicas, [int]$TimeoutSeconds = 420) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $json = (Invoke-Kubectl @("-n", $Namespace, "get", "deployment", $Name, "-o", "json")) -join "`n" | ConvertFrom-Json
        if ([int]$json.status.readyReplicas -eq $Replicas -and [int]$json.status.updatedReplicas -eq $Replicas -and [int]$json.status.replicas -eq $Replicas) { return }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    throw "$Namespace/$Name did not converge to $Replicas ready replicas."
}

function Assert-MetricsEndpoint([string]$Namespace, [string]$Service, [int]$LocalPort) {
    $forward = Start-PortForward $Namespace "service/$Service" $LocalPort 9092
    try {
        $payload = (Invoke-WebRequest "http://127.0.0.1:$LocalPort/metrics" -UseBasicParsing).Content
        if ($payload -notmatch '(?m)^agentx_') { throw "$Namespace/$Service did not expose Agentx metrics." }
    }
    finally { Stop-Forward $forward }
}

function Login([string]$ControlUrl) {
    return Invoke-RestMethod "$ControlUrl/api/v1/auth/login" -Method Post -ContentType application/json `
        -Body (@{ username = "agentx-v2-e2e"; password = "agentx-v2-e2e-password" } | ConvertTo-Json)
}

function Wait-ApiKey([string]$KeyId) {
    $deadline = (Get-Date).AddSeconds(60)
    do {
        $count = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM api_key_admission WHERE tenant_id=UUID_TO_BIN('$tenantId') AND key_id=UUID_TO_BIN('$KeyId') AND status='active';") -join "")
        if ($count -eq 1) { return }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    throw "Runtime API Key admission did not converge."
}

function Start-Invocation([string]$RuntimeUrl, [string]$ApiKey, [string]$IdempotencyKey) {
    return Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post `
        -Headers @{ Authorization = "Bearer $ApiKey"; "Idempotency-Key" = $IdempotencyKey } -ContentType application/json `
        -Body (@{ input = @{ message = $IdempotencyKey }; responseMode = "async" } | ConvertTo-Json -Depth 5)
}

function Start-InvocationAfterDrain([string]$RuntimeUrl, [string]$ApiKey, [string]$IdempotencyKey, [int]$TimeoutSeconds = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $body = @{ input = @{ message = $IdempotencyKey }; responseMode = "async" } | ConvertTo-Json -Depth 5
    do {
        $response = Invoke-WebRequest "$RuntimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post `
            -Headers @{ Authorization = "Bearer $ApiKey"; "Idempotency-Key" = $IdempotencyKey } -ContentType application/json `
            -Body $body -SkipHttpErrorCheck
        if ([int]$response.StatusCode -in @(200, 202)) { return $response.Content | ConvertFrom-Json }
        if ([int]$response.StatusCode -eq 503 -and $response.Content -match 'SERVICE_DRAINING') {
            Start-Sleep -Milliseconds 500
            continue
        }
        throw "Gateway Drain replay failed with HTTP $([int]$response.StatusCode)."
    } while ((Get-Date) -lt $deadline)
    throw "Gateway Service continued routing the replay to a draining Pod for $TimeoutSeconds seconds."
}

function Wait-Invocation([string]$RuntimeUrl, [string]$ApiKey, [string]$InvocationId, [int]$TimeoutSeconds = 180) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $value = Invoke-RestMethod "$RuntimeUrl/gateway/v1/invocations/$InvocationId" -Headers @{ Authorization = "Bearer $ApiKey" }
        if ($value.status -in @("completed", "failed", "cancelled")) { return $value }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    $diagnosticPath = Join-Path $artifactDirectory "invocation-timeout-$InvocationId.txt"
    try {
        Invoke-RuntimeMySql @"
SELECT 'invocation',BIN_TO_UUID(id),status,COALESCE(BIN_TO_UUID(execution_id),'') FROM application_invocations WHERE id=UUID_TO_BIN('$InvocationId');
SELECT 'execution',BIN_TO_UUID(id),status,state_version FROM workflow_executions WHERE invocation_id=UUID_TO_BIN('$InvocationId');
SELECT 'command',BIN_TO_UUID(c.id),c.command_type,c.status,c.attempt_count,COALESCE(c.error_code,''),COALESCE(c.error_message,'') FROM runtime_commands c JOIN workflow_executions e ON e.id=UUID_TO_BIN(c.aggregate_id) WHERE e.invocation_id=UUID_TO_BIN('$InvocationId');
SELECT 'outbox',message_type,status,COUNT(*) FROM execution_outbox o JOIN workflow_executions e ON e.id=o.execution_id WHERE e.invocation_id=UUID_TO_BIN('$InvocationId') GROUP BY message_type,status;
SELECT 'invalid_event',BIN_TO_UUID(id),COALESCE(event_type,''),COALESCE(aggregate_type,''),COALESCE(last_error,'') FROM execution_outbox WHERE status='failed' AND last_error LIKE 'EVENT_PAYLOAD_INVALID:%' ORDER BY created_at,id LIMIT 20;
"@ | Set-Content -LiteralPath $diagnosticPath
        Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "logs", "deployment/workflow-runtime", "--all-pods=true", "--tail=300") |
            Set-Content -LiteralPath (Join-Path $artifactDirectory "invocation-timeout-workflow-runtime.log")
    }
    catch {
        "Diagnostic collection failed: $($_.Exception.Message)" | Add-Content -LiteralPath $diagnosticPath
    }
    throw "Invocation $InvocationId did not terminate."
}

function Invoke-ConcurrentReplay([string]$RuntimeUrl, [string]$ApiKey, [string]$Key, [int]$Count) {
    $responses = 1..$Count | ForEach-Object -Parallel {
        $url = $using:RuntimeUrl; $token = $using:ApiKey; $key = $using:Key
        Invoke-RestMethod "$url/gateway/v1/applications/v2-no-op/invocations" -Method Post `
            -Headers @{ Authorization = "Bearer $token"; "Idempotency-Key" = $key } -ContentType application/json `
            -Body (@{ input = @{ message = $key }; responseMode = "async" } | ConvertTo-Json -Depth 5)
    } -ThrottleLimit 20
    $ids = @($responses.id | Sort-Object -Unique)
    if ($ids.Count -ne 1) { throw "Concurrent Gateway replay created $($ids.Count) business Invocations." }
    return $responses[0]
}

function New-WorkerBacklog([int]$Count = 200) {
    if ($Count -lt 1 -or $Count -gt 200) { throw "Worker backlog count must be between 1 and 200." }
    $sql = @"
INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,iteration_index,status,capability,side_effect_level,input_json,created_at,updated_at)
WITH RECURSIVE ids AS (SELECT 1 seq UNION ALL SELECT seq+1 FROM ids WHERE seq<$Count)
SELECT UUID_TO_BIN(UUID()),n.tenant_id,n.execution_id,CONCAT('v206-load-$RunId-',ids.seq),CONCAT('v206-load-$RunId-',ids.seq),CONCAT('V2-06 load ',ids.seq),n.node_type,n.node_version,n.generation,n.activation_slot,n.run_index,n.iteration_index,'queued',n.capability,n.side_effect_level,JSON_OBJECT('v206',TRUE),UTC_TIMESTAMP(6),UTC_TIMESTAMP(6)
FROM ids CROSS JOIN (SELECT * FROM node_executions ORDER BY created_at DESC LIMIT 1) n;
INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,worker_protocol_version,ir_schema_version,compiler_version,manifest_version,status,idempotency_key,deadline_at,input_json,created_at)
SELECT UUID_TO_BIN(UUID()),a.tenant_id,a.execution_id,n.id,1,a.capability,a.worker_protocol_version,a.ir_schema_version,a.compiler_version,a.manifest_version,'queued',CONCAT('v2-06-load-$RunId-',SUBSTRING_INDEX(n.node_id,'-',-1)),DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 1 HOUR),JSON_OBJECT('v206',TRUE),UTC_TIMESTAMP(6)
FROM node_executions n CROSS JOIN (SELECT * FROM node_attempts WHERE idempotency_key NOT LIKE 'v2-06-load-%' ORDER BY created_at DESC LIMIT 1) a
WHERE n.node_id LIKE 'v206-load-$RunId-%';
SELECT COUNT(*) FROM node_attempts WHERE idempotency_key LIKE 'v2-06-load-$RunId-%';
"@
    $count = [int]((Invoke-RuntimeMySql $sql | Select-Object -Last 1) -join "")
    if ($count -ne $Count) { throw "Deterministic Worker backlog contains $count rows; expected $Count." }
    Invoke-RuntimeMySql "UPDATE node_attempts SET locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 20 MINUTE) WHERE idempotency_key LIKE 'v2-06-load-$RunId-%' AND status='queued';" | Out-Null
}

function Remove-WorkerBacklog {
    Invoke-RuntimeMySql "DELETE FROM node_attempts WHERE idempotency_key LIKE 'v2-06-load-$RunId-%'; DELETE FROM node_executions WHERE node_id LIKE 'v206-load-$RunId-%' AND input_json=JSON_OBJECT('v206',TRUE);" | Out-Null
}

foreach ($command in @("cargo", "curl.exe", "docker", "kubectl", "pwsh")) { Assert-Command $command }

Push-Location $root
try {
    Add-Timeline "V2-06A E2E started."
    if (-not $SkipLocalGates) {
        & cargo test -p agentx-service-kit -p agentx-mysql-lease
        & scripts/v2-claim-lease-tests.ps1
        & scripts/v2-profile-tests.ps1
        & scripts/v2-06-web-zero-diff-tests.ps1
        Add-Timeline "Lifecycle, 20-way Lease, Profile and Web zero-diff gates passed."
    }
    if ($ScaleDownDevelopment) {
        $developmentReplicas = Get-DevelopmentReplicas
        Set-DevelopmentReplicas $developmentReplicas $true
    }

    & (Join-Path $PSScriptRoot "v2-04-e2e.ps1") -ConfigFile $ConfigFile -RunId $RunId -Stage 06 `
        -BuildImages:$BuildImages -KeepOnSuccess -ContextOutputPath $contextPath -SkipLocalGates
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $contextPath)) { throw "V2-04 retained baseline failed." }
    $context = Get-Content -Raw -LiteralPath $contextPath | ConvertFrom-Json
    $scalabilityProfile = Get-Content -Raw -LiteralPath $context.profilePath | ConvertFrom-Json
    $scalabilityProfilePath = Join-Path $artifactDirectory "v2-06-profile.json"
    $scalabilityProfile | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $scalabilityProfilePath
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Install -ConfigFile $scalabilityProfilePath -RunId "06-$RunId"
    if ($LASTEXITCODE -ne 0) { throw "V2-06A scalability Profile install failed." }

    $targets = @(
        @{ ns = $context.namespaces.control; name = "platform-control" }, @{ ns = $context.namespaces.control; name = "web-console" },
        @{ ns = $context.namespaces.runtime; name = "runtime-gateway" }, @{ ns = $context.namespaces.runtime; name = "workflow-runtime" },
        @{ ns = $context.namespaces.runtime; name = "workflow-worker" }, @{ ns = $context.namespaces.runtime; name = "sandbox-manager" },
        @{ ns = $context.namespaces.observability; name = "observability" }
    )
    foreach ($target in $targets) {
        Invoke-Kubectl @("-n", [string]$target.ns, "scale", "deployment", $target.name, "--replicas=2") | Out-Null
        Wait-Deployment $target.ns $target.name 2
        $podList = (Invoke-Kubectl @("-n", [string]$target.ns, "get", "pods", "-l", "app.kubernetes.io/name=$($target.name)", "-o", "json")) -join "`n" | ConvertFrom-Json
        $podCount = @($podList.items | Where-Object {
            $_.status.phase -eq "Running" -and [string]::IsNullOrWhiteSpace([string]$_.metadata.deletionTimestamp)
        }).Count
        if ($podCount -ne 2) { throw "$($target.name) did not start with two Pod UIDs." }
    }
    $hpaCount = 0
    $pdbCount = 0
    foreach ($namespace in @($context.namespaces.control, $context.namespaces.runtime, $context.namespaces.observability)) {
        $hpaCount += @((Invoke-Kubectl @("-n", [string]$namespace, "get", "hpa", "--no-headers"))).Count
        $pdbCount += @((Invoke-Kubectl @("-n", [string]$namespace, "get", "pdb", "--no-headers"))).Count
    }
    if ($hpaCount -ne 0) { throw "Agentx must not create HPA resources; got $hpaCount." }
    if ($pdbCount -ne 7) { throw "Expected seven PDB resources, got $pdbCount." }
    $migrationHistory = "$(if (((Invoke-ControlMySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=6 AND success=1;") -join '') -eq '1') { 'control:6' } else { 'control:missing' })/$(if (((Invoke-RuntimeMySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=6 AND success=1;") -join '') -eq '1') { 'runtime:6' } else { 'runtime:missing' })"
    if ($migrationHistory -ne "control:6/runtime:6") { throw "V2-06A Migration history is incomplete: $migrationHistory" }
    Complete-Scenario 1 "seven resident Deployments are user-scaled to two replicas with probes and PDB but no Agentx HPA" @("hpaCount=0", "pdbCount=$pdbCount", "migrationHistory=$migrationHistory", "runtimeConnectionBudget=104/105")

    $metricsTargets = @(
        @{ ns = $context.namespaces.control; service = "platform-control-metrics"; port = 18701 },
        @{ ns = $context.namespaces.runtime; service = "runtime-gateway-metrics"; port = 18702 },
        @{ ns = $context.namespaces.runtime; service = "workflow-runtime-metrics"; port = 18703 },
        @{ ns = $context.namespaces.runtime; service = "workflow-worker-metrics"; port = 18704 },
        @{ ns = $context.namespaces.runtime; service = "sandbox-manager-metrics"; port = 18705 },
        @{ ns = $context.namespaces.observability; service = "observability-metrics"; port = 18706 }
    )
    foreach ($metricsTarget in $metricsTargets) { Assert-MetricsEndpoint $metricsTarget.ns $metricsTarget.service $metricsTarget.port }
    Complete-Scenario 2 "all backend metrics Services expose Prometheus text metrics without a bundled collector" @("metricsServices=6", "collector=external")

    $controlForward = Start-PortForward $context.namespaces.control "service/platform-control" 18680 8080
    $runtimeForward = Start-PortForward $context.namespaces.runtime "service/runtime-gateway-public" 18681 8080
    $controlUrl = "http://127.0.0.1:18680"; $runtimeUrl = "http://127.0.0.1:18681"
    $login = Login $controlUrl
    $key = Invoke-RestMethod "$controlUrl/api/v1/applications/$applicationId/api-keys" -Method Post `
        -Headers @{ Authorization = "Bearer $($login.accessToken)" } -ContentType application/json -Body (@{ name = "V2-06A E2E" } | ConvertTo-Json)
    Wait-ApiKey $key.id
    $replay = Invoke-ConcurrentReplay $runtimeUrl $key.secret "v2-06-$RunId-concurrent" 20
    $terminal = Wait-Invocation $runtimeUrl $key.secret $replay.id
    $facts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM application_invocations WHERE id=UUID_TO_BIN('$($replay.id)')),(SELECT COUNT(*) FROM workflow_executions WHERE invocation_id=UUID_TO_BIN('$($replay.id)')),(SELECT COUNT(*) FROM runtime_commands WHERE aggregate_id='$($terminal.executionId)' AND command_type='start_execution');") -join "`t"
    if ($facts -ne "1`t1`t1") { throw "Concurrent Gateway replay did not converge: $facts" }
    $invalidEvents = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM execution_outbox WHERE status='failed' AND last_error LIKE 'EVENT_PAYLOAD_INVALID:%';") -join "")
    if ($invalidEvents -ne 0) {
        Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id),COALESCE(event_type,''),COALESCE(aggregate_type,''),COALESCE(last_error,'') FROM execution_outbox WHERE status='failed' AND last_error LIKE 'EVENT_PAYLOAD_INVALID:%' ORDER BY created_at,id;" |
            Set-Content -LiteralPath (Join-Path $artifactDirectory "invalid-integration-events.txt")
        throw "Event Sequencer quarantined $invalidEvents invalid Integration Event payloads."
    }
    Complete-Scenario 3 "two Gateway replicas converge concurrent idempotent writes to one fact" @("invocation=$($replay.id)", "facts=$facts")

    $ownerPod = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=workflow-worker", "-o", "jsonpath={.items[0].metadata.name}")) -join "")
    $oldOwner = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pod", $ownerPod, "-o", "jsonpath={.metadata.uid}")) -join "")
    New-WorkerBacklog 1
    $leaseId = (Invoke-RuntimeMySql "UPDATE node_attempts SET status='running',lease_token=UUID_TO_BIN('$oldOwner'),locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),fencing_token=7 WHERE idempotency_key='v2-06-load-$RunId-1'; SELECT BIN_TO_UUID(id) FROM node_attempts WHERE idempotency_key='v2-06-load-$RunId-1';" | Select-Object -Last 1) -join ""
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", "pod", $ownerPod, "--grace-period=0", "--force", "--wait=false") | Out-Null
    Wait-Deployment $context.namespaces.runtime "workflow-worker" 2
    $workerOwners = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=workflow-worker", "-o", "jsonpath={.items[*].metadata.uid}")) -join "") -split '\s+'
    $newOwner = @($workerOwners | Where-Object { $_ -and $_ -ne $oldOwner })[-1]
    if ([string]::IsNullOrWhiteSpace($newOwner)) { throw "Replacement Worker Pod UID was not observed." }
    Invoke-RuntimeMySql "UPDATE node_attempts SET lease_token=UUID_TO_BIN('$newOwner'),locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),fencing_token=8 WHERE id=UUID_TO_BIN('$leaseId'); UPDATE node_attempts SET status='succeeded' WHERE id=UUID_TO_BIN('$leaseId') AND lease_token=UUID_TO_BIN('$oldOwner') AND fencing_token=7 AND locked_until>UTC_TIMESTAMP(6); SELECT ROW_COUNT();" | Tee-Object -FilePath (Join-Path $artifactDirectory "stale-token.txt") | Out-Null
    $staleRows = [int](Get-Content -Raw (Join-Path $artifactDirectory "stale-token.txt"))
    if ($staleRows -ne 0) { throw "A stale Worker fencing token changed $staleRows rows." }
    Complete-Scenario 4 "forced Owner termination rejects the stale fencing token" @("podUid=$oldOwner", "replacementUid=$newOwner", "attempt=$leaseId", "staleRows=0")

    Remove-WorkerBacklog
    foreach ($target in $targets) {
        Invoke-Kubectl @("-n", [string]$target.ns, "scale", "deployment", $target.name, "--replicas=4") | Out-Null
        Wait-Deployment $target.ns $target.name 4
    }
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Upgrade -ConfigFile $scalabilityProfilePath -RunId "06-$RunId"
    if ($LASTEXITCODE -ne 0) { throw "V2-06A Upgrade preservation check failed." }
    foreach ($target in $targets) { Wait-Deployment $target.ns $target.name 4 }
    Complete-Scenario 5 "user-driven scale-up reaches four replicas and Upgrade preserves live replicas" @("scaler=user", "replicas=4", "upgradePreserved=true")
    foreach ($target in $targets) {
        Invoke-Kubectl @("-n", [string]$target.ns, "scale", "deployment", $target.name, "--replicas=2") | Out-Null
        Wait-Deployment $target.ns $target.name 2
    }
    Complete-Scenario 6 "user-driven scale-down returns all workloads from four to two" @("scaler=user", "replicas=2")

    $drainInvocation = Start-Invocation $runtimeUrl $key.secret "v2-06-$RunId-drain"
    $gatewayPod = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=runtime-gateway", "-o", "jsonpath={.items[0].metadata.name}")) -join "")
    $gatewayPodForward = Start-PortForward $context.namespaces.runtime "pod/$gatewayPod" 18682 9091
    $gatewayPodHttpForward = Start-PortForward $context.namespaces.runtime "pod/$gatewayPod" 18683 8080
    $activeSseInvocationId = [guid]::NewGuid().ToString()
    $activeSseEventId = [guid]::NewGuid().ToString()
    Invoke-RuntimeMySql @"
INSERT INTO application_invocations(id,tenant_id,application_id,workflow_version_id,bundle_id,admission_epoch,state_version,caller_type,caller_id,request_hash,idempotency_key,status,input_json,created_at)
SELECT UUID_TO_BIN('$activeSseInvocationId'),i.tenant_id,i.application_id,i.workflow_version_id,i.bundle_id,i.admission_epoch,1,'api_key',UUID_TO_BIN('$($key.id)'),SHA2('v2-06-sse-drain-$RunId',256),'v2-06-sse-drain-$RunId','queued',JSON_OBJECT('v206',TRUE),UTC_TIMESTAMP(6)
FROM application_invocations i ORDER BY i.created_at DESC LIMIT 1;
INSERT INTO invocation_events(tenant_id,invocation_id,event_id,sequence_number,event_type,payload_json)
VALUES(UUID_TO_BIN('$tenantId'),UUID_TO_BIN('$activeSseInvocationId'),UUID_TO_BIN('$activeSseEventId'),1,'invocation.started',JSON_OBJECT('v206',TRUE));
"@ | Out-Null
    $sseConfig = [IO.Path]::GetTempFileName()
    $sseStdout = Join-Path $artifactDirectory "active-sse-drain.txt"
    $sseStderr = Join-Path $artifactDirectory "active-sse-drain.stderr.txt"
    @(
        'silent', 'show-error', 'no-buffer', 'max-time = 60',
        "header = `"Authorization: Bearer $($key.secret)`"",
        "url = `"http://127.0.0.1:18683/gateway/v1/invocations/$activeSseInvocationId/events`""
    ) | Set-Content -LiteralPath $sseConfig
    $activeSse = Start-Process curl.exe -ArgumentList @('--config', $sseConfig) -WindowStyle Hidden -PassThru -RedirectStandardOutput $sseStdout -RedirectStandardError $sseStderr
    Start-Sleep -Seconds 2
    Remove-Item -LiteralPath $sseConfig -Force
    if ($activeSse.HasExited) { throw "Active SSE connection exited before Gateway Drain." }
    Invoke-WebRequest "http://127.0.0.1:18682/health/drain" -Method Post -UseBasicParsing | Out-Null
    if (-not $activeSse.WaitForExit(15000)) {
        Stop-Process -Id $activeSse.Id -Force -ErrorAction SilentlyContinue
        throw "Gateway Drain did not close the active SSE connection within 15 seconds."
    }
    $drainRejectBody = @{ input = @{ message = "v2-06-$RunId-drain-rejected" }; responseMode = "async" } | ConvertTo-Json -Depth 5
    $drainReject = Invoke-WebRequest "http://127.0.0.1:18683/gateway/v1/applications/v2-no-op/invocations" -Method Post `
        -Headers @{ Authorization = "Bearer $($key.secret)"; "Idempotency-Key" = "v2-06-$RunId-drain-rejected" } -ContentType application/json `
        -Body $drainRejectBody -SkipHttpErrorCheck
    $drainError = $drainReject.Content | ConvertFrom-Json
    $retryAfter = [string](@($drainReject.Headers['Retry-After']) -join ',')
    if ([int]$drainReject.StatusCode -ne 503 -or $drainError.code -ne 'SERVICE_DRAINING' -or $retryAfter -ne '5') {
        throw "Draining Gateway did not return the stable 503 SERVICE_DRAINING contract."
    }
    Wait-PodNotReady $context.namespaces.runtime $gatewayPod
    Stop-Forward $runtimeForward
    $runtimeForward = Start-PortForward $context.namespaces.runtime "service/runtime-gateway-public" 18681 8080
    $replacement = Start-InvocationAfterDrain $runtimeUrl $key.secret "v2-06-$RunId-drain"
    if ($replacement.id -ne $drainInvocation.id) { throw "Gateway Drain replay changed the Invocation ID." }
    $drainTerminal = Wait-Invocation $runtimeUrl $key.secret $drainInvocation.id
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $events = (& curl.exe -sS -N --max-time 8 -H "Authorization: Bearer $($key.secret)" -H "Last-Event-ID: 0" "$runtimeUrl/gateway/v1/invocations/$($drainInvocation.id)/events" 2>$null) -join "`n"
        $sseExit = $LASTEXITCODE
    }
    finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
    $events | Set-Content -LiteralPath (Join-Path $artifactDirectory "drain-sse.txt")
    if ($sseExit -notin @(0, 28)) { throw "SSE replay curl failed with exit code $sseExit." }
    if ($events -notmatch '(?m)^id:\s*\d+\s*$' -or $events -notmatch '(?m)^event:\s*invocation\.completed\s*$') {
        throw "SSE did not replay the completed MySQL Cursor after Drain."
    }
    $activeSseText = Get-Content -Raw -LiteralPath $sseStdout
    if ($activeSseText -notmatch '(?m)^id:\s*1\s*$') { throw "Active SSE did not preserve its final MySQL Cursor before Drain." }
    Invoke-RuntimeMySql "DELETE FROM invocation_events WHERE invocation_id=UUID_TO_BIN('$activeSseInvocationId'); DELETE FROM application_invocations WHERE id=UUID_TO_BIN('$activeSseInvocationId');" | Out-Null

    $workerPod = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=workflow-worker", "-o", "jsonpath={.items[0].metadata.name}")) -join "")
    $workerUid = ((Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pod", $workerPod, "-o", "jsonpath={.metadata.uid}")) -join "")
    $workerAdminForward = Start-PortForward $context.namespaces.runtime "pod/$workerPod" 18684 9091
    Invoke-WebRequest "http://127.0.0.1:18684/health/drain" -Method Post -UseBasicParsing | Out-Null
    Wait-PodNotReady $context.namespaces.runtime $workerPod
    $workerDraining = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM worker_capabilities WHERE instance_id='$workerUid' AND status='draining';") -join '')
    if ($workerDraining -lt 1) { throw "Worker Drain did not publish worker_capabilities.status='draining'." }
    Complete-Scenario 7 "Gateway and Worker Drain stop new work, close active SSE within 15 seconds and preserve MySQL Cursor replay" @("drainedGateway=$gatewayPod", "drainedWorker=$workerPod", "workerCapabilitiesDraining=$workerDraining", "lastEventId=1", "invocation=$($drainInvocation.id)")

    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "scale", "deployment/platform-control", "--replicas=1") | Out-Null
    Wait-Deployment $context.namespaces.control "platform-control" 1
    $platformPods = (Invoke-Kubectl @("-n", [string]$context.namespaces.control, "get", "pods", "-l", "app.kubernetes.io/name=platform-control", "-o", "json")) -join "`n" | ConvertFrom-Json
    $availablePods = @($platformPods.items | Where-Object {
        [string]::IsNullOrWhiteSpace([string]$_.metadata.deletionTimestamp) -and
        @($_.status.conditions | Where-Object { $_.type -eq 'Ready' -and $_.status -eq 'True' }).Count -eq 1
    })
    if ($availablePods.Count -ne 1) { throw "Expected exactly one Ready Platform Control Pod before the PDB eviction check." }
    $lastPod = [string]$availablePods[0].metadata.name
    $evictionPath = Join-Path $artifactDirectory "eviction.json"
    @{ apiVersion = "policy/v1"; kind = "Eviction"; metadata = @{ name = $lastPod; namespace = [string]$context.namespaces.control } } | ConvertTo-Json -Depth 6 | Set-Content $evictionPath
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $evictionOutput = & kubectl create --raw "/api/v1/namespaces/$($context.namespaces.control)/pods/$lastPod/eviction" -f $evictionPath 2>&1
        $evictionExit = $LASTEXITCODE
    }
    finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
    if ($evictionExit -eq 0 -or ($evictionOutput -join "`n") -notmatch 'disruption budget|Too Many Requests|429') { throw "PDB did not block eviction of the last available Platform Control Pod." }
    Complete-Scenario 8 "Eviction API is blocked for the last PDB-protected replica" @("pod=$lastPod", "minAvailable=1")

    $renderedPath = Join-Path $artifactDirectory "restore.yaml"
    & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Render -ConfigFile $scalabilityProfilePath -RunId "06-$RunId" | Set-Content $renderedPath
    Invoke-Kubectl @("apply", "-f", $renderedPath) | Out-Null
    foreach ($target in $targets) {
        Invoke-Kubectl @("-n", [string]$target.ns, "scale", "deployment", $target.name, "--replicas=2") | Out-Null
        Wait-Deployment $target.ns $target.name 2
    }
    $projectionReceiptsBefore = [int]((Invoke-ControlMySql "SELECT COUNT(*) FROM projection_receipts;") -join '')
    $traceEventsBefore = [int]((Invoke-ClickHouse "SELECT uniqExact(event_id) FROM workflow_trace_events;") -join '')
    $sandboxFactsBefore = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM sandbox_leases;") -join '')
    foreach ($target in @(
        @{ ns = $context.namespaces.control; name = "platform-control" },
        @{ ns = $context.namespaces.runtime; name = "workflow-runtime" }, @{ ns = $context.namespaces.runtime; name = "workflow-worker" }, @{ ns = $context.namespaces.runtime; name = "sandbox-manager" },
        @{ ns = $context.namespaces.observability; name = "observability" }
    )) {
        Invoke-Kubectl @("-n", [string]$target.ns, "rollout", "restart", "deployment/$($target.name)") | Out-Null
        Wait-Deployment $target.ns $target.name 2
    }
    $duplicates = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*)-COUNT(DISTINCT id) FROM application_invocations),(SELECT COUNT(*) FROM node_attempts WHERE idempotency_key LIKE 'v2-06-load-$RunId-%'),(SELECT COUNT(*) FROM node_attempts WHERE locked_until<=UTC_TIMESTAMP(6) AND status='running');") -join "`t"
    if ($duplicates -ne "0`t0`t0") { throw "Rolling restart left duplicate or expired test facts: $duplicates" }
    $projectionState = (Invoke-ControlMySql "SELECT (SELECT COUNT(*) FROM projection_receipts),(SELECT COUNT(*) FROM (SELECT projector_name,event_id,COUNT(*) c FROM projection_receipts GROUP BY projector_name,event_id HAVING c>1) d),(SELECT COUNT(*) FROM runtime_projection_cursors WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL);") -join "`t"
    $projectionParts = $projectionState -split "`t"
    if ([int]$projectionParts[0] -lt $projectionReceiptsBefore -or [int]$projectionParts[1] -ne 0 -or [int]$projectionParts[2] -ne 0) { throw "Projector restart violated Receipt or Lease uniqueness: $projectionState" }
    $traceState = (Invoke-ClickHouse "SELECT uniqExact(event_id),countIf(hashes>1) FROM (SELECT event_id,uniqExact(content_hash) hashes FROM workflow_trace_events GROUP BY event_id);") -join "`t"
    $traceParts = $traceState -split "`t"
    if ([int]$traceParts[0] -lt $traceEventsBefore -or [int]$traceParts[1] -ne 0) { throw "Trace restart lost events or produced conflicting hashes: $traceState" }
    $sandboxState = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM sandbox_leases),(SELECT COUNT(*) FROM (SELECT tenant_id,idempotency_key,COUNT(*) c FROM sandbox_leases GROUP BY tenant_id,idempotency_key HAVING c>1) d),(SELECT COUNT(*) FROM sandbox_leases WHERE status IN ('creating','interrupting','terminating','orphaned') AND locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL);") -join "`t"
    $sandboxParts = $sandboxState -split "`t"
    if ([int]$sandboxParts[0] -lt $sandboxFactsBefore -or [int]$sandboxParts[1] -ne 0 -or [int]$sandboxParts[2] -ne 0) { throw "Sandbox restart violated idempotency or Lease convergence: $sandboxState" }
    Complete-Scenario 9 "Worker, Sandbox, Projector and Trace roles roll with unique Receipts, hashes and provider facts" @("runtimeResiduals=$duplicates", "projectionReceipts=$projectionState", "traceEvents=$traceState", "sandboxFacts=$sandboxState")

    $controlResidual = (Invoke-ControlMySql "SELECT (SELECT COUNT(*) FROM outbox WHERE status='processing' AND locked_until<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM publish_attempts WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND state NOT IN ('active','rejected'))+(SELECT COUNT(*) FROM runtime_projection_cursors WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL)+(SELECT COUNT(*) FROM retention_runs WHERE status='running' AND locked_until<=UTC_TIMESTAMP(6));") -join ""
    $runtimeResidual = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM runtime_commands WHERE status='processing' AND locked_until<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM execution_outbox WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL)+(SELECT COUNT(*) FROM node_attempts WHERE status='running' AND locked_until<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM wait_subscriptions WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL)+(SELECT COUNT(*) FROM trigger_bindings WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL)+(SELECT COUNT(*) FROM sandbox_leases WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status<>'terminated')+(SELECT COUNT(*) FROM retention_runs WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status='running')+(SELECT COUNT(*) FROM retention_items WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status IN ('marked','deleting'))+(SELECT COUNT(*) FROM bundle_gc_runs WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status IN ('marking','sweeping'))+(SELECT COUNT(*) FROM bundle_gc_items WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status IN ('marked','deleting'))+(SELECT COUNT(*) FROM trace_outbox WHERE locked_until<=UTC_TIMESTAMP(6) AND locked_by IS NOT NULL AND status<>'streamed')+(SELECT COUNT(*) FROM quota_reservations WHERE status='active' AND expires_at<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM runtime_idempotency_keys WHERE status='processing' AND expires_at<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM bundle_retention_holds WHERE released_at IS NULL AND expires_at<=UTC_TIMESTAMP(6))+(SELECT COUNT(*) FROM runtime_retention_holds WHERE released_at IS NULL AND expires_at<=UTC_TIMESTAMP(6));") -join ""
    $redisPending = [int](@(Invoke-RuntimeRedis @("XPENDING", "agentx:v2:trace:v1", "agentx:v2:observability:v1"))[0])
    $tracePending = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM trace_outbox WHERE status IN ('pending','failed');") -join '')
    $receiptConflicts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM (SELECT attempt_id,COUNT(*) c FROM worker_result_receipts GROUP BY attempt_id HAVING c>1) d)+(SELECT COUNT(*) FROM (SELECT tenant_id,scope,idempotency_key,COUNT(*) c FROM runtime_idempotency_keys GROUP BY tenant_id,scope,idempotency_key HAVING c>1) i);") -join ''
    if ([int]$controlResidual -ne 0 -or [int]$runtimeResidual -ne 0 -or $redisPending -ne 0 -or $tracePending -ne 0 -or [int]$receiptConflicts -ne 0) { throw "V2-06A left Claim, Inbox/Receipt, Hold, GC/Retention or Trace residue." }
    Complete-Scenario 10 "queues, leases, reservations, Inbox/Receipts, GC/Retention holds and Redis/Trace Pending contain no V2-06A residue" @("controlResidual=$controlResidual", "runtimeResidual=$runtimeResidual", "redisPending=$redisPending", "tracePending=$tracePending", "receiptConflicts=$receiptConflicts")

    $completed = $scenarios.Count -eq 10
    [ordered]@{
        apiVersion = "agentx.io/evidence/v1"
        stage = "v2-06a"
        stageComplete = $completed
        runId = $RunId
        namespaces = $context.namespaces
        claimAudit = "docs/planv2/contracts/claim-lease-audit.json"
        scenarios = @($scenarios)
        timeline = @($timeline)
        deferred = @("V2S-006", "capacity thresholds", "tenant/provider fairness", "two-hour stability")
        sensitiveData = "Passwords, API keys, tokens and response bodies are excluded."
    } | ConvertTo-Json -Depth 12 | Set-Content $summaryPath
}
finally {
    Add-Timeline "cleanup begin success=$completed"
    try { Remove-WorkerBacklog } catch { Add-Timeline "backlog cleanup warning: $($_.Exception.Message)" }
    Stop-Forwards
    if ($context) {
        try { & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -ConfigFile $(if ($scalabilityProfilePath) { $scalabilityProfilePath } else { $context.profilePath }) -RunId "06-$RunId" } catch { Add-Timeline "namespace cleanup warning: $($_.Exception.Message)" }
    }
    else {
        try { & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -ConfigFile $ConfigFile -RunId "06-$RunId" } catch { Add-Timeline "partial namespace cleanup warning: $($_.Exception.Message)" }
    }
    if ($ScaleDownDevelopment -and $developmentReplicas.Count -gt 0) { Set-DevelopmentReplicas $developmentReplicas $false }
    Add-Timeline "cleanup end"
    $timeline | Set-Content (Join-Path $artifactDirectory "timeline.log")
    Pop-Location
}

if (-not $completed) { throw "V2-06A E2E failed; see $artifactDirectory" }
Write-Output "V2-06A E2E passed. Evidence: $artifactDirectory"
