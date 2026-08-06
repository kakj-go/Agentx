param(
    [Parameter(Mandatory = $true)][string]$Namespace,
    [Parameter(Mandatory = $true)][string]$Selector,
    [Parameter(Mandatory = $true)][string]$ExpectedRuntimeClass,
    [ValidateSet("standard", "strong")][string]$IsolationLevel = "standard",
    [Parameter(Mandatory = $true)][string]$OutputPath
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$failures = [Collections.Generic.List[string]]::new()
$podNames = [Collections.Generic.List[string]]::new()

if ($IsolationLevel -eq "strong" -and $ExpectedRuntimeClass -eq "runc") {
    throw "runc cannot satisfy strong isolation."
}

$context = (kubectl config current-context).Trim()
$pods = kubectl -n $Namespace get pods -l $Selector -o json | ConvertFrom-Json -Depth 100
if ($LASTEXITCODE -ne 0) { throw "Could not query Sandbox Pods." }
$policies = kubectl -n $Namespace get networkpolicy -o json | ConvertFrom-Json -Depth 100
if ($LASTEXITCODE -ne 0) { throw "Could not query Sandbox NetworkPolicies." }

foreach ($pod in @($pods.items)) {
    $name = [string]$pod.metadata.name
    $podNames.Add($name)
    $actualRuntimeClass = [string]$pod.spec.runtimeClassName
    $runtimeMatches = if ($ExpectedRuntimeClass -eq "runc") { -not $actualRuntimeClass -or $actualRuntimeClass -eq "runc" } else { $actualRuntimeClass -eq $ExpectedRuntimeClass }
    if (-not $runtimeMatches) { $failures.Add("$name runtimeClassName '$actualRuntimeClass' does not match '$ExpectedRuntimeClass'") }
    if ($pod.spec.automountServiceAccountToken -ne $false) { $failures.Add("$name mounts a ServiceAccount token") }
    if ($pod.spec.hostNetwork -eq $true -or $pod.spec.hostPID -eq $true -or $pod.spec.hostIPC -eq $true) { $failures.Add("$name enables a host namespace") }
    if (@($pod.spec.volumes | Where-Object { $_.hostPath }).Count -gt 0) { $failures.Add("$name uses a hostPath volume") }
    $podSeccomp = [string]$pod.spec.securityContext.seccompProfile.type
    foreach ($container in @($pod.spec.containers)) {
        $prefix = "$name/$($container.name)"
        $security = $container.securityContext
        if ($security.allowPrivilegeEscalation -ne $false) { $failures.Add("$prefix allows privilege escalation") }
        if ($security.readOnlyRootFilesystem -ne $true) { $failures.Add("$prefix root filesystem is writable") }
        if ($security.runAsNonRoot -ne $true -and $pod.spec.securityContext.runAsNonRoot -ne $true) { $failures.Add("$prefix does not require a non-root user") }
        if (@($security.capabilities.drop) -notcontains "ALL") { $failures.Add("$prefix does not drop all capabilities") }
        $containerSeccomp = [string]$security.seccompProfile.type
        if ($podSeccomp -ne "RuntimeDefault" -and $containerSeccomp -ne "RuntimeDefault") { $failures.Add("$prefix does not use RuntimeDefault seccomp") }
        foreach ($limit in @("cpu", "memory", "ephemeral-storage")) {
            if (-not $container.resources.limits.PSObject.Properties[$limit]) { $failures.Add("$prefix has no $limit limit") }
        }
    }
}

$podCount = @($pods.items).Count
$networkPolicyCount = @($policies.items).Count
if ($podCount -lt 1) { $failures.Add("No Sandbox Pods matched selector '$Selector'") }
if ($networkPolicyCount -lt 1) { $failures.Add("No NetworkPolicy exists in Namespace '$Namespace'") }
$evidence = [ordered]@{
    schemaVersion = "agentx.io/isolation-evidence/v1"
    status = $(if ($failures.Count -eq 0) { "passed" } else { "failed" })
    isolationLevel = $IsolationLevel
    runtimeClass = $ExpectedRuntimeClass
    context = $context
    namespace = $Namespace
    selector = $Selector
    podCount = $podCount
    networkPolicyCount = $networkPolicyCount
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    pods = @($podNames)
    failures = @($failures)
}
$parent = Split-Path -Parent $OutputPath
if ($parent) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
$evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $OutputPath -Encoding utf8NoBOM
$schema = Join-Path $root "deploy/release/isolation-evidence.schema.json"
if (-not ((Get-Content -Raw -LiteralPath $OutputPath) | Test-Json -SchemaFile $schema)) { throw "Isolation evidence is invalid." }
if ($failures.Count -gt 0) { throw "Sandbox isolation verification failed: $($failures -join '; ')" }
Write-Output $OutputPath
