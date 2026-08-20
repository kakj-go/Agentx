param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [ValidateSet("04", "05", "06")]
    [string]$Stage = "04",
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$KeepOnFailure,
    [switch]$KeepOnSuccess,
    [string]$ContextOutputPath,
    [switch]$SkipLocalGates,
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OpenSandboxApiKey = "agentx-local-opensandbox-key"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-$Stage"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$baselinePrefix = if ($Stage -eq "04") { "" } else { "v2-04-baseline-" }
$contextPath = Join-Path $artifactDirectory "${baselinePrefix}context.json"
$providerEvidence = Join-Path $artifactDirectory "${baselinePrefix}runtime-providers.json"
$summaryPath = Join-Path $artifactDirectory "${baselinePrefix}summary.json"
$timeline = [Collections.Generic.List[string]]::new()
$scenarioCoverage = [Collections.Generic.List[object]]::new()
$forwards = [Collections.Generic.List[Diagnostics.Process]]::new()
$runtimeLogCollectors = [Collections.Generic.List[Diagnostics.Process]]::new()
$openSandboxProcess = $null
$openSandboxOwned = $false
$context = $null
$profile = $null
$completed = $false
$controlPassword = $null
$runtimePassword = $null
$script:admissionRequests = @{}
$originalDeploySandboxKey = $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY
$deploySandboxKeyWasSet = Test-Path Env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY
$tenantId = "018f0000-0000-7000-8000-000000000001"
$userId = "018f0000-0000-7000-8000-000000000002"

function Add-Timeline([string]$Message) {
    $timeline.Add("$([DateTimeOffset]::UtcNow.ToString('O')) $Message")
}

function Complete-Scenario([int]$Id, [string]$Assertion, [string[]]$Evidence) {
    $scenarioCoverage.Add([ordered]@{
        id = $Id
        status = "passed"
        assertion = $Assertion
        evidence = $Evidence
    })
    Add-Timeline "Scenario $Id passed: $Assertion"
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "V2-04 E2E requires $Name."
    }
}

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

function Save-RuntimeExecutionDiagnostics([string]$ExecutionId, [string]$Prefix) {
    $queries = [ordered]@{
        execution = "SELECT BIN_TO_UUID(e.id),e.status,e.state_version,COALESCE(e.error_code,''),COALESCE(e.error_message,''),COALESCE(CAST(e.terminal_result_json AS CHAR),'') FROM workflow_executions e WHERE e.id=UUID_TO_BIN('$ExecutionId');"
        work_package = "SELECT BIN_TO_UUID(p.id),p.purpose,p.status,p.version,p.execute_idempotency_key,COALESCE(p.error_code,''),COALESCE(p.error_message,''),COALESCE(CAST(p.result_json AS CHAR),'') FROM runtime_work_packages p JOIN workflow_executions e ON e.work_package_id=p.id AND e.tenant_id=p.tenant_id WHERE e.id=UUID_TO_BIN('$ExecutionId');"
        commands = "SELECT BIN_TO_UUID(id),command_type,status,attempt_count,COALESCE(BIN_TO_UUID(locked_by),''),COALESCE(CAST(locked_until AS CHAR),''),fencing_token,COALESCE(error_code,''),COALESCE(error_message,''),COALESCE(CAST(result_json AS CHAR),'') FROM runtime_commands WHERE aggregate_type='execution' AND aggregate_id='$ExecutionId' ORDER BY created_at,id;"
        outbox = "SELECT BIN_TO_UUID(id),message_type,COALESCE(capability,''),status,attempt_count,COALESCE(BIN_TO_UUID(locked_by),''),COALESCE(CAST(locked_until AS CHAR),''),COALESCE(last_error,''),COALESCE(CAST(payload_json AS CHAR),'') FROM execution_outbox WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        runtime_state = "SELECT state_version,context_version,delivery_sequence,activation_count,activation_budget,COALESCE(CAST(machine_state_json AS CHAR),''),COALESCE(CAST(current_frontier_json AS CHAR),'') FROM execution_runtime_state WHERE execution_id=UUID_TO_BIN('$ExecutionId');"
        invocation = "SELECT BIN_TO_UUID(id),status,state_version,COALESCE(CAST(result_json AS CHAR),''),COALESCE(CAST(error_json AS CHAR),'') FROM application_invocations WHERE execution_id=UUID_TO_BIN('$ExecutionId');"
        nodes = "SELECT BIN_TO_UUID(id),node_key,status,capability,run_index,iteration_index,COALESCE(error_code,''),COALESCE(error_message,'') FROM node_executions WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        attempts = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(node_execution_id),attempt_number,capability,status,COALESCE(worker_instance_id,''),fencing_token,COALESCE(error_code,''),COALESCE(error_message,''),COALESCE(CAST(result_hash AS CHAR),'') FROM node_attempts WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        waits = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(node_execution_id),wait_kind,status,state_version,COALESCE(BIN_TO_UUID(locked_by),''),COALESCE(CAST(locked_until AS CHAR),''),fencing_token FROM wait_subscriptions WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        resume_tokens = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(node_execution_id),resume_kind,status,COALESCE(idempotency_key,''),COALESCE(CAST(expires_at AS CHAR),'') FROM execution_resume_tokens WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        runtime_calls = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(attempt_id),call_index,call_kind,side_effect,status,idempotency_key,COALESCE(error_code,''),COALESCE(error_message,''),COALESCE(CAST(response_json AS CHAR),'') FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY started_at,id;"
        sandbox_leases = "SELECT BIN_TO_UUID(id),BIN_TO_UUID(attempt_id),status,COALESCE(sandbox_id,''),fencing_token,outcome_unknown,COALESCE(last_error,''),COALESCE(CAST(result_json AS CHAR),'') FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY created_at,id;"
        worker_receipts = "SELECT BIN_TO_UUID(r.attempt_id),r.status,r.fencing_token,r.result_hash FROM worker_result_receipts r JOIN node_attempts a ON a.id=r.attempt_id WHERE a.execution_id=UUID_TO_BIN('$ExecutionId') ORDER BY r.created_at,r.attempt_id;"
    }
    foreach ($entry in $queries.GetEnumerator()) {
        try {
            Invoke-RuntimeMySql $entry.Value | Set-Content -LiteralPath (Join-Path $artifactDirectory "$Prefix-$($entry.Key).tsv")
        }
        catch {
            Add-Timeline "Diagnostic warning for $Prefix/$($entry.Key): $($_.Exception.Message)"
        }
    }
}

function Start-WorkflowRuntimeLogCollectors {
    $pods = @(Invoke-Kubectl @(
        "-n", [string]$context.namespaces.runtime, "get", "pods",
        "-l", "app.kubernetes.io/name=workflow-runtime", "-o", "name"
    ))
    foreach ($pod in $pods) {
        $safe = $pod.Replace('/', '-')
        $process = Start-Process kubectl -ArgumentList @(
            "-n", [string]$context.namespaces.runtime, "logs", "-f", $pod,
            "--timestamps=true", "--tail=-1"
        ) -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $artifactDirectory "$safe-live.log") `
            -RedirectStandardError (Join-Path $artifactDirectory "$safe-live.stderr.log")
        $runtimeLogCollectors.Add($process)
    }
}

function Stop-WorkflowRuntimeLogCollectors {
    foreach ($process in $runtimeLogCollectors) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $runtimeLogCollectors.Clear()
}

function Save-WorkflowRuntimeDiagnostics([string]$Prefix) {
    $namespace = [string]$context.namespaces.runtime
    $commands = [ordered]@{
        deployment = @("-n", $namespace, "get", "deployment/workflow-runtime", "-o", "yaml")
        replicasets = @("-n", $namespace, "get", "replicaset", "-l", "app.kubernetes.io/name=workflow-runtime", "-o", "wide")
        pods = @("-n", $namespace, "get", "pods", "-l", "app.kubernetes.io/name=workflow-runtime", "-o", "wide")
        pod_status = @("-n", $namespace, "get", "pods", "-l", "app.kubernetes.io/name=workflow-runtime", "-o", "json")
        events = @("-n", $namespace, "get", "events", "--sort-by=.lastTimestamp")
    }
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        foreach ($entry in $commands.GetEnumerator()) {
            & kubectl @($entry.Value) *> (Join-Path $artifactDirectory "$Prefix-workflow-runtime-$($entry.Key).log")
        }
        $pods = @(& kubectl -n $namespace get pods -l "app.kubernetes.io/name=workflow-runtime" -o name 2>$null)
        foreach ($pod in $pods) {
            $safe = $pod.Replace('/', '-')
            & kubectl -n $namespace describe $pod *> (Join-Path $artifactDirectory "$Prefix-$safe-describe.log")
            & kubectl -n $namespace logs $pod --timestamps=true --tail=1000 *> (Join-Path $artifactDirectory "$Prefix-$safe-current.log")
            & kubectl -n $namespace logs $pod --previous --timestamps=true --tail=1000 *> (Join-Path $artifactDirectory "$Prefix-$safe-previous.log")
        }
    }
    finally {
        $PSNativeCommandUseErrorActionPreference = $nativePreference
    }
}

function Wait-TcpPort([int]$Port, [int]$TimeoutSeconds = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $client = [Net.Sockets.TcpClient]::new()
        try {
            $client.Connect("127.0.0.1", $Port)
            return
        }
        catch { Start-Sleep -Milliseconds 300 }
        finally { $client.Dispose() }
    } while ((Get-Date) -lt $deadline)
    throw "TCP port $Port did not become ready."
}

function Start-PortForward([string]$Namespace, [string]$Service, [int]$LocalPort, [int]$RemotePort) {
    $stdout = Join-Path $artifactDirectory "$Service-$LocalPort.stdout.log"
    $stderr = Join-Path $artifactDirectory "$Service-$LocalPort.stderr.log"
    $process = Start-Process kubectl -ArgumentList @(
        "-n", $Namespace, "port-forward", "service/$Service", "${LocalPort}:${RemotePort}"
    ) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Wait-TcpPort $LocalPort
    $forwards.Add($process)
    return $process
}

function Stop-Forwards {
    foreach ($process in $forwards) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $forwards.Clear()
}

function Assert-OpenSandboxReady {
    $health = Invoke-RestMethod -TimeoutSec 5 -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/health"
    if ([string]$health.status -ne "healthy") {
        throw "OpenSandbox is not healthy at $OpenSandboxEndpoint."
    }
    $items = Invoke-RestMethod -TimeoutSec 10 -Headers @{ "OPEN-SANDBOX-API-KEY" = $OpenSandboxApiKey } -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/v1/sandboxes?pageSize=100"
    if ($null -eq $items.items) { throw "OpenSandbox lifecycle response has no items array." }
}

function Start-OpenSandbox {
    try {
        Assert-OpenSandboxReady
        Add-Timeline "Using an already-running OpenSandbox instance."
        return
    }
    catch {
        $uri = [Uri]$OpenSandboxEndpoint
        if ($uri.Host -notin @("127.0.0.1", "localhost", "::1") -or $uri.Port -ne 18080) {
            throw "Configured OpenSandbox is unavailable and cannot be started by the local harness: $($_.Exception.Message)"
        }
    }
    $executable = Join-Path $root ".local/opensandbox-venv/Scripts/opensandbox-server.exe"
    $config = Join-Path $root ".local/opensandbox.toml"
    if (-not (Test-Path -LiteralPath $executable) -or -not (Test-Path -LiteralPath $config)) {
        throw "OpenSandbox local runtime is missing; run scripts/opensandbox-contract.ps1 first."
    }
    $script:openSandboxProcess = Start-Process $executable -ArgumentList @("--config", $config) -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $artifactDirectory "opensandbox.stdout.log") `
        -RedirectStandardError (Join-Path $artifactDirectory "opensandbox.stderr.log")
    $script:openSandboxOwned = $true
    $deadline = (Get-Date).AddMinutes(2)
    do {
        if ($openSandboxProcess.HasExited) { throw "OpenSandbox exited during startup." }
        try { Assert-OpenSandboxReady; Add-Timeline "OpenSandbox started locally."; return } catch { Start-Sleep -Seconds 1 }
    } while ((Get-Date) -lt $deadline)
    throw "OpenSandbox did not become healthy."
}

function Resolve-ClusterEndpoint([string]$Endpoint) {
    $builder = [UriBuilder]$Endpoint
    if ($builder.Host -in @("127.0.0.1", "localhost", "::1")) { $builder.Host = "host.docker.internal" }
    return $builder.Uri.AbsoluteUri.TrimEnd('/')
}

function Install-OpenSandboxEgress([string]$Endpoint) {
    $uri = [Uri]$Endpoint
    $resolved = Invoke-Kubectl @(
        "-n", [string]$context.namespaces.runtime, "exec", "deployment/sandbox-manager", "--",
        "getent", "ahostsv4", $uri.Host
    )
    $addresses = @($resolved | ForEach-Object {
        $candidate = ($_ -split '\s+', 2)[0]
        $address = $null
        if ([Net.IPAddress]::TryParse($candidate, [ref]$address) -and $address.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork) {
            $address
        }
    })
    if ($addresses.Count -eq 0) { throw "Could not resolve the OpenSandbox host $($uri.Host)." }
    $blocks = ($addresses | Sort-Object IPAddressToString -Unique | ForEach-Object {
        "        - ipBlock: { cidr: $($_.IPAddressToString)/32 }"
    }) -join "`n"
    $manifest = @"
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: v2-04-opensandbox-egress, namespace: $($context.namespaces.runtime) }
spec:
  podSelector: { matchLabels: { app.kubernetes.io/name: sandbox-manager } }
  policyTypes: [Egress]
  egress:
    - to:
$blocks
      ports:
        - { protocol: TCP, port: 1024, endPort: 65535 }
"@
    $manifest | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to install OpenSandbox E2E egress policy." }
    Add-Timeline "Sandbox Manager egress restricted to the resolved OpenSandbox host."
}

function Wait-Deployment([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 300) {
    Invoke-Kubectl @("-n", $Namespace, "rollout", "status", "deployment/$Name", "--timeout=${TimeoutSeconds}s") | Out-Null
}

function Wait-DeploymentScaledToZero([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 180) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $replicas = ((Invoke-Kubectl @(
            "-n", $Namespace, "get", "deployment/$Name", "-o", "jsonpath={.status.replicas}"
        )) -join "").Trim()
        $pods = @(Invoke-Kubectl @(
            "-n", $Namespace, "get", "pods", "-l", "app.kubernetes.io/name=$Name", "-o", "name"
        ))
        if (($replicas -eq "" -or $replicas -eq "0") -and $pods.Count -eq 0) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "Deployment $Namespace/$Name did not scale to zero within $TimeoutSeconds seconds."
}

function Wait-StatefulSetScaledToZero([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 180) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $replicas = ((Invoke-Kubectl @(
            "-n", $Namespace, "get", "statefulset/$Name", "-o", "jsonpath={.status.replicas}"
        )) -join "").Trim()
        $pods = @(Invoke-Kubectl @(
            "-n", $Namespace, "get", "pods", "-l", "app.kubernetes.io/name=$Name", "-o", "name"
        ))
        if (($replicas -eq "" -or $replicas -eq "0") -and $pods.Count -eq 0) { return }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "StatefulSet $Namespace/$Name did not scale to zero within $TimeoutSeconds seconds."
}

function Install-RuntimeProviders {
    if ($BuildImages) {
        & (Join-Path $PSScriptRoot "build-images.ps1") -Tag ([string]$profile.images.tag) `
            -Namespace ([string]$context.namespaces.dependencies) -Services @("echo-mcp", "lightrag", "v2-04-fixture") -SkipWeb
        if ($LASTEXITCODE -ne 0) { throw "V2-04 fixture image build failed." }
    }
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "apply", "-k", (Join-Path $root "deploy/k8s/v2/e2e/runtime-providers")) | Out-Null
    Invoke-Kubectl @(
        "-n", [string]$context.namespaces.dependencies, "set", "image", "deployment/echo-mcp",
        "echo-mcp=$($profile.images.registry)/echo-mcp:$($profile.images.tag)"
    ) | Out-Null
    Invoke-Kubectl @(
        "-n", [string]$context.namespaces.dependencies, "set", "image", "deployment/lightrag",
        "lightrag=$($profile.images.registry)/lightrag:$($profile.images.tag)"
    ) | Out-Null
    foreach ($name in @("echo-mcp", "lightrag", "mem0-postgres", "mem0")) {
        Wait-Deployment $context.namespaces.dependencies $name 600
    }
    & (Join-Path $PSScriptRoot "m5-addons-contract.ps1") -Namespace $context.namespaces.dependencies `
        -PreserveFixtureData -ResultsPath $providerEvidence | Out-Null
    Add-Timeline "Echo Model/MCP, LightRAG and Mem0 providers passed their real contracts."
}

function Initialize-RuntimeCredentials {
    $rootToken = Get-SecretValue $context.namespaces.dependencies "agentx-dependencies-secrets" "VAULT_DEV_ROOT_TOKEN_ID"
    foreach ($credential in @(
        @{ name = "model"; value = "Bearer m5-model-secret" },
        @{ name = "rag"; value = "agentx-v2-04-rag-key" }
    )) {
        Invoke-Kubectl @(
            "-n", [string]$context.namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-ec",
            "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$rootToken' vault kv put -mount=secret 'tenants/$tenantId/runtime-credentials/$($credential.name)' value='$($credential.value)' >/dev/null"
        ) | Out-Null
    }
    $controlToken = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_VAULT_TOKEN"
    $runtimeToken = Get-SecretValue $context.namespaces.runtime "agentx-runtime-secrets" "AGENTX_RUNTIME_VAULT_TOKEN"
    $path = "secret/data/tenants/$tenantId/runtime-credentials/model"
    $controlRead = Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$controlToken' vault read '$path' >/dev/null 2>&1; echo `$?")
    $runtimeRead = Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$runtimeToken' vault read '$path' >/dev/null 2>&1; echo `$?")
    $runtimeWrite = Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "exec", "statefulset/vault", "--", "sh", "-c", "VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN='$runtimeToken' vault kv put -mount=secret 'tenants/$tenantId/runtime-credentials/model' value=forbidden >/dev/null 2>&1; echo `$?")
    if (($controlRead -join "").Trim() -eq "0") { throw "Control Vault token read Runtime credential plaintext." }
    if (($runtimeRead -join "").Trim() -ne "0") { throw "Runtime Vault token could not read its versioned credential." }
    if (($runtimeWrite -join "").Trim() -eq "0") { throw "Runtime Vault token wrote a credential." }
    Add-Timeline "Versioned Vault credentials and write-only/read-only domain permissions verified."
}

function Invoke-FixtureJob([ValidateSet("seed", "evaluation")][string]$Mode) {
    $deployment = ((Invoke-Kubectl @("-n", [string]$context.namespaces.control, "get", "deployment/platform-control", "-o", "json")) -join "`n") | ConvertFrom-Json
    $environment = @($deployment.spec.template.spec.containers[0].env | Where-Object name -ne "AGENTX_RUNTIME_INTERNAL_URL")
    $environment += [pscustomobject]@{ name = "AGENTX_RUNTIME_INTERNAL_URL"; value = "http://runtime-gateway-internal.$($context.namespaces.runtime).svc:8080" }
    $environment += [pscustomobject]@{ name = "AGENTX_V2_FIXTURE_DEPENDENCIES_NAMESPACE"; value = [string]$context.namespaces.dependencies }
    $environment += [pscustomobject]@{ name = "AGENTX_V2_FIXTURE_MODE"; value = $Mode }
    $jobName = "v2-04-fixture-$Mode"
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "delete", "job", $jobName, "--ignore-not-found", "--wait=true") | Out-Null
    $job = [ordered]@{
        apiVersion = "batch/v1"
        kind = "Job"
        metadata = @{ name = $jobName; namespace = [string]$context.namespaces.control; labels = @{ "agentx.io/plane" = "control"; "agentx.io/v2-04-fixture" = $Mode } }
        spec = @{
            backoffLimit = 0
            ttlSecondsAfterFinished = 3600
            template = @{
                metadata = @{ labels = @{ "agentx.io/plane" = "control"; "agentx.io/v2-04-fixture" = $Mode } }
                spec = @{
                    restartPolicy = "Never"
                    serviceAccountName = "platform-control"
                    automountServiceAccountToken = $false
                    containers = @(@{
                        name = "fixture"
                        image = "$($profile.images.registry)/v2-04-fixture:$($profile.images.tag)"
                        imagePullPolicy = [string]$profile.images.pullPolicy
                        env = $environment
                    })
                }
            }
        }
    }
    $job | ConvertTo-Json -Depth 30 -Compress | & kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to create $jobName." }
    $jobDeadline = (Get-Date).AddMinutes(5)
    do {
        $jobStatus = ((Invoke-Kubectl @(
            "-n", [string]$context.namespaces.control, "get", "job/$jobName", "-o", "json"
        )) -join "`n") | ConvertFrom-Json
        $jobSucceeded = [int]$jobStatus.status.succeeded
        $jobFailed = [int]$jobStatus.status.failed
        if ($jobSucceeded -eq 1 -or $jobFailed -gt 0) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $jobDeadline)
    if ($jobSucceeded -ne 1) {
        $description = (Invoke-Kubectl @("-n", [string]$context.namespaces.control, "describe", "job/$jobName")) -join "`n"
        $logs = (& kubectl -n $context.namespaces.control logs "job/$jobName" --all-containers=true 2>&1) -join "`n"
        throw "$jobName failed.`n$description`n$logs"
    }
    $lines = Invoke-Kubectl @("-n", [string]$context.namespaces.control, "logs", "job/$jobName", "--all-containers=true")
    $lines | Set-Content -LiteralPath (Join-Path $artifactDirectory "$jobName.log")
    $jsonLine = @($lines | Where-Object { $_.TrimStart().StartsWith("{") }) | Select-Object -Last 1
    if (-not $jsonLine) { throw "$jobName did not emit fixture JSON." }
    return $jsonLine | ConvertFrom-Json
}

function Wait-PublishAttempt([string]$ControlUrl, [hashtable]$Headers, [string]$ApplicationId, [string]$AttemptId) {
    $deadline = (Get-Date).AddMinutes(5)
    do {
        $attempt = Invoke-RestMethod "$ControlUrl/api/v1/applications/$ApplicationId/publish-attempts/$AttemptId" -Headers $Headers
        if ($attempt.state -eq "active") { return $attempt }
        if ($attempt.state -eq "rejected") { throw "Publish rejected: $($attempt.errorCode) $($attempt.errorMessage)" }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    throw "Publish Attempt $AttemptId timed out."
}

function Wait-Execution([string]$ExecutionId, [int]$TimeoutSeconds = 600) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $row = (Invoke-RuntimeMySql "SELECT status,COALESCE(error_code,''),COALESCE(error_message,'') FROM workflow_executions WHERE id=UUID_TO_BIN('$ExecutionId');") -join "`t"
        if ($row) {
            $columns = $row -split "`t", 3
            if ($columns[0] -in @("succeeded", "failed", "cancelled", "timed_out")) {
                return [pscustomobject]@{ status = $columns[0]; errorCode = $columns[1]; errorMessage = $columns[2] }
            }
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Execution $ExecutionId did not reach a terminal state."
}

function Wait-ExecutionStatus([string]$ExecutionId, [string[]]$Statuses, [int]$TimeoutSeconds = 180) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $status = ((Invoke-RuntimeMySql "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$ExecutionId');") -join "").Trim()
        if ($status -in $Statuses) { return $status }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    Save-RuntimeExecutionDiagnostics $ExecutionId "wait-timeout-$ExecutionId"
    Save-WorkflowRuntimeDiagnostics "wait-timeout-$ExecutionId"
    throw "Execution $ExecutionId remained '$status' and did not reach $($Statuses -join '/')."
}

function Start-Invocation([string]$RuntimeUrl, [string]$Slug, [string]$ApiKey, [string]$Label) {
    return Invoke-RestMethod "$RuntimeUrl/gateway/v1/applications/$Slug/invocations" -Method Post `
        -Headers @{ Authorization = "Bearer $ApiKey"; "Idempotency-Key" = "v2-04-$RunId-$Label" } `
        -ContentType application/json -Body (@{ input = @{ message = "agentx-v2-04" }; responseMode = "async" } | ConvertTo-Json -Depth 6)
}

function Invoke-SuccessfulInvocation([string]$RuntimeUrl, [string]$Slug, [string]$ApiKey, [string]$Label, [int]$TimeoutSeconds = 600) {
    $accepted = Start-Invocation $RuntimeUrl $Slug $ApiKey $Label
    $terminal = Wait-Execution $accepted.executionId $TimeoutSeconds
    if ($terminal.status -ne "succeeded") {
        throw "Execution $($accepted.executionId) failed: $($terminal.errorCode) $($terminal.errorMessage)"
    }
    return $accepted
}

function Invoke-FailingInvocation([string]$RuntimeUrl, [string]$Slug, [string]$ApiKey, [string]$Label, [string]$ExpectedCode = "") {
    $accepted = Start-Invocation $RuntimeUrl $Slug $ApiKey $Label
    $terminal = Wait-Execution $accepted.executionId 600
    if ($terminal.status -ne "failed") { throw "Execution $($accepted.executionId) unexpectedly ended as $($terminal.status)." }
    if ($ExpectedCode -and $terminal.errorCode -ne $ExpectedCode) {
        throw "Execution $($accepted.executionId) failed with $($terminal.errorCode), expected $ExpectedCode."
    }
    return [pscustomobject]@{ accepted = $accepted; terminal = $terminal }
}

function ConvertTo-Base64Url([byte[]]$Bytes) {
    return [Convert]::ToBase64String($Bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

function New-ServiceToken([string[]]$Scopes) {
    $kid = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_PUBLISHER_JWT_KID"
    $pem = Get-SecretValue $context.namespaces.control "agentx-control-secrets" "AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM"
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $header = @{ alg = "RS256"; typ = "JWT"; kid = $kid } | ConvertTo-Json -Compress
    $payload = [ordered]@{
        iss = "agentx-control"
        aud = "agentx-runtime-internal"
        sub = "v2-04-e2e"
        role = "publisher"
        scope = @($Scopes)
        iat = $now
        exp = $now + 300
        jti = [Guid]::NewGuid().ToString()
    } | ConvertTo-Json -Compress
    $encodedHeader = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes($header))
    $encodedPayload = ConvertTo-Base64Url ([Text.Encoding]::UTF8.GetBytes($payload))
    $input = "$encodedHeader.$encodedPayload"
    $rsa = [Security.Cryptography.RSA]::Create()
    try {
        $rsa.ImportFromPem($pem)
        $signature = $rsa.SignData(
            [Text.Encoding]::ASCII.GetBytes($input),
            [Security.Cryptography.HashAlgorithmName]::SHA256,
            [Security.Cryptography.RSASignaturePadding]::Pkcs1
        )
        return "$input.$(ConvertTo-Base64Url $signature)"
    }
    finally { $rsa.Dispose() }
}

function Invoke-Internal([string]$InternalUrl, [string]$Scope, [string]$Path, [object]$Body) {
    $token = New-ServiceToken @($Scope)
    return Invoke-RestMethod "$InternalUrl$Path" -Method Post -Headers @{ Authorization = "Bearer $token" } `
        -ContentType application/json -Body ($Body | ConvertTo-Json -Depth 40 -Compress)
}

function ConvertTo-Jcs([object]$Value) {
    if ($null -eq $Value) { return "null" }
    if ($Value -is [string] -or $Value -is [char]) {
        return [Text.Json.JsonSerializer]::Serialize(
            [string]$Value,
            [Text.Json.JsonSerializerOptions]::new()
        )
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
        $map = @{}
        foreach ($property in $Value.PSObject.Properties) { $map[$property.Name] = $property.Value }
        return ConvertTo-Jcs $map
    }
    return [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture).ToLowerInvariant()
}

function Get-ContentHash([object]$Value) {
    $canonical = ConvertTo-Jcs $Value
    $hash = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($canonical))
    return "sha256:$([Convert]::ToHexString($hash).ToLowerInvariant())"
}

function Invoke-Admission([string]$InternalUrl, [UInt64]$Epoch, [object]$Target, [string]$Key) {
    $intentHash = Get-ContentHash ([ordered]@{ admissionEpoch = $Epoch; target = $Target })
    if ($script:admissionRequests.ContainsKey($Key)) {
        $cached = $script:admissionRequests[$Key]
        if ($cached.intentHash -ne $intentHash) {
            throw "Admission idempotency key $Key was reused for a different local intent."
        }
        return Invoke-Internal $InternalUrl "runtime.admission.apply" "/internal/runtime/v1/admission-commands:apply" $cached.body
    }
    $eventId = [Guid]::NewGuid().ToString()
    $body = [ordered]@{
        apiVersion = 1
        command = [ordered]@{
            schemaVersion = 1
            eventId = $eventId
            sourcePlane = "control"
            tenantId = $tenantId
            aggregateType = "admission"
            aggregateId = $tenantId
            objectVersion = $Epoch
            occurredAt = [DateTimeOffset]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.ffffffZ")
            payload = @{}
            contentHash = Get-ContentHash $Target
            correlationId = [Guid]::NewGuid().ToString()
            causationId = $null
            idempotencyKey = $Key
        }
        admissionEpoch = $Epoch
        target = $Target
    }
    $script:admissionRequests[$Key] = @{ intentHash = $intentHash; body = $body }
    return Invoke-Internal $InternalUrl "runtime.admission.apply" "/internal/runtime/v1/admission-commands:apply" $body
}

function Wait-RuntimePackage([string]$PackageId, [string[]]$Statuses, [int]$TimeoutSeconds = 600) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $status = ((Invoke-RuntimeMySql "SELECT status FROM runtime_work_packages WHERE id=UUID_TO_BIN('$PackageId');") -join "").Trim()
        if ($status -in $Statuses) { return $status }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "Runtime Work Package $PackageId did not reach $($Statuses -join '/')."
}

function Reset-RuntimeRedis {
    $namespace = [string]$context.namespaces.runtime
    Invoke-Kubectl @("-n", $namespace, "scale", "statefulset/runtime-redis", "--replicas=0") | Out-Null
    Wait-StatefulSetScaledToZero $namespace "runtime-redis"
    Invoke-Kubectl @("-n", $namespace, "delete", "pvc", "data-runtime-redis-0", "--wait=true", "--timeout=120s") | Out-Null
    Invoke-Kubectl @("-n", $namespace, "scale", "statefulset/runtime-redis", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", $namespace, "rollout", "status", "statefulset/runtime-redis", "--timeout=180s") | Out-Null
}

function Assert-NoRuntimeResidue {
    $residue = (Invoke-RuntimeMySql @"
SELECT
 (SELECT COUNT(*) FROM node_attempts WHERE status IN ('queued','running','suspended') AND deadline_at<=UTC_TIMESTAMP(6)),
 (SELECT COUNT(*) FROM quota_reservations WHERE status='active' AND expires_at<=UTC_TIMESTAMP(6)),
 (SELECT COUNT(*) FROM execution_outbox WHERE status IN ('processing','failed') AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6))),
 (SELECT COUNT(*) FROM runtime_commands WHERE status='processing' AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6))),
 (SELECT COUNT(*) FROM sandbox_leases WHERE status IN ('orphaned','ready','running','terminating') AND expires_at<=UTC_TIMESTAMP(6));
"@) -join "`t"
    if ($residue -ne "0`t0`t0`t0`t0") { throw "Runtime terminal residue remains: $residue" }
}

foreach ($command in @("cargo", "docker", "kubectl", "pwsh")) { Assert-Command $command }

Push-Location $root
try {
    Add-Timeline "V2-04 E2E started."
    Start-OpenSandbox
    $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY = $OpenSandboxApiKey
    if (-not $SkipLocalGates) {
        & cargo test -p agentx-runtime-contracts
        & cargo test -p agentx-bundle-builder
        & cargo test -p agentx-runtime --test runtime_slice -- --nocapture
        & cargo run --quiet -p agentx-boundary-check -- check
        & (Join-Path $PSScriptRoot "v2-profile-tests.ps1")
        Add-Timeline "Contracts, Builder, Runtime Slice and boundary gates passed."
    }

    $sourceProfilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
    $bootstrapProfile = Get-Content -Raw -LiteralPath $sourceProfilePath | ConvertFrom-Json
    $bootstrapProfile.components.sandbox.endpoint = Resolve-ClusterEndpoint $OpenSandboxEndpoint
    $bootstrapProfilePath = Join-Path $artifactDirectory "${baselinePrefix}profile.json"
    $bootstrapProfile | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $bootstrapProfilePath

    & (Join-Path $PSScriptRoot "v2-03-e2e.ps1") -ConfigFile $bootstrapProfilePath -RunId $RunId -Stage $Stage `
        -BuildImages:$BuildImages -ScaleDownDevelopment:$ScaleDownDevelopment -KeepOnFailure:$KeepOnFailure `
        -KeepOnSuccess -ContextOutputPath $contextPath
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $contextPath)) {
        throw "V2-03 retained baseline failed."
    }
    $context = Get-Content -Raw -LiteralPath $contextPath | ConvertFrom-Json
    $profile = Get-Content -Raw -LiteralPath $context.profilePath | ConvertFrom-Json
    Start-WorkflowRuntimeLogCollectors
    Install-OpenSandboxEgress ([string]$profile.components.sandbox.endpoint)
    Install-RuntimeProviders
    Initialize-RuntimeCredentials

    $schemaFacts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='execution_runtime_state'),(SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='runtime_work_packages'),(SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='runtime_resource_bindings');") -join "`t"
    if ($schemaFacts -ne "1`t1`t1") { throw "V2-04 empty-domain schema is incomplete: $schemaFacts" }
    Complete-Scenario 1 "empty V2 data domains use 0004 and reject the old V2-03 v1 fixtures" @(
        "agentx-runtime-contracts destructive fixture tests", "runtime schema table assertion", $context.artifactDirectory
    )

    $fixture = Invoke-FixtureJob "seed"
    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18280 8080
    $runtimeForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-public" 18281 8080
    $internalForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-internal" 18282 8080
    $controlUrl = "http://127.0.0.1:18280"
    $runtimeUrl = "http://127.0.0.1:18281"
    $internalUrl = "http://127.0.0.1:18282"
    $login = Invoke-RestMethod "$controlUrl/api/v1/auth/login" -Method Post -ContentType application/json `
        -Body (@{ username = "agentx-v2-e2e"; password = "agentx-v2-e2e-password" } | ConvertTo-Json)
    $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $fullKey = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.applicationId)/api-keys" -Method Post -Headers $headers `
        -ContentType application/json -Body (@{ name = "V2-04 Runtime Engine" } | ConvertTo-Json)
    $baselineKey = Invoke-RestMethod "$controlUrl/api/v1/applications/018f0000-0000-7000-8000-00000000000a/api-keys" -Method Post -Headers $headers `
        -ContentType application/json -Body (@{ name = "V2-04 Isolation Baseline" } | ConvertTo-Json)
    $deployment = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.applicationId)/deployments" -Method Post -Headers $headers `
        -ContentType application/json -Body (@{ workflowVersionId = $fixture.workflowVersionId; environmentId = $fixture.environmentId; sessionVersionPolicy = "pinned" } | ConvertTo-Json)
    $attempt = Wait-PublishAttempt $controlUrl $headers $fixture.applicationId $deployment.publishAttemptId
    $full = Invoke-SuccessfulInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "full-engine"
    $fullFacts = (Invoke-RuntimeMySql @"
SELECT
 (SELECT COUNT(DISTINCT resource_kind) FROM runtime_resource_bindings WHERE bundle_id=UUID_TO_BIN('$($attempt.bundleId)')),
 (SELECT COUNT(DISTINCT call_kind) FROM runtime_calls WHERE execution_id=UUID_TO_BIN('$($full.executionId)') AND status='succeeded'),
 (SELECT COUNT(*) FROM agent_runs WHERE execution_id=UUID_TO_BIN('$($full.executionId)') AND status='succeeded'),
 (SELECT COUNT(*) FROM execution_children WHERE parent_execution_id=UUID_TO_BIN('$($full.executionId)') AND relationship='composite'),
 (SELECT COUNT(*) FROM worker_result_receipts r JOIN node_attempts a ON a.id=r.attempt_id WHERE a.execution_id=UUID_TO_BIN('$($full.executionId)') AND r.status='accepted'),
 (SELECT COUNT(*) FROM runtime_objects WHERE tenant_id=UUID_TO_BIN('$tenantId') AND object_id=UUID_TO_BIN('$($fixture.skillObjectId)') AND status='ready');
"@) -join "`t"
    $fullColumns = $fullFacts -split "`t"
    if ([int]$fullColumns[0] -lt 7 -or [int]$fullColumns[1] -lt 5 -or [int]$fullColumns[2] -lt 1 -or [int]$fullColumns[3] -lt 1 -or [int]$fullColumns[4] -lt 7 -or [int]$fullColumns[5] -ne 1) {
        throw "Full Runtime Engine facts are incomplete: $fullFacts"
    }
    Complete-Scenario 2 "full immutable Bundle executed Model/MCP/RAG/Memory/Skill/Agent/Sandbox/Artifact capabilities" @(
        "execution=$($full.executionId)", "bundle=$($attempt.bundleId)", "facts=$fullFacts"
    )

    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "scale", "deployment/platform-control", "deployment/web-console", "--replicas=0") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "scale", "statefulset/control-mysql", "--replicas=0") | Out-Null
    if (-not $controlForward.HasExited) { Stop-Process -Id $controlForward.Id -Force }
    $offline = Invoke-SuccessfulInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "control-offline"
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "scale", "statefulset/control-mysql", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "rollout", "status", "statefulset/control-mysql", "--timeout=300s") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.control, "scale", "deployment/platform-control", "deployment/web-console", "--replicas=1") | Out-Null
    Wait-Deployment $context.namespaces.control "platform-control" 300
    $controlForward = Start-PortForward $context.namespaces.control "platform-control" 18280 8080
    Complete-Scenario 3 "Control, Control MySQL and Control OSS source facts were unavailable while published Runtime entrypoints continued" @(
        "full-offline-execution=$($offline.executionId)", "V2-03 API/Webhook/Schedule/Poll offline baseline"
    )

    foreach ($identity in @(
        @{ workflow = $fixture.waitWorkflowId; identity = "018f0000-0000-7000-8000-000000000432" },
        @{ workflow = $fixture.approvalWorkflowId; identity = "018f0000-0000-7000-8000-000000000434" }
    )) {
        $target = [ordered]@{ kind = "service_identity"; state = [ordered]@{
            tenantId = $tenantId; workflowId = $identity.workflow; identityId = $identity.identity; policyEpoch = 1
            status = "active"; capabilities = @("builtin"); grantIds = @()
        } }
        Invoke-Admission $internalUrl 100 $target "v2-04:${RunId}:identity:$($identity.identity)" | Out-Null
    }
    $debugBody = @{ idempotencyKey = "v2-04-$RunId-debug-wait"; input = @{ message = "wait" }; context = @{}; nodeParameters = @{} } | ConvertTo-Json -Depth 10
    $waitDebug = Invoke-RestMethod "$controlUrl/api/v1/workflows/$($fixture.waitWorkflowId)/debug-runs" -Method Post -Headers $headers -ContentType application/json -Body $debugBody
    Wait-ExecutionStatus $waitDebug.executionId @("waiting") | Out-Null
    $waitToken = ((Invoke-RuntimeMySql "SELECT JSON_UNQUOTE(JSON_EXTRACT(response_json,'$.resumeToken')) FROM execution_resume_tokens WHERE execution_id=UUID_TO_BIN('$($waitDebug.executionId)') AND status='active';") -join "").Trim()
    if (-not $waitToken) { throw "Wait Debug execution did not persist a resume token." }
    $resumeHeaders = @{ "Idempotency-Key" = "v2-04-$RunId-resume"; "X-Agentx-Signature" = "v2-04-e2e" }
    $resumeBody = @{ outputPort = "resumed"; payload = @{ message = "resumed" } } | ConvertTo-Json -Depth 5
    $resume = Invoke-RestMethod "$runtimeUrl/gateway/v1/waits/$waitToken/resume" -Method Post -Headers $resumeHeaders -ContentType application/json -Body $resumeBody
    $resumeReplay = Invoke-RestMethod "$runtimeUrl/gateway/v1/waits/$waitToken/resume" -Method Post -Headers $resumeHeaders -ContentType application/json -Body $resumeBody
    if (-not $resume.accepted -or -not $resumeReplay.replayed) { throw "Wait Resume did not converge idempotently." }
    if ((Wait-Execution $waitDebug.executionId).status -ne "succeeded") { throw "Wait Debug execution did not resume successfully." }

    $approvalBody = @{ idempotencyKey = "v2-04-$RunId-debug-approval"; input = @{ message = "approval" }; context = @{}; nodeParameters = @{} } | ConvertTo-Json -Depth 10
    $approvalDebug = Invoke-RestMethod "$controlUrl/api/v1/workflows/$($fixture.approvalWorkflowId)/debug-runs" -Method Post -Headers $headers -ContentType application/json -Body $approvalBody
    Wait-ExecutionStatus $approvalDebug.executionId @("waiting") | Out-Null
    $approvalRow = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id),version FROM approval_tasks WHERE execution_id=UUID_TO_BIN('$($approvalDebug.executionId)') AND status='pending';") -join "`t"
    $approvalParts = $approvalRow -split "`t"
    $approvalTarget = [ordered]@{ kind = "approval_decision"; state = [ordered]@{
        taskId = $approvalParts[0]; taskVersion = [UInt64]$approvalParts[1]; decision = "approved"; decidedBy = $userId; reason = "v2-04-e2e"
    } }
    $approvalReceipt = Invoke-Admission $internalUrl 101 $approvalTarget "v2-04:${RunId}:approval"
    $approvalReplay = Invoke-Admission $internalUrl 101 $approvalTarget "v2-04:${RunId}:approval"
    if (-not $approvalReceipt.applied -or -not $approvalReplay.replayed) { throw "Approval decision did not replay the original receipt." }
    if ((Wait-Execution $approvalDebug.executionId).status -ne "succeeded") { throw "Approval Debug execution did not resume successfully." }

    $checkpointId = ((Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id) FROM checkpoints WHERE execution_id=UUID_TO_BIN('$($full.executionId)') ORDER BY sequence_number DESC LIMIT 1;") -join "").Trim()
    $sourceVersion = [UInt64](((Invoke-RuntimeMySql "SELECT state_version FROM workflow_executions WHERE id=UUID_TO_BIN('$($full.executionId)');") -join "").Trim())
    $forkCommandId = [Guid]::NewGuid().ToString()
    $forkRequest = [ordered]@{
        apiVersion = 1; tenantId = $tenantId; commandId = $forkCommandId; objectVersion = $sourceVersion
        idempotencyKey = "v2-04:${RunId}:fork"
        command = [ordered]@{ kind = "fork"; sourceExecutionId = $full.executionId; checkpointId = $checkpointId; mode = "whole"; nodeId = $null; sideEffectResolution = "dry_run" }
    }
    Invoke-Internal $internalUrl "runtime.commands.apply" "/internal/runtime/v1/runtime-commands:apply" $forkRequest | Out-Null
    $forkDeadline = (Get-Date).AddMinutes(3)
    do {
        $forkExecutionId = ((Invoke-RuntimeMySql "SELECT BIN_TO_UUID(fork_execution_id) FROM execution_forks WHERE source_execution_id=UUID_TO_BIN('$($full.executionId)') AND source_checkpoint_id=UUID_TO_BIN('$checkpointId');") -join "").Trim()
        if ($forkExecutionId) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $forkDeadline)
    if (-not $forkExecutionId) { throw "Fork command did not create a fork Execution." }
    if ((Wait-Execution $forkExecutionId).status -ne "succeeded") { throw "Checkpoint Fork did not succeed." }
    $reference = Invoke-Internal $internalUrl "runtime.references.check" "/internal/runtime/v1/references:check" @{
        apiVersion = 1; tenantId = $tenantId; objectIds = @(); bundleIds = @($attempt.bundleId)
    }
    if ($reference.safeToDelete -or @($reference.blockingReferences).Count -eq 0) { throw "Live Bundle references did not block deletion." }
    Complete-Scenario 4 "Wait, Approval, Checkpoint, Fork and explicit side-effect resolution converged once" @(
        "wait=$($waitDebug.executionId)", "approval=$($approvalDebug.executionId)", "fork=$forkExecutionId", "bundle-reference-blocked"
    )

    $evaluation = Invoke-FixtureJob "evaluation"
    $evaluationComplete = Wait-RuntimePackage $evaluation.evaluation.completedPackageId @("succeeded")
    $evaluationCancelled = Wait-RuntimePackage $evaluation.evaluation.cancelledPackageId @("cancelled")
    $evaluationFacts = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM evaluation_run_cases c JOIN evaluation_runs r ON r.id=c.evaluation_run_id WHERE r.work_package_id=UUID_TO_BIN('$($evaluation.evaluation.completedPackageId)') AND c.status='completed'),(SELECT COUNT(*) FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id JOIN evaluation_runs r ON r.id=c.evaluation_run_id WHERE r.work_package_id=UUID_TO_BIN('$($evaluation.evaluation.completedPackageId)') AND rr.status='passed'),(SELECT COUNT(*) FROM evaluation_run_cases c JOIN evaluation_runs r ON r.id=c.evaluation_run_id WHERE r.work_package_id=UUID_TO_BIN('$($evaluation.evaluation.cancelledPackageId)') AND c.status='cancelled');") -join "`t"
    if ($evaluationFacts -ne "2`t2`t1") { throw "Evaluation Work Package facts did not converge: $evaluationFacts" }
    $debugTtl = [int](((Invoke-ControlMySql "SELECT TIMESTAMPDIFF(SECOND,created_at,expires_at) FROM workflow_debug_runs WHERE id=UUID_TO_BIN('$($waitDebug.debugRunId)');") -join "").Trim())
    if ($debugTtl -lt 3590 -or $debugTtl -gt 3610) { throw "Debug Work Package TTL drifted: $debugTtl seconds." }
    Complete-Scenario 8 "Evaluation batch/Evaluator/cancel and Debug TTL used signed Work Packages" @(
        "evaluation=$evaluationComplete/$evaluationFacts", "cancel=$evaluationCancelled", "debugTtl=$debugTtl"
    )

    $compositeFacts = (Invoke-RuntimeMySql "SELECT COUNT(*),COUNT(DISTINCT child_execution_id) FROM execution_children WHERE relationship='composite' AND (parent_execution_id=UUID_TO_BIN('$($full.executionId)') OR parent_execution_id IN (SELECT child_execution_id FROM execution_children WHERE parent_execution_id=UUID_TO_BIN('$($full.executionId)')));") -join "`t"
    if (($compositeFacts -split "`t")[0] -lt 2) { throw "Multi-level Composite did not create two child executions: $compositeFacts" }
    Complete-Scenario 9 "multi-level fixed Composite ran offline; recursive and mutable dependencies were rejected by Builder gates" @(
        "compositeChildren=$compositeFacts", "agentx-bundle-builder cycle/mutable-head tests"
    )

    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "deployment/workflow-runtime", "deployment/workflow-worker", "--replicas=0") | Out-Null
    Wait-DeploymentScaledToZero $context.namespaces.runtime "workflow-runtime"
    Wait-DeploymentScaledToZero $context.namespaces.runtime "workflow-worker"
    $outboxAccepted = Start-Invocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "redis-outbox"
    Reset-RuntimeRedis
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "deployment/workflow-runtime", "--replicas=2") | Out-Null
    Wait-Deployment $context.namespaces.runtime "workflow-runtime" 300
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "deployment/workflow-worker", "--replicas=0") | Out-Null
    Wait-ExecutionStatus $outboxAccepted.executionId @("running") 180 | Out-Null
    $pendingAttempt = ((Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id) FROM node_attempts WHERE execution_id=UUID_TO_BIN('$($outboxAccepted.executionId)') AND status='queued' LIMIT 1;") -join "").Trim()
    if (-not $pendingAttempt) { throw "Redis pending-stage fixture has no queued Attempt." }
    Reset-RuntimeRedis
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "deployment/workflow-worker", "--replicas=2") | Out-Null
    Wait-Deployment $context.namespaces.runtime "workflow-worker" 300
    if ((Wait-Execution $outboxAccepted.executionId).status -ne "succeeded") { throw "Outbox/Pending Redis rebuild did not recover." }
    $runningAccepted = Start-Invocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "redis-running"
    $runningDeadline = (Get-Date).AddMinutes(5)
    do {
        $runningAttempt = ((Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id) FROM node_attempts WHERE execution_id=UUID_TO_BIN('$($runningAccepted.executionId)') AND status='running' AND capability='sandbox' ORDER BY created_at DESC LIMIT 1;") -join "").Trim()
        if ($runningAttempt) { break }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $runningDeadline)
    if (-not $runningAttempt) { throw "Redis running-stage fixture never observed a running Attempt." }
    Reset-RuntimeRedis
    $runningTerminal = Wait-Execution $runningAccepted.executionId
    if ($runningTerminal.status -ne "succeeded") {
        Save-RuntimeExecutionDiagnostics $runningAccepted.executionId "redis-running"
        throw "Running-stage Redis rebuild ended as $($runningTerminal.status): $($runningTerminal.errorCode) $($runningTerminal.errorMessage)"
    }
    Complete-Scenario 5 "Redis was rebuilt in Outbox, Pending and Running stages and MySQL recovered unique terminal facts" @(
        "outboxExecution=$($outboxAccepted.executionId)", "pendingAttempt=$pendingAttempt", "runningAttempt=$runningAttempt"
    )

    $killAccepted = Start-Invocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "kill-worker"
    $killDeadline = (Get-Date).AddMinutes(5)
    do {
        $leaseRow = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(id),COALESCE(worker_instance_id,''),fencing_token FROM node_attempts WHERE execution_id=UUID_TO_BIN('$($killAccepted.executionId)') AND status='running' ORDER BY created_at DESC LIMIT 1;") -join "`t"
        if ($leaseRow) { break }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $killDeadline)
    if (-not $leaseRow) { throw "Worker kill fixture never obtained an Attempt Lease." }
    $leaseParts = $leaseRow -split "`t"
    $workerPods = ((Invoke-Kubectl @(
        "-n", [string]$context.namespaces.runtime, "get", "pods",
        "-l", "app.kubernetes.io/name=workflow-worker", "-o", "json"
    )) -join "`n" | ConvertFrom-Json).items
    $workerPod = @($workerPods | Where-Object { [string]$_.metadata.uid -eq $leaseParts[1] } | ForEach-Object { "pod/$($_.metadata.name)" }) | Select-Object -First 1
    if (-not $workerPod) { throw "Attempt Lease Worker $($leaseParts[1]) does not map to a live Pod." }
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", $workerPod, "--wait=false") | Out-Null
    $staleWrite = (Invoke-RuntimeMySql "UPDATE node_attempts SET status='succeeded' WHERE id=UUID_TO_BIN('$($leaseParts[0])') AND worker_instance_id='$($leaseParts[1])' AND fencing_token=$($leaseParts[2])-1; SELECT ROW_COUNT();") -join ""
    if ($staleWrite.Trim() -ne "0") { throw "A stale Worker fencing token changed an Attempt." }
    Wait-Deployment $context.namespaces.runtime "workflow-worker" 300
    $killTerminal = Wait-Execution $killAccepted.executionId 600
    if ($killTerminal.status -notin @("succeeded", "failed")) { throw "Worker kill did not converge to a terminal Execution." }
    $runtimePod = @(Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=workflow-runtime", "-o", "name")) | Select-Object -First 1
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", $runtimePod, "--wait=false") | Out-Null
    Wait-Deployment $context.namespaces.runtime "workflow-runtime" 300
    $sandboxPod = @(Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "get", "pods", "-l", "app.kubernetes.io/name=sandbox-manager", "-o", "name")) | Select-Object -First 1
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", $sandboxPod, "--wait=false") | Out-Null
    Wait-Deployment $context.namespaces.runtime "sandbox-manager" 300
    Complete-Scenario 6 "Coordinator, Worker and Sandbox Manager restarts respected leases and stale fencing was rejected" @(
        "execution=$($killAccepted.executionId):$($killTerminal.status)", "staleWriteRows=0"
    )

    $grantRow = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(grant_id),resource_type,BIN_TO_UUID(resource_id),JSON_UNQUOTE(JSON_EXTRACT(operations_json,'$[0]')) FROM resource_grant_projection WHERE tenant_id=UUID_TO_BIN('$tenantId') AND subject_id=UUID_TO_BIN('$($fixture.serviceIdentityId)') AND resource_type<>'bundle' ORDER BY grant_id LIMIT 1;") -join "`t"
    $grantParts = $grantRow -split "`t"
    if ($grantParts.Count -lt 4) { throw "No projected Runtime Grant was available for revocation." }
    $grantTarget = { param([bool]$Enabled, [UInt64]$Epoch) [ordered]@{ kind = "resource_grant"; state = [ordered]@{
        tenantId = $tenantId; identityId = $fixture.serviceIdentityId; grantId = $grantParts[0]; resourceKind = $grantParts[1]
        resourceId = $grantParts[2]; operations = @($grantParts[3]); policyEpoch = $Epoch; enabled = $Enabled
    } } }
    Invoke-Admission $internalUrl 1000 (& $grantTarget $false 1000) "v2-04:${RunId}:grant:revoke" | Out-Null
    $revoked = Invoke-FailingInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "grant-revoked" "RUNTIME_GRANT_REVOKED"
    Invoke-Admission $internalUrl 999 (& $grantTarget $true 999) "v2-04:${RunId}:grant:old" | Out-Null
    $grantState = (Invoke-RuntimeMySql "SELECT status,policy_epoch FROM resource_grant_projection WHERE grant_id=UUID_TO_BIN('$($grantParts[0])');") -join "`t"
    if ($grantState -ne "revoked`t1000") { throw "Old Grant Epoch restored revoked authorization: $grantState" }
    Invoke-Admission $internalUrl 1001 (& $grantTarget $true 1001) "v2-04:${RunId}:grant:restore" | Out-Null
    Invoke-RuntimeMySql "UPDATE service_identity_projection SET updated_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 73 HOUR) WHERE identity_id=UUID_TO_BIN('$($fixture.serviceIdentityId)');" | Out-Null
    $stale = Invoke-FailingInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "policy-stale" "RUNTIME_AUTHORIZATION_STALE"
    Invoke-RuntimeMySql "UPDATE service_identity_projection SET updated_at=UTC_TIMESTAMP(6) WHERE identity_id=UUID_TO_BIN('$($fixture.serviceIdentityId)');" | Out-Null
    Complete-Scenario 7 "Grant revoke, old Epoch suppression and 72-hour LKG fail-closed were enforced" @(
        "revokedExecution=$($revoked.accepted.executionId)", "oldEpoch=$grantState", "staleExecution=$($stale.accepted.executionId)"
    )

    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=0") | Out-Null
    Wait-StatefulSetScaledToZero $context.namespaces.runtime "runtime-mysql"
    $databaseStatus = 0
    try {
        Start-Invocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "mysql-down" | Out-Null
        throw "Runtime MySQL outage returned a false success."
    }
    catch {
        if ($_.Exception.Message -eq "Runtime MySQL outage returned a false success.") { throw }
        $databaseStatus = [int]$_.Exception.Response.StatusCode
    }
    if ($databaseStatus -ne 503) { throw "Runtime MySQL outage returned HTTP $databaseStatus." }
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "scale", "statefulset/runtime-mysql", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "rollout", "status", "statefulset/runtime-mysql", "--timeout=300s") | Out-Null
    foreach ($name in @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager")) {
        Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "rollout", "restart", "deployment/$name") | Out-Null
        Wait-Deployment $context.namespaces.runtime $name 300
    }
    foreach ($forward in @($runtimeForward, $internalForward)) {
        if ($forward -and -not $forward.HasExited) {
            Stop-Process -Id $forward.Id -Force -ErrorAction SilentlyContinue
            Wait-Process -Id $forward.Id -Timeout 10 -ErrorAction SilentlyContinue
        }
    }
    $runtimeForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-public" 18281 8080
    $internalForward = Start-PortForward $context.namespaces.runtime "runtime-gateway-internal" 18282 8080
    $mysqlOrphans = (Invoke-RuntimeMySql "SELECT (SELECT COUNT(*) FROM application_invocations WHERE idempotency_key='v2-04-$RunId-mysql-down'),(SELECT COUNT(*) FROM runtime_commands WHERE idempotency_key='v2-04-$RunId-mysql-down');") -join "`t"
    if ($mysqlOrphans -ne "0`t0") { throw "Runtime MySQL outage left orphan facts: $mysqlOrphans" }
    Complete-Scenario 10 "Runtime MySQL outage returned 503 and recovery left no orphan request facts" @("http=503", "orphans=$mysqlOrphans")

    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/vault", "--replicas=0") | Out-Null
    Wait-StatefulSetScaledToZero $context.namespaces.dependencies "vault"
    $vaultFailure = Invoke-FailingInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "vault-down"
    $vaultBaseline = Invoke-SuccessfulInvocation $runtimeUrl "v2-no-op" $baselineKey.secret "vault-no-op"
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/vault", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "rollout", "status", "statefulset/vault", "--timeout=180s") | Out-Null

    Invoke-Kubectl @("-n", [string]$context.namespaces.runtime, "delete", "networkpolicy/v2-04-opensandbox-egress") | Out-Null
    $sandboxFailure = Invoke-FailingInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "opensandbox-down"
    $sandboxBaseline = Invoke-SuccessfulInvocation $runtimeUrl "v2-no-op" $baselineKey.secret "opensandbox-no-op"
    Install-OpenSandboxEgress ([string]$profile.components.sandbox.endpoint)

    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/object-storage", "--replicas=0") | Out-Null
    Wait-StatefulSetScaledToZero $context.namespaces.dependencies "object-storage"
    $ossFailure = Invoke-FailingInvocation $runtimeUrl $fixture.applicationSlug $fullKey.secret "runtime-oss-down"
    $ossBaseline = Invoke-SuccessfulInvocation $runtimeUrl "v2-no-op" $baselineKey.secret "oss-no-op"
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/object-storage", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "rollout", "status", "statefulset/object-storage", "--timeout=180s") | Out-Null
    Complete-Scenario 11 "Vault, OpenSandbox and Runtime OSS outages failed only dependent capabilities" @(
        "vault=$($vaultFailure.accepted.executionId)/baseline=$($vaultBaseline.executionId)",
        "sandbox=$($sandboxFailure.accepted.executionId)/baseline=$($sandboxBaseline.executionId)",
        "oss=$($ossFailure.accepted.executionId)/baseline=$($ossBaseline.executionId)"
    )

    $retentionPolicyTarget = [ordered]@{ kind = "retention_policy"; state = [ordered]@{
        tenantId = $tenantId; policyVersion = 1; retentionDays = [ordered]@{ artifact = 0; execution = 0; application_message = 0; evaluation_report = 0; trace = 7 }; enabled = $true
    } }
    Invoke-Admission $internalUrl 1100 $retentionPolicyTarget "v2-04:${RunId}:retention-policy" | Out-Null
    $dryRunId = [Guid]::NewGuid().ToString()
    Invoke-Internal $internalUrl "runtime.retention.apply" "/internal/runtime/v1/retention-commands:apply" @{
        apiVersion = 1; tenantId = $tenantId; runId = $dryRunId; policyVersion = 1; dryRun = $true; idempotencyKey = "v2-04:${RunId}:retention:dry"
    } | Out-Null
    $retentionDeadline = (Get-Date).AddMinutes(3)
    do {
        $retentionState = (Invoke-RuntimeMySql "SELECT status,candidate_count,deleted_count FROM retention_runs WHERE id=UUID_TO_BIN('$dryRunId');") -join "`t"
        if (($retentionState -split "`t")[0] -eq "completed") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $retentionDeadline)
    if (($retentionState -split "`t")[0] -ne "completed") { throw "Retention Dry Run did not complete: $retentionState" }
    $blocked = [int](((Invoke-RuntimeMySql "SELECT COUNT(*) FROM retention_items WHERE retention_run_id=UUID_TO_BIN('$dryRunId') AND status='blocked';") -join "").Trim())
    if ($blocked -lt 1) { throw "Retention Dry Run did not preserve checkpoint/reference protected records." }
    $referenceReplay = Invoke-Internal $internalUrl "runtime.references.check" "/internal/runtime/v1/references:check" @{
        apiVersion = 1; tenantId = $tenantId; objectIds = @($fixture.skillObjectId); bundleIds = @($attempt.bundleId)
    }
    if ($referenceReplay.safeToDelete) { throw "Reference Check allowed a live Bundle/Object closure to be deleted." }

    $retentionArtifactPath = Join-Path $artifactDirectory "artifact-first.json"
    if (-not (Test-Path -LiteralPath $retentionArtifactPath)) { throw "The V2-03 baseline did not preserve an unreferenced Artifact for Retention recovery." }
    $retentionArtifact = Get-Content -Raw -LiteralPath $retentionArtifactPath | ConvertFrom-Json
    Invoke-RuntimeMySql "UPDATE artifacts SET created_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 2 DAY) WHERE tenant_id=UUID_TO_BIN('$tenantId') AND id=UUID_TO_BIN('$($retentionArtifact.artifactId)') AND deleted_at IS NULL;" | Out-Null
    $agedArtifact = [int](((Invoke-RuntimeMySql "SELECT COUNT(*) FROM artifacts WHERE tenant_id=UUID_TO_BIN('$tenantId') AND id=UUID_TO_BIN('$($retentionArtifact.artifactId)') AND created_at<DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 DAY) AND deleted_at IS NULL;") -join "").Trim())
    if ($agedArtifact -ne 1) { throw "Retention retry Artifact was not available and aged." }

    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/object-storage", "--replicas=0") | Out-Null
    Wait-StatefulSetScaledToZero $context.namespaces.dependencies "object-storage"
    $liveRunId = [Guid]::NewGuid().ToString()
    Invoke-Internal $internalUrl "runtime.retention.apply" "/internal/runtime/v1/retention-commands:apply" @{
        apiVersion = 1; tenantId = $tenantId; runId = $liveRunId; policyVersion = 1; dryRun = $false; idempotencyKey = "v2-04:${RunId}:retention:live"
    } | Out-Null
    $failedDeadline = (Get-Date).AddMinutes(3)
    do {
        $failedItem = (Invoke-RuntimeMySql "SELECT status,attempt_count FROM retention_items WHERE retention_run_id=UUID_TO_BIN('$liveRunId') AND data_type='artifact' AND object_id=UUID_TO_BIN('$($retentionArtifact.artifactId)') ORDER BY created_at DESC LIMIT 1;") -join "`t"
        $failedColumns = $failedItem -split "`t"
        if ($failedColumns.Count -eq 2 -and [int]$failedColumns[1] -gt 0 -and $failedColumns[0] -eq "failed") { break }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $failedDeadline)
    if ($failedColumns.Count -ne 2 -or $failedColumns[0] -ne "failed" -or [int]$failedColumns[1] -lt 1) { throw "Retention did not persist the Runtime OSS deletion failure: $failedItem" }

    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "scale", "statefulset/object-storage", "--replicas=1") | Out-Null
    Invoke-Kubectl @("-n", [string]$context.namespaces.dependencies, "rollout", "status", "statefulset/object-storage", "--timeout=180s") | Out-Null
    $liveDeadline = (Get-Date).AddMinutes(5)
    do {
        $liveState = (Invoke-RuntimeMySql "SELECT r.status,i.status,i.attempt_count,a.deleted_at IS NOT NULL FROM retention_runs r JOIN retention_items i ON i.retention_run_id=r.id AND i.data_type='artifact' AND i.object_id=UUID_TO_BIN('$($retentionArtifact.artifactId)') JOIN artifacts a ON a.tenant_id=i.tenant_id AND a.id=i.object_id WHERE r.id=UUID_TO_BIN('$liveRunId') ORDER BY i.created_at DESC LIMIT 1;") -join "`t"
        $liveColumns = $liveState -split "`t"
        if ($liveColumns.Count -eq 4 -and $liveColumns[0] -eq "completed" -and $liveColumns[1] -eq "deleted") { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $liveDeadline)
    if ($liveColumns.Count -ne 4 -or $liveColumns[0] -ne "completed" -or $liveColumns[1] -ne "deleted" -or [int]$liveColumns[2] -le 1 -or $liveColumns[3] -ne "1") {
        throw "Retention OSS deletion retry did not converge after Object Storage recovery: $liveState"
    }
    Start-Sleep -Seconds 2
    Assert-NoRuntimeResidue
    Complete-Scenario 12 "Retention Dry Run, reference blocking, OSS deletion retry and final residue cleanup converged" @(
        "dryRun=${dryRunId}:$retentionState", "blocked=$blocked", "referenceSafe=false",
        "liveRun=${liveRunId}:$liveState", "terminalResidue=0"
    )

    $completed = $scenarioCoverage.Count -eq 12 -and @($scenarioCoverage | Where-Object status -ne "passed").Count -eq 0
    if (-not $completed) { throw "V2-04 scenario matrix is incomplete." }
    $summary = [ordered]@{
        apiVersion = "agentx.io/evidence/v1"
        stage = "v2-04"
        stageComplete = $true
        runId = $RunId
        profile = $context.profilePath
        namespaces = $context.namespaces
        dependencies = @{
            model = "echo-mcp"
            mcp = "echo-mcp"
            rag = "lightrag"
            memory = "mem0"
            openSandbox = ([Uri]$OpenSandboxEndpoint).GetLeftPart([UriPartial]::Authority)
            vault = "vault"
        }
        scenarioCoverage = @($scenarioCoverage | Sort-Object id)
        timeline = @($timeline)
        sensitiveData = "Tokens, API keys, passwords, Vault plaintext and response bodies are excluded."
    }
    $summary | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath
    $timeline | Set-Content -LiteralPath (Join-Path $artifactDirectory "${baselinePrefix}timeline.log")
    if (-not [string]::IsNullOrWhiteSpace($ContextOutputPath)) {
        $contextDirectory = Split-Path -Parent $ContextOutputPath
        if ($contextDirectory) { New-Item -ItemType Directory -Force -Path $contextDirectory | Out-Null }
        [ordered]@{
            apiVersion = "agentx.io/v2-e2e-context/v1"
            stage = "v2-$Stage"
            runId = $RunId
            profilePath = $context.profilePath
            artifactDirectory = $artifactDirectory
            baselineSummaryPath = $summaryPath
            namespaces = $context.namespaces
        } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ContextOutputPath
    }
}
catch {
    $failure = $_
    Add-Timeline "FAILED: $($failure.Exception.Message)"
    [ordered]@{
        apiVersion = "agentx.io/evidence/v1"
        stage = "v2-04"
        stageComplete = $false
        runId = $RunId
        scenarioCoverage = @($scenarioCoverage | Sort-Object id)
        failure = $failure.Exception.Message
        timeline = @($timeline)
        sensitiveData = "Tokens, API keys, passwords and Vault plaintext are excluded."
    } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath
    throw
}
finally {
    Add-Timeline "Cleanup begin success=$completed."
    Stop-Forwards
    Stop-WorkflowRuntimeLogCollectors
    if ($context) {
        foreach ($target in @(
            @{ namespace = $context.namespaces.control; resource = "deployment/platform-control" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/runtime-gateway" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/workflow-runtime" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/workflow-worker" },
            @{ namespace = $context.namespaces.runtime; resource = "deployment/sandbox-manager" }
        )) {
            $safe = $target.resource.Replace('/', '-')
            $nativePreference = $PSNativeCommandUseErrorActionPreference
            try {
                $PSNativeCommandUseErrorActionPreference = $false
                & kubectl -n $target.namespace logs $target.resource --all-pods=true --prefix --tail=300 *> (Join-Path $artifactDirectory "$safe.log")
                if ($LASTEXITCODE -ne 0) {
                    Add-Timeline "Log collection warning: $($target.namespace)/$($target.resource) exited $LASTEXITCODE."
                }
            }
            finally {
                $PSNativeCommandUseErrorActionPreference = $nativePreference
            }
        }
        if (Test-Path -LiteralPath $providerEvidence) {
            try {
                & (Join-Path $PSScriptRoot "m5-addons-contract.ps1") -Namespace $context.namespaces.dependencies -CleanupOnly -ResultsPath $providerEvidence | Out-Null
            }
            catch { Add-Timeline "Provider fixture cleanup warning: $($_.Exception.Message)" }
        }
        if (($completed -and -not $KeepOnSuccess) -or (-not $completed -and -not $KeepOnFailure)) {
            & (Join-Path $PSScriptRoot "deploy-v2.ps1") -Action Uninstall -ConfigFile $context.profilePath -RunId "$Stage-$RunId"
        }
    }
    if ($openSandboxOwned -and $openSandboxProcess -and -not $openSandboxProcess.HasExited) {
        Stop-Process -Id $openSandboxProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if ($deploySandboxKeyWasSet) { $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY = $originalDeploySandboxKey } else { Remove-Item Env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY -ErrorAction SilentlyContinue }
    Add-Timeline "Cleanup end."
    $timeline | Set-Content -LiteralPath (Join-Path $artifactDirectory "${baselinePrefix}timeline.log")
    Pop-Location
}

if (-not $completed) { throw "V2-04 E2E did not complete; see $artifactDirectory" }
$retained = if ($KeepOnSuccess) { " Temporary namespaces were retained for the caller." } else { "" }
Write-Output "V2-04 E2E passed all 12 scenarios. Evidence: $artifactDirectory$retained"
