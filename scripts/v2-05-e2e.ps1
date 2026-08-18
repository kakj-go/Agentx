param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$KeepOnFailure,
    [switch]$SkipLocalGates
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-05"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$contextPath = Join-Path $artifactDirectory "v2-05-context.json"
$summaryPath = Join-Path $artifactDirectory "summary.json"
$timeline = [Collections.Generic.List[string]]::new()
$scenarios = [Collections.Generic.List[object]]::new()
$forwards = [Collections.Generic.List[Diagnostics.Process]]::new()
$context = $null
$completed = $false
$controlPassword = $null
$runtimePassword = $null
$clickhousePassword = $null
$tenantId = "018f0000-0000-7000-8000-000000000001"
$userId = "018f0000-0000-7000-8000-000000000002"
$applicationId = "018f0000-0000-7000-8000-00000000000a"
$applicationSlug = "v2-no-op"

function Add-Timeline([string]$Message) {
    $timeline.Add("$([DateTimeOffset]::UtcNow.ToString('O')) $Message")
}

function Complete-Scenario([int]$Id, [string]$Assertion, [string[]]$Evidence) {
    $scenarios.Add([ordered]@{ id = $Id; status = "passed"; assertion = $Assertion; evidence = $Evidence })
    Add-Timeline "Scenario $Id passed: $Assertion"
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "V2-05 E2E requires $Name." }
}

function Invoke-Kubectl([string[]]$Arguments) {
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        $output = & kubectl @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) {
            $command = ($Arguments -join ' ') -replace '(?i)\b(MYSQL_PWD|CLICKHOUSE_PASSWORD|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>'
            $details = ($output -join [Environment]::NewLine) -replace '(?i)\b(MYSQL_PWD|CLICKHOUSE_PASSWORD|PASSWORD|TOKEN)=[^\s]+', '$1=<redacted>'
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

function Start-PortForward([string]$Namespace, [string]$Service, [int]$LocalPort) {
    $stdout = Join-Path $artifactDirectory "$Service-$LocalPort.stdout.log"
    $stderr = Join-Path $artifactDirectory "$Service-$LocalPort.stderr.log"
    $process = Start-Process kubectl -ArgumentList @(
        "-n", $Namespace, "port-forward", "service/$Service", "${LocalPort}:8080"
    ) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Wait-TcpPort $LocalPort
    $forwards.Add($process)
    return $process
}

function Stop-Forward([Diagnostics.Process]$Process) {
    if ($Process -and -not $Process.HasExited) { Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue }
}

function Stop-Forwards {
    foreach ($process in $forwards) { Stop-Forward $process }
    $forwards.Clear()
}

function Wait-Deployment([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 300) {
    Invoke-Kubectl @("-n", $Namespace, "rollout", "status", "deployment/$Name", "--timeout=${TimeoutSeconds}s") | Out-Null
}

function Scale-Deployment([string]$Namespace, [string]$Name, [int]$Replicas) {
    Invoke-Kubectl @("-n", $Namespace, "scale", "deployment/$Name", "--replicas=$Replicas") | Out-Null
    if ($Replicas -gt 0) { Wait-Deployment $Namespace $Name }
}

function Wait-StatefulSet([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 300) {
    Invoke-Kubectl @("-n", $Namespace, "rollout", "status", "statefulset/$Name", "--timeout=${TimeoutSeconds}s") | Out-Null
}

function Invoke-Http([string]$Uri, [string]$Method = "GET", [hashtable]$Headers = @{}, [object]$Body = $null) {
    $arguments = @{ Uri = $Uri; Method = $Method; Headers = $Headers; SkipHttpErrorCheck = $true }
    if ($null -ne $Body) {
        $arguments.ContentType = "application/json"
        $arguments.Body = if ($Body -is [string]) { $Body } else { $Body | ConvertTo-Json -Depth 30 -Compress }
    }
    $response = Invoke-WebRequest @arguments
    $parsed = $null
    if ($response.Content) {
        try { $parsed = $response.Content | ConvertFrom-Json } catch { $parsed = $response.Content }
    }
    return [pscustomobject]@{ status = [int]$response.StatusCode; body = $parsed; headers = $response.Headers }
}

function Login([string]$ControlUrl) {
    $response = Invoke-Http "$ControlUrl/api/v1/auth/login" "POST" @{} @{
        username = "agentx-v2-e2e"; password = "agentx-v2-e2e-password"
    }
    if ($response.status -ne 200) { throw "Control login failed with $($response.status)." }
    return $response.body
}

function Wait-ApiKeyAdmission([string]$KeyId) {
    $deadline = (Get-Date).AddMinutes(2)
    do {
        $count = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM api_key_admission WHERE tenant_id=UUID_TO_BIN('$tenantId') AND key_id=UUID_TO_BIN('$KeyId') AND status='active';") -join "")
        if ($count -eq 1) { return }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "API Key $KeyId was not projected into Runtime."
}

function Start-Invocation([string]$RuntimeUrl, [string]$ApiKey, [string]$Label) {
    $response = Invoke-Http "$RuntimeUrl/gateway/v1/applications/$applicationSlug/invocations" "POST" @{
        Authorization = "Bearer $ApiKey"; "Idempotency-Key" = "v2-05-$RunId-$Label"
    } @{ input = @{ message = $Label }; responseMode = "async" }
    if ($response.status -ne 202) { throw "Invocation $Label returned $($response.status)." }
    return $response.body
}

function Wait-Invocation([string]$RuntimeUrl, [string]$ApiKey, [string]$InvocationId, [int]$TimeoutSeconds = 180) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $response = Invoke-Http "$RuntimeUrl/gateway/v1/invocations/$InvocationId" "GET" @{ Authorization = "Bearer $ApiKey" }
        if ($response.status -eq 200 -and $response.body.status -in @("completed", "failed", "cancelled")) { return $response.body }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Invocation $InvocationId did not reach a terminal state."
}

function Wait-ProjectionReady([int]$MinimumCursor = 0, [int]$MinimumGeneration = 1, [int]$TimeoutSeconds = 300) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $value = (Invoke-ControlMySql "SELECT state,current_cursor,active_generation FROM runtime_projection_status WHERE projection_name='runtime_governance_v1' AND partition_key='global';") -join "`t"
        $parts = $value -split "`t"
        if ($parts.Count -eq 3 -and $parts[0] -eq "ready" -and [uint64]$parts[1] -ge $MinimumCursor -and [uint64]$parts[2] -ge $MinimumGeneration) { return $value }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Governance Projection did not become ready: $value"
}

function Wait-RuntimePackage([string]$PackageId, [string[]]$Statuses, [int]$TimeoutSeconds = 300) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $status = (Invoke-RuntimeMySql "SELECT status FROM runtime_work_packages WHERE tenant_id=UUID_TO_BIN('$tenantId') AND id=UUID_TO_BIN('$PackageId');") -join ""
        if ($status -in $Statuses) { return $status }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Runtime Work Package $PackageId did not reach $($Statuses -join '/'): $status"
}

function Invoke-EvaluationFixture {
    $profile = Get-Content -Raw -LiteralPath $context.profilePath | ConvertFrom-Json
    $deployment = ((Invoke-Kubectl @("-n", [string]$context.namespaces.control, "get", "deployment/platform-control", "-o", "json")) -join "`n") | ConvertFrom-Json
    $environment = @($deployment.spec.template.spec.containers[0].env | Where-Object name -notin @(
        "AGENTX_RUNTIME_INTERNAL_URL", "AGENTX_V2_FIXTURE_DEPENDENCIES_NAMESPACE", "AGENTX_V2_FIXTURE_MODE", "AGENTX_V2_FIXTURE_ID_SEED"
    ))
    $environment += [pscustomobject]@{ name = "AGENTX_RUNTIME_INTERNAL_URL"; value = "http://runtime-gateway-internal.$($context.namespaces.runtime).svc:8080" }
    $environment += [pscustomobject]@{ name = "AGENTX_V2_FIXTURE_DEPENDENCIES_NAMESPACE"; value = [string]$context.namespaces.dependencies }
    $environment += [pscustomobject]@{ name = "AGENTX_V2_FIXTURE_MODE"; value = "evaluation" }
    $environment += [pscustomobject]@{ name = "AGENTX_V2_FIXTURE_ID_SEED"; value = "v2-05-$RunId" }
    $jobName = "v2-05-fixture-evaluation"
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "delete", "job", $jobName, "--ignore-not-found", "--wait=true") | Out-Null
    $job = [ordered]@{
        apiVersion = "batch/v1"; kind = "Job"
        metadata = @{ name = $jobName; namespace = [string]$context.namespaces.control; labels = @{ "agentx.io/plane" = "control"; "agentx.io/v2-05-fixture" = "evaluation" } }
        spec = @{
            backoffLimit = 0; ttlSecondsAfterFinished = 3600
            template = @{
                metadata = @{ labels = @{ "agentx.io/plane" = "control"; "agentx.io/v2-05-fixture" = "evaluation" } }
                spec = @{
                    restartPolicy = "Never"; serviceAccountName = "platform-control"; automountServiceAccountToken = $false
                    containers = @(@{
                        name = "fixture"; image = "$($profile.images.registry)/v2-04-fixture:$($profile.images.tag)"
                        imagePullPolicy = [string]$profile.images.pullPolicy; env = $environment
                    })
                }
            }
        }
    }
    $job | ConvertTo-Json -Depth 30 -Compress | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to create $jobName." }
    $deadline = (Get-Date).AddMinutes(5)
    do {
        $jobStatus = ((Invoke-Kubectl @("-n", [string]$context.namespaces.control, "get", "job/$jobName", "-o", "json")) -join "`n") | ConvertFrom-Json
        if ([int]$jobStatus.status.succeeded -eq 1 -or [int]$jobStatus.status.failed -gt 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    $lines = Invoke-Kubectl @("-n", [string]$context.namespaces.control, "logs", "job/$jobName", "--all-containers=true")
    $lines | Set-Content -LiteralPath (Join-Path $artifactDirectory "$jobName.log")
    if ([int]$jobStatus.status.succeeded -ne 1) { throw "$jobName failed: $($lines -join '`n')" }
    $jsonLine = @($lines | Where-Object { $_.TrimStart().StartsWith("{") }) | Select-Object -Last 1
    if (-not $jsonLine) { throw "$jobName did not emit fixture JSON." }
    return $jsonLine | ConvertFrom-Json
}

function Wait-ControlGovernanceFixture([string]$EvaluationPackageId, [string]$RetentionRunId, [int]$TimeoutSeconds = 300) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $facts = (Invoke-ControlMySql "SELECT (SELECT status FROM evaluation_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND work_package_id=UUID_TO_BIN('$EvaluationPackageId')),(SELECT status FROM retention_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND id=UUID_TO_BIN('$RetentionRunId'));") -join "`t"
        $parts = $facts -split "`t"
        if ($parts.Count -eq 2 -and $parts[0] -eq "completed" -and $parts[1] -eq "completed") { return $facts }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Evaluation/Retention Control projections did not converge: $facts"
}

function Wait-Trace([string]$ControlUrl, [hashtable]$Headers, [string]$ExecutionId, [int]$TimeoutSeconds = 300) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $response = Invoke-Http "$ControlUrl/api/v1/executions/$ExecutionId/trace" "GET" $Headers
        if ($response.status -eq 200 -and @($response.body.events).Count -gt 0) { return $response }
        if ($response.status -notin @(202, 503)) { throw "Trace query returned $($response.status)." }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    throw "Trace for Execution $ExecutionId did not catch up."
}

function ConvertTo-Base64Url([byte[]]$Bytes) {
    return [Convert]::ToBase64String($Bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

function ConvertTo-Jcs([object]$Value) {
    if ($null -eq $Value) { return "null" }
    if ($Value -is [string] -or $Value -is [char]) {
        return [Text.Json.JsonSerializer]::Serialize([string]$Value, [Text.Json.JsonSerializerOptions]::new())
    }
    if ($Value -is [bool]) { return $(if ($Value) { "true" } else { "false" }) }
    if ($Value -is [Collections.IDictionary]) {
        $parts = foreach ($key in @($Value.Keys | ForEach-Object { [string]$_ } | Sort-Object)) {
            "$([Text.Json.JsonSerializer]::Serialize([string]$key, [Text.Json.JsonSerializerOptions]::new())):$(ConvertTo-Jcs $Value[$key])"
        }
        return "{$($parts -join ',')}"
    }
    if ($Value -is [Collections.IEnumerable] -and $Value -isnot [string]) {
        $parts = foreach ($item in $Value) { ConvertTo-Jcs $item }
        return "[$($parts -join ',')]"
    }
    if ($Value -is [Management.Automation.PSCustomObject]) {
        $map = @{}; foreach ($property in $Value.PSObject.Properties) { $map[$property.Name] = $property.Value }
        return ConvertTo-Jcs $map
    }
    return [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture).ToLowerInvariant()
}

function Get-ContentHash([object]$Value) {
    $canonical = ConvertTo-Jcs $Value
    $hash = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($canonical))
    return "sha256:$([Convert]::ToHexString($hash).ToLowerInvariant())"
}

function New-DelegationToken(
    [string]$Audience,
    [string]$Scope,
    [string]$RequestHash,
    [string[]]$ExecutionIds,
    [string]$Tenant = $tenantId,
    [int64]$IssuedAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds(),
    [string]$Jti = [Guid]::NewGuid().ToString()
) {
    $kid = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_BFF_JWT_KID"
    $pem = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM"
    $tokenVersion = [uint64]((Invoke-RuntimeMySql "SELECT token_version FROM runtime_user_admission WHERE tenant_id=UUID_TO_BIN('$tenantId') AND user_id=UUID_TO_BIN('$userId');") -join "")
    $header = @{ alg = "RS256"; typ = "JWT"; kid = $kid } | ConvertTo-Json -Compress
    $payload = [ordered]@{
        iss = "agentx-control"; aud = $Audience; sub = $userId; tenantId = $Tenant
        tokenVersion = $tokenVersion; tenantWide = $false; scope = @($Scope)
        applicationIds = @(); workflowIds = @(); executionIds = @($ExecutionIds); sessionIds = @()
        requestHash = $RequestHash; iat = $IssuedAt; exp = $IssuedAt + 60; jti = $Jti
    } | ConvertTo-Json -Compress
    $encodedHeader = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes($header))
    $encodedPayload = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes($payload))
    $input = "$encodedHeader.$encodedPayload"
    $rsa = [Security.Cryptography.RSA]::Create()
    try {
        $rsa.ImportFromPem($pem)
        $signature = $rsa.SignData([Text.Encoding]::ASCII.GetBytes($input), [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
        return "$input.$(ConvertTo-Base64Url $signature)"
    }
    finally { $rsa.Dispose() }
}

function Run-NetworkProbe([string]$Namespace, [string]$Name, [string]$Labels, [string]$Url, [bool]$ExpectSuccess) {
    try {
        $nativePreference = $PSNativeCommandUseErrorActionPreference
        $PSNativeCommandUseErrorActionPreference = $false
        & kubectl -n $Namespace delete pod $Name --ignore-not-found --wait=true *> $null
        & kubectl -n $Namespace run $Name --image=curlimages/curl:8.12.1 --restart=Never --labels=$Labels --command -- sh -ec "curl --silent --show-error --fail --connect-timeout 3 '$Url' >/dev/null" *> $null
        if ($LASTEXITCODE -ne 0) { throw "Failed to create network probe $Name." }
        & kubectl -n $Namespace wait --for=jsonpath='{.status.phase}'=Succeeded "pod/$Name" --timeout=15s *> $null
        $succeeded = $LASTEXITCODE -eq 0
        if ($succeeded -ne $ExpectSuccess) {
            $logs = (& kubectl -n $Namespace logs $Name 2>&1) -join "`n"
            throw "Network probe $Name expected success=$ExpectSuccess observed success=${succeeded}: $logs"
        }
    }
    finally {
        $PSNativeCommandUseErrorActionPreference = $false
        & kubectl -n $Namespace delete pod $Name --ignore-not-found --wait=false *> $null
        $PSNativeCommandUseErrorActionPreference = $nativePreference
    }
}

foreach ($command in @("cargo", "docker", "kubectl", "pwsh")) { Assert-Command $command }

Push-Location $root
try {
    Add-Timeline "V2-05 E2E started."
    if (-not $SkipLocalGates) {
        & cargo test -p agentx-runtime-contracts -p agentx-control-infrastructure -p agentx-runtime-infrastructure -p agentx-v2-ops -p agentx-observability
        & cargo run --quiet -p agentx-boundary-check -- check
        & (Join-Path $PSScriptRoot "v2-profile-tests.ps1")
        Add-Timeline "V2-05 Contracts, migration, Observability and boundary gates passed."
    }

    & (Join-Path $PSScriptRoot "v2-04-e2e.ps1") -ConfigFile $ConfigFile -RunId $RunId -Stage 05 `
        -BuildImages:$BuildImages -ScaleDownDevelopment:$ScaleDownDevelopment -KeepOnFailure:$KeepOnFailure `
        -KeepOnSuccess -ContextOutputPath $contextPath -SkipLocalGates
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $contextPath)) { throw "V2-04 retained baseline failed." }
    $context = Get-Content -Raw -LiteralPath $contextPath | ConvertFrom-Json
    $baseline = Get-Content -Raw -LiteralPath $context.baselineSummaryPath | ConvertFrom-Json
    $baselineExecution = (($baseline.scenarioCoverage | Where-Object id -eq 2).evidence | Where-Object { $_ -like "execution=*" } | Select-Object -First 1).Substring(10)
    if (-not $baselineExecution) { throw "The retained V2-04 baseline did not expose a full Execution ID." }

    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18480
    $runtimePublicForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-public" 18481
    $runtimeInternalForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-internal" 18482
    $observabilityForward = Start-PortForward $context.namespaces.observability "observability" 18483
    $controlUrl = "http://127.0.0.1:18480"
    $runtimeUrl = "http://127.0.0.1:18481"
    $internalUrl = "http://127.0.0.1:18482"
    $login = Login $controlUrl
    $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $keyResponse = Invoke-Http "$controlUrl/api/v1/applications/$applicationId/api-keys" "POST" $headers @{ name = "V2-05 Query E2E" }
    if ($keyResponse.status -ne 201 -and $keyResponse.status -ne 200) { throw "API Key creation returned $($keyResponse.status)." }
    $apiKey = $keyResponse.body
    Wait-ApiKeyAdmission $apiKey.id

    $evaluationFixture = Invoke-EvaluationFixture
    $evaluationPackageId = [string]$evaluationFixture.evaluation.completedPackageId
    $cancelledEvaluationPackageId = [string]$evaluationFixture.evaluation.cancelledPackageId
    Wait-RuntimePackage $evaluationPackageId @("succeeded") | Out-Null
    Wait-RuntimePackage $cancelledEvaluationPackageId @("cancelled") | Out-Null
    $retentionPolicyAdvance = Invoke-Http "$controlUrl/api/v1/retention-runs" "POST" $headers @{
        dryRun = $true; artifactRetentionDays = 3650; traceRetentionDays = 3650
        messageRetentionDays = 3650; evaluationRetentionDays = 3650
    }
    if ($retentionPolicyAdvance.status -ne 202) { throw "Control Retention Policy advance returned $($retentionPolicyAdvance.status)." }
    $retention = Invoke-Http "$controlUrl/api/v1/retention-runs" "POST" $headers @{
        dryRun = $true; artifactRetentionDays = 3650; traceRetentionDays = 3650
        messageRetentionDays = 3650; evaluationRetentionDays = 3650
    }
    if ($retention.status -ne 202) { throw "Control Retention Intent returned $($retention.status)." }
    $governanceFixture = Wait-ControlGovernanceFixture $evaluationPackageId $retention.body.id
    $runtimeGovernanceSources = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM approval_tasks WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_deleted=FALSE),(SELECT COUNT(*) FROM evaluation_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_deleted=FALSE),(SELECT COUNT(*) FROM notifications WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_deleted=FALSE),(SELECT COUNT(*) FROM runtime_work_packages WHERE tenant_id=UUID_TO_BIN('$tenantId') AND purpose='debug' AND projection_deleted=FALSE),(SELECT COUNT(*) FROM retention_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_deleted=FALSE);") -join "`t"
    if (@($runtimeGovernanceSources -split "`t" | Where-Object { [int]$_ -lt 1 }).Count -ne 0) {
        throw "Runtime governance fixture coverage is incomplete: $runtimeGovernanceSources"
    }
    Add-Timeline "Evaluation, Notification and Control Retention governance fixtures converged: $governanceFixture / $runtimeGovernanceSources"

    $schemaFacts = @(
        ((Invoke-ControlMySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=5 AND success=1;") -join "")
        ((Invoke-RuntimeMySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=5 AND success=1;") -join "")
        ((Invoke-ClickHouse "SELECT count() FROM observability_schema_migrations WHERE version=2") -join "")
    ) -join "/"
    if ($schemaFacts -ne "1/1/1") { throw "V2-05 migration history is incomplete: $schemaFacts" }
    Complete-Scenario 1 "empty V2 domains applied the destructive V2-05 Contract and all three migration histories exactly once" @(
        "migrationHistory=$schemaFacts", "concurrent/replay migration container tests", "old V2-04 fixture rejection contract tests"
    )

    Scale-Deployment $context.namespaces.control "platform-control" 0
    Scale-Deployment $context.namespaces.observability "observability" 0
    Scale-Deployment $context.namespaces.runtime "workflow-runtime" 0
    $requestHash = Get-ContentHash ([ordered]@{ operation = "get_execution"; executionId = $baselineExecution })
    $directToken = New-DelegationToken "agentx-runtime-internal" "runtime.query.execution" $requestHash @($baselineExecution)
    $direct = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $directToken" }
    if ($direct.status -ne 200 -or $direct.body.summary.executionId -ne $baselineExecution) { throw "Runtime authoritative Detail failed with consumers stopped." }
    Scale-Deployment $context.namespaces.runtime "workflow-runtime" 2
    Scale-Deployment $context.namespaces.observability "observability" 2
    Scale-Deployment $context.namespaces.control "platform-control" 2
    Stop-Forward $controlForward; $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18480
    $login = Login $controlUrl; $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    Complete-Scenario 2 "Runtime current state remained queryable while the Sequencer, Control Projector and Trace Consumer were stopped" @(
        "execution=$baselineExecution", "runtimeDetailStatus=200", "consumerReplicas=0"
    )

    $firstPage = Invoke-Http "$controlUrl/api/v1/executions?pageSize=2" "GET" $headers
    if ($firstPage.status -ne 200 -or -not $firstPage.body.nextCursor) { throw "Execution snapshot first page is incomplete." }
    $firstIds = @($firstPage.body.items | ForEach-Object id)
    $between = Start-Invocation $runtimeUrl $apiKey.secret "between-pages"
    $betweenTerminal = Wait-Invocation $runtimeUrl $apiKey.secret $between.id
    $secondPage = Invoke-Http "$controlUrl/api/v1/executions?pageSize=2&cursor=$([Uri]::EscapeDataString($firstPage.body.nextCursor))" "GET" $headers
    if ($secondPage.status -ne 200 -or $secondPage.body.snapshotId -ne $firstPage.body.snapshotId) { throw "Execution snapshot cursor changed identity." }
    $secondIds = @($secondPage.body.items | ForEach-Object id)
    if (@($firstIds | Where-Object { $secondIds -contains $_ }).Count -ne 0 -or $secondIds -contains $betweenTerminal.executionId) {
        throw "Strong-consistency pagination duplicated an item or admitted a post-snapshot Execution."
    }
    Complete-Scenario 3 "concurrent Execution creation did not duplicate, omit within the snapshot, or leak into later pages" @(
        "snapshot=$($firstPage.body.snapshotId)", "first=$($firstIds -join ',')", "second=$($secondIds -join ',')", "concurrent=$($betweenTerminal.executionId)"
    )

    $jti = [Guid]::NewGuid().ToString()
    $replayToken = New-DelegationToken "agentx-runtime-internal" "runtime.query.execution" $requestHash @($baselineExecution) $tenantId ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds()) $jti
    $accepted = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $replayToken" }
    $replayed = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $replayToken" }
    $wrongScope = New-DelegationToken "agentx-runtime-internal" "runtime.query.executions" $requestHash @($baselineExecution)
    $wrongScopeResult = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $wrongScope" }
    $wrongTenant = New-DelegationToken "agentx-runtime-internal" "runtime.query.execution" $requestHash @($baselineExecution) "018f0000-0000-7000-8000-000000000099"
    $wrongTenantResult = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $wrongTenant" }
    $expiredAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - 120
    $expired = New-DelegationToken "agentx-runtime-internal" "runtime.query.execution" $requestHash @($baselineExecution) $tenantId $expiredAt
    $expiredResult = Invoke-Http "$internalUrl/internal/runtime/v1/query/executions/$baselineExecution" "GET" @{ Authorization = "Bearer $expired" }
    $queryReceiptCount = [int]((Invoke-RuntimeMySql "SELECT COUNT(*) FROM runtime_query_receipts WHERE jti=UUID_TO_BIN('$jti');") -join "")
    if ($accepted.status -ne 200 -or $replayed.status -ne 401 -or $wrongScopeResult.status -ne 401 -or $wrongTenantResult.status -ne 401 -or $expiredResult.status -ne 401) {
        throw "Delegation isolation failed: $($accepted.status)/$($replayed.status)/$($wrongScopeResult.status)/$($wrongTenantResult.status)/$($expiredResult.status)"
    }
    if ($queryReceiptCount -ne 1) { throw "Delegation replay created $queryReceiptCount Runtime Query receipts." }
    Complete-Scenario 4 "Delegation JWT rejected replay, cross-Tenant, wrong-Scope and expired access" @(
        "statuses=200/401/401/401/401", "jti=$jti", "receipts=$queryReceiptCount"
    )

    $projection = Wait-ProjectionReady
    $receiptFacts = (Invoke-ControlMySql "SELECT COUNT(*),COUNT(DISTINCT event_id) FROM projection_receipts WHERE projector_name='runtime_governance_v1';") -join "`t"
    $receiptParts = $receiptFacts -split "`t"
    if ($receiptParts.Count -ne 2 -or $receiptParts[0] -ne $receiptParts[1]) { throw "Projector replicas created duplicate receipts: $receiptFacts" }
    Complete-Scenario 5 "two compact-profile Projector replicas converged through one Lease and one Receipt per Event" @(
        "projection=$projection", "receipts=$receiptFacts", "replicas=2"
    )

    $beforeBacklog = [uint64]((Invoke-RuntimeMySql "SELECT next_cursor-1 FROM integration_event_sequence WHERE sequence_key='runtime';") -join "")
    Scale-Deployment $context.namespaces.control "platform-control" 0
    Stop-Forward $controlForward
    $backlogExecutions = @()
    foreach ($index in 1..10) {
        $created = Start-Invocation $runtimeUrl $apiKey.secret "backlog-$index"
        $terminal = Wait-Invocation $runtimeUrl $apiKey.secret $created.id
        $backlogExecutions += $terminal.executionId
    }
    $backlogUpper = [uint64]((Invoke-RuntimeMySql "SELECT next_cursor-1 FROM integration_event_sequence WHERE sequence_key='runtime';") -join "")
    if ($backlogUpper -le $beforeBacklog) { throw "The offline period created no Integration Event backlog." }
    Invoke-RuntimeMySql "UPDATE integration_event_log SET occurred_at=DATE_SUB(occurred_at,INTERVAL 72 HOUR) WHERE event_cursor>$beforeBacklog;" | Out-Null
    Scale-Deployment $context.namespaces.control "platform-control" 2
    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18480
    $login = Login $controlUrl; $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $backlogProjection = Wait-ProjectionReady $backlogUpper
    $backlogReceipts = [int]((Invoke-ControlMySql "SELECT COUNT(*) FROM projection_receipts WHERE projector_name='runtime_governance_v1' AND event_cursor>$beforeBacklog AND event_cursor<=$backlogUpper;") -join "")
    if ($backlogReceipts -ne [int]($backlogUpper - $beforeBacklog)) { throw "The 72-hour-equivalent backlog did not converge by Cursor." }
    Complete-Scenario 6 "a 72-hour-equivalent Control outage backlog converged by continuous Cursor without duplicate Receipts" @(
        "cursor=$beforeBacklog->$backlogUpper", "receipts=$backlogReceipts", "projection=$backlogProjection"
    )

    $oldGeneration = [uint64]((Invoke-ControlMySql "SELECT active_generation FROM runtime_projection_status WHERE projection_name='runtime_governance_v1' AND partition_key='global';") -join "")
    Scale-Deployment $context.namespaces.control "platform-control" 0
    Stop-Forward $controlForward
    Scale-Deployment $context.namespaces.runtime "runtime-gateway" 0
    Stop-Forward $runtimePublicForward; Stop-Forward $runtimeInternalForward
    Invoke-ControlMySql "UPDATE runtime_projection_status SET state='rebuilding',active_generation=0,building_generation=$($oldGeneration+1) WHERE projection_name='runtime_governance_v1' AND partition_key='global';" | Out-Null
    Scale-Deployment $context.namespaces.control "platform-control" 2
    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18480
    $login = Login $controlUrl; $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $rebuilding = Invoke-Http "$controlUrl/api/v1/approvals" "GET" $headers
    if ($rebuilding.status -ne 503 -or $rebuilding.body.code -ne "PROJECTION_REBUILDING") {
        throw "An empty Projection Generation returned HTTP $($rebuilding.status) code '$($rebuilding.body.code)' instead of 503 PROJECTION_REBUILDING."
    }
    Scale-Deployment $context.namespaces.control "platform-control" 0
    Stop-Forward $controlForward
    Invoke-ControlMySql "UPDATE runtime_projection_status SET state='ready',active_generation=$oldGeneration,building_generation=NULL WHERE projection_name='runtime_governance_v1' AND partition_key='global'; UPDATE runtime_projection_cursors SET last_cursor=0,locked_by=NULL,locked_until=NULL WHERE projection_name='runtime_governance_v1' AND partition_key='global';" | Out-Null
    Invoke-RuntimeMySql "UPDATE integration_event_sequence SET retention_floor_cursor=$backlogUpper WHERE sequence_key='runtime';" | Out-Null
    Scale-Deployment $context.namespaces.runtime "runtime-gateway" 2
    $runtimePublicForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-public" 18481
    $runtimeInternalForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-internal" 18482
    Scale-Deployment $context.namespaces.control "platform-control" 2
    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18480
    $login = Login $controlUrl; $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $rebuilt = Wait-ProjectionReady $backlogUpper ($oldGeneration + 1)
    Complete-Scenario 7 "an expired Event Cursor rebuilt a Shadow Generation from Snapshot and atomically caught up" @(
        "oldGeneration=$oldGeneration", "rebuilt=$rebuilt", "expiredFloor=$backlogUpper", "rebuildingHttp=503"
    )

    $governanceResponse = Invoke-WebRequest "$controlUrl/api/v1/notifications" -Headers $headers -SkipHttpErrorCheck
    $governanceCounts = (Invoke-ControlMySql @"
SELECT
 (SELECT COUNT(*) FROM approval_task_projection WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_generation=$($oldGeneration+1) AND projection_deleted=FALSE),
 (SELECT COUNT(*) FROM evaluation_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_generation=$($oldGeneration+1) AND projection_deleted=FALSE),
 (SELECT COUNT(*) FROM notifications WHERE tenant_id=UUID_TO_BIN('$tenantId') AND source_plane='runtime' AND projection_generation=$($oldGeneration+1) AND projection_deleted=FALSE),
 (SELECT COUNT(*) FROM workflow_debug_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_generation=$($oldGeneration+1) AND projection_deleted=FALSE),
 (SELECT COUNT(*) FROM retention_runs WHERE tenant_id=UUID_TO_BIN('$tenantId') AND projection_generation=$($oldGeneration+1) AND projection_deleted=FALSE);
"@) -join "`t"
    $governanceColumns = @($governanceCounts -split "`t")
    if ([int]$governanceResponse.StatusCode -ne 200 -or $governanceResponse.Headers['X-Agentx-Projection-State'] -ne "ready" -or $governanceColumns.Count -ne 5 -or @($governanceColumns | Where-Object { [int]$_ -lt 1 }).Count -ne 0) {
        throw "Governance Generation did not expose an atomic ready view: $governanceCounts"
    }
    Complete-Scenario 8 "Approval, Evaluation, Notification, Debug and Retention projections switched as one complete Generation" @(
        "generation=$($oldGeneration+1)", "counts=$governanceCounts", "headerState=$($governanceResponse.Headers['X-Agentx-Projection-State'])"
    )

    $traceBefore = Wait-Trace $controlUrl $headers $baselineExecution
    $traceEventId = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(event_id) FROM trace_outbox WHERE execution_id=UUID_TO_BIN('$baselineExecution') ORDER BY execution_sequence LIMIT 1;") -join ""
    $beforeDedup = (Invoke-ClickHouse "SELECT count() FROM workflow_trace_events FINAL WHERE event_id=toUUID('$traceEventId')") -join ""
    Invoke-RuntimeMySql "UPDATE trace_outbox SET status='pending',stream_id=NULL,streamed_at=NULL,locked_by=NULL,locked_until=NULL WHERE event_id=UUID_TO_BIN('$traceEventId');" | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", "pod", "-l", "app.kubernetes.io/name=workflow-runtime", "--wait=false") | Out-Null
    Wait-Deployment $context.namespaces.runtime "workflow-runtime"
    $deadline = (Get-Date).AddMinutes(3)
    do {
        $state = (Invoke-RuntimeMySql "SELECT status FROM trace_outbox WHERE event_id=UUID_TO_BIN('$traceEventId');") -join ""
        if ($state -eq "streamed") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    $afterDedup = (Invoke-ClickHouse "SELECT count() FROM workflow_trace_events FINAL WHERE event_id=toUUID('$traceEventId')") -join ""
    $conflicts = (Invoke-ClickHouse "SELECT count() FROM trace_ingest_conflicts WHERE event_id=toUUID('$traceEventId')") -join ""
    if ($state -ne "streamed" -or $beforeDedup -ne "1" -or $afterDedup -ne "1" -or $conflicts -ne "0") { throw "Trace replay did not deduplicate: $state/$beforeDedup/$afterDedup/$conflicts" }
    Complete-Scenario 9 "Trace Relay crash/replay produced one business Trace and no silent hash conflict" @(
        "event=$traceEventId", "before=$beforeDedup", "after=$afterDedup", "conflicts=$conflicts"
    )

    Invoke-Kubectl @("-n", [string]$context.namespaces.observability, "scale", "statefulset/clickhouse", "--replicas=0") | Out-Null
    Start-Sleep -Seconds 3
    $duringOutage = Start-Invocation $runtimeUrl $apiKey.secret "clickhouse-down"
    $duringTerminal = Wait-Invocation $runtimeUrl $apiKey.secret $duringOutage.id
    $detailDuring = Invoke-Http "$controlUrl/api/v1/executions/$($duringTerminal.executionId)" "GET" $headers
    $traceDuring = Invoke-Http "$controlUrl/api/v1/executions/$($duringTerminal.executionId)/trace" "GET" $headers
    if ($duringTerminal.status -ne "completed" -or $detailDuring.status -ne 200 -or $traceDuring.status -notin @(202, 503)) {
        throw "ClickHouse outage affected Runtime authority or was hidden: $($duringTerminal.status)/$($detailDuring.status)/$($traceDuring.status)"
    }
    Complete-Scenario 11 "Browser Detail kept the Runtime terminal state while Trace explicitly reported delayed or unavailable" @(
        "execution=$($duringTerminal.executionId)", "detail=200", "trace=$($traceDuring.status)"
    )
    $recoveryStarted = [DateTimeOffset]::UtcNow
    Invoke-Kubectl @("-n", [string]$context.namespaces.observability, "scale", "statefulset/clickhouse", "--replicas=1") | Out-Null
    Wait-StatefulSet $context.namespaces.observability "clickhouse"
    Wait-Deployment $context.namespaces.observability "observability"
    Stop-Forward $observabilityForward; $observabilityForward = Start-PortForward $context.namespaces.observability "observability" 18483
    $recoveredTrace = Wait-Trace $controlUrl $headers $duringTerminal.executionId 300
    $recoverySeconds = [int]([DateTimeOffset]::UtcNow - $recoveryStarted).TotalSeconds
    if ($recoverySeconds -gt 300) { throw "Trace backlog recovery exceeded 300 seconds." }
    Complete-Scenario 10 "ClickHouse outage did not affect Workflow terminal state and Trace backlog recovered within 300 seconds" @(
        "execution=$($duringTerminal.executionId)", "recoverySeconds=$recoverySeconds", "traceEvents=$(@($recoveredTrace.body.events).Count)"
    )

    $controlManifest = (Invoke-Kubectl @("-n", [string]$context.namespaces.control, "get", "deployment/platform-control", "-o", "json")) -join ""
    $observabilityManifest = (Invoke-Kubectl @("-n", [string]$context.namespaces.observability, "get", "deployment/observability", "-o", "json")) -join ""
    if ($controlManifest -match "AGENTX_RUNTIME_MYSQL" -or $observabilityManifest -match "AGENTX_.*MYSQL|AGENTX_RUNTIME_S3|AGENTX_CONTROL_S3") {
        throw "A Control or Observability Pod holds a forbidden Runtime/MySQL/Object credential."
    }
    Run-NetworkProbe $context.namespaces.control "v205-positive-runtime-api" "agentx.io/plane=control" "http://runtime-gateway-internal.$($context.namespaces.runtime).svc:8080/health/live" $true
    Run-NetworkProbe $context.namespaces.runtime "v205-negative-control-mysql" "agentx.io/plane=runtime" "telnet://control-mysql.$($context.namespaces.control).svc:3306" $false
    $controlRuntimeSummaryTables = [int]((Invoke-ControlMySql "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('workflow_executions','application_invocations','runtime_query_snapshots','workflow_trace_events');") -join "")
    if ($controlRuntimeSummaryTables -ne 0) { throw "Control MySQL contains a forbidden generic Runtime/Trace authority table." }
    Complete-Scenario 12 "E2E-V2-009 credentials, table ownership and NetworkPolicy positive/negative boundaries passed" @(
        "controlToRuntimeInternal=allowed", "runtimeToControlMySql=denied", "controlRuntimeSummaryTables=0", "forbiddenCredentials=0"
    )

    $completed = $scenarios.Count -eq 12 -and @($scenarios | Where-Object status -ne "passed").Count -eq 0
    if (-not $completed) { throw "V2-05 scenario matrix is incomplete." }
    [ordered]@{
        apiVersion = "agentx.io/evidence/v1"
        stage = "v2-05"
        stageComplete = $true
        runId = $RunId
        profile = $context.profilePath
        namespaces = $context.namespaces
        baseline = $context.baselineSummaryPath
        scenarioCoverage = @($scenarios | Sort-Object id)
        timeline = @($timeline)
        sensitiveData = "Tokens, API keys, passwords, raw Trace attributes and business payloads are excluded."
    } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath
}
catch {
    $failure = $_
    Add-Timeline "FAILED: $($failure.Exception.Message)"
    [ordered]@{
        apiVersion = "agentx.io/evidence/v1"; stage = "v2-05"; stageComplete = $false; runId = $RunId
        scenarioCoverage = @($scenarios | Sort-Object id); failure = $failure.Exception.Message; timeline = @($timeline)
        sensitiveData = "Tokens, API keys, passwords, raw Trace attributes and business payloads are excluded."
    } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath
    throw
}
finally {
    Add-Timeline "Cleanup begin success=$completed."
    Stop-Forwards
    if ($context) {
        foreach ($target in @(
            @{ namespace = $context.namespaces.control; resource = "deployment/platform-control" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/runtime-gateway" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/workflow-runtime" },
            @{ namespace = $context.namespaces.observability; resource = "deployment/observability" }
        )) {
            $safe = $target.resource.Replace('/', '-')
            $nativePreference = $PSNativeCommandUseErrorActionPreference
            try {
                $PSNativeCommandUseErrorActionPreference = $false
                & kubectl -n $target.namespace logs $target.resource --all-pods=true --prefix --tail=500 *> (Join-Path $artifactDirectory "$safe.log")
            }
            finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
        }
        try {
            Scale-Deployment $context.namespaces.runtime "runtime-gateway" 2
            Scale-Deployment $context.namespaces.runtime "workflow-runtime" 2
            Scale-Deployment $context.namespaces.observability "observability" 2
            Invoke-Kubectl @("-n", [string]$context.namespaces.observability, "scale", "statefulset/clickhouse", "--replicas=1") | Out-Null
            Scale-Deployment $context.namespaces.control "platform-control" 2
        }
        catch { Add-Timeline "Workload recovery warning: $($_.Exception.Message)" }
        if ($completed -or -not $KeepOnFailure) {
            & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -ConfigFile $context.profilePath -RunId "05-$RunId"
        }
    }
    Add-Timeline "Cleanup end."
    $timeline | Set-Content -LiteralPath (Join-Path $artifactDirectory "timeline.log")
    Pop-Location
}

if (-not $completed) { throw "V2-05 E2E did not complete; see $artifactDirectory" }
Write-Output "V2-05 E2E passed all 12 scenarios. Evidence: $artifactDirectory"
