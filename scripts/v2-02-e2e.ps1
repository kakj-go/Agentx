param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$KeepOnFailure
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$deploy = Join-Path $PSScriptRoot "deploy-v2.ps1"
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-02"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$profilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json
$namespaces = @{
    control = "agentx-v2-01-control-$RunId"
    runtime = "agentx-v2-01-runtime-$RunId"
    observability = "agentx-v2-01-runtime-$RunId"
    dependencies = "agentx-v2-01-deps-$RunId"
}
$timeline = [Collections.Generic.List[string]]::new()
$developmentReplicas = @()
$objectStorageAdminReady = $false
$succeeded = $false
$faultProxyForward = $null

function Invoke-Kubectl([string[]]$Arguments) {
    $PSNativeCommandUseErrorActionPreference = $false
    $output = & kubectl @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed: $($output -join [Environment]::NewLine)" }
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
    if ($LASTEXITCODE -ne 0) { throw "Failed to install the V2-02 fault proxy." }
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
    return @{ application = $application; version = $version; versionTwo = $versionTwo; workflowDeploymentTwo = $workflowDeploymentTwo; environment = $environment }
}

try {
    $timeline.Add("$(Get-Date -Format o) V2-02 E2E start")
    $developmentReplicas = @(Get-DevelopmentReplicas)
    $developmentReplicas | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $artifactDirectory "development-replicas.json")
    if ($ScaleDownDevelopment) { Set-DevelopmentReplicas $developmentReplicas $true }

    & $deploy -Action Install -ConfigFile $profilePath -RunId $RunId -BuildImages:$BuildImages -CleanupOnFailure:(!$KeepOnFailure)
    if ($LASTEXITCODE -ne 0) { throw "V2 deployment failed." }
    if ($BuildImages) {
        & (Join-Path $PSScriptRoot "build-images.ps1") -Tag ([string]$profile.images.tag) -Namespace $namespaces.dependencies -Services @("runtime-fault-proxy")
        if ($LASTEXITCODE -ne 0) { throw "V2-02 fault proxy image build failed." }
    }
    $controlForward = Start-PortForward $namespaces.control "platform-control" 18080 8080
    $runtimeForward = Start-PortForward $namespaces.runtime "runtime-gateway-public" 18081 8080
    $controlUrl = "http://127.0.0.1:18080"
    $runtimeUrl = "http://127.0.0.1:18081"

    $login = Invoke-RestMethod "$controlUrl/api/v1/auth/login" -Method Post -ContentType application/json -Body (@{ username = "agentx-v2-e2e"; password = "agentx-v2-e2e-password" } | ConvertTo-Json)
    $headers = @{ Authorization = "Bearer $($login.accessToken)" }
    $fixture = Seed-NoOpWorkflow
    $key = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/api-keys" -Method Post -Headers $headers -ContentType application/json -Body (@{ name = "V2 E2E" } | ConvertTo-Json)
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
    $acceptedBeforeRollback = Invoke-RestMethod "$runtimeUrl/gateway/v1/applications/v2-no-op/invocations" -Method Post -Headers @{ Authorization = "Bearer $($key.secret)"; "Idempotency-Key" = "v2-02-$RunId-in-flight" } -ContentType application/json -Body (@{ input = @{ message = "in-flight-v2" }; responseMode = "async" } | ConvertTo-Json -Depth 5)
    $rollback = Invoke-RestMethod "$controlUrl/api/v1/applications/$($fixture.application)/deployments/$($deployment.id):rollback" -Method Post -Headers $headers
    $rollbackAttempt = Wait-PublishAttempt $controlUrl $headers $fixture.application $rollback.id
    Wait-ApplicationHead $attempt.bundleId
    $inFlightBundle = (Invoke-RuntimeMySql "SELECT BIN_TO_UUID(bundle_id) FROM workflow_executions WHERE id=UUID_TO_BIN('$($acceptedBeforeRollback.executionId)');" -join "")
    if ($inFlightBundle.ToLowerInvariant() -ne $secondAttempt.bundleId.ToLowerInvariant()) { throw "In-flight Execution changed its pinned Bundle during rollback." }
    $rollbackEpoch = [int](Invoke-RuntimeMySql "SELECT admission_epoch FROM deployment_heads WHERE application_id=UUID_TO_BIN('$($fixture.application)');" -join "")
    $timeline.Add("$(Get-Date -Format o) second Bundle activated and first Bundle rolled back")

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
        if ($columns[0] -ne "succeeded" -or $columns[1] -ne "agentx-v2" -or $columns[2].ToLowerInvariant() -ne $attempt.bundleId.ToLowerInvariant()) {
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
        $query = Invoke-RestMethod "$controlUrl/api/v1/runtime-query/executions/$($result.executionId)" -Headers $headers
        if ($query.summary.executionId -ne $result.executionId -or $query.summary.bundleId -ne $result.bundleId -or $query.output.message -ne "agentx-v2") {
            throw "Runtime Query returned a non-authoritative result for $($result.executionId)."
        }
    }
    $duplicates = Invoke-MySql "SELECT (SELECT COUNT(*) FROM execution_spec_bundles WHERE id=UUID_TO_BIN('$($attempt.bundleId)')),(SELECT COUNT(*) FROM execution_spec_bundles WHERE id=UUID_TO_BIN('$($secondAttempt.bundleId)')),(SELECT COUNT(*) FROM publish_attempts WHERE id IN (UUID_TO_BIN('$($attempt.id)'),UUID_TO_BIN('$($secondAttempt.id)'),UUID_TO_BIN('$($rollbackAttempt.id)'),UUID_TO_BIN('$($postRevokeAttempt.id)'))),(SELECT COUNT(*) FROM outbox WHERE status IN ('pending','processing','failed'));" -join "`t"
    if ($duplicates -ne "1`t1`t4`t0") { throw "Control recovery did not converge: $duplicates" }
    $timeline.Add("$(Get-Date -Format o) Control recovery converged")
    $succeeded = $true
}
finally {
    $timeline.Add("$(Get-Date -Format o) cleanup begin success=$succeeded")
    foreach ($process in @($controlForward, $runtimeForward, $faultProxyForward)) { if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force } }
    if ($succeeded -or !$KeepOnFailure) {
        & $deploy -Action Uninstall -ConfigFile $profilePath -RunId $RunId
    }
    if ($ScaleDownDevelopment -and $developmentReplicas.Count -gt 0) { Set-DevelopmentReplicas $developmentReplicas $false }
    $timeline.Add("$(Get-Date -Format o) cleanup end")
    $timeline | Set-Content (Join-Path $artifactDirectory "timeline.txt")
}

if (!$succeeded) { throw "V2-02 E2E failed; see $artifactDirectory" }
Write-Output "V2-02 E2E passed. Evidence: $artifactDirectory"
