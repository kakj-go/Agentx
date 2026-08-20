param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RuntimeNamespace = "",
    [string]$DependenciesNamespace = "",
    [string]$ImageTag = "dev",
    [int]$StabilityMinutes = 0,
    [int]$Concurrency = 8,
    [int]$StabilityIntervalSeconds = 10,
    [string]$StabilityOnlyEndpoint = "",
    [switch]$SkipDirectNetworkPolicyAssertion,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$configPath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $configPath | ConvertFrom-Json
if ($profile.apiVersion -ne "agentx.io/deployment/v2alpha3") { throw "Egress E2E requires deployment/v2alpha3." }
if (-not $RuntimeNamespace) { $RuntimeNamespace = [string]$profile.namespaces.runtime }
if (-not $DependenciesNamespace) { $DependenciesNamespace = [string]$profile.namespaces.dependencies }
if ($StabilityMinutes -lt 0 -or $StabilityMinutes -gt 120) { throw "StabilityMinutes must be in 0..120." }
if ($Concurrency -lt 1 -or $Concurrency -gt 64) { throw "Concurrency must be in 1..64." }
if ($StabilityIntervalSeconds -lt 1 -or $StabilityIntervalSeconds -gt 60) { throw "StabilityIntervalSeconds must be in 1..60." }
$stabilityOnly = -not [string]::IsNullOrWhiteSpace($StabilityOnlyEndpoint)
if ($stabilityOnly) {
    try { $stabilityUri = [Uri]$StabilityOnlyEndpoint } catch { throw "StabilityOnlyEndpoint must be an absolute HTTPS URL." }
    if (-not $stabilityUri.IsAbsoluteUri -or $stabilityUri.Scheme -ne "https" -or -not $stabilityUri.Host -or $stabilityUri.UserInfo) {
        throw "StabilityOnlyEndpoint must be an absolute HTTPS URL without userinfo."
    }
}

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Get-WorkloadSecret {
    param([string]$Workload, [string]$Fallback)
    if ($profile.secrets.workloads -and $profile.secrets.workloads.PSObject.Properties[$Workload]) {
        return [string]$profile.secrets.workloads.PSObject.Properties[$Workload].Value
    }
    return $Fallback
}

$runId = [Guid]::NewGuid().ToString("N").Substring(0, 10)
$jobName = "agentx-egress-smoke-$runId"
$bypassPod = "agentx-egress-bypass-$runId"
$evidenceDirectory = Join-Path $root ".local/evidence/egress-$runId"
New-Item -ItemType Directory -Force -Path $evidenceDirectory | Out-Null
$fixtureLog = Join-Path $evidenceDirectory "fixture.log"
$fixtureError = Join-Path $evidenceDirectory "fixture-error.log"
$tunnelContainer = $null
$cloudflaredImage = "cloudflare/cloudflared@sha256:0aa26e284f05e6c77ae375b8c9c11d9eb6a448fb7bcd8d40f31cb6176189eb38"
$fixtureProcess = $null
$tunnelProcess = $null
try {
    if (-not $SkipBuild) {
        & (Join-Path $root "scripts/build-images.ps1") -Tag $ImageTag -Namespace $DependenciesNamespace -Services @("agentx-egress-smoke") -SkipWeb
        if ($LASTEXITCODE -ne 0) { throw "Egress smoke image build failed." }
    }

    $tunnelProvider = "direct-public-endpoint"
    if ($stabilityOnly) {
        $publicEndpoint = $StabilityOnlyEndpoint
        $stabilityEndpoint = $StabilityOnlyEndpoint
    } else {
    $python = (Get-Command python -ErrorAction Stop).Source
    $fixtureProcess = Start-Process -FilePath $python -ArgumentList @((Join-Path $root "scripts/fixtures/egress-https-fixture.py"), "18090") -WindowStyle Hidden -RedirectStandardOutput $fixtureLog -RedirectStandardError $fixtureError -PassThru
    $fixtureReady = $false
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        try {
            if ((Invoke-RestMethod -Uri "http://127.0.0.1:18090/health" -TimeoutSec 1).ok) {
                $fixtureReady = $true
                break
            }
        } catch { Start-Sleep -Milliseconds 500 }
    }
    if (-not $fixtureReady) { throw "Local Egress HTTPS fixture did not start." }

    $docker = (Get-Command docker -ErrorAction Stop).Source
    $publicEndpoint = $null
    for ($tunnelAttempt = 1; $tunnelAttempt -le 3 -and -not $publicEndpoint; $tunnelAttempt++) {
        $candidateContainer = "agentx-cloudflared-$runId-$tunnelAttempt"
        $candidateLog = Join-Path $evidenceDirectory "cloudflared-$tunnelAttempt.log"
        $candidateError = Join-Path $evidenceDirectory "cloudflared-$tunnelAttempt-error.log"
        $candidateProcess = Start-Process -FilePath $docker -ArgumentList @(
            "run", "--rm", "--name", $candidateContainer, $cloudflaredImage,
            "tunnel", "--no-autoupdate", "--url", "http://host.docker.internal:18090"
        ) -WindowStyle Hidden -RedirectStandardOutput $candidateLog -RedirectStandardError $candidateError -PassThru
        $candidateEndpoint = $null
        for ($attempt = 0; $attempt -lt 60 -and -not $candidateEndpoint; $attempt++) {
            foreach ($logPath in @($candidateLog, $candidateError)) {
                if (Test-Path -LiteralPath $logPath) {
                    $match = Get-Content -LiteralPath $logPath -Tail 100 -ErrorAction SilentlyContinue |
                        Select-String -Pattern 'https://[a-z0-9-]+\.trycloudflare\.com' |
                        Select-Object -Last 1
                    if ($match -and $match.Matches.Count -gt 0) {
                        $candidateEndpoint = $match.Matches[0].Value
                        break
                    }
                }
            }
            if ($candidateProcess.HasExited -and -not $candidateEndpoint) { break }
            if (-not $candidateEndpoint) { Start-Sleep -Seconds 1 }
        }
        $candidateReady = $false
        if ($candidateEndpoint) {
            for ($attempt = 0; $attempt -lt 30; $attempt++) {
                try {
                    if ((Invoke-RestMethod -Uri "$candidateEndpoint/health" -TimeoutSec 3).ok) {
                        $candidateReady = $true
                        break
                    }
                } catch { }
                Start-Sleep -Seconds 1
            }
        }
        if ($candidateReady) {
            $publicEndpoint = $candidateEndpoint
            $tunnelContainer = $candidateContainer
            $tunnelProcess = $candidateProcess
            break
        }
        try { & $docker rm --force $candidateContainer 2>$null | Out-Null } catch { }
        if (-not $candidateProcess.HasExited) { Stop-Process -Id $candidateProcess.Id -Force -ErrorAction SilentlyContinue }
    }
    if (-not $publicEndpoint) { throw "Cloudflare Quick Tunnel HTTPS fixture did not become ready after 3 candidates." }
    Start-Sleep -Seconds 2
    $stabilityEndpoint = "$publicEndpoint/health"
    $tunnelProvider = "cloudflare-quick-tunnel"
    }

    $workerSecret = Get-WorkloadSecret "workflowWorker" ([string]$profile.secrets.runtime)
    $stabilitySeconds = $StabilityMinutes * 60
    $activeDeadline = [Math]::Max(600, $stabilitySeconds + 600)
    $job = @"
apiVersion: batch/v1
kind: Job
metadata:
  name: $jobName
  namespace: $RuntimeNamespace
  labels: { agentx.io/managed-by: agentx-v2-egress-e2e }
spec:
  backoffLimit: 0
  activeDeadlineSeconds: $activeDeadline
  template:
    metadata:
      labels: { app.kubernetes.io/name: agentx-egress-smoke, agentx.io/plane: runtime, agentx.io/egress-client: managed }
    spec:
      restartPolicy: Never
      serviceAccountName: workflow-worker
      automountServiceAccountToken: false
      securityContext: { runAsNonRoot: true, seccompProfile: { type: RuntimeDefault } }
      containers:
        - name: smoke
          image: agentx/agentx-egress-smoke:$ImageTag
          imagePullPolicy: IfNotPresent
          env:
            - { name: AGENTX_EGRESS_PROXY_URL, value: "http://agentx-egress-gateway.$DependenciesNamespace.svc:3128" }
            - name: AGENTX_EGRESS_JWT_KEY_ID
              valueFrom: { secretKeyRef: { name: $workerSecret, key: AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID } }
            - name: AGENTX_EGRESS_JWT_PRIVATE_KEY_PEM
              valueFrom: { secretKeyRef: { name: $workerSecret, key: AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM } }
            - { name: AGENTX_EGRESS_SMOKE_ENDPOINT, value: "$publicEndpoint" }
            - { name: AGENTX_EGRESS_SMOKE_STABILITY_ENDPOINT, value: "$stabilityEndpoint" }
            - { name: AGENTX_EGRESS_SMOKE_STABILITY_ONLY, value: "$($stabilityOnly.ToString().ToLowerInvariant())" }
            - { name: AGENTX_EGRESS_SMOKE_CONCURRENCY, value: "$Concurrency" }
            - { name: AGENTX_EGRESS_SMOKE_STABILITY_SECONDS, value: "$stabilitySeconds" }
            - { name: AGENTX_EGRESS_SMOKE_STABILITY_INTERVAL_SECONDS, value: "$StabilityIntervalSeconds" }
          resources: { requests: { cpu: 25m, memory: 32Mi }, limits: { cpu: 500m, memory: 128Mi } }
          securityContext: { allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, runAsNonRoot: true, capabilities: { drop: [ALL] } }
"@
    $job | kubectl apply -f - | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Egress smoke Job apply failed." }
    $jobCompleted = $false
    $jobFailed = $false
    for ($attempt = 0; $attempt -lt $activeDeadline; $attempt++) {
        $jobObject = (& kubectl -n $RuntimeNamespace get job $jobName -o json | ConvertFrom-Json)
        $jobCompleted = @($jobObject.status.conditions | Where-Object { $_.type -eq "Complete" -and $_.status -eq "True" }).Count -gt 0
        $jobFailed = @($jobObject.status.conditions | Where-Object { $_.type -eq "Failed" -and $_.status -eq "True" }).Count -gt 0
        if ($jobCompleted -or $jobFailed) { break }
        Start-Sleep -Seconds 1
    }
    $jobLogs = (& kubectl -n $RuntimeNamespace logs "job/$jobName" --tail=-1) -join "`n"
    $jobLogs | Set-Content -LiteralPath (Join-Path $evidenceDirectory "job.log") -Encoding utf8NoBOM
    if (-not $jobCompleted -or $jobFailed -or $jobLogs -notmatch '"status":"passed"') { throw "Egress smoke Job failed: $jobLogs" }

    $directNetworkPolicyAssertion = "passed"
    if ($SkipDirectNetworkPolicyAssertion) {
        $directNetworkPolicyAssertion = "skipped-explicitly"
    } else {
        $bypass = @"
apiVersion: v1
kind: Pod
metadata:
  name: $bypassPod
  namespace: $RuntimeNamespace
  labels: { app.kubernetes.io/name: agentx-egress-bypass, agentx.io/plane: runtime, agentx.io/egress-client: managed }
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  containers:
    - name: smoke
      image: agentx/agentx-egress-smoke:$ImageTag
      imagePullPolicy: IfNotPresent
      env:
        - { name: AGENTX_EGRESS_SMOKE_ASSERT_DIRECT_BLOCKED, value: "true" }
      securityContext: { allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, runAsNonRoot: true, capabilities: { drop: [ALL] } }
"@
        $bypass | kubectl apply -f - | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Runtime bypass Pod apply failed." }
        for ($attempt = 0; $attempt -lt 30; $attempt++) {
            $phase = (& kubectl -n $RuntimeNamespace get pod $bypassPod -o jsonpath='{.status.phase}') -join ""
            if ($phase -in @("Succeeded", "Failed")) { break }
            Start-Sleep -Seconds 1
        }
        $bypassLogs = (& kubectl -n $RuntimeNamespace logs $bypassPod --tail=-1 2>&1) -join "`n"
        $bypassLogs | Set-Content -LiteralPath (Join-Path $evidenceDirectory "bypass.log") -Encoding utf8NoBOM
        $exitCode = (& kubectl -n $RuntimeNamespace get pod $bypassPod -o jsonpath='{.status.containerStatuses[0].state.terminated.exitCode}') -join ""
        if ($phase -ne "Succeeded" -or $exitCode -ne "0" -or $bypassLogs -notmatch '"negative":"direct-public-egress"') {
            throw "Runtime direct-egress assertion did not execute successfully: phase=$phase exitCode=$exitCode logs=$bypassLogs"
        }
    }

    Write-Output (@{
        status = "passed"
        publicFixture = ([Uri]$publicEndpoint).Host
        tunnelProvider = $tunnelProvider
        runtimeNamespace = $RuntimeNamespace
        dependenciesNamespace = $DependenciesNamespace
        stabilityMinutes = $StabilityMinutes
        stabilityIntervalSeconds = $StabilityIntervalSeconds
        concurrency = $Concurrency
        directNetworkPolicyAssertion = $directNetworkPolicyAssertion
        evidenceDirectory = $evidenceDirectory
    } | ConvertTo-Json -Depth 5 -Compress)
} finally {
    & kubectl -n $RuntimeNamespace delete job $jobName --ignore-not-found --wait=true | Out-Null
    & kubectl -n $RuntimeNamespace delete pod $bypassPod --ignore-not-found --wait=true | Out-Null
    if ($tunnelContainer) { try { & docker rm --force $tunnelContainer 2>$null | Out-Null } catch { } }
    if ($tunnelProcess -and -not $tunnelProcess.HasExited) { Stop-Process -Id $tunnelProcess.Id -Force -ErrorAction SilentlyContinue }
    if ($fixtureProcess -and -not $fixtureProcess.HasExited) { Stop-Process -Id $fixtureProcess.Id -Force -ErrorAction SilentlyContinue }
}
