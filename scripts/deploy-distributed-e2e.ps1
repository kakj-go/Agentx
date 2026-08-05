param(
    [string]$DependencyNamespace = "agentx-e2e-deps",
    [string]$AgentxNamespace = "agentx-e2e",
    [switch]$SkipBuild,
    [switch]$KeepNamespaces
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$profilePath = Join-Path ([IO.Path]::GetTempPath()) "agentx-distributed-$PID.json"
function New-RandomSecret([int]$Length) {
    $bytes = [byte[]]::new($Length)
    [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    return [Convert]::ToBase64String($bytes)
}

$mysqlPassword = New-RandomSecret 24
$mysqlRootPassword = New-RandomSecret 24
$redisPassword = New-RandomSecret 24
$clickHousePassword = New-RandomSecret 24
$s3AccessKey = "agentx-e2e"
$s3SecretKey = New-RandomSecret 24

function Apply-DependencySecret {
    $values = @{
        AGENTX_MYSQL_PASSWORD = $mysqlPassword
        AGENTX_MYSQL_ROOT_PASSWORD = $mysqlRootPassword
        AGENTX_REDIS_PASSWORD = $redisPassword
        AGENTX_CLICKHOUSE_PASSWORD = $clickHousePassword
        AGENTX_S3_ACCESS_KEY = $s3AccessKey
        AGENTX_S3_SECRET_KEY = $s3SecretKey
    }
    $data = @{}
    foreach ($entry in $values.GetEnumerator()) { $data[$entry.Key] = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($entry.Value)) }
    @{ apiVersion = "v1"; kind = "Secret"; metadata = @{ name = "agentx-secrets"; namespace = $DependencyNamespace; labels = @{ "app.kubernetes.io/managed-by" = "agentx-deploy-e2e" } }; type = "Opaque"; data = $data } | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}

function Invoke-DependencyMySql([string]$Query) {
    $Query | kubectl -n $DependencyNamespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"'
}

$script:disabledPort = 18082
$script:disabledForward = $null
$script:disabledForwardOut = Join-Path ([IO.Path]::GetTempPath()) "agentx-disabled-forward-$PID.out"
$script:disabledForwardError = Join-Path ([IO.Path]::GetTempPath()) "agentx-disabled-forward-$PID.err"

function Wait-TcpPort([int]$Port) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if (Test-NetConnection -ComputerName 127.0.0.1 -Port $Port -InformationLevel Quiet -WarningAction SilentlyContinue) { return }
        Start-Sleep -Seconds 1
    }
    throw "Timed out waiting for local port $Port."
}

function Write-KubernetesJobDiagnostics([string]$Namespace, [string]$Name) {
    Write-Warning "Kubernetes Job $Namespace/$Name did not complete successfully."
    foreach ($arguments in @(
        @("-n", $Namespace, "get", "job", $Name, "-o", "wide"),
        @("-n", $Namespace, "get", "pods", "-l", "job-name=$Name", "-o", "wide"),
        @("-n", $Namespace, "logs", "job/$Name", "--all-containers=true", "--prefix=true"),
        @("-n", $Namespace, "describe", "job", $Name)
    )) {
        try {
            $output = kubectl @arguments 2>&1
            Write-Host ($output -join [Environment]::NewLine)
        }
        catch {
            Write-Warning $_.Exception.Message
        }
    }
}

function Wait-KubernetesJob([string]$Namespace, [string]$Name, [int]$TimeoutSeconds) {
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTimeOffset]::UtcNow -lt $deadline) {
        $job = kubectl -n $Namespace get job $Name -o json | ConvertFrom-Json -Depth 30
        $conditions = @($job.status.conditions)
        if ($conditions | Where-Object { $_.type -eq "Complete" -and $_.status -eq "True" }) {
            return
        }
        if ($conditions | Where-Object { $_.type -in @("Failed", "FailureTarget") -and $_.status -eq "True" }) {
            Write-KubernetesJobDiagnostics -Namespace $Namespace -Name $Name
            throw "Kubernetes Job $Namespace/$Name failed."
        }
        Start-Sleep -Seconds 2
    }

    Write-KubernetesJobDiagnostics -Namespace $Namespace -Name $Name
    throw "Timed out after $TimeoutSeconds seconds waiting for Kubernetes Job $Namespace/$Name."
}

function Invoke-DisabledSandboxE2E([string]$FixtureImage) {
    if (kubectl -n $AgentxNamespace get deployment sandbox-manager --ignore-not-found -o name) {
        throw "Sandbox disabled profile unexpectedly deployed sandbox-manager."
    }

    $script:disabledForward = Start-Process kubectl -ArgumentList @(
        "-n", $AgentxNamespace, "port-forward", "service/platform-api", "$($script:disabledPort):8080"
    ) -PassThru -WindowStyle Hidden -RedirectStandardOutput $script:disabledForwardOut -RedirectStandardError $script:disabledForwardError
    try {
        Wait-TcpPort $script:disabledPort
        $baseUrl = "http://127.0.0.1:$($script:disabledPort)/api/v1"
        $bootstrapBody = @{
            companyName = "Distributed Sandbox Disabled"
            adminUsername = "admin"
            adminDisplayName = "Distributed E2E Admin"
            password = "agentx-distributed-e2e-password"
            locale = "zh-CN"
            timezone = "Asia/Shanghai"
        } | ConvertTo-Json -Compress
        $bootstrap = Invoke-RestMethod -Method Post -Uri "$baseUrl/bootstrap" -ContentType "application/json" -Body $bootstrapBody
        $token = [string]$bootstrap.accessToken
        if (-not $token) { throw "Distributed disabled Sandbox bootstrap did not return an access token." }

        $fixtureJob = @{
            apiVersion = "batch/v1"
            kind = "Job"
            metadata = @{ name = "m5-disabled-fixture"; namespace = $AgentxNamespace; labels = @{ "agentx.io/component" = "e2e-fixture" } }
            spec = @{
                backoffLimit = 0
                template = @{
                    metadata = @{ labels = @{ "app.kubernetes.io/name" = "m5-disabled-fixture" } }
                    spec = @{
                        restartPolicy = "Never"
                        containers = @(@{
                            name = "fixture"
                            image = $FixtureImage
                            imagePullPolicy = "IfNotPresent"
                            envFrom = @(
                                @{ configMapRef = @{ name = "agentx-config" } }
                                @{ secretRef = @{ name = "agentx-secrets" } }
                            )
                            env = @(
                                @{ name = "AGENTX_M5_SANDBOX_IMAGE"; value = "opensandbox/code-interpreter@sha256:133a3c1720dd52291a019740c2987e7164ea6de79e23d8198798e58950ae2e6e" }
                                @{ name = "AGENTX_M5_BROWSER_IMAGE"; value = "opensandbox/playwright@sha256:09709684c785db3107fc3357e7af5b921f5d5a60e75071601122a473d344b475" }
                                @{ name = "LIGHTRAG_API_KEY"; value = "m5-disabled-fixture-key" }
                            )
                        })
                    }
                }
            }
        }
        $fixtureJob | ConvertTo-Json -Depth 30 | kubectl apply -f - | Out-Null
        Wait-KubernetesJob -Namespace $AgentxNamespace -Name "m5-disabled-fixture" -TimeoutSeconds 300

        $headers = @{ Authorization = "Bearer $token" }
        $version = [string]((Invoke-DependencyMySql "SELECT BIN_TO_UUID(v.id) FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE w.name='M5 Code Fixture' ORDER BY v.version_number DESC LIMIT 1;" | Select-Object -Last 1)).Trim()
        if (-not $version) { throw "M5 Code Fixture version was not created for Sandbox disabled E2E." }
        $executionBody = @{ input = @{}; idempotencyKey = "m5-disabled-sandbox-$PID" } | ConvertTo-Json -Compress
        $execution = Invoke-RestMethod -Method Post -Uri "$baseUrl/workflow-versions/$version/executions" -Headers $headers -ContentType "application/json" -Body $executionBody
        $executionId = [string]$execution.executionId
        if (-not $executionId) { throw "Sandbox disabled execution did not return an execution ID." }

        $status = $null
        for ($attempt = 0; $attempt -lt 90; $attempt++) {
            $status = [string]((Invoke-DependencyMySql "SELECT status FROM workflow_executions WHERE id=UUID_TO_BIN('$executionId');" | Select-Object -Last 1)).Trim()
            if ($status -in @("succeeded", "failed", "cancelled", "timed_out")) { break }
            Start-Sleep -Seconds 2
        }
        $errorCode = [string]((Invoke-DependencyMySql "SELECT error_code FROM node_attempts WHERE execution_id=UUID_TO_BIN('$executionId') AND error_code IS NOT NULL ORDER BY created_at DESC LIMIT 1;" | Select-Object -Last 1)).Trim()
        $leases = [int]((Invoke-DependencyMySql "SELECT COUNT(*) FROM sandbox_leases WHERE execution_id=UUID_TO_BIN('$executionId');" | Select-Object -Last 1)).Trim()
        if ($status -ne "failed") { throw "Sandbox disabled Code execution did not fail; status=$status." }
        if ($errorCode -ne "RUNTIME_UNAVAILABLE") { throw "Sandbox disabled Code execution returned $errorCode instead of RUNTIME_UNAVAILABLE." }
        if ($leases -ne 0) { throw "Sandbox disabled Code execution created $leases Sandbox lease(s)." }
        return [pscustomobject]@{ status = $status; errorCode = $errorCode; sandboxLeases = $leases; managerDeployed = $false }
    }
    finally {
        if ($script:disabledForward -and -not $script:disabledForward.HasExited) { Stop-Process -Id $script:disabledForward.Id -Force }
        $script:disabledForward = $null
        Remove-Item -LiteralPath $script:disabledForwardOut, $script:disabledForwardError -Force -ErrorAction SilentlyContinue
    }
}

try {
    foreach ($namespace in @($AgentxNamespace, $DependencyNamespace)) { if (kubectl get namespace $namespace --ignore-not-found -o name) { kubectl delete namespace $namespace --wait=true --timeout=300s } }
    kubectl create namespace $DependencyNamespace | Out-Null
    Apply-DependencySecret
    foreach ($component in @("mysql", "redis", "clickhouse", "minio")) { kubectl -n $DependencyNamespace apply -k "$root/deploy/k8s/infrastructure/$component" | Out-Null }
    foreach ($component in @("mysql", "redis", "clickhouse", "minio")) { kubectl -n $DependencyNamespace rollout status "statefulset/$component" --timeout=420s }
    Wait-KubernetesJob -Namespace $DependencyNamespace -Name "minio-bucket-init" -TimeoutSeconds 300

    if (-not $SkipBuild) { & "$PSScriptRoot/build-images.ps1" -Namespace $AgentxNamespace -Services @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer", "m5-fixture") }
    if (kubectl get namespace $AgentxNamespace --ignore-not-found -o name) { kubectl delete namespace $AgentxNamespace --wait=true --timeout=300s }

    $profile = Get-Content "$root/deploy/profiles/full-local.json" -Raw | ConvertFrom-Json -Depth 30
    $profile.namespace = $AgentxNamespace
    $profile.environment = "test"
    $profile.images.mode = "registry"
    $profile.images.registry = "agentx"
    $profile.ingress.host = "agentx-distributed.localhost"
    $profile.components.mysql.mode = "external"
    $profile.components.mysql.host = "mysql.$DependencyNamespace.svc.cluster.local"
    $profile.components.redis.mode = "external"
    $profile.components.redis.url = "redis://redis.$DependencyNamespace.svc.cluster.local:6379/"
    $profile.components.clickhouse.mode = "external"
    $profile.components.clickhouse.url = "http://clickhouse.$DependencyNamespace.svc.cluster.local:8123"
    $profile.components.objectStorage.mode = "external-s3"
    $profile.components.objectStorage.endpoint = "http://minio.$DependencyNamespace.svc.cluster.local:9000"
    $profile.components.rag.mode = "disabled"
    $profile.components.memory.mode = "disabled"
    $profile.components.sandbox.mode = "disabled"
    [IO.File]::WriteAllText($profilePath, ($profile | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))

    $env:AGENTX_DEPLOY_MYSQL_PASSWORD = $mysqlPassword
    $env:AGENTX_DEPLOY_REDIS_PASSWORD = $redisPassword
    $env:AGENTX_DEPLOY_CLICKHOUSE_PASSWORD = $clickHousePassword
    $env:AGENTX_DEPLOY_S3_ACCESS_KEY = $s3AccessKey
    $env:AGENTX_DEPLOY_S3_SECRET_KEY = $s3SecretKey
    & "$PSScriptRoot/deploy.ps1" -Action Install -Profile Custom -ConfigFile $profilePath -NonInteractive

    $fixtureImage = if ($profile.images.mode -eq "registry") { "$($profile.images.registry.TrimEnd('/'))/m5-fixture:$($profile.images.tag)" } else { "agentx/m5-fixture:$($profile.images.tag)" }
    $disabledSandboxEvidence = Invoke-DisabledSandboxE2E -FixtureImage $fixtureImage

    Invoke-DependencyMySql "CREATE TABLE IF NOT EXISTS deployment_e2e_marker(id INT PRIMARY KEY); INSERT IGNORE INTO deployment_e2e_marker VALUES(1);" | Out-Null
    & "$PSScriptRoot/deploy.ps1" -Action Upgrade -Profile Custom -ConfigFile $profilePath -Target services -NonInteractive
    $marker = Invoke-DependencyMySql "SELECT COUNT(*) FROM deployment_e2e_marker WHERE id=1;" | Select-Object -Last 1
    if ([int]$marker -ne 1) { throw "External MySQL data was not preserved by Upgrade." }

    & "$PSScriptRoot/deploy.ps1" -Action Uninstall -ConfigFile $profilePath -Target all -DeleteNamespace -NonInteractive
    foreach ($component in @("mysql", "redis", "clickhouse", "minio")) { if (-not (kubectl -n $DependencyNamespace get statefulset $component --ignore-not-found -o name)) { throw "Agentx uninstall modified external dependency $component." } }
    [pscustomobject]@{ status = "passed"; dependencyNamespace = $DependencyNamespace; agentxNamespace = $AgentxNamespace; externalResourcesPreserved = $true; upgradeDataPreserved = $true; sandboxDisabled = $disabledSandboxEvidence } | ConvertTo-Json -Depth 10
}
finally {
    foreach ($name in @("AGENTX_DEPLOY_MYSQL_PASSWORD", "AGENTX_DEPLOY_REDIS_PASSWORD", "AGENTX_DEPLOY_CLICKHOUSE_PASSWORD", "AGENTX_DEPLOY_S3_ACCESS_KEY", "AGENTX_DEPLOY_S3_SECRET_KEY")) { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $profilePath -Force -ErrorAction SilentlyContinue
    if (-not $KeepNamespaces) { foreach ($namespace in @($AgentxNamespace, $DependencyNamespace)) { kubectl delete namespace $namespace --ignore-not-found --wait=true --timeout=300s | Out-Null } }
}
