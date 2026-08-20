param(
    [ValidateSet("Validate", "Install", "Upgrade", "Render", "Status", "Rollback", "Uninstall", "Doctor", "SyncSecrets")]
    [string]$Action = "Install",
    [ValidateSet("Control", "Runtime", "Observability", "Dependencies", "All")]
    [string]$Target = "All",
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = "",
    [string]$ReleaseManifest = "",
    [string]$PreviousReleaseManifest = "",
    [switch]$BuildImages,
    [switch]$CleanupOnFailure,
    [switch]$RecreateV2Data,
    [switch]$PurgeTestResources
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$script:IngressChartVersion = "4.15.1"
$script:IngressChartDigest = "3eff0bd18151d6e6b1c441463410571443dda1ac78292cb189346628de784f0c"
$script:IngressChartUrl = "https://github.com/kubernetes/ingress-nginx/releases/download/helm-chart-4.15.1/ingress-nginx-4.15.1.tgz"
$script:HelmVersion = "3.18.4"
$script:HelmDigests = @{
    "windows-amd64" = "0af12a2233d71ef4207db1eabbf103b554631206ed5b2b34fc56b73a52596888"
    "linux-amd64" = "f8180838c23d7c7d797b208861fecb591d9ce1690d8704ed1e4cb8e2add966c1"
    "linux-arm64" = "c0a45e67eef0c7416a8a8c9e9d5d2d30d70e4f4d3f7bea5de28241fffa8f3b89"
    "darwin-amd64" = "860a7238285b44b5dc7b3c4dad6194316885d7015d77c34e23177e0e9554af8f"
    "darwin-arm64" = "041849741550b20710d7ad0956e805ebd960b483fe978864f8e7fdd03ca84ec8"
}

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Invoke-KubectlInput {
    param([string]$Content, [string[]]$Arguments)
    $Content | & kubectl @Arguments
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed." }
}

function Get-HelmBinary {
    $existing = Get-Command helm -ErrorAction SilentlyContinue
    if ($existing) { return $existing.Source }
    $runtime = [Runtime.InteropServices.RuntimeInformation]
    $os = if ($runtime::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Windows)) { "windows" } elseif ($runtime::IsOSPlatform([Runtime.InteropServices.OSPlatform]::OSX)) { "darwin" } else { "linux" }
    $arch = if ($runtime::OSArchitecture -eq [Runtime.InteropServices.Architecture]::Arm64) { "arm64" } else { "amd64" }
    $platform = "$os-$arch"
    if (-not $script:HelmDigests.ContainsKey($platform)) { throw "Helm auto-download is not supported on $platform; install Helm $($script:HelmVersion) manually." }
    $extension = if ($os -eq "windows") { "zip" } else { "tar.gz" }
    $asset = "helm-v$($script:HelmVersion)-$platform.$extension"
    $cache = Join-Path $root ".local/deploy-cache/helm/$platform"
    $binary = Join-Path $cache $(if ($os -eq "windows") { "helm.exe" } else { "helm" })
    if (-not (Test-Path -LiteralPath $binary)) {
        New-Item -ItemType Directory -Force -Path $cache | Out-Null
        $archive = Join-Path $cache $asset
        Invoke-WebRequest -UseBasicParsing -Uri "https://get.helm.sh/$asset" -OutFile $archive
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant() -ne $script:HelmDigests[$platform]) {
            Remove-Item -LiteralPath $archive -Force
            throw "Helm archive SHA-256 mismatch."
        }
        if ($os -eq "windows") {
            Expand-Archive -LiteralPath $archive -DestinationPath $cache -Force
            Move-Item -Force (Join-Path $cache "$platform/helm.exe") $binary
        } else {
            & tar -xzf $archive -C $cache
            Move-Item -Force (Join-Path $cache "$platform/helm") $binary
            & chmod +x $binary
        }
        Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    }
    return $binary
}

function Get-IngressChartPath {
    $cache = Join-Path $root ".local/deploy-cache"
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $chart = Join-Path $cache "ingress-nginx-$($script:IngressChartVersion).tgz"
    if (-not (Test-Path -LiteralPath $chart) -or (Get-FileHash -Algorithm SHA256 -LiteralPath $chart).Hash.ToLowerInvariant() -ne $script:IngressChartDigest) {
        Invoke-WebRequest -UseBasicParsing -Uri $script:IngressChartUrl -OutFile $chart
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $chart).Hash.ToLowerInvariant() -ne $script:IngressChartDigest) { throw "ingress-nginx chart SHA-256 mismatch." }
    return $chart
}

function Get-IngressHelmArguments {
    param($Profile, [hashtable]$Namespaces)
    $arguments = @(
        "--namespace", [string]$Namespaces.dependencies,
        "--version", $script:IngressChartVersion,
        "-f", (Join-Path $root "deploy/ingress-nginx/values.yaml"),
        "--set-string", "controller.ingressClass=$($Namespaces.ingressClass)",
        "--set-string", "controller.ingressClassResource.name=$($Namespaces.ingressClass)",
        "--set-string", "controller.ingressClassResource.controllerValue=k8s.io/$($Namespaces.ingressClass)"
    )
    if ([string]$Namespaces.ingressClass -ne [string]$Profile.ingress.className) {
        $arguments += @(
            "--set-string", "fullnameOverride=$($Namespaces.ingressClass)",
            "--set-string", "controller.service.type=ClusterIP"
        )
    }
    return $arguments
}

function Get-IngressRenderedManifest {
    param($Profile, [hashtable]$Namespaces)
    $helm = Get-HelmBinary
    $arguments = @("template", "agentx-ingress-nginx", (Get-IngressChartPath)) + (Get-IngressHelmArguments $Profile $Namespaces)
    $rendered = (& $helm @arguments) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "ingress-nginx Helm render failed." }
    return $rendered
}

function Install-IngressController {
    param($Profile, [hashtable]$Namespaces)
    $namespace = [string]$Namespaces.dependencies
    $className = [string]$Namespaces.ingressClass
    $classPayload = (& kubectl get ingressclass $className --ignore-not-found -o json 2>$null) -join "`n"
    if ($classPayload) {
        $class = $classPayload | ConvertFrom-Json
        if ([string]$class.metadata.annotations.'meta.helm.sh/release-name' -ne "agentx-ingress-nginx" -or [string]$class.metadata.annotations.'meta.helm.sh/release-namespace' -ne $namespace) {
            throw "IngressClass $className is not owned by $namespace/agentx-ingress-nginx; refusing to take it over."
        }
    }
    $helm = Get-HelmBinary
    $releaseExists = $false
    try {
        & $helm status agentx-ingress-nginx --namespace $namespace 2>$null | Out-Null
        $releaseExists = $true
    } catch {
        $releaseExists = $false
    }
    if ($releaseExists) {
        $ownership = (& kubectl -n $namespace get configmap agentx-ingress-ownership --ignore-not-found -o jsonpath='{.metadata.labels.agentx\.io/managed-by}' 2>$null) -join ""
        if ($ownership -ne "agentx-v2-deploy") { throw "Ingress release $namespace/agentx-ingress-nginx exists without Agentx V2 ownership." }
    }
    $arguments = @("upgrade", "--install", "agentx-ingress-nginx", (Get-IngressChartPath), "--create-namespace", "--wait", "--timeout", "10m") + (Get-IngressHelmArguments $Profile $Namespaces)
    & $helm @arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "ingress-nginx Helm install failed." }
    $ownershipManifest = @{
        apiVersion = "v1"; kind = "ConfigMap"
        metadata = @{ name = "agentx-ingress-ownership"; namespace = $namespace; labels = @{ "agentx.io/managed-by" = "agentx-v2-deploy" } }
        data = @{ release = "agentx-ingress-nginx"; ingressClass = $className }
    } | ConvertTo-Json -Depth 10 -Compress
    Invoke-KubectlInput $ownershipManifest @("apply", "-f", "-")
}

function Remove-IngressController {
    param([hashtable]$Namespaces)
    $namespace = [string]$Namespaces.dependencies
    $className = [string]$Namespaces.ingressClass
    $ownership = (& kubectl -n $namespace get configmap agentx-ingress-ownership --ignore-not-found -o jsonpath='{.metadata.labels.agentx\.io/managed-by}' 2>$null) -join ""
    if ($ownership -ne "agentx-v2-deploy") { return }
    $ingresses = (& kubectl get ingress -A -o json 2>$null) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Failed to inspect IngressClass users." }
    $users = @(($ingresses | ConvertFrom-Json).items | Where-Object { $_.spec.ingressClassName -eq $className })
    if ($users.Count -gt 0) { return }
    $helm = Get-HelmBinary
    & $helm uninstall agentx-ingress-nginx --namespace $namespace --ignore-not-found | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "ingress-nginx Helm uninstall failed." }
    Invoke-Kubectl -Arguments @("delete", "ingressclass", $className, "--ignore-not-found")
}

function Wait-V2Job {
    param([string]$Namespace, [string]$Name, [int]$TimeoutSeconds = 300)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $json = & kubectl -n $Namespace get job $Name -o json 2>$null
        if ($LASTEXITCODE -ne 0) { throw "Failed to read Job $Namespace/$Name." }
        $job = ($json -join "`n") | ConvertFrom-Json
        if ([int]$job.status.succeeded -ge 1) { return }
        if ([int]$job.status.failed -ge 1) {
            $logs = (& kubectl -n $Namespace logs "job/$Name" --all-containers=true 2>&1) -join "`n"
            throw "Job $Namespace/$Name failed.`n$logs"
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    $description = (& kubectl -n $Namespace describe job $Name 2>&1) -join "`n"
    $logs = (& kubectl -n $Namespace logs "job/$Name" --all-containers=true 2>&1) -join "`n"
    throw "Job $Namespace/$Name timed out.`n$description`n$logs"
}

function New-RandomPassword {
    $bytes = New-Object byte[] 24
    [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    return [Convert]::ToHexString($bytes).ToLowerInvariant()
}

function Get-SecretData {
    param([string]$Namespace, [string]$Name)
    $raw = & kubectl -n $Namespace get secret $Name --ignore-not-found -o json
    if ($LASTEXITCODE -ne 0) { throw "Failed to read Secret $Namespace/$Name." }
    if (-not $raw) { return @{} }
    $secret = ($raw -join "`n") | ConvertFrom-Json
    $values = @{}
    if ($secret.data) {
        foreach ($property in $secret.data.PSObject.Properties) {
            $values[$property.Name] = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String([string]$property.Value))
        }
    }
    return $values
}

function Set-DomainSecret {
    param([string]$Namespace, [string]$Name, [hashtable]$Values)
    $encoded = @{}
    foreach ($entry in $Values.GetEnumerator()) {
        $encoded[$entry.Key] = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$entry.Value))
    }
    $manifest = @{
        apiVersion = "v1"
        kind = "Secret"
        metadata = @{ name = $Name; namespace = $Namespace; labels = @{ "agentx.io/managed-by" = "agentx-v2-deploy" } }
        type = "Opaque"
        data = $encoded
    } | ConvertTo-Json -Depth 10 -Compress
    Invoke-KubectlInput $manifest @("apply", "-f", "-")
}

function Get-OrCreateValue {
    param([hashtable]$Existing, [string]$Key, [string]$Default = "")
    if ($Existing.ContainsKey($Key) -and $Existing[$Key]) { return $Existing[$Key] }
    if ($Default) { return $Default }
    return New-RandomPassword
}

function Get-CanonicalOrLegacyValue {
    param([hashtable]$Canonical, [string]$Key, [hashtable]$Legacy, [string]$LegacyKey, [string]$Default)
    if ($Canonical.ContainsKey($Key) -and $Canonical[$Key]) { return $Canonical[$Key] }
    return Get-OrCreateValue $Legacy $LegacyKey $Default
}

function Get-CanonicalOrDeployedKeyId {
    param([hashtable]$Canonical, [string]$Key, [hashtable]$Legacy, [string]$Deployment, [string]$Namespace, [string]$Default)
    if ($Canonical.ContainsKey($Key) -and $Canonical[$Key]) { return $Canonical[$Key] }
    if ($Legacy.ContainsKey($Key) -and $Legacy[$Key]) { return $Legacy[$Key] }
    $payload = (& kubectl -n $Namespace get deployment $Deployment --ignore-not-found -o json 2>$null) -join "`n"
    if ($payload) {
        $entry = @((@(($payload | ConvertFrom-Json).spec.template.spec.containers)[0].env) | Where-Object name -eq "AGENTX_EGRESS_JWT_KEY_ID")
        if ($entry.Count -eq 1 -and $entry[0].value) { return [string]$entry[0].value }
    }
    return $Default
}

function Merge-DomainSecret {
    param([string]$Namespace, [string]$Name, [hashtable]$Values)
    $merged = Get-SecretData $Namespace $Name
    foreach ($entry in $Values.GetEnumerator()) { $merged[$entry.Key] = [string]$entry.Value }
    Set-DomainSecret $Namespace $Name $merged
}

function Get-CanonicalSigningMaterial {
    param([hashtable]$DependenciesExisting, [hashtable]$ControlExisting, [hashtable]$RuntimeExisting, [hashtable]$ObservabilityExisting, [hashtable]$EgressExisting)
    $required = @(
        "AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM",
        "AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM",
        "AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON", "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON",
        "AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON",
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON", "AGENTX_EGRESS_TLS_CERTIFICATE_PEM", "AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM"
    )
    $present = @($required | Where-Object { $DependenciesExisting.ContainsKey($_) -and $DependenciesExisting[$_] })
    if ($present.Count -eq $required.Count) {
        return @{
            servicePrivate = $DependenciesExisting.AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM; projectorPrivate = $DependenciesExisting.AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM
            bffPrivate = $DependenciesExisting.AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM; bundlePrivate = $DependenciesExisting.AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM
            workPackagePrivate = $DependenciesExisting.AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM; userPrivate = $DependenciesExisting.AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM
            servicePublicJson = $DependenciesExisting.AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON; bffPublicJson = $DependenciesExisting.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON
            bundlePublicJson = $DependenciesExisting.AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON; workPackagePublicJson = $DependenciesExisting.AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON
            userPublicJson = $DependenciesExisting.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON; runtimeGatewayEgressPrivate = $DependenciesExisting.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM
            workflowRuntimeEgressPrivate = $DependenciesExisting.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM; workflowWorkerEgressPrivate = $DependenciesExisting.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM
            sandboxEgressPrivate = $DependenciesExisting.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM; egressPublicJson = $DependenciesExisting.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON
            egressTlsCertificate = $DependenciesExisting.AGENTX_EGRESS_TLS_CERTIFICATE_PEM; egressTlsPrivateKey = $DependenciesExisting.AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM
        }
    }
    if ($present.Count -gt 0) {
        $missing = @($required | Where-Object { -not $DependenciesExisting.ContainsKey($_) -or -not $DependenciesExisting[$_] })
        throw "Dependencies Secret contains partial canonical signing material; refusing to combine unrelated key pairs. Missing: $($missing -join ', ')."
    }
    return Get-SigningMaterial $ControlExisting $RuntimeExisting $ObservabilityExisting $EgressExisting
}

function Publish-DependencySecretMirrors {
    param([hashtable]$Namespaces, $Profile)
    $dependenciesSecretName = [string]$Profile.secrets.dependencies
    if ([string]::IsNullOrWhiteSpace($dependenciesSecretName)) { throw "Profile secrets.dependencies must name the canonical Dependencies Secret." }
    $dependencies = Get-SecretData $Namespaces.dependencies $dependenciesSecretName
    $required = @(
        "CONTROL_VAULT_TOKEN", "RUNTIME_VAULT_TOKEN", "OBSERVABILITY_REDIS_PASSWORD",
        "AGENTX_CONTROL_PUBLISHER_JWT_KID", "AGENTX_CONTROL_PROJECTOR_JWT_KID", "AGENTX_CONTROL_BFF_JWT_KID",
        "AGENTX_CONTROL_BUNDLE_KEY_ID", "AGENTX_CONTROL_WORK_PACKAGE_KEY_ID", "AGENTX_CONTROL_USER_JWT_KID",
        "AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM",
        "AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM",
        "AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM", "AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON", "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON",
        "AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON",
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID",
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID", "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID",
        "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON", "AGENTX_EGRESS_TLS_CERTIFICATE_PEM", "AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM"
    )
    foreach ($key in $required) {
        if (-not $dependencies.ContainsKey($key) -or [string]::IsNullOrWhiteSpace([string]$dependencies[$key])) {
            throw "Dependencies Secret $($Namespaces.dependencies)/$dependenciesSecretName is missing $key."
        }
    }
    $controlValues = @{
        AGENTX_CONTROL_VAULT_TOKEN = $dependencies.CONTROL_VAULT_TOKEN
        AGENTX_CONTROL_PUBLISHER_JWT_KID = $dependencies.AGENTX_CONTROL_PUBLISHER_JWT_KID
        AGENTX_CONTROL_PROJECTOR_JWT_KID = $dependencies.AGENTX_CONTROL_PROJECTOR_JWT_KID
        AGENTX_CONTROL_BFF_JWT_KID = $dependencies.AGENTX_CONTROL_BFF_JWT_KID
        AGENTX_CONTROL_BUNDLE_KEY_ID = $dependencies.AGENTX_CONTROL_BUNDLE_KEY_ID
        AGENTX_CONTROL_WORK_PACKAGE_KEY_ID = $dependencies.AGENTX_CONTROL_WORK_PACKAGE_KEY_ID
        AGENTX_CONTROL_USER_JWT_KID = $dependencies.AGENTX_CONTROL_USER_JWT_KID
        AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM
        AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM
        AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM
        AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM
        AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM
        AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM
        AGENTX_CONTROL_USER_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON
    }
    $runtimeCommon = @{
        AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON
        AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON
        AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON
    }
    if ($Profile.secrets.mode -eq "generated-local") {
        Merge-DomainSecret $Namespaces.control ([string]$Profile.secrets.control) $controlValues
        $runtime = Get-SecretData $Namespaces.runtime ([string]$Profile.secrets.runtime)
        if (-not $runtime.AGENTX_RUNTIME_REDIS_PASSWORD) { throw "Runtime Secret is missing AGENTX_RUNTIME_REDIS_PASSWORD; refusing to rewrite Redis ACL credentials." }
        foreach ($entry in $runtimeCommon.GetEnumerator()) { $runtime[$entry.Key] = $entry.Value }
        $runtime.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON
        $runtime.AGENTX_RUNTIME_VAULT_TOKEN = $dependencies.RUNTIME_VAULT_TOKEN
        $runtime.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM
        $runtime.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID
        $runtime.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM
        $runtime.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID
        $runtime.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM
        $runtime.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID
        $runtime.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM
        $runtime.AGENTX_SANDBOX_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_SANDBOX_EGRESS_JWT_KEY_ID
        $runtime.AGENTX_RUNTIME_REDIS_ACL_FILE = "user default on >$($runtime.AGENTX_RUNTIME_REDIS_PASSWORD) ~* &agentx:v2:invocation:wakeup:* +@all`nuser observability on >$($dependencies.OBSERVABILITY_REDIS_PASSWORD) ~agentx:v2:trace:v1 ~agentx:v2:observability:jti:* +ping +xgroup +xreadgroup +xpending +xautoclaim +xack +set +get +del +exists"
        Set-DomainSecret $Namespaces.runtime ([string]$Profile.secrets.runtime) $runtime
        Merge-DomainSecret $Namespaces.observability ([string]$Profile.secrets.observability) @{ AGENTX_OBSERVABILITY_REDIS_PASSWORD = $dependencies.OBSERVABILITY_REDIS_PASSWORD; AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON }
        Merge-DomainSecret $Namespaces.dependencies "agentx-egress-gateway-secrets" @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON }
    } else {
        Merge-DomainSecret $Namespaces.control ([string]$Profile.secrets.workloads.platformControl) $controlValues
        $gatewayValues = @{} + $runtimeCommon
        $gatewayValues.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON
        $gatewayValues.AGENTX_RUNTIME_VAULT_TOKEN = $dependencies.RUNTIME_VAULT_TOKEN
        $gatewayValues.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM
        $gatewayValues.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID
        Merge-DomainSecret $Namespaces.runtime ([string]$Profile.secrets.workloads.runtimeGateway) $gatewayValues
        Merge-DomainSecret $Namespaces.runtime ([string]$Profile.secrets.workloads.workflowRuntime) (@{} + $runtimeCommon + @{ AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM; AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID })
        Merge-DomainSecret $Namespaces.runtime ([string]$Profile.secrets.workloads.workflowWorker) (@{} + $runtimeCommon + @{ AGENTX_RUNTIME_VAULT_TOKEN = $dependencies.RUNTIME_VAULT_TOKEN; AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM; AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID })
        Merge-DomainSecret $Namespaces.runtime ([string]$Profile.secrets.workloads.sandboxManager) @{ AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM = $dependencies.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM; AGENTX_SANDBOX_EGRESS_JWT_KEY_ID = $dependencies.AGENTX_SANDBOX_EGRESS_JWT_KEY_ID }
        Merge-DomainSecret $Namespaces.observability ([string]$Profile.secrets.workloads.observability) @{ AGENTX_OBSERVABILITY_REDIS_PASSWORD = $dependencies.OBSERVABILITY_REDIS_PASSWORD; AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON }
        Merge-DomainSecret $Namespaces.dependencies ([string]$Profile.secrets.workloads.egressGateway) @{ AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = $dependencies.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON }
    }
    $egressTlsValues = @{ "tls.crt" = $dependencies.AGENTX_EGRESS_TLS_CERTIFICATE_PEM; "tls.key" = $dependencies.AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM; "ca.crt" = $dependencies.AGENTX_EGRESS_TLS_CERTIFICATE_PEM }
    Set-DomainSecret $Namespaces.dependencies ([string]$Profile.network.egressGateway.sandboxAccess.tlsSecretName) $egressTlsValues
    if ($Profile.network.egressGateway.sandboxAccess.caSecretName) { Set-DomainSecret $Namespaces.runtime ([string]$Profile.network.egressGateway.sandboxAccess.caSecretName) @{ "ca.crt" = $dependencies.AGENTX_EGRESS_TLS_CERTIFICATE_PEM } }
}

function Sync-DependencySecrets {
    param([hashtable]$Namespaces, $Profile, [string[]]$Planes)
    $dependenciesSecretName = [string]$Profile.secrets.dependencies
    Publish-DependencySecretMirrors $Namespaces $Profile

    if ((kubectl -n $Namespaces.dependencies get statefulset vault --ignore-not-found -o name) -join "") {
        $renderedDependencies = Get-RenderedManifest @("dependencies") $Profile $Namespaces
        $vaultBootstrap = (($renderedDependencies -split '(?m)^---\s*$') | Where-Object { (Get-YamlResourceKind $_) -eq "Job" -and (Get-YamlResourceName $_) -eq "vault-bootstrap" } | Select-Object -First 1)
        if (-not $vaultBootstrap) { throw "Rendered Dependencies manifest is missing vault-bootstrap." }
        Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "job", "vault-bootstrap", "--ignore-not-found", "--wait=true")
        Invoke-KubectlInput $vaultBootstrap @("apply", "-f", "-")
        Wait-V2Job $Namespaces.dependencies "vault-bootstrap"
    }

    $restart = @()
    if ((kubectl -n $Namespaces.dependencies get deployment agentx-egress-gateway --ignore-not-found -o name) -join "") {
        Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "rollout", "restart", "deployment/agentx-egress-gateway")
        Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "rollout", "status", "deployment/agentx-egress-gateway", "--timeout=300s")
        $restart += @{ namespace = $Namespaces.dependencies; kind = "deployment"; name = "agentx-egress-gateway" }
    }
    if ((kubectl -n $Namespaces.runtime get statefulset runtime-redis --ignore-not-found -o name) -join "") {
        Invoke-Kubectl -Arguments @("-n", $Namespaces.runtime, "rollout", "restart", "statefulset/runtime-redis")
        Invoke-Kubectl -Arguments @("-n", $Namespaces.runtime, "rollout", "status", "statefulset/runtime-redis", "--timeout=300s")
        $restart += @{ namespace = $Namespaces.runtime; kind = "statefulset"; name = "runtime-redis" }
    }
    foreach ($name in @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager")) {
        if ((kubectl -n $Namespaces.runtime get deployment $name --ignore-not-found -o name) -join "") {
            Invoke-Kubectl -Arguments @("-n", $Namespaces.runtime, "rollout", "restart", "deployment/$name")
            Invoke-Kubectl -Arguments @("-n", $Namespaces.runtime, "rollout", "status", "deployment/$name", "--timeout=300s")
            $restart += @{ namespace = $Namespaces.runtime; kind = "deployment"; name = $name }
        }
    }
    if ((kubectl -n $Namespaces.observability get deployment observability --ignore-not-found -o name) -join "") {
        Invoke-Kubectl -Arguments @("-n", $Namespaces.observability, "rollout", "restart", "deployment/observability")
        Invoke-Kubectl -Arguments @("-n", $Namespaces.observability, "rollout", "status", "deployment/observability", "--timeout=300s")
        $restart += @{ namespace = $Namespaces.observability; kind = "deployment"; name = "observability" }
    }
    if ((kubectl -n $Namespaces.control get deployment platform-control --ignore-not-found -o name) -join "") {
        Invoke-Kubectl -Arguments @("-n", $Namespaces.control, "rollout", "restart", "deployment/platform-control")
        Invoke-Kubectl -Arguments @("-n", $Namespaces.control, "rollout", "status", "deployment/platform-control", "--timeout=300s")
        $restart += @{ namespace = $Namespaces.control; kind = "deployment"; name = "platform-control" }
    }
    Write-Output (@{ status = "synced"; source = "$($Namespaces.dependencies)/$dependenciesSecretName"; restarted = $restart } | ConvertTo-Json -Depth 6 -Compress)
}

function Get-SigningMaterial {
    param([hashtable]$ControlExisting, [hashtable]$RuntimeExisting, [hashtable]$ObservabilityExisting, [hashtable]$EgressExisting)
    $requiredControl = @("AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM", "AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_USER_JWT_PUBLIC_KEYS_JSON")
    $requiredRuntime = @("AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON", "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON")
    $requiredObservability = @("AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON")
    $requiredRuntime += @("AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM")
    $requiredEgress = @("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON", "tls.crt", "tls.key")
    $allExisting = (($requiredControl | Where-Object { -not $ControlExisting.ContainsKey($_) -or -not $ControlExisting[$_] }).Count -eq 0 -and
        ($requiredRuntime | Where-Object { -not $RuntimeExisting.ContainsKey($_) -or -not $RuntimeExisting[$_] }).Count -eq 0 -and
        ($requiredObservability | Where-Object { -not $ObservabilityExisting.ContainsKey($_) -or -not $ObservabilityExisting[$_] }).Count -eq 0 -and
        ($requiredEgress | Where-Object { -not $EgressExisting.ContainsKey($_) -or -not $EgressExisting[$_] }).Count -eq 0)
    if ($allExisting) {
        return @{ servicePrivate = $ControlExisting.AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM; projectorPrivate = $ControlExisting.AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM; bffPrivate = $ControlExisting.AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM; bundlePrivate = $ControlExisting.AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM; workPackagePrivate = $ControlExisting.AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM; servicePublicJson = $RuntimeExisting.AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON; bffPublicJson = $ObservabilityExisting.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON; bundlePublicJson = $RuntimeExisting.AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON; workPackagePublicJson = $RuntimeExisting.AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON; userPrivate = $ControlExisting.AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM; userPublicJson = $RuntimeExisting.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON; runtimeGatewayEgressPrivate = $RuntimeExisting.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM; workflowRuntimeEgressPrivate = $RuntimeExisting.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM; workflowWorkerEgressPrivate = $RuntimeExisting.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM; sandboxEgressPrivate = $RuntimeExisting.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM; egressPublicJson = $EgressExisting.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON; egressTlsCertificate = $EgressExisting.'tls.crt'; egressTlsPrivateKey = $EgressExisting.'tls.key' }
    }
    $json = (& cargo run --quiet -p agentx-v2-ops --bin agentx-keygen) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate V2 signing material." }
    $material = $json | ConvertFrom-Json
    $generated = @{
        servicePrivate = [string]$material.servicePrivateKeyPem
        projectorPrivate = [string]$material.projectorPrivateKeyPem
        bffPrivate = [string]$material.bffPrivateKeyPem
        bundlePrivate = [string]$material.bundlePrivateKeyPem
        workPackagePrivate = [string]$material.workPackagePrivateKeyPem
        servicePublicJson = (@{ "publisher-current" = [string]$material.servicePublicKeyPem; "projector-current" = [string]$material.projectorPublicKeyPem; "bff-current" = [string]$material.bffPublicKeyPem } | ConvertTo-Json -Compress)
        bffPublicJson = (@{ "bff-current" = [string]$material.bffPublicKeyPem } | ConvertTo-Json -Compress)
        bundlePublicJson = (@{ "bundle-current" = [string]$material.bundlePublicKeyBase64 } | ConvertTo-Json -Compress)
        workPackagePublicJson = (@{ "work-package-current" = [string]$material.workPackagePublicKeyBase64 } | ConvertTo-Json -Compress)
        userPrivate = [string]$material.userPrivateKeyPem
        userPublicJson = (@{ "user-current" = [string]$material.userPublicKeyPem } | ConvertTo-Json -Compress)
        runtimeGatewayEgressPrivate = [string]$material.runtimeGatewayEgressPrivateKeyPem
        workflowRuntimeEgressPrivate = [string]$material.workflowRuntimeEgressPrivateKeyPem
        workflowWorkerEgressPrivate = [string]$material.workflowWorkerEgressPrivateKeyPem
        sandboxEgressPrivate = [string]$material.sandboxEgressPrivateKeyPem
        egressPublicJson = (@{ "runtime-gateway-current" = [string]$material.runtimeGatewayEgressPublicKeyPem; "workflow-runtime-current" = [string]$material.workflowRuntimeEgressPublicKeyPem; "workflow-worker-current" = [string]$material.workflowWorkerEgressPublicKeyPem; "sandbox-current" = [string]$material.sandboxEgressPublicKeyPem } | ConvertTo-Json -Compress)
        egressTlsCertificate = [string]$material.egressTlsCertificatePem
        egressTlsPrivateKey = [string]$material.egressTlsPrivateKeyPem
    }
    $serviceFamily = @("AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM", "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM")
    if (($serviceFamily | Where-Object { -not $ControlExisting.ContainsKey($_) -or -not $ControlExisting[$_] }).Count -eq 0 -and $RuntimeExisting.AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON -and $ObservabilityExisting.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON) {
        $generated.servicePrivate = $ControlExisting.AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM
        $generated.projectorPrivate = $ControlExisting.AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM
        $generated.bffPrivate = $ControlExisting.AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM
        $generated.servicePublicJson = $RuntimeExisting.AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON
        $generated.bffPublicJson = $ObservabilityExisting.AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON
    }
    if ($ControlExisting.AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM -and $RuntimeExisting.AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON) {
        $generated.bundlePrivate = $ControlExisting.AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM
        $generated.bundlePublicJson = $RuntimeExisting.AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON
    }
    if ($ControlExisting.AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM -and $RuntimeExisting.AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON) {
        $generated.workPackagePrivate = $ControlExisting.AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM
        $generated.workPackagePublicJson = $RuntimeExisting.AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON
    }
    if ($ControlExisting.AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM -and $RuntimeExisting.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON) {
        $generated.userPrivate = $ControlExisting.AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM
        $generated.userPublicJson = $RuntimeExisting.AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON
    }
    $egressPrivateKeys = @("AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM", "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM")
    if (($egressPrivateKeys | Where-Object { -not $RuntimeExisting.ContainsKey($_) -or -not $RuntimeExisting[$_] }).Count -eq 0 -and $EgressExisting.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON) {
        $generated.runtimeGatewayEgressPrivate = $RuntimeExisting.AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM
        $generated.workflowRuntimeEgressPrivate = $RuntimeExisting.AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM
        $generated.workflowWorkerEgressPrivate = $RuntimeExisting.AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM
        $generated.sandboxEgressPrivate = $RuntimeExisting.AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM
        $generated.egressPublicJson = $EgressExisting.AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON
    }
    if ($EgressExisting.'tls.crt' -and $EgressExisting.'tls.key') {
        $generated.egressTlsCertificate = $EgressExisting.'tls.crt'
        $generated.egressTlsPrivateKey = $EgressExisting.'tls.key'
    }
    return $generated
}

function Resolve-Namespaces {
    param($Profile, [string]$Suffix, [string]$Stage = "01")
    if (-not $Suffix) {
        return @{
            control = [string]$Profile.namespaces.control
            runtime = [string]$Profile.namespaces.runtime
            observability = [string]$Profile.namespaces.runtime
            dependencies = [string]$Profile.namespaces.dependencies
            ingressClass = [string]$Profile.ingress.className
        }
    }
    if ($Suffix -notmatch '^[a-z0-9]([-a-z0-9]*[a-z0-9])?$') { throw "RunId must be a DNS label." }
    $ingressClass = "$($Profile.ingress.className)-$Stage-$Suffix"
    if ($ingressClass.Length -gt 63) { throw "Run-scoped IngressClass exceeds 63 characters: $ingressClass" }
    return @{
        control = "agentx-e2e-$Stage-control-$Suffix"
        runtime = "agentx-e2e-$Stage-runtime-$Suffix"
        observability = "agentx-e2e-$Stage-runtime-$Suffix"
        dependencies = "agentx-e2e-$Stage-deps-$Suffix"
        ingressClass = $ingressClass
    }
}

function Set-RunScopedSandboxAccess {
    param($Profile, [string]$Suffix, [string]$Stage)
    if (-not $Suffix -or [string]$Profile.network.egressGateway.sandboxAccess.mode -ne "nodePort") { return }
    $hash = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes("$Stage-$Suffix"))
    $nodePort = 32000 + ((([int]$hash[0] * 256) + [int]$hash[1]) % 768)
    $endpoint = [UriBuilder][string]$Profile.network.egressGateway.sandboxAccess.endpoint
    $endpoint.Port = $nodePort
    $Profile.network.egressGateway.sandboxAccess.endpoint = $endpoint.Uri.AbsoluteUri.TrimEnd('/')
}

function Assert-SafeNamespaces {
    param([hashtable]$Namespaces)
    $values = @($Namespaces.control, $Namespaces.runtime, $Namespaces.dependencies)
    if (($values | Select-Object -Unique).Count -ne 3) { throw "Physical namespaces must contain distinct control, runtime, and dependencies targets." }
    if ($Namespaces.observability -ne $Namespaces.runtime) { throw "Observability must share the Runtime namespace." }
    foreach ($name in $values) {
        if ($name.Length -gt 63 -or $name -notmatch '^agentx-(?!v2-)[a-z0-9-]+$') {
            throw "Refusing unsafe Namespace target (must use agentx- prefix and no v2 segment): $name"
        }
    }
}

function Assert-V2Isolation {
    param($Profile)
    $control = $Profile.components.controlMysql
    $runtime = $Profile.components.runtimeMysql
    if ($control.host -eq $runtime.host) { throw "Control and Runtime MySQL must use independent endpoints." }
    if ($Profile.ingress.controlHost -eq $Profile.ingress.runtimeHost) { throw "Control and Runtime hosts must be independent." }
    if ($Profile.environment -eq "production" -and (-not $Profile.ingress.controlTlsSecretName -or -not $Profile.ingress.runtimeTlsSecretName)) { throw "Production V2 ingress requires TLS." }
    if ($Profile.environment -eq "production" -and -not [bool]$Profile.components.sandbox.secureAccess) { throw "Production V2 OpenSandbox requires secureAccess." }
    if ($control.appUser -eq $runtime.appUser -or $control.migrateUser -eq $runtime.migrateUser) { throw "Control and Runtime MySQL users must be distinct." }
    if ($control.appUser -eq $control.migrateUser -or $runtime.appUser -eq $runtime.migrateUser) { throw "Application and Migration MySQL users must be distinct." }
    foreach ($mysql in @($control, $runtime)) {
        if ([int64]$mysql.maxConnections * 100 -gt [int64]$mysql.serverMaxConnections * 70) {
            throw "MySQL pool budget must not exceed 70% of serverMaxConnections."
        }
    }
    foreach ($service in @($Profile.services.webConsole, $Profile.services.platformControl, $Profile.services.runtimeGateway, $Profile.services.workflowRuntime, $Profile.services.workflowWorker, $Profile.services.sandboxManager, $Profile.services.egressGateway, $Profile.services.observability)) {
        if ([int64]$service.replicas -gt [int64]$service.maxReplicas) { throw "Service replicas must not exceed maxReplicas." }
    }
    $controlPool = [int64]$Profile.services.platformControl.maxReplicas * [int64]$Profile.services.platformControl.mysqlPool
    $runtimePool = ([int64]$Profile.services.runtimeGateway.maxReplicas * [int64]$Profile.services.runtimeGateway.mysqlPool) +
        ([int64]$Profile.services.workflowRuntime.maxReplicas * [int64]$Profile.services.workflowRuntime.mysqlPool) +
        ([int64]$Profile.services.workflowWorker.maxReplicas * [int64]$Profile.services.workflowWorker.mysqlPool) +
        ([int64]$Profile.services.sandboxManager.maxReplicas * [int64]$Profile.services.sandboxManager.mysqlPool)
    if ($controlPool * 100 -gt [int64]$control.serverMaxConnections * 70) { throw "Control service pool budget exceeds 70%." }
    if ($runtimePool * 100 -gt [int64]$runtime.serverMaxConnections * 70) { throw "Runtime service pool budget exceeds 70%." }
    $buckets = @($Profile.components.objectStorage.domains.control.bucket, $Profile.components.objectStorage.domains.runtime.bucket, $Profile.components.objectStorage.domains.observability.bucket)
    if (($buckets | Select-Object -Unique).Count -ne 3) { throw "Control, Runtime and Observability buckets must be distinct." }
    $objectUsers = @($Profile.components.objectStorage.domains.control.user, $Profile.components.objectStorage.domains.runtime.user, $Profile.components.objectStorage.domains.observability.user)
    if (($objectUsers | Select-Object -Unique).Count -ne 3) { throw "Object storage domain users must be distinct." }
    if ($Profile.components.clickhouse.queryUser -eq $Profile.components.clickhouse.migrateUser) { throw "ClickHouse Query and Migration users must be distinct." }
    $clickhouseUsers = @($Profile.components.clickhouse.queryUser, $Profile.components.clickhouse.consumerUser, $Profile.components.clickhouse.migrateUser)
    if (($clickhouseUsers | Select-Object -Unique).Count -ne 3) { throw "ClickHouse Query, Consumer and Migration users must be distinct." }
}

function Test-InternalLoadBalancerAnnotation {
    param($Annotations)
    if (-not $Annotations) { return $false }
    $supported = @{
        "service.beta.kubernetes.io/aws-load-balancer-internal" = @("true")
        "service.beta.kubernetes.io/aws-load-balancer-scheme" = @("internal")
        "service.beta.kubernetes.io/azure-load-balancer-internal" = @("true")
        "networking.gke.io/load-balancer-type" = @("internal")
        "cloud.google.com/load-balancer-type" = @("internal")
    }
    foreach ($property in @($Annotations.PSObject.Properties)) {
        if ($supported.ContainsKey($property.Name) -and
            @($supported[$property.Name]) -contains ([string]$property.Value).Trim().ToLowerInvariant()) {
            return $true
        }
    }
    return $false
}

function Assert-ProductionProfile {
    param($Profile)
    if ($Profile.environment -ne "production") { return }
    if (-not $Profile.namespaces.dependencies) { throw "Production requires the dependencies Namespace for the managed ingress controller." }
    if ($Profile.secrets.mode -ne "existing-kubernetes") { throw "Production requires secrets.mode=existing-kubernetes." }
    if ($Profile.components.controlMysql.mode -ne "external" -or $Profile.components.runtimeMysql.mode -ne "external" -or
        $Profile.components.runtimeRedis.mode -ne "external" -or $Profile.components.clickhouse.mode -ne "external" -or
        $Profile.components.objectStorage.mode -ne "external-s3") {
        throw "Production requires external MySQL, Redis, S3, and ClickHouse."
    }
    foreach ($mysql in @($Profile.components.controlMysql, $Profile.components.runtimeMysql)) {
        if ($mysql.tlsMode -ne "verify_identity" -or -not $mysql.caSecretName) { throw "Production MySQL requires verify_identity and a CA Secret." }
    }
    if (-not ([string]$Profile.components.runtimeRedis.url).StartsWith("rediss://") -or -not $Profile.components.runtimeRedis.caSecretName) { throw "Production Redis requires rediss and a CA Secret." }
    foreach ($endpoint in @($Profile.components.objectStorage.endpoint, $Profile.components.secretProvider.endpoint, $Profile.components.sandbox.endpoint, $Profile.components.clickhouse.url)) {
        if (-not ([string]$endpoint).StartsWith("https://")) { throw "Production external endpoints must use HTTPS." }
    }
    foreach ($ca in @($Profile.components.objectStorage.caSecretName, $Profile.components.secretProvider.caSecretName, $Profile.components.sandbox.caSecretName, $Profile.components.clickhouse.caSecretName)) {
        if (-not $ca) { throw "Every production external endpoint requires a CA Secret." }
    }
    if ([bool]$Profile.components.objectStorage.allowHttp -or -not [bool]$Profile.components.sandbox.secureAccess) { throw "Production S3 HTTP and insecure OpenSandbox access are forbidden." }
    if (-not $Profile.network.externalEgress -or -not $Profile.backup) { throw "Production requires external Egress and backup policy declarations." }
    $requiredEgress = @("controlMysql", "runtimeMysql", "runtimeRedis", "objectStorage", "vault", "opensandbox", "clickhouse")
    $legacyProviderTargets = @($Profile.network.externalEgress.PSObject.Properties.Name | Where-Object { $_ -like "provider*" })
    if ($legacyProviderTargets.Count -gt 0) { throw "externalEgress provider* targets were removed in v2alpha3; public providers must use agentx-egress-gateway." }
    foreach ($name in $requiredEgress) {
        $entry = $Profile.network.externalEgress.PSObject.Properties[$name].Value
        if (-not $entry) { throw "Production external Egress target '$name' is missing." }
        foreach ($cidr in @($entry.cidrs)) { if ($cidr -in @("0.0.0.0/0", "::/0")) { throw "Broad production Egress CIDR is forbidden." } }
    }
    $sandboxAccess = $Profile.network.egressGateway.sandboxAccess
    if (@($Profile.network.egressGateway.allowedPublicPorts) -notcontains 443) { throw "Egress Gateway public ports must contain 443." }
    if ($Profile.environment -eq "production") {
        if ($sandboxAccess.mode -eq "nodePort") { throw "Production Sandbox egress cannot use NodePort." }
        if ($sandboxAccess.mode -eq "privateLoadBalancer" -and
            (@($sandboxAccess.sourceCidrs).Count -eq 0 -or -not (Test-InternalLoadBalancerAnnotation $sandboxAccess.serviceAnnotations))) {
            throw "Production privateLoadBalancer requires source CIDRs and a supported internal load balancer annotation for AWS, Azure, or GCP."
        }
        if (-not $sandboxAccess.tlsSecretName) { throw "Production Sandbox egress requires a TLS Secret." }
    }
    $requiredImages = @("agentx-migrate", "agentx-bootstrap", "agentx-doctor", "platform-control", "web-console", "runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability")
    foreach ($name in $requiredImages) {
        $digest = [string]$Profile.images.digests.PSObject.Properties[$name].Value
        if ($digest -notmatch '^sha256:[0-9a-f]{64}$') { throw "Production image '$name' must have an immutable digest." }
    }
    $requiredSecrets = @("platformControl", "controlMigration", "runtimeGateway", "workflowRuntime", "workflowWorker", "sandboxManager", "egressGateway", "runtimeMigration", "observability", "observabilityMigration", "controlBackup", "runtimeBackup", "observabilityBackup")
    foreach ($name in $requiredSecrets) { if (-not $Profile.secrets.workloads.PSObject.Properties[$name].Value) { throw "Production workload Secret '$name' is missing." } }
}

function Get-TargetPlanes {
    param([string]$SelectedTarget, $Profile)
    switch ($SelectedTarget) {
        "Control" { return @("control") }
        "Runtime" { return @("runtime") }
        "Observability" { return @("observability") }
        "Dependencies" {
            return @("dependencies")
        }
        default {
            $planes = @("control", "runtime", "observability")
            $planes += "dependencies"
            return $planes
        }
    }
}

function Get-ImageReference {
    param($Profile, [string]$Name)
    if ($Profile.environment -eq "production") {
        return "$($Profile.images.registry)/$Name@$($Profile.images.digests.PSObject.Properties[$Name].Value)"
    }
    return "$($Profile.images.registry)/$Name`:$($Profile.images.tag)"
}

function Set-RenderedEnvValue {
    param([string]$Document, [string]$Name, [string]$Value)
    $yamlString = ConvertTo-Json -InputObject $Value -Compress
    return [regex]::Replace($Document, "(?ms)(- name: $([regex]::Escape($Name))\r?\n\s+value: )[^\r\n]+", { param($match) "$($match.Groups[1].Value)$yamlString" })
}

function Add-ProjectedCaBundle {
    param([string]$Document, [object[]]$Entries)
    if ($Entries.Count -eq 0) { return $Document }
    $sources = [Collections.Generic.List[string]]::new()
    $envLines = [Collections.Generic.List[string]]::new()
    foreach ($entry in $Entries) {
        $sources.Add("          - secret:`n              name: $($entry.secret)`n              items:`n                - key: ca.crt`n                  path: $($entry.file)")
        $envLines.Add("        - name: $($entry.env)`n          value: /etc/agentx-ca/$($entry.file)")
    }
    $volumeEntry = "      - name: external-ca`n        projected:`n          sources:`n$($sources -join "`n")`n"
    if ($Document -match '(?m)^      volumes:\s*$') {
        $Document = [regex]::Replace($Document, '(?m)^      volumes:\s*$', "      volumes:`n$volumeEntry", 1)
    } else {
        $Document = [regex]::Replace($Document, '(?m)^      serviceAccountName:', "      volumes:`n$volumeEntry      serviceAccountName:", 1)
    }
    $beforeEnv = $Document
    $Document = [regex]::Replace($Document, '(?m)^        env:\s*$', "        env:`n$($envLines -join "`n")", 1)
    if ($Document -eq $beforeEnv) {
        $Document = [regex]::Replace($Document, '(?m)^      - env:\s*$', "      - env:`n$($envLines -join "`n")", 1)
    }
    $mountEntry = "        - name: external-ca`n          mountPath: /etc/agentx-ca`n          readOnly: true`n"
    if ($Document -match '(?m)^        volumeMounts:\s*$') {
        $Document = [regex]::Replace($Document, '(?m)^        volumeMounts:\s*$', "        volumeMounts:`n$mountEntry", 1)
    } else {
        $mount = "        volumeMounts:`n$mountEntry"
        $Document = [regex]::Replace($Document, '(?m)^        ports:', "$mount        ports:", 1)
        if ($Document -notmatch '(?m)^        ports:') {
            $Document = [regex]::Replace($Document, '(?m)^        image:', "$mount        image:", 1)
        }
    }
    return $Document
}

function Import-ReleaseImages {
    param($Profile, [string]$ManifestPath)
    if (-not $ManifestPath) { return }
    $resolved = if ([IO.Path]::IsPathRooted($ManifestPath)) { $ManifestPath } else { Join-Path $root $ManifestPath }
    $manifestText = Get-Content -Raw -LiteralPath $resolved
    $schema = Join-Path $root "deploy/release/v2-release-manifest.schema.json"
    if (Get-Command Test-Json -ErrorAction SilentlyContinue) {
        if (-not ($manifestText | Test-Json -SchemaFile $schema)) { throw "Release Manifest does not match agentx.io/v2-release/v1." }
    }
    $manifest = $manifestText | ConvertFrom-Json
    $requiredImages = @("agentx-migrate", "agentx-bootstrap", "agentx-doctor", "platform-control", "web-console", "runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability")
    $names = @($manifest.images | ForEach-Object { [string]$_.name })
    if (@($names | Select-Object -Unique).Count -ne $requiredImages.Count -or @($requiredImages | Where-Object { $_ -notin $names }).Count -gt 0) {
        throw "Release Manifest must contain every V2 image exactly once."
    }
    foreach ($image in @($manifest.images)) {
        $name = [string]$image.name
        if (-not $name) { $name = [string]$image.service }
        $digest = [string]$image.digest
        if ($name -and $digest -match '^sha256:[0-9a-f]{64}$' -and $Profile.images.digests.PSObject.Properties[$name]) {
            $Profile.images.digests.PSObject.Properties[$name].Value = $digest
        }
    }
}

function Get-ReleaseDescriptor {
    param($Profile, [string]$ManifestPath, [string]$AppliedAction)
    $descriptor = [ordered]@{
        version = [string]$Profile.images.tag
        gitCommit = $null
        manifestSha256 = $null
        protocolVersion = 1
        compatibleProtocolVersions = @(1)
        action = $AppliedAction.ToLowerInvariant()
        images = [ordered]@{}
    }
    if ($ManifestPath) {
        $resolved = if ([IO.Path]::IsPathRooted($ManifestPath)) { $ManifestPath } else { Join-Path $root $ManifestPath }
        $manifest = Get-Content -Raw -LiteralPath $resolved | ConvertFrom-Json
        if ($manifest.version) { $descriptor.version = [string]$manifest.version }
        if ($manifest.gitCommit) { $descriptor.gitCommit = [string]$manifest.gitCommit }
        if ($manifest.protocolVersion) { $descriptor.protocolVersion = [int]$manifest.protocolVersion }
        if ($manifest.compatibleProtocolVersions) { $descriptor.compatibleProtocolVersions = @($manifest.compatibleProtocolVersions | ForEach-Object { [int]$_ }) }
        $descriptor.manifestSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToLowerInvariant()
    }
    foreach ($property in $Profile.images.digests.PSObject.Properties) {
        $descriptor.images[$property.Name] = [string]$property.Value
    }
    return $descriptor
}

function Set-ReleaseState {
    param([string[]]$Planes, [hashtable]$Namespaces, $Descriptor)
    $releaseJson = $Descriptor | ConvertTo-Json -Depth 10 -Compress
    foreach ($plane in $Planes | Where-Object { $_ -ne "dependencies" }) {
        $manifest = @{
            apiVersion = "v1"
            kind = "ConfigMap"
            metadata = @{
                name = "agentx-v2-release-state-$plane"
                namespace = [string]$Namespaces[$plane]
                labels = @{ "agentx.io/managed-by" = "agentx-v2-deploy"; "agentx.io/plane" = $plane }
            }
            data = @{
                release = $releaseJson
                appliedAt = [DateTimeOffset]::UtcNow.ToString("O")
            }
        } | ConvertTo-Json -Depth 20 -Compress
        Invoke-KubectlInput $manifest @("apply", "-f", "-")
    }
}

function Get-ExternalDependencyStatus {
    param([string]$Plane, $Profile, [hashtable]$Namespaces)
    $definitions = switch ($Plane) {
        "control" { @(
            @{ name = "control-mysql"; endpoint = "$($Profile.components.controlMysql.host):$($Profile.components.controlMysql.port)"; ca = $Profile.components.controlMysql.caSecretName },
            @{ name = "control-objects"; endpoint = $Profile.components.objectStorage.endpoint; ca = $Profile.components.objectStorage.caSecretName },
            @{ name = "vault"; endpoint = $Profile.components.secretProvider.endpoint; ca = $Profile.components.secretProvider.caSecretName },
            @{ name = "runtime-internal-api"; endpoint = "cluster-internal"; ca = $null },
            @{ name = "observability-internal-api"; endpoint = "cluster-internal"; ca = $null }
        ) }
        "runtime" { @(
            @{ name = "runtime-mysql"; endpoint = "$($Profile.components.runtimeMysql.host):$($Profile.components.runtimeMysql.port)"; ca = $Profile.components.runtimeMysql.caSecretName },
            @{ name = "runtime-redis"; endpoint = $Profile.components.runtimeRedis.url; ca = $Profile.components.runtimeRedis.caSecretName },
            @{ name = "runtime-objects"; endpoint = $Profile.components.objectStorage.endpoint; ca = $Profile.components.objectStorage.caSecretName },
            @{ name = "vault"; endpoint = $Profile.components.secretProvider.endpoint; ca = $Profile.components.secretProvider.caSecretName },
            @{ name = "opensandbox"; endpoint = $Profile.components.sandbox.endpoint; ca = $Profile.components.sandbox.caSecretName }
        ) }
        "observability" { @(
            @{ name = "runtime-trace-redis"; endpoint = $Profile.components.runtimeRedis.url; ca = $Profile.components.runtimeRedis.caSecretName },
            @{ name = "clickhouse"; endpoint = $Profile.components.clickhouse.url; ca = $Profile.components.clickhouse.caSecretName },
            @{ name = "observability-objects"; endpoint = $Profile.components.objectStorage.endpoint; ca = $Profile.components.objectStorage.caSecretName }
        ) }
    }
    $result = @()
    foreach ($definition in $definitions) {
        $caPresent = $true
        if ($definition.ca) {
            & kubectl -n $Namespaces[$Plane] get secret ([string]$definition.ca) -o name 2>$null | Out-Null
            $caPresent = $LASTEXITCODE -eq 0
        }
        $result += [ordered]@{
            name = $definition.name
            endpoint = [string](Resolve-Endpoint ([string]$definition.endpoint) $Profile $Namespaces)
            caSecret = if ($definition.ca) { [string]$definition.ca } else { $null }
            configured = [bool]$definition.endpoint
            caPresent = $caPresent
        }
    }
    return $result
}

function Assert-ExistingSecrets {
    param([string[]]$Planes, $Profile, [hashtable]$Namespaces)
    if ($Profile.secrets.mode -ne "existing-kubernetes") { return }
    $fields = @{
        control = @("platformControl", "controlMigration", "controlBackup")
        runtime = @("runtimeGateway", "workflowRuntime", "workflowWorker", "sandboxManager", "runtimeMigration", "runtimeBackup")
        observability = @("observability", "observabilityMigration", "observabilityBackup")
        dependencies = @("egressGateway")
    }
    foreach ($plane in $Planes) {
        foreach ($field in $fields[$plane]) {
            $name = [string]$Profile.secrets.workloads.PSObject.Properties[$field].Value
            & kubectl -n $Namespaces[$plane] get secret $name -o name | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Required production Secret $($Namespaces[$plane])/$name does not exist." }
        }
    }

    $requiredKeys = @(
        @{ namespace = $Namespaces.runtime; secret = [string]$Profile.secrets.workloads.runtimeGateway; key = "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM" },
        @{ namespace = $Namespaces.runtime; secret = [string]$Profile.secrets.workloads.workflowRuntime; key = "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM" },
        @{ namespace = $Namespaces.runtime; secret = [string]$Profile.secrets.workloads.workflowWorker; key = "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM" },
        @{ namespace = $Namespaces.runtime; secret = [string]$Profile.secrets.workloads.sandboxManager; key = "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM" },
        @{ namespace = $Namespaces.dependencies; secret = [string]$Profile.secrets.workloads.egressGateway; key = "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON" },
        @{ namespace = $Namespaces.dependencies; secret = [string]$Profile.network.egressGateway.sandboxAccess.tlsSecretName; key = "tls.crt" },
        @{ namespace = $Namespaces.dependencies; secret = [string]$Profile.network.egressGateway.sandboxAccess.tlsSecretName; key = "tls.key" }
    )
    if ($Profile.network.egressGateway.sandboxAccess.caSecretName) {
        $requiredKeys += @{ namespace = $Namespaces.runtime; secret = [string]$Profile.network.egressGateway.sandboxAccess.caSecretName; key = "ca.crt" }
    }
    foreach ($requirement in $requiredKeys) {
        $payload = (& kubectl -n $requirement.namespace get secret $requirement.secret -o json 2>$null) -join "`n"
        if (-not $payload) { throw "Required production Secret $($requirement.namespace)/$($requirement.secret) does not exist." }
        $secret = $payload | ConvertFrom-Json
        if (-not $secret.data.PSObject.Properties[$requirement.key]) {
            throw "Required production Secret $($requirement.namespace)/$($requirement.secret) is missing key $($requirement.key)."
        }
    }
}

function Replace-ProfileValues {
    param(
        [string]$Manifest,
        $Profile,
        [hashtable]$Namespaces,
        [string[]]$PreserveReplicaWorkloadNames = @()
    )
    $result = $Manifest
    $canonicalNamespaces = @{
        control = "agentx-control"
        runtime = "agentx-runtime"
        observability = "agentx-runtime"
        dependencies = "agentx-deps"
    }
    foreach ($plane in @("control", "runtime", "observability", "dependencies")) {
        $sourceNamespace = [string]$Profile.namespaces.$plane
        $targetNamespace = [string]$Namespaces[$plane]
        if (-not $targetNamespace) { continue }
        if ($sourceNamespace) { $result = $result.Replace($sourceNamespace, $targetNamespace) }
        $result = $result.Replace([string]$canonicalNamespaces[$plane], $targetNamespace)
    }
    $control = $Profile.components.controlMysql
    $runtime = $Profile.components.runtimeMysql
    $clickhouse = $Profile.components.clickhouse
    $secretProvider = $Profile.components.secretProvider
    $controlOriginScheme = if ($Profile.ingress.controlTlsSecretName) { "https" } else { "http" }
    $runtimeOriginScheme = if ($Profile.ingress.runtimeTlsSecretName) { "https" } else { "http" }
    $controlOrigin = "$controlOriginScheme`://$($Profile.ingress.controlHost)"
    $runtimeOrigin = "$runtimeOriginScheme`://$($Profile.ingress.runtimeHost)"
    $gatewayAllowedOrigins = if ($Profile.environment -in @("local", "test")) {
        "$controlOrigin,http://127.0.0.1:18081"
    } else {
        $controlOrigin
    }
    $result = $result.Replace("value: control-mysql", "value: $(Resolve-Endpoint ([string]$control.host) $Profile $Namespaces)")
    $result = $result.Replace("value: runtime-mysql", "value: $(Resolve-Endpoint ([string]$runtime.host) $Profile $Namespaces)")
    $result = $result.Replace("value: redis://runtime-redis:6379/", "value: $(Resolve-Endpoint ([string]$Profile.components.runtimeRedis.url) $Profile $Namespaces)")
    $result = $result.Replace("value: redis://runtime-redis.$($Namespaces.runtime).svc:6379/", "value: $(Resolve-Endpoint ([string]$Profile.components.runtimeRedis.url) $Profile $Namespaces)")
    $result = $result.Replace("value: http://clickhouse:8123", "value: $(Resolve-Endpoint ([string]$clickhouse.url) $Profile $Namespaces)")
    $result = $result.Replace("value: http://object-storage.agentx-deps.svc:9000", "value: $(Resolve-Endpoint ([string]$Profile.components.objectStorage.endpoint) $Profile $Namespaces)")
    $result = $result.Replace("value: http://vault.agentx-deps.svc:8200", "value: $(Resolve-Endpoint ([string]$secretProvider.endpoint) $Profile $Namespaces)")
    $result = $result.Replace("value: http://opensandbox.agentx-deps.svc:8080", "value: $(Resolve-Endpoint ([string]$Profile.components.sandbox.endpoint) $Profile $Namespaces)")
    $result = $result.Replace("agentx_control", [string]$control.database)
    $result = $result.Replace("control_app", [string]$control.appUser)
    $result = $result.Replace("control_migrate", [string]$control.migrateUser)
    $result = $result.Replace("agentx_runtime", [string]$runtime.database)
    $result = $result.Replace("runtime_app", [string]$runtime.appUser)
    $result = $result.Replace("runtime_migrate", [string]$runtime.migrateUser)
    $result = $result.Replace("agentx_observability", [string]$clickhouse.database)
    $result = $result.Replace("observability_query", [string]$clickhouse.queryUser)
    $result = $result.Replace("observability_consumer", [string]$clickhouse.consumerUser)
    $result = $result.Replace("observability_migrate", [string]$clickhouse.migrateUser)
    $result = $result.Replace("ingressClassName: agentx-nginx", "ingressClassName: $($Namespaces.ingressClass)")
    $result = $result.Replace("host: agentx.localhost", "host: $($Profile.ingress.controlHost)")
    $result = $result.Replace("host: run.agentx.localhost", "host: $($Profile.ingress.runtimeHost)")
    $result = $result.Replace('nginx.ingress.kubernetes.io/cors-allow-origin: "http://agentx.localhost"', "nginx.ingress.kubernetes.io/cors-allow-origin: `"$gatewayAllowedOrigins`"")
    $result = $result.Replace("nginx.ingress.kubernetes.io/cors-allow-origin: http://agentx.localhost", "nginx.ingress.kubernetes.io/cors-allow-origin: $gatewayAllowedOrigins")
    $result = $result.Replace("value: http://run.agentx.localhost", "value: $runtimeOrigin")
    $result = $result.Replace("value: https://host.docker.internal:31429", "value: $([string]$Profile.network.egressGateway.sandboxAccess.endpoint)")
    if ($Namespaces.dependencies) { $result = $result.Replace("value: http://vault.$($Namespaces.dependencies).svc:8200", "value: $([string](Resolve-Endpoint $secretProvider.endpoint $Profile $Namespaces))") }
    $result = $result.Replace("value: secret", "value: $([string]$secretProvider.mount)")
    if ($Namespaces.dependencies) { $result = $result.Replace("value: http://opensandbox.$($Namespaces.dependencies).svc:8080", "value: $([string](Resolve-Endpoint $Profile.components.sandbox.endpoint $Profile $Namespaces))") }
    $sandboxSecureAccess = ([bool]$Profile.components.sandbox.secureAccess).ToString().ToLowerInvariant()
    $result = [regex]::Replace(
        $result,
        '(?m)(name: AGENTX_OPENSANDBOX_SECURE_ACCESS\r?\n\s+value:\s+)"?true"?',
        { param($match) "$($match.Groups[1].Value)`"$sandboxSecureAccess`"" }
    )
    $documents = $result -split "(?m)^---\r?$"
    for ($documentIndex = 0; $documentIndex -lt $documents.Count; $documentIndex++) {
        $document = $documents[$documentIndex]
        $resourceName = Get-YamlResourceName $document
        $resourceKind = Get-YamlResourceKind $document
        if ($resourceName -eq "agentx-egress-gateway" -and $resourceKind -eq "Deployment") {
            $document = Set-RenderedEnvValue $document "AGENTX_EGRESS_ALLOWED_PUBLIC_PORTS" (@($Profile.network.egressGateway.allowedPublicPorts) -join ',')
            $allowDockerDesktopDns = ($Profile.environment -in @("local", "test")).ToString().ToLowerInvariant()
            $document = Set-RenderedEnvValue $document "AGENTX_EGRESS_ALLOW_DOCKER_DESKTOP_DNS" $allowDockerDesktopDns
            $document = $document.Replace("secretName: agentx-egress-tls", "secretName: $([string]$Profile.network.egressGateway.sandboxAccess.tlsSecretName)")
        }
        if ($resourceName -eq "sandbox-manager" -and $resourceKind -eq "Deployment") {
            $document = Set-RenderedEnvValue $document "AGENTX_EGRESS_SANDBOX_PROXY_URL" ([string]$Profile.network.egressGateway.sandboxAccess.endpoint)
            $caSecretName = [string]$Profile.network.egressGateway.sandboxAccess.caSecretName
            if ($caSecretName) {
                $document = $document.Replace("secretName: agentx-egress-tls", "secretName: $caSecretName")
            } else {
                $document = [regex]::Replace($document, '(?ms)^\s*- \{ name: AGENTX_EGRESS_SANDBOX_CA_PATH, value: [^\r\n]+\}\r?\n', '')
                $document = [regex]::Replace($document, '(?ms)^\s*volumeMounts:\r?\n\s*- \{ name: egress-ca[^\r\n]+\}\r?\n', '')
                $document = [regex]::Replace($document, '(?ms)^\s*volumes:\r?\n\s*- name: egress-ca\r?\n\s*secret: \{ secretName: agentx-egress-tls, optional: true \}\r?\n', '')
            }
        }
        if ($resourceName -eq "agentx-egress-sandbox" -and $resourceKind -eq "Service") {
            $sandboxAccess = $Profile.network.egressGateway.sandboxAccess
            $endpointUri = [Uri][string]$sandboxAccess.endpoint
            $serviceSpec = [ordered]@{
                selector = @{ "app.kubernetes.io/name" = "agentx-egress-gateway" }
                ports = @(@{ name = "sandbox-proxy"; port = $endpointUri.Port; targetPort = "sandbox-proxy" })
            }
            switch ([string]$sandboxAccess.mode) {
                "nodePort" {
                    if ($endpointUri.Port -lt 30000 -or $endpointUri.Port -gt 32767) { throw "Sandbox nodePort endpoint must use a port in 30000..32767." }
                    $serviceSpec.type = "NodePort"
                    $serviceSpec.ports[0].nodePort = $endpointUri.Port
                }
                "cluster" { $serviceSpec.type = "ClusterIP" }
                "privateLoadBalancer" {
                    $serviceSpec.type = "LoadBalancer"
                    $serviceSpec.loadBalancerSourceRanges = @($sandboxAccess.sourceCidrs)
                }
                default { throw "Unsupported Sandbox egress access mode $($sandboxAccess.mode)." }
            }
            $annotations = @{}
            foreach ($annotation in @($sandboxAccess.serviceAnnotations.PSObject.Properties)) { $annotations[$annotation.Name] = [string]$annotation.Value }
            $metadata = @{ name = "agentx-egress-sandbox"; namespace = [string]$Namespaces.dependencies }
            if ($annotations.Count -gt 0) { $metadata.annotations = $annotations }
            $document = @{ apiVersion = "v1"; kind = "Service"; metadata = $metadata; spec = $serviceSpec } | ConvertTo-Json -Depth 12
        }
        if ($resourceName -eq "agentx-egress-gateway-ingress" -and $resourceKind -eq "NetworkPolicy") {
            $sandboxPeers = [Collections.Generic.List[object]]::new()
            if ($Profile.network.egressGateway.sandboxAccess.mode -eq "cluster") {
                $sandboxPeers.Add(@{ namespaceSelector = @{ matchLabels = @{ "agentx.io/plane" = "dependencies" } } })
            }
            foreach ($cidr in @($Profile.network.egressGateway.sandboxAccess.sourceCidrs)) { $sandboxPeers.Add(@{ ipBlock = @{ cidr = [string]$cidr } }) }
            $document = @{
                apiVersion = "networking.k8s.io/v1"; kind = "NetworkPolicy"
                metadata = @{ name = "agentx-egress-gateway-ingress"; namespace = [string]$Namespaces.dependencies }
                spec = @{
                    podSelector = @{ matchLabels = @{ "app.kubernetes.io/name" = "agentx-egress-gateway" } }
                    policyTypes = @("Ingress")
                    ingress = @(
                        @{ from = @(@{ namespaceSelector = @{ matchLabels = @{ "agentx.io/plane" = "runtime" } }; podSelector = @{ matchLabels = @{ "agentx.io/egress-client" = "managed" } } }); ports = @(@{ protocol = "TCP"; port = 3128 }) },
                        @{ from = @($sandboxPeers); ports = @(@{ protocol = "TCP"; port = 3129 }) }
                    )
                }
            } | ConvertTo-Json -Depth 20
        }
        if ($resourceName -eq "agentx-egress-gateway-public-egress" -and $resourceKind -eq "NetworkPolicy") {
            $publicPorts = @($Profile.network.egressGateway.allowedPublicPorts | ForEach-Object { @{ protocol = "TCP"; port = [int]$_ } })
            $blockedV4 = @("0.0.0.0/8", "10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16", "172.16.0.0/12", "192.0.0.0/24", "192.168.0.0/16", "224.0.0.0/4", "240.0.0.0/4")
            if ($Profile.environment -eq "production") { $blockedV4 += "198.18.0.0/15" }
            $blockedV6 = @("::/128", "::1/128", "64:ff9b::/96", "100::/64", "2001::/23", "2001:db8::/32", "2002::/16", "3fff::/20", "fc00::/7", "fe80::/10", "ff00::/8")
            $document = @{
                apiVersion = "networking.k8s.io/v1"; kind = "NetworkPolicy"
                metadata = @{ name = "agentx-egress-gateway-public-egress"; namespace = [string]$Namespaces.dependencies }
                spec = @{
                    podSelector = @{ matchLabels = @{ "app.kubernetes.io/name" = "agentx-egress-gateway" } }
                    policyTypes = @("Egress")
                    egress = @(
                        @{ to = @(@{ namespaceSelector = @{}; podSelector = @{ matchLabels = @{ "k8s-app" = "kube-dns" } } }); ports = @(@{ protocol = "UDP"; port = 53 }, @{ protocol = "TCP"; port = 53 }) },
                        @{ to = @(@{ ipBlock = @{ cidr = "0.0.0.0/0"; except = $blockedV4 } }, @{ ipBlock = @{ cidr = "::/0"; except = $blockedV6 } }); ports = $publicPorts }
                    )
                }
            } | ConvertTo-Json -Depth 20
        }
        if ($Profile.environment -eq "production" -and $document -match '(?m)^kind: Namespace$') {
            $document = [regex]::Replace($document, '(?m)^(\s+)agentx\.io/plane: (control|runtime|observability)\s*$', { param($match) "$($match.Groups[1].Value)agentx.io/plane: $($match.Groups[2].Value)`n$($match.Groups[1].Value)pod-security.kubernetes.io/enforce: restricted`n$($match.Groups[1].Value)pod-security.kubernetes.io/audit: restricted`n$($match.Groups[1].Value)pod-security.kubernetes.io/warn: restricted" })
        }
        if ($Profile.environment -eq "production") {
            if ($document -match '(?m)^kind: Job$') {
                $document = [regex]::Replace($document, '(?m)^      containers:', "      securityContext:`n        runAsNonRoot: true`n        seccompProfile:`n          type: RuntimeDefault`n      containers:", 1)
                $document = [regex]::Replace($document, '(?m)^        imagePullPolicy:', "        resources:`n          requests: { cpu: 25m, memory: 32Mi, ephemeral-storage: 32Mi }`n          limits: { cpu: 250m, memory: 128Mi, ephemeral-storage: 128Mi }`n        securityContext:`n          allowPrivilegeEscalation: false`n          readOnlyRootFilesystem: true`n          runAsNonRoot: true`n          capabilities: { drop: [ALL] }`n        imagePullPolicy:", 1)
            }
            if ($document -match 'AGENTX_CONTROL_MYSQL_') {
                $document = Set-RenderedEnvValue $document "AGENTX_CONTROL_MYSQL_TLS_MODE" ([string]$control.tlsMode)
                $document = Set-RenderedEnvValue $document "AGENTX_CONTROL_S3_ALLOW_HTTP" (([string]$Profile.components.objectStorage.allowHttp).ToLowerInvariant())
                $document = Set-RenderedEnvValue $document "AGENTX_CONTROL_S3_PATH_STYLE" (([string]$Profile.components.objectStorage.pathStyle).ToLowerInvariant())
            }
            if ($document -match 'AGENTX_RUNTIME_MYSQL_') {
                $document = Set-RenderedEnvValue $document "AGENTX_RUNTIME_MYSQL_TLS_MODE" ([string]$runtime.tlsMode)
                $document = Set-RenderedEnvValue $document "AGENTX_RUNTIME_S3_ALLOW_HTTP" (([string]$Profile.components.objectStorage.allowHttp).ToLowerInvariant())
                $document = Set-RenderedEnvValue $document "AGENTX_RUNTIME_S3_PATH_STYLE" (([string]$Profile.components.objectStorage.pathStyle).ToLowerInvariant())
            }
            if ($document -match 'AGENTX_OBSERVABILITY_S3_') {
                $document = Set-RenderedEnvValue $document "AGENTX_OBSERVABILITY_S3_ALLOW_HTTP" (([string]$Profile.components.objectStorage.allowHttp).ToLowerInvariant())
                $document = Set-RenderedEnvValue $document "AGENTX_OBSERVABILITY_S3_PATH_STYLE" (([string]$Profile.components.objectStorage.pathStyle).ToLowerInvariant())
            }
            $caEntries = switch ($resourceName) {
                "platform-control" { @(
                    @{ secret = $control.caSecretName; file = "mysql.pem"; env = "AGENTX_CONTROL_MYSQL_TLS_CA_PATH" },
                    @{ secret = $Profile.components.objectStorage.caSecretName; file = "s3.pem"; env = "AGENTX_CONTROL_S3_TLS_CA_PATH" },
                    @{ secret = $secretProvider.caSecretName; file = "vault.pem"; env = "AGENTX_CONTROL_VAULT_TLS_CA_PATH" }
                ) }
                "control-migrate" { @(@{ secret = $control.caSecretName; file = "mysql.pem"; env = "AGENTX_CONTROL_MYSQL_TLS_CA_PATH" }) }
                "runtime-gateway" { @(
                    @{ secret = $runtime.caSecretName; file = "mysql.pem"; env = "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" },
                    @{ secret = $Profile.components.runtimeRedis.caSecretName; file = "redis.pem"; env = "AGENTX_RUNTIME_REDIS_TLS_CA_PATH" },
                    @{ secret = $Profile.components.objectStorage.caSecretName; file = "s3.pem"; env = "AGENTX_RUNTIME_S3_TLS_CA_PATH" },
                    @{ secret = $secretProvider.caSecretName; file = "vault.pem"; env = "AGENTX_RUNTIME_VAULT_TLS_CA_PATH" }
                ) }
                "workflow-runtime" { @(
                    @{ secret = $runtime.caSecretName; file = "mysql.pem"; env = "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" },
                    @{ secret = $Profile.components.runtimeRedis.caSecretName; file = "redis.pem"; env = "AGENTX_RUNTIME_REDIS_TLS_CA_PATH" },
                    @{ secret = $Profile.components.objectStorage.caSecretName; file = "s3.pem"; env = "AGENTX_RUNTIME_S3_TLS_CA_PATH" }
                ) }
                "workflow-worker" { @(
                    @{ secret = $runtime.caSecretName; file = "mysql.pem"; env = "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" },
                    @{ secret = $Profile.components.runtimeRedis.caSecretName; file = "redis.pem"; env = "AGENTX_RUNTIME_REDIS_TLS_CA_PATH" },
                    @{ secret = $Profile.components.objectStorage.caSecretName; file = "s3.pem"; env = "AGENTX_RUNTIME_S3_TLS_CA_PATH" },
                    @{ secret = $secretProvider.caSecretName; file = "vault.pem"; env = "AGENTX_RUNTIME_VAULT_TLS_CA_PATH" }
                ) }
                "sandbox-manager" { @(
                    @{ secret = $runtime.caSecretName; file = "mysql.pem"; env = "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" },
                    @{ secret = $Profile.components.sandbox.caSecretName; file = "opensandbox.pem"; env = "AGENTX_OPENSANDBOX_TLS_CA_PATH" }
                ) }
                "runtime-migrate" { @(@{ secret = $runtime.caSecretName; file = "mysql.pem"; env = "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" }) }
                "observability" { @(
                    @{ secret = $Profile.components.clickhouse.caSecretName; file = "clickhouse.pem"; env = "SSL_CERT_FILE" },
                    @{ secret = $Profile.components.runtimeRedis.caSecretName; file = "redis.pem"; env = "AGENTX_OBSERVABILITY_REDIS_TLS_CA_PATH" }
                ) }
                "clickhouse-migrate" { @(@{ secret = $Profile.components.clickhouse.caSecretName; file = "clickhouse.pem"; env = "SSL_CERT_FILE" }) }
                default { @() }
            }
            if ($caEntries.Count -gt 0 -and $document -match '(?m)^kind: (Deployment|Job)$') { $document = Add-ProjectedCaBundle $document $caEntries }
        }
        if ($Profile.ingress.controlTlsSecretName -and $document -match "(?m)^  name: control-web$") {
            $tls = "  tls:`n  - hosts:`n    - $($Profile.ingress.controlHost)`n    secretName: $($Profile.ingress.controlTlsSecretName)"
            $document = $document.Replace("  tls: []", $tls)
        }
        if ($Profile.ingress.runtimeTlsSecretName -and $document -match "(?m)^  name: runtime-gateway$") {
            $tls = "  tls:`n  - hosts:`n    - $($Profile.ingress.runtimeHost)`n    secretName: $($Profile.ingress.runtimeTlsSecretName)"
            $document = $document.Replace("  tls: []", $tls)
        }
        if ($Profile.secrets.mode -eq "existing-kubernetes" -and $Profile.secrets.workloads) {
            $secretMap = @(
                @{ workload = "platform-control"; old = "agentx-control-secrets"; field = "platformControl" },
                @{ workload = "control-migrate"; old = "agentx-control-secrets"; field = "controlMigration" },
                @{ workload = "runtime-gateway"; old = "agentx-runtime-secrets"; field = "runtimeGateway" },
                @{ workload = "workflow-runtime"; old = "agentx-runtime-secrets"; field = "workflowRuntime" },
                @{ workload = "workflow-worker"; old = "agentx-runtime-secrets"; field = "workflowWorker" },
                @{ workload = "sandbox-manager"; old = "agentx-runtime-secrets"; field = "sandboxManager" },
                @{ workload = "agentx-egress-gateway"; old = "agentx-egress-gateway-secrets"; field = "egressGateway" },
                @{ workload = "runtime-migrate"; old = "agentx-runtime-secrets"; field = "runtimeMigration" },
                @{ workload = "observability"; old = "agentx-observability-secrets"; field = "observability" },
                @{ workload = "clickhouse-migrate"; old = "agentx-observability-secrets"; field = "observabilityMigration" }
            )
            foreach ($entry in $secretMap) {
                if ($document -match "(?m)^  name: $([regex]::Escape($entry.workload))$") {
                    $secretName = [string]$Profile.secrets.workloads.PSObject.Properties[$entry.field].Value
                    $document = $document.Replace($entry.old, $secretName)
                }
            }
        }
        foreach ($service in @(
            @{ name = "platform-control"; replicas = $Profile.services.platformControl.replicas },
            @{ name = "web-console"; replicas = $Profile.services.webConsole.replicas },
            @{ name = "runtime-gateway"; replicas = $Profile.services.runtimeGateway.replicas },
            @{ name = "workflow-runtime"; replicas = $Profile.services.workflowRuntime.replicas },
            @{ name = "workflow-worker"; replicas = $Profile.services.workflowWorker.replicas },
            @{ name = "sandbox-manager"; replicas = $Profile.services.sandboxManager.replicas },
            @{ name = "agentx-egress-gateway"; replicas = $Profile.services.egressGateway.replicas },
            @{ name = "observability"; replicas = $Profile.services.observability.replicas }
        )) {
            if ($document -match "(?m)^kind: Deployment$" -and $document -match "(?m)^  name: $([regex]::Escape($service.name))$") {
                $document = $document -replace "(?m)^  replicas: \d+$", "  replicas: $($service.replicas)"
                if ($PreserveReplicaWorkloadNames -contains $service.name) {
                    $document = $document -replace "(?m)^  replicas: \d+\r?\n", ""
                }
                switch ($service.name) {
                    "platform-control" {
                        $document = $document -replace '(AGENTX_CONTROL_MYSQL_MAX_CONNECTIONS, value: ")\d+(" \})', "`${1}$($Profile.services.platformControl.mysqlPool)`${2}"
                        $document = $document -replace '(AGENTX_CONTROL_ROLES, value: ")[^"]+(" \})', "`${1}$(@($Profile.services.platformControl.roles) -join ',')`${2}"
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_CONTROL_MYSQL_MAX_CONNECTIONS\r?\n\s*value: ).+$', ('${{1}}"{0}"' -f $Profile.services.platformControl.mysqlPool)
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_CONTROL_ROLES\r?\n\s*value: ).+$', "`${1}$(@($Profile.services.platformControl.roles) -join ',')"
                    }
                    "runtime-gateway" {
                        $document = $document -replace '(AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS, value: ")\d+(" \})', "`${1}$($Profile.services.runtimeGateway.mysqlPool)`${2}"
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS\r?\n\s*value: ).+$', ('${{1}}"{0}"' -f $Profile.services.runtimeGateway.mysqlPool)
                    }
                    "workflow-runtime" {
                        $document = $document -replace '(AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS, value: ")\d+(" \})', "`${1}$($Profile.services.workflowRuntime.mysqlPool)`${2}"
                        $document = $document -replace '(AGENTX_RUNTIME_ROLES, value: ")[^"]+(" \})', "`${1}$(@($Profile.services.workflowRuntime.roles) -join ',')`${2}"
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS\r?\n\s*value: ).+$', ('${{1}}"{0}"' -f $Profile.services.workflowRuntime.mysqlPool)
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_RUNTIME_ROLES\r?\n\s*value: ).+$', "`${1}$(@($Profile.services.workflowRuntime.roles) -join ',')"
                    }
                    "workflow-worker" {
                        $document = $document -replace '(AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS, value: ")\d+(" \})', "`${1}$($Profile.services.workflowWorker.mysqlPool)`${2}"
                        $document = $document -replace '(AGENTX_WORKER_CAPABILITIES, value: )[A-Za-z0-9_,\-]+( \})', "`${1}$($Profile.services.workflowWorker.capability)`${2}"
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS\r?\n\s*value: ).+$', ('${{1}}"{0}"' -f $Profile.services.workflowWorker.mysqlPool)
                        $document = $document -replace '(?m)^(\s*- name: AGENTX_WORKER_CAPABILITIES\r?\n\s*value: ).+$', "`${1}$($Profile.services.workflowWorker.capability)"
                    }
                    "sandbox-manager" {
                        $document = $document -replace '(AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS, value: ")\d+(" \})', "`${1}$($Profile.services.sandboxManager.mysqlPool)`${2}"
                    }
                    "observability" {
                        $document = $document -replace '(AGENTX_OBSERVABILITY_ROLES, value: ")[^"]+(" \})', "`${1}$(@($Profile.services.observability.roles) -join ',')`${2}"
                    }
                }
            }
        }
        $documents[$documentIndex] = $document
    }
    $result = $documents -join "`n---`n"
    $migrateImage = Get-ImageReference $Profile "agentx-migrate"
    $result = $result.Replace("image: agentx/agentx-migrate:dev", "image: $migrateImage")
    foreach ($image in @("platform-control", "web-console", "runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability")) {
        $imageReference = Get-ImageReference $Profile $image
        $result = $result.Replace("image: agentx/$image`:dev", "image: $imageReference")
    }
    $result = $result.Replace("imagePullPolicy: IfNotPresent", "imagePullPolicy: $($Profile.images.pullPolicy)")
    return $result
}

function Resolve-Endpoint {
    param([string]$Value, $Profile, [hashtable]$Namespaces)
    $result = $Value
    $canonicalNamespaces = @{
        control = "agentx-control"
        runtime = "agentx-runtime"
        observability = "agentx-runtime"
        dependencies = "agentx-deps"
    }
    foreach ($plane in @("control", "runtime", "observability", "dependencies")) {
        $sourceNamespace = [string]$Profile.namespaces.$plane
        $targetNamespace = [string]$Namespaces[$plane]
        if (-not $targetNamespace) { continue }
        if ($sourceNamespace) { $result = $result.Replace($sourceNamespace, $targetNamespace) }
        $result = $result.Replace([string]$canonicalNamespaces[$plane], $targetNamespace)
    }
    return $result
}

function Get-YamlResourceName {
    param([string]$Document)
    $inline = [regex]::Match($Document, '(?m)^metadata:\s*\{\s*name:\s*([^,}\s]+)')
    if ($inline.Success) { return $inline.Groups[1].Value }
    $block = [regex]::Match($Document, '(?ms)^metadata:\s*\r?\n(?:\s+.*\r?\n)*?\s+name:\s*([^\s]+)')
    if ($block.Success) { return $block.Groups[1].Value }
    return ""
}

function Get-YamlResourceKind {
    param([string]$Document)
    $match = [regex]::Match($Document, '(?m)^kind:\s*([^\s]+)')
    if ($match.Success) { return $match.Groups[1].Value }
    return ""
}

function Remove-BundledDocuments {
    param([string]$Manifest, $Profile)
    if ($Profile.environment -ne "production") { return $Manifest }
    $remove = @(
        "control-mysql", "control-mysql-init", "control-mysql-access", "control-egress",
        "runtime-mysql", "runtime-mysql-init", "runtime-redis", "runtime-data-access", "runtime-migration-data-access",
        "runtime-gateway-data-access", "workflow-worker-vault-egress", "runtime-mysql-ingress", "runtime-redis-ingress", "runtime-provider-egress",
        "clickhouse", "clickhouse-init", "observability-data-access", "observability-migration-data-access", "observability-ops-data-access", "clickhouse-ingress",
        "object-storage", "object-storage-bootstrap", "object-storage-ingress", "vault", "vault-bootstrap", "vault-ingress",
        "dependencies-egress", "runtime-provider-egress", "runtime-provider-ingress"
    )
    $kept = foreach ($document in ($Manifest -split '(?m)^---\s*$')) {
        if ((Get-YamlResourceName $document) -notin $remove) {
            [regex]::Replace($document, '(?ms)^      initContainers:\r?\n.*?(?=^      restartPolicy:)', '')
        }
    }
    return ($kept -join "---`n")
}

function New-ExternalEgressManifest {
    param([string]$Plane, $Profile, [hashtable]$Namespaces)
    if ($Profile.environment -ne "production") { return "" }
    $workloads = switch ($Plane) {
        "control" {
            @(
                @{ name = "platform-control"; targets = @("controlMysql", "objectStorage", "vault") },
                @{ name = "control-migrate"; targets = @("controlMysql") }
            )
        }
        "runtime" {
            @(
                @{ name = "runtime-gateway"; targets = @("runtimeMysql", "runtimeRedis", "objectStorage", "vault") },
                @{ name = "workflow-runtime"; targets = @("runtimeMysql", "runtimeRedis", "objectStorage", "vault") },
                @{ name = "workflow-worker"; targets = @("runtimeMysql", "runtimeRedis", "objectStorage", "vault") },
                @{ name = "sandbox-manager"; targets = @("runtimeMysql", "opensandbox") },
                @{ name = "runtime-migrate"; targets = @("runtimeMysql") }
            )
        }
        "observability" {
            @(
                @{ name = "observability"; targets = @("clickhouse", "runtimeRedis", "objectStorage") },
                @{ name = "clickhouse-migrate"; targets = @("clickhouse") }
            )
        }
        default { @() }
    }
    $documents = foreach ($workload in $workloads) {
        $egress = [Collections.Generic.List[object]]::new()
        $egress.Add(@{ to = @(@{ namespaceSelector = @{}; podSelector = @{ matchLabels = @{ "k8s-app" = "kube-dns" } } }); ports = @(@{ protocol = "UDP"; port = 53 }, @{ protocol = "TCP"; port = 53 }) })
        if ($Plane -eq "control" -and $workload.name -eq "platform-control") {
            $egress.Add(@{ to = @(@{ namespaceSelector = @{ matchLabels = @{ "agentx.io/plane" = "runtime" } }; podSelector = @{ matchLabels = @{ "agentx.io/internal-api" = "runtime-v1" } } }); ports = @(@{ protocol = "TCP"; port = 8080 }) })
            $egress.Add(@{ to = @(@{ namespaceSelector = @{ matchLabels = @{ "agentx.io/plane" = "runtime" } }; podSelector = @{ matchLabels = @{ "agentx.io/internal-api" = "observability-v1" } } }); ports = @(@{ protocol = "TCP"; port = 8080 }) })
        }
        foreach ($targetName in $workload.targets) {
            $target = $Profile.network.externalEgress.PSObject.Properties[$targetName].Value
            foreach ($cidr in @($target.cidrs)) {
                $ports = @($target.ports | ForEach-Object { @{ protocol = "TCP"; port = [int]$_ } })
                $egress.Add(@{ to = @(@{ ipBlock = @{ cidr = [string]$cidr } }); ports = $ports })
            }
        }
        @{
            apiVersion = "networking.k8s.io/v1"
            kind = "NetworkPolicy"
            metadata = @{ name = "$($workload.name)-external-egress"; namespace = [string]$Namespaces[$Plane]; labels = @{ "agentx.io/managed-by" = "agentx-v2-deploy" } }
            spec = @{ podSelector = @{ matchLabels = @{ "app.kubernetes.io/name" = $workload.name } }; policyTypes = @("Egress"); egress = @($egress) }
        } | ConvertTo-Json -Depth 20
    }
    return ($documents -join "`n---`n")
}

function Get-RenderedManifest {
    param([string[]]$Planes, $Profile, [hashtable]$Namespaces, [string[]]$PreserveReplicaWorkloadNames = @(), [switch]$IncludeIngressController)
    $parts = foreach ($plane in $Planes) {
        $planeParts = [Collections.Generic.List[string]]::new()
        $rendered = (& kubectl kustomize (Join-Path $root "deploy/k8s/v2/$plane")) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "V2 $plane Kustomize render failed." }
        $rendered = Replace-ProfileValues $rendered $Profile $Namespaces $PreserveReplicaWorkloadNames
        $rendered = Remove-BundledDocuments $rendered $Profile
        if ($rendered.Trim()) { $planeParts.Add($rendered) }
        $externalPolicy = New-ExternalEgressManifest $plane $Profile $Namespaces
        if ($externalPolicy) { $planeParts.Add($externalPolicy) }
        if ($plane -eq "dependencies" -and $IncludeIngressController) { $planeParts.Add((Get-IngressRenderedManifest $Profile $Namespaces)) }
        $planeParts -join "`n---`n"
    }
    return ($parts -join "`n---`n")
}

function Remove-LegacyAutoscalingResources {
    param([string[]]$Planes, [hashtable]$Namespaces)
    $cleanupAnnotationPrefix = "agentx.io/legacy-autoscaling-cleaned"
    $hpas = @{
        control = @("platform-control", "web-console")
        runtime = @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager")
        observability = @("observability")
    }
    foreach ($plane in @("control", "runtime", "observability") | Where-Object { $Planes -contains $_ }) {
        $cleanupAnnotation = "$cleanupAnnotationPrefix-$plane"
        $namespacePayload = (& kubectl get namespace $Namespaces[$plane] --ignore-not-found -o json 2>$null) -join "`n"
        $alreadyCleaned = $namespacePayload -and (($namespacePayload | ConvertFrom-Json).metadata.annotations.$cleanupAnnotation -eq "true")
        if ($alreadyCleaned) { continue }
        Invoke-Kubectl -Arguments (@("-n", $Namespaces[$plane], "delete", "hpa") + @($hpas[$plane]) + @("--ignore-not-found"))
        Invoke-Kubectl -Arguments @("annotate", "namespace", $Namespaces[$plane], "$cleanupAnnotation=true", "--overwrite")
    }
    if ($Planes -notcontains "dependencies" -or -not $Namespaces.dependencies) { return }

    $cleanupAnnotation = "$cleanupAnnotationPrefix-dependencies"
    $dependenciesPayload = (& kubectl get namespace $Namespaces.dependencies --ignore-not-found -o json 2>$null) -join "`n"
    $dependenciesCleaned = $dependenciesPayload -and (($dependenciesPayload | ConvertFrom-Json).metadata.annotations.$cleanupAnnotation -eq "true")
    if ($dependenciesCleaned) { return }

    Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "deployment", "prometheus", "prometheus-adapter", "metrics-server", "--ignore-not-found")
    Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "service", "prometheus", "prometheus-adapter", "metrics-server", "--ignore-not-found")
    Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "serviceaccount", "prometheus", "prometheus-adapter", "metrics-server", "--ignore-not-found")
    Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "configmap", "prometheus-config", "prometheus-adapter-config", "--ignore-not-found")
    Invoke-Kubectl -Arguments @("-n", $Namespaces.dependencies, "delete", "networkpolicy", "prometheus-egress", "prometheus-ingress", "prometheus-adapter-access", "metrics-server-access", "--ignore-not-found")

    $legacyApis = @(
        @{
            apiService = "v1beta1.external.metrics.k8s.io"
            clusterRole = "agentx-v2-prometheus-adapter"
            clusterRoleBindings = @("agentx-v2-prometheus-adapter", "agentx-v2-prometheus-adapter-auth-delegator")
            authReader = "agentx-v2-prometheus-adapter-auth-reader"
        },
        @{
            apiService = "v1beta1.metrics.k8s.io"
            clusterRole = "agentx-v2-metrics-server"
            clusterRoleBindings = @("agentx-v2-metrics-server", "agentx-v2-metrics-server-auth-delegator")
            authReader = "agentx-v2-metrics-server-auth-reader"
        }
    )
    foreach ($legacy in $legacyApis) {
        $payload = (& kubectl get apiservice $legacy.apiService --ignore-not-found -o json 2>$null) -join "`n"
        if (-not $payload) { continue }
        $owner = (($payload | ConvertFrom-Json).metadata.annotations.'agentx.io/metrics-owner')
        if ($owner -ne $Namespaces.dependencies) { continue }
        Invoke-Kubectl -Arguments @("delete", "apiservice", $legacy.apiService, "--ignore-not-found")
        Invoke-Kubectl -Arguments @("delete", "clusterrole", $legacy.clusterRole, "--ignore-not-found")
        Invoke-Kubectl -Arguments (@("delete", "clusterrolebinding") + @($legacy.clusterRoleBindings) + @("--ignore-not-found"))
        Invoke-Kubectl -Arguments @("-n", "kube-system", "delete", "rolebinding", $legacy.authReader, "--ignore-not-found")
    }
    Invoke-Kubectl -Arguments @("annotate", "namespace", $Namespaces.dependencies, "$cleanupAnnotation=true", "--overwrite")
}

function New-EnvironmentVariable {
    param([string]$Name, [string]$Value = "", [string]$SecretName = "", [string]$SecretKey = "")
    if ($SecretName) {
        return @{ name = $Name; valueFrom = @{ secretKeyRef = @{ name = $SecretName; key = $SecretKey } } }
    }
    return @{ name = $Name; value = $Value }
}

function Get-OpsSecretName {
    param([string]$Plane, $Profile, [string]$Command)
    if ($Profile.secrets.mode -ne "existing-kubernetes") {
        if ($Plane -eq "control") { return "agentx-control-secrets" }
        if ($Plane -eq "runtime") { return "agentx-runtime-secrets" }
        return "agentx-observability-secrets"
    }
    if ($Plane -eq "control") { return [string]$Profile.secrets.workloads.controlMigration }
    if ($Plane -eq "runtime") { return [string]$Profile.secrets.workloads.runtimeMigration }
    return [string]$Profile.secrets.workloads.observabilityMigration
}

function Get-OpsEnvironment {
    param([string]$Plane, $Profile, [hashtable]$Namespaces, [string]$Command = "doctor")
    $object = $Profile.components.objectStorage
    $secretName = Get-OpsSecretName $Plane $Profile $Command
    switch ($Plane) {
        "control" {
            $mysql = $Profile.components.controlMysql
            $domain = $object.domains.control
            $caEnv = if ($Profile.environment -eq "production") { @(
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_TLS_CA_PATH" "/etc/agentx-ca/mysql.pem"),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_TLS_CA_PATH" "/etc/agentx-ca/s3.pem")
            ) } else { @() }
            return @($caEnv) + @(
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_HOST" (Resolve-Endpoint ([string]$mysql.host) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_PORT" ([string]$mysql.port)),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_DATABASE" ([string]$mysql.database)),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_USER" ([string]$mysql.appUser)),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_PASSWORD" "" $secretName "AGENTX_CONTROL_MYSQL_PASSWORD"),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_MAX_CONNECTIONS" ([string]$mysql.maxConnections)),
                (New-EnvironmentVariable "AGENTX_CONTROL_MYSQL_TLS_MODE" ([string]$mysql.tlsMode)),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_ENDPOINT" (Resolve-Endpoint ([string]$object.endpoint) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_BUCKET" ([string]$domain.bucket)),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_REGION" ([string]$object.region)),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_ACCESS_KEY" ([string]$domain.user)),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_SECRET_KEY" "" $secretName "AGENTX_CONTROL_S3_SECRET_KEY"),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_ALLOW_HTTP" ([string]$object.allowHttp).ToLowerInvariant()),
                (New-EnvironmentVariable "AGENTX_CONTROL_S3_PATH_STYLE" ([string]$object.pathStyle).ToLowerInvariant())
            )
        }
        "runtime" {
            $mysql = $Profile.components.runtimeMysql
            $redis = $Profile.components.runtimeRedis
            $domain = $object.domains.runtime
            $caEnv = if ($Profile.environment -eq "production") { @(
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_TLS_CA_PATH" "/etc/agentx-ca/mysql.pem"),
                (New-EnvironmentVariable "AGENTX_RUNTIME_REDIS_TLS_CA_PATH" "/etc/agentx-ca/redis.pem"),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_TLS_CA_PATH" "/etc/agentx-ca/s3.pem")
            ) } else { @() }
            return @($caEnv) + @(
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_HOST" (Resolve-Endpoint ([string]$mysql.host) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_PORT" ([string]$mysql.port)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_DATABASE" ([string]$mysql.database)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_USER" ([string]$mysql.appUser)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_PASSWORD" "" $secretName "AGENTX_RUNTIME_MYSQL_PASSWORD"),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS" ([string]$mysql.maxConnections)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_MYSQL_TLS_MODE" ([string]$mysql.tlsMode)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_REDIS_URL" (Resolve-Endpoint ([string]$redis.url) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_REDIS_PASSWORD" "" $secretName "AGENTX_RUNTIME_REDIS_PASSWORD"),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_ENDPOINT" (Resolve-Endpoint ([string]$object.endpoint) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_BUCKET" ([string]$domain.bucket)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_REGION" ([string]$object.region)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_ACCESS_KEY" ([string]$domain.user)),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_SECRET_KEY" "" $secretName "AGENTX_RUNTIME_S3_SECRET_KEY"),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_ALLOW_HTTP" ([string]$object.allowHttp).ToLowerInvariant()),
                (New-EnvironmentVariable "AGENTX_RUNTIME_S3_PATH_STYLE" ([string]$object.pathStyle).ToLowerInvariant())
            )
        }
        "observability" {
            $clickhouse = $Profile.components.clickhouse
            $domain = $object.domains.observability
            $caEnv = if ($Profile.environment -eq "production") { @(
                (New-EnvironmentVariable "SSL_CERT_FILE" "/etc/agentx-ca/clickhouse.pem")
            ) } else { @() }
            return @($caEnv) + @(
                (New-EnvironmentVariable "AGENTX_CLICKHOUSE_URL" (Resolve-Endpoint ([string]$clickhouse.url) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_CLICKHOUSE_DATABASE" ([string]$clickhouse.database)),
                (New-EnvironmentVariable "AGENTX_CLICKHOUSE_USER" ([string]$clickhouse.queryUser)),
                (New-EnvironmentVariable "AGENTX_CLICKHOUSE_PASSWORD" "" $secretName "AGENTX_CLICKHOUSE_QUERY_PASSWORD"),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_ENDPOINT" (Resolve-Endpoint ([string]$object.endpoint) $Profile $Namespaces)),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_BUCKET" ([string]$domain.bucket)),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_REGION" ([string]$object.region)),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_ACCESS_KEY" ([string]$domain.user)),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_SECRET_KEY" "" $secretName "AGENTX_OBSERVABILITY_S3_SECRET_KEY"),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_ALLOW_HTTP" ([string]$object.allowHttp).ToLowerInvariant()),
                (New-EnvironmentVariable "AGENTX_OBSERVABILITY_S3_PATH_STYLE" ([string]$object.pathStyle).ToLowerInvariant())
            )
        }
    }
}

function Invoke-OpsJob {
    param([string]$Command, [string]$Plane, [string]$Name, $Profile, [hashtable]$Namespaces)
    $namespace = [string]$Namespaces[$Plane]
    & kubectl -n $namespace delete job $Name --ignore-not-found --wait=true | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to delete prior Job $namespace/$Name." }
    $jobObject = @{
        apiVersion = "batch/v1"
        kind = "Job"
        metadata = @{ name = $Name; namespace = $namespace; labels = @{ "agentx.io/plane" = $Plane; "agentx.io/managed-by" = "agentx-v2-deploy" } }
        spec = @{
            backoffLimit = 0
            template = @{
                metadata = @{ labels = @{ "agentx.io/plane" = $Plane; "agentx.io/ops-job" = "true"; "app.kubernetes.io/name" = $Name } }
                spec = @{
                    serviceAccountName = if ($Plane -eq "control") { "platform-control" } elseif ($Plane -eq "runtime") { "workflow-runtime" } else { "observability" }
                    automountServiceAccountToken = $false
                    restartPolicy = "Never"
                    securityContext = @{ runAsNonRoot = $true; seccompProfile = @{ type = "RuntimeDefault" } }
                    containers = @(@{
                        name = $Command
                        image = (Get-ImageReference $Profile "agentx-$Command")
                        imagePullPolicy = [string]$Profile.images.pullPolicy
                        args = @($Plane)
                        env = @(Get-OpsEnvironment $Plane $Profile $Namespaces $Command)
                        resources = @{ requests = @{ cpu = "25m"; memory = "32Mi"; "ephemeral-storage" = "32Mi" }; limits = @{ cpu = "250m"; memory = "128Mi"; "ephemeral-storage" = "128Mi" } }
                        securityContext = @{ allowPrivilegeEscalation = $false; readOnlyRootFilesystem = $true; runAsNonRoot = $true; capabilities = @{ drop = @("ALL") } }
                    })
                }
            }
        }
    }
    if ($Profile.environment -eq "production") {
        $sources = switch ($Plane) {
            "control" { @(
                @{ secret = @{ name = [string]$Profile.components.controlMysql.caSecretName; items = @(@{ key = "ca.crt"; path = "mysql.pem" }) } },
                @{ secret = @{ name = [string]$Profile.components.objectStorage.caSecretName; items = @(@{ key = "ca.crt"; path = "s3.pem" }) } }
            ) }
            "runtime" { @(
                @{ secret = @{ name = [string]$Profile.components.runtimeMysql.caSecretName; items = @(@{ key = "ca.crt"; path = "mysql.pem" }) } },
                @{ secret = @{ name = [string]$Profile.components.runtimeRedis.caSecretName; items = @(@{ key = "ca.crt"; path = "redis.pem" }) } },
                @{ secret = @{ name = [string]$Profile.components.objectStorage.caSecretName; items = @(@{ key = "ca.crt"; path = "s3.pem" }) } }
            ) }
            default { @(@{ secret = @{ name = [string]$Profile.components.clickhouse.caSecretName; items = @(@{ key = "ca.crt"; path = "clickhouse.pem" }) } }) }
        }
        $jobObject.spec.template.spec.volumes = @(@{ name = "external-ca"; projected = @{ sources = $sources } })
        $jobObject.spec.template.spec.containers[0].volumeMounts = @(@{ name = "external-ca"; mountPath = "/etc/agentx-ca"; readOnly = $true })
    }
    $job = $jobObject | ConvertTo-Json -Depth 20 -Compress
    if ($Plane -eq "runtime") {
        $job = $job | ConvertFrom-Json
        $job.spec.template.metadata.labels | Add-Member -NotePropertyName "agentx.io/runtime-data" -NotePropertyValue "mysql-redis"
        $job = $job | ConvertTo-Json -Depth 20 -Compress
    }
    Invoke-KubectlInput $job @("apply", "-f", "-")
    Wait-V2Job $namespace $Name
}

function Remove-V2Namespaces {
    param([hashtable]$Namespaces)
    Assert-SafeNamespaces $Namespaces
    foreach ($name in @($Namespaces.control, $Namespaces.runtime, $Namespaces.dependencies) | Select-Object -Unique) {
        Invoke-Kubectl -Arguments @("delete", "namespace", $name, "--ignore-not-found", "--wait=true", "--timeout=300s")
    }
}

function Reset-V2DataDomains {
    param([hashtable]$Namespaces)
    Assert-SafeNamespaces $Namespaces
    foreach ($deployment in @("platform-control")) {
        Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.control, "delete", "deployment", $deployment, "--ignore-not-found", "--wait=true")
    }
    foreach ($deployment in @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager")) {
        Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.runtime, "delete", "deployment", $deployment, "--ignore-not-found", "--wait=true")
    }
    Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.observability, "delete", "deployment", "observability", "--ignore-not-found", "--wait=true")
    foreach ($job in @("control-migrate", "control-bootstrap-1", "control-bootstrap-2", "control-doctor")) {
        Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.control, "delete", "job", $job, "--ignore-not-found", "--wait=true")
    }
    foreach ($job in @("runtime-migrate", "runtime-bootstrap-1", "runtime-bootstrap-2", "runtime-doctor")) {
        Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.runtime, "delete", "job", $job, "--ignore-not-found", "--wait=true")
    }
    foreach ($job in @("clickhouse-migrate", "observability-bootstrap-1", "observability-bootstrap-2", "observability-doctor")) {
        Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.observability, "delete", "job", $job, "--ignore-not-found", "--wait=true")
    }
    Invoke-Kubectl -Arguments @("-n", [string]$Namespaces.dependencies, "delete", "job", "object-storage-bootstrap", "--ignore-not-found", "--wait=true")
    foreach ($target in @(
        @{ Namespace = [string]$Namespaces.control; StatefulSet = "control-mysql"; Pvc = "data-control-mysql-0" },
        @{ Namespace = [string]$Namespaces.runtime; StatefulSet = "runtime-mysql"; Pvc = "data-runtime-mysql-0" },
        @{ Namespace = [string]$Namespaces.runtime; StatefulSet = "runtime-redis"; Pvc = "data-runtime-redis-0" },
        @{ Namespace = [string]$Namespaces.observability; StatefulSet = "clickhouse"; Pvc = "data-clickhouse-0" },
        @{ Namespace = [string]$Namespaces.dependencies; StatefulSet = "object-storage"; Pvc = "data-object-storage-0" }
    )) {
        Invoke-Kubectl -Arguments @("-n", $target.Namespace, "delete", "statefulset", $target.StatefulSet, "--ignore-not-found", "--wait=true")
        Invoke-Kubectl -Arguments @("-n", $target.Namespace, "delete", "pvc", $target.Pvc, "--ignore-not-found", "--wait=true")
    }
    Write-Output "V2 data domains were explicitly recreated: Control MySQL, Runtime MySQL, Runtime Redis, ClickHouse, and all three OSS buckets."
}

$configPath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profileJson = Get-Content -Raw -LiteralPath $configPath
$profile = $profileJson | ConvertFrom-Json
if ($profile.apiVersion -ne "agentx.io/deployment/v2alpha3") { throw "Only deployment/v2alpha3 Profiles are accepted; v2alpha2 and earlier must be upgraded." }
$schemaPath = Join-Path $root "deploy/profiles/deployment-profile-v2.schema.json"
if (Get-Command Test-Json -ErrorAction SilentlyContinue) {
    if (-not ($profileJson | Test-Json -SchemaFile $schemaPath)) { throw "V2 Profile does not match the v2alpha3 three-namespace egress schema." }
}
Assert-V2Isolation $profile
Assert-ProductionProfile $profile
if ($Action -eq "Rollback") {
    if (-not $PreviousReleaseManifest) { throw "Rollback requires -PreviousReleaseManifest." }
    Import-ReleaseImages $profile $PreviousReleaseManifest
} elseif ($ReleaseManifest) {
    Import-ReleaseImages $profile $ReleaseManifest
}
$stage = if ($RunId -match '^08-') { "08" } elseif ($RunId -match '^07-') { "07" } elseif ($RunId -match '^06-') { "06" } elseif ($RunId -match '^05-') { "05" } elseif ($RunId -match '^04-') { "04" } elseif ($RunId -match '^03-') { "03" } elseif ($RunId -match '^02-') { "02" } else { "01" }
$namespaceRunId = if ($RunId -match '^(02|03|04|05|06|07|08)-(.+)$') { $Matches[2] } else { $RunId }
$namespaces = Resolve-Namespaces $profile $namespaceRunId $stage
Set-RunScopedSandboxAccess $profile $namespaceRunId $stage
Assert-SafeNamespaces $namespaces
$planes = @(Get-TargetPlanes $Target $profile)

if ($Action -eq "Validate") {
    Write-Output (@{ status = "valid"; apiVersion = $profile.apiVersion; environment = $profile.environment; target = $Target; planes = $planes } | ConvertTo-Json -Compress)
    exit 0
}

if ($Action -eq "SyncSecrets") {
    if ($Target -ne "All") { throw "SyncSecrets coordinates shared credentials and requires -Target All." }
    Sync-DependencySecrets $namespaces $profile $planes
    exit 0
}

if ($Action -eq "Uninstall") {
    if ($planes -contains "dependencies" -and $planes -notcontains "runtime") {
        $runtimeDeployments = (& kubectl -n $namespaces.runtime get deployment --ignore-not-found -o json 2>$null | ConvertFrom-Json)
        $gatewayUsers = @($runtimeDeployments.items | Where-Object {
            $_.spec.template.metadata.labels.'agentx.io/egress-client' -eq 'managed'
        })
        if ($gatewayUsers.Count -gt 0) { throw "Dependencies uninstall is refused while Runtime deployments reference agentx-egress-gateway." }
    }
    Remove-LegacyAutoscalingResources $planes $namespaces
    $rendered = Get-RenderedManifest $planes $profile $namespaces
    $resourceManifest = (($rendered -split '(?m)^---\s*$') | Where-Object { $_.Trim() -and (Get-YamlResourceKind $_) -ne "Namespace" }) -join "---`n"
    if ($resourceManifest.Trim()) { Invoke-KubectlInput $resourceManifest @("delete", "-f", "-", "--ignore-not-found") }
    if ($planes -contains "dependencies") { Remove-IngressController $namespaces }
    if (($PurgeTestResources -or $RunId) -and $profile.environment -ne "production" -and $Target -eq "All") {
        Remove-V2Namespaces $namespaces
    } else {
        Write-Output (@{ status = "workloads-removed"; namespacesPreserved = $true; target = $Target } | ConvertTo-Json -Compress)
    }
    exit 0
}

if ($Action -eq "Status") {
    $status = [ordered]@{ status = "observed"; apiVersion = $profile.apiVersion; environment = $profile.environment; target = $Target; planes = @() }
    foreach ($plane in $planes) {
        $namespace = [string]$namespaces[$plane]
        $payload = (& kubectl -n $namespace get deployment,statefulset,job,hpa,pdb -o json 2>$null) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Failed to query status for $plane." }
        $items = @(($payload | ConvertFrom-Json).items)
        $planeWorkloads = switch ($plane) {
            "control" { @("platform-control", "web-console") }
            "runtime" { @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager") }
            "observability" { @("observability") }
            "dependencies" { @("agentx-egress-gateway") }
            default { @() }
        }
        $scopedItems = if ($plane -eq "dependencies") { $items } else {
            @($items | Where-Object {
                ($_.kind -in @("Deployment", "HorizontalPodAutoscaler", "PodDisruptionBudget") -and $_.metadata.name -in $planeWorkloads) -or
                ($_.kind -eq "Job" -and $_.spec.template.metadata.labels.'agentx.io/plane' -eq $plane) -or
                ($_.kind -eq "StatefulSet" -and $_.spec.template.metadata.labels.'agentx.io/plane' -eq $plane)
            })
        }
        $deployments = @($scopedItems | Where-Object kind -eq "Deployment" | ForEach-Object {
            $containers = @($_.spec.template.spec.containers)
            [ordered]@{
                name = [string]$_.metadata.name
                generation = [int64]$_.metadata.generation
                desiredReplicas = [int]$_.spec.replicas
                readyReplicas = [int]$_.status.readyReplicas
                updatedReplicas = [int]$_.status.updatedReplicas
                availableReplicas = [int]$_.status.availableReplicas
                images = @($containers | ForEach-Object { [string]$_.image })
                probes = @($containers | ForEach-Object { @{ container = [string]$_.name; startup = $null -ne $_.startupProbe; readiness = $null -ne $_.readinessProbe; liveness = $null -ne $_.livenessProbe } })
                conditions = @($_.status.conditions | ForEach-Object { @{ type = [string]$_.type; status = [string]$_.status; reason = [string]$_.reason } })
            }
        })
        $migrations = @($scopedItems | Where-Object { $_.kind -eq "Job" -and $_.metadata.name -match 'migrate$' } | ForEach-Object {
            [ordered]@{ name = [string]$_.metadata.name; succeeded = [int]$_.status.succeeded; failed = [int]$_.status.failed; completionTime = [string]$_.status.completionTime }
        })
        $doctorJobs = @($scopedItems | Where-Object { $_.kind -eq "Job" -and $_.metadata.name -match 'doctor' } | ForEach-Object {
            [ordered]@{ name = [string]$_.metadata.name; succeeded = [int]$_.status.succeeded; failed = [int]$_.status.failed; completionTime = [string]$_.status.completionTime }
        })
        $hpas = @($scopedItems | Where-Object kind -eq "HorizontalPodAutoscaler" | ForEach-Object {
            [ordered]@{ name = [string]$_.metadata.name; managedBy = "external"; minReplicas = [int]$_.spec.minReplicas; maxReplicas = [int]$_.spec.maxReplicas; currentReplicas = [int]$_.status.currentReplicas; desiredReplicas = [int]$_.status.desiredReplicas; conditions = @($_.status.conditions | ForEach-Object { @{ type = [string]$_.type; status = [string]$_.status; reason = [string]$_.reason } }) }
        })
        $pdbs = @($scopedItems | Where-Object kind -eq "PodDisruptionBudget" | ForEach-Object {
            [ordered]@{ name = [string]$_.metadata.name; minAvailable = $_.spec.minAvailable; currentHealthy = [int]$_.status.currentHealthy; desiredHealthy = [int]$_.status.desiredHealthy; disruptionsAllowed = [int]$_.status.disruptionsAllowed }
        })
        $releasePayload = (& kubectl -n $namespace get configmap "agentx-v2-release-state-$plane" --ignore-not-found -o json 2>$null) -join "`n"
        $releaseState = $null
        if ($releasePayload) {
            $releaseConfig = $releasePayload | ConvertFrom-Json
            $releaseState = [ordered]@{ descriptor = ([string]$releaseConfig.data.release | ConvertFrom-Json); appliedAt = [string]$releaseConfig.data.appliedAt }
        }
        $probeStatus = @($deployments | ForEach-Object {
            [ordered]@{
                name = $_.name
                configured = @($_.probes | Where-Object { $_.startup -and $_.readiness -and $_.liveness }).Count -eq $_.probes.Count
            }
        })
        $status.planes += [ordered]@{
            plane = $plane
            namespace = $namespace
            release = $releaseState
            deployments = $deployments
            migrations = $migrations
            probes = [ordered]@{ deployments = $probeStatus; doctorJobs = $doctorJobs }
            autoscaling = $hpas
            disruptionBudgets = $pdbs
            externalDependencies = @(Get-ExternalDependencyStatus $plane $profile $namespaces)
        }
    }
    Write-Output ($status | ConvertTo-Json -Depth 10 -Compress)
    exit 0
}

if ($Action -eq "Render") {
    Write-Output (Get-RenderedManifest $planes $profile $namespaces -IncludeIngressController)
    exit 0
}

try {
    if ($BuildImages) {
        if ($profile.environment -eq "production") { throw "Production images must be supplied by immutable digest; -BuildImages is forbidden." }
        & (Join-Path $PSScriptRoot "build-images.ps1") -Tag ([string]$profile.images.tag) -Namespace ([string]$namespaces.dependencies) -Services @("agentx-migrate", "agentx-bootstrap", "agentx-doctor", "platform-control", "runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability", "web-console")
    }

    $physicalTargets = @(
        @{ plane = "control"; namespace = [string]$namespaces.control },
        @{ plane = "runtime"; namespace = [string]$namespaces.runtime },
        @{ plane = "dependencies"; namespace = [string]$namespaces.dependencies }
    )
    foreach ($physicalTarget in $physicalTargets) {
        $namespace = [string]$physicalTarget.namespace
        $labels = @{ "agentx.io/plane" = [string]$physicalTarget.plane; "app.kubernetes.io/managed-by" = "agentx-v2-deploy" }
        if ($physicalTarget.plane -ne "dependencies") {
            $labels["pod-security.kubernetes.io/enforce"] = "restricted"
            $labels["pod-security.kubernetes.io/audit"] = "restricted"
            $labels["pod-security.kubernetes.io/warn"] = "restricted"
        } else {
            $labels["agentx.io/ingress"] = "allowed"
        }
        $namespaceManifest = @{ apiVersion = "v1"; kind = "Namespace"; metadata = @{ name = $namespace; labels = $labels } } | ConvertTo-Json -Depth 8 -Compress
        Invoke-KubectlInput $namespaceManifest @("apply", "-f", "-")
    }
    if ($planes -contains "dependencies" -and $Action -in @("Install", "Upgrade", "Rollback")) { Install-IngressController $profile $namespaces }
    if ($RecreateV2Data) {
        if ($profile.environment -eq "production" -or $Target -ne "All") { throw "-RecreateV2Data is limited to a complete local/test environment." }
        Reset-V2DataDomains $namespaces
    }

    if ($profile.secrets.mode -eq "generated-local") {
    $controlOld = Get-SecretData $namespaces.control "agentx-control-secrets"
    $runtimeOld = Get-SecretData $namespaces.runtime "agentx-runtime-secrets"
    $observabilityOld = Get-SecretData $namespaces.observability "agentx-observability-secrets"
    $dependenciesOld = Get-SecretData $namespaces.dependencies "agentx-dependencies-secrets"
    $controlObjectPassword = Get-OrCreateValue $dependenciesOld "CONTROL_OBJECT_PASSWORD"
    $runtimeObjectPassword = Get-OrCreateValue $dependenciesOld "RUNTIME_OBJECT_PASSWORD"
    $observabilityObjectPassword = Get-OrCreateValue $dependenciesOld "OBSERVABILITY_OBJECT_PASSWORD"
    # Dependencies owns cross-plane Vault identities. Fall back to the old
    # domain Secret only for one-time migration when the canonical Secret is absent.
    $controlVaultToken = if ($dependenciesOld.ContainsKey("CONTROL_VAULT_TOKEN") -and $dependenciesOld.CONTROL_VAULT_TOKEN) { $dependenciesOld.CONTROL_VAULT_TOKEN } else { Get-OrCreateValue $controlOld "AGENTX_CONTROL_VAULT_TOKEN" }
    $runtimeVaultToken = if ($dependenciesOld.ContainsKey("RUNTIME_VAULT_TOKEN") -and $dependenciesOld.RUNTIME_VAULT_TOKEN) { $dependenciesOld.RUNTIME_VAULT_TOKEN } else { Get-OrCreateValue $runtimeOld "AGENTX_RUNTIME_VAULT_TOKEN" }
    $egressOld = Get-SecretData $namespaces.dependencies "agentx-egress-gateway-secrets"
    $egressTlsOld = Get-SecretData $namespaces.dependencies ([string]$profile.network.egressGateway.sandboxAccess.tlsSecretName)
    foreach ($entry in $egressTlsOld.GetEnumerator()) { $egressOld[$entry.Key] = $entry.Value }
    $signing = Get-CanonicalSigningMaterial $dependenciesOld $controlOld $runtimeOld $observabilityOld $egressOld
    $runtimeRedisPassword = Get-OrCreateValue $runtimeOld "AGENTX_RUNTIME_REDIS_PASSWORD"
    $observabilityRedisPassword = if ($dependenciesOld.ContainsKey("OBSERVABILITY_REDIS_PASSWORD") -and $dependenciesOld.OBSERVABILITY_REDIS_PASSWORD) { $dependenciesOld.OBSERVABILITY_REDIS_PASSWORD } else { Get-OrCreateValue $observabilityOld "AGENTX_OBSERVABILITY_REDIS_PASSWORD" }
    $publisherKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_PUBLISHER_JWT_KID" $controlOld "AGENTX_CONTROL_PUBLISHER_JWT_KID" "publisher-current"
    $projectorKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_PROJECTOR_JWT_KID" $controlOld "AGENTX_CONTROL_PROJECTOR_JWT_KID" "projector-current"
    $bffKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_BFF_JWT_KID" $controlOld "AGENTX_CONTROL_BFF_JWT_KID" "bff-current"
    $bundleKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_BUNDLE_KEY_ID" $controlOld "AGENTX_CONTROL_BUNDLE_KEY_ID" "bundle-current"
    $workPackageKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_WORK_PACKAGE_KEY_ID" $controlOld "AGENTX_CONTROL_WORK_PACKAGE_KEY_ID" "work-package-current"
    $userKid = Get-CanonicalOrLegacyValue $dependenciesOld "AGENTX_CONTROL_USER_JWT_KID" $controlOld "AGENTX_CONTROL_USER_JWT_KID" "user-current"
    $runtimeGatewayEgressKid = Get-CanonicalOrDeployedKeyId $dependenciesOld "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID" $runtimeOld "runtime-gateway" $namespaces.runtime "runtime-gateway-current"
    $workflowRuntimeEgressKid = Get-CanonicalOrDeployedKeyId $dependenciesOld "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID" $runtimeOld "workflow-runtime" $namespaces.runtime "workflow-runtime-current"
    $workflowWorkerEgressKid = Get-CanonicalOrDeployedKeyId $dependenciesOld "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID" $runtimeOld "workflow-worker" $namespaces.runtime "workflow-worker-current"
    $sandboxEgressKid = Get-CanonicalOrDeployedKeyId $dependenciesOld "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID" $runtimeOld "sandbox-manager" $namespaces.runtime "sandbox-current"

    Set-DomainSecret $namespaces.control "agentx-control-secrets" @{
        AGENTX_CONTROL_MYSQL_PASSWORD = Get-OrCreateValue $controlOld "AGENTX_CONTROL_MYSQL_PASSWORD"
        AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD = Get-OrCreateValue $controlOld "AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD"
        AGENTX_CONTROL_MYSQL_ROOT_PASSWORD = Get-OrCreateValue $controlOld "AGENTX_CONTROL_MYSQL_ROOT_PASSWORD"
        AGENTX_CONTROL_S3_ACCESS_KEY = [string]$profile.components.objectStorage.domains.control.user
        AGENTX_CONTROL_S3_SECRET_KEY = $controlObjectPassword
        AGENTX_CONTROL_PUBLISHER_JWT_KID = $publisherKid
        AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM = $signing.servicePrivate
        AGENTX_CONTROL_PROJECTOR_JWT_KID = $projectorKid
        AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM = $signing.projectorPrivate
        AGENTX_CONTROL_BFF_JWT_KID = $bffKid
        AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM = $signing.bffPrivate
        AGENTX_CONTROL_BUNDLE_KEY_ID = $bundleKid
        AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM = $signing.bundlePrivate
        AGENTX_CONTROL_WORK_PACKAGE_KEY_ID = $workPackageKid
        AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM = $signing.workPackagePrivate
        AGENTX_CONTROL_API_REFRESH_JWT_SIGNING_SECRET = Get-OrCreateValue $controlOld "AGENTX_CONTROL_API_REFRESH_JWT_SIGNING_SECRET"
        AGENTX_CONTROL_USER_JWT_KID = $userKid
        AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM = $signing.userPrivate
        AGENTX_CONTROL_USER_JWT_PUBLIC_KEYS_JSON = $signing.userPublicJson
        AGENTX_CONTROL_VAULT_TOKEN = $controlVaultToken
    }
    Set-DomainSecret $namespaces.runtime "agentx-runtime-secrets" @{
        AGENTX_RUNTIME_MYSQL_PASSWORD = Get-OrCreateValue $runtimeOld "AGENTX_RUNTIME_MYSQL_PASSWORD"
        AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD = Get-OrCreateValue $runtimeOld "AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD"
        AGENTX_RUNTIME_MYSQL_ROOT_PASSWORD = Get-OrCreateValue $runtimeOld "AGENTX_RUNTIME_MYSQL_ROOT_PASSWORD"
        AGENTX_RUNTIME_REDIS_PASSWORD = $runtimeRedisPassword
        AGENTX_RUNTIME_REDIS_ACL_FILE = "user default on >$runtimeRedisPassword ~* &agentx:v2:invocation:wakeup:* +@all`nuser observability on >$observabilityRedisPassword ~agentx:v2:trace:v1 ~agentx:v2:observability:jti:* +ping +xgroup +xreadgroup +xpending +xautoclaim +xack +set +get +del +exists"
        AGENTX_RUNTIME_S3_ACCESS_KEY = [string]$profile.components.objectStorage.domains.runtime.user
        AGENTX_RUNTIME_S3_SECRET_KEY = $runtimeObjectPassword
        AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON = $signing.servicePublicJson
        AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON = $signing.bundlePublicJson
        AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON = $signing.workPackagePublicJson
        AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON = $signing.userPublicJson
        AGENTX_RUNTIME_VAULT_TOKEN = $runtimeVaultToken
        AGENTX_OPENSANDBOX_API_KEY = Get-OrCreateValue $runtimeOld "AGENTX_OPENSANDBOX_API_KEY" "agentx-local-opensandbox-key"
        AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.runtimeGatewayEgressPrivate
        AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID = $runtimeGatewayEgressKid
        AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.workflowRuntimeEgressPrivate
        AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID = $workflowRuntimeEgressKid
        AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.workflowWorkerEgressPrivate
        AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID = $workflowWorkerEgressKid
        AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.sandboxEgressPrivate
        AGENTX_SANDBOX_EGRESS_JWT_KEY_ID = $sandboxEgressKid
    }
    Set-DomainSecret $namespaces.observability "agentx-observability-secrets" @{
        AGENTX_CLICKHOUSE_QUERY_PASSWORD = Get-OrCreateValue $observabilityOld "AGENTX_CLICKHOUSE_QUERY_PASSWORD"
        AGENTX_CLICKHOUSE_CONSUMER_PASSWORD = Get-OrCreateValue $observabilityOld "AGENTX_CLICKHOUSE_CONSUMER_PASSWORD"
        AGENTX_CLICKHOUSE_MIGRATE_PASSWORD = Get-OrCreateValue $observabilityOld "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD"
        AGENTX_OBSERVABILITY_REDIS_PASSWORD = $observabilityRedisPassword
        AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON = $signing.bffPublicJson
        AGENTX_OBSERVABILITY_S3_ACCESS_KEY = [string]$profile.components.objectStorage.domains.observability.user
        AGENTX_OBSERVABILITY_S3_SECRET_KEY = $observabilityObjectPassword
    }
    Set-DomainSecret $namespaces.dependencies "agentx-dependencies-secrets" @{
        MINIO_ROOT_USER = Get-OrCreateValue $dependenciesOld "MINIO_ROOT_USER" "agentx_admin"
        MINIO_ROOT_PASSWORD = Get-OrCreateValue $dependenciesOld "MINIO_ROOT_PASSWORD"
        CONTROL_OBJECT_PASSWORD = $controlObjectPassword
        RUNTIME_OBJECT_PASSWORD = $runtimeObjectPassword
        OBSERVABILITY_OBJECT_PASSWORD = $observabilityObjectPassword
        VAULT_DEV_ROOT_TOKEN_ID = Get-OrCreateValue $dependenciesOld "VAULT_DEV_ROOT_TOKEN_ID"
        CONTROL_VAULT_TOKEN = $controlVaultToken
        RUNTIME_VAULT_TOKEN = $runtimeVaultToken
        OBSERVABILITY_REDIS_PASSWORD = $observabilityRedisPassword
        AGENTX_CONTROL_PUBLISHER_JWT_KID = $publisherKid
        AGENTX_CONTROL_PROJECTOR_JWT_KID = $projectorKid
        AGENTX_CONTROL_BFF_JWT_KID = $bffKid
        AGENTX_CONTROL_BUNDLE_KEY_ID = $bundleKid
        AGENTX_CONTROL_WORK_PACKAGE_KEY_ID = $workPackageKid
        AGENTX_CONTROL_USER_JWT_KID = $userKid
        AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM = $signing.servicePrivate
        AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM = $signing.projectorPrivate
        AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM = $signing.bffPrivate
        AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM = $signing.bundlePrivate
        AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM = $signing.workPackagePrivate
        AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM = $signing.userPrivate
        AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON = $signing.servicePublicJson
        AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON = $signing.bffPublicJson
        AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON = $signing.bundlePublicJson
        AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON = $signing.workPackagePublicJson
        AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON = $signing.userPublicJson
        AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.runtimeGatewayEgressPrivate
        AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID = $runtimeGatewayEgressKid
        AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.workflowRuntimeEgressPrivate
        AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID = $workflowRuntimeEgressKid
        AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.workflowWorkerEgressPrivate
        AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID = $workflowWorkerEgressKid
        AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM = $signing.sandboxEgressPrivate
        AGENTX_SANDBOX_EGRESS_JWT_KEY_ID = $sandboxEgressKid
        AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = $signing.egressPublicJson
        AGENTX_EGRESS_TLS_CERTIFICATE_PEM = $signing.egressTlsCertificate
        AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM = $signing.egressTlsPrivateKey
    }
    Set-DomainSecret $namespaces.dependencies "agentx-egress-gateway-secrets" @{
        AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON = $signing.egressPublicJson
    }
    $egressTlsValues = @{ "tls.crt" = $signing.egressTlsCertificate; "tls.key" = $signing.egressTlsPrivateKey; "ca.crt" = $signing.egressTlsCertificate }
    Set-DomainSecret $namespaces.dependencies ([string]$profile.network.egressGateway.sandboxAccess.tlsSecretName) $egressTlsValues
    if ($profile.network.egressGateway.sandboxAccess.caSecretName) {
        Set-DomainSecret $namespaces.runtime ([string]$profile.network.egressGateway.sandboxAccess.caSecretName) @{ "ca.crt" = $signing.egressTlsCertificate }
    }
    } else {
        Publish-DependencySecretMirrors $namespaces $profile
        Assert-ExistingSecrets $planes $profile $namespaces
    }

    if ($Action -eq "Doctor") {
        if ($planes -contains "dependencies") {
            Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "deployment/agentx-egress-gateway", "--timeout=300s")
            Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "get", "endpoints", "agentx-egress-gateway", "-o", "name")
        }
        foreach ($plane in $planes | Where-Object { $_ -ne "dependencies" }) { Invoke-OpsJob "doctor" $plane "$plane-doctor" $profile $namespaces }
        Write-Output (@{ status = "healthy"; target = $Target; planes = $planes } | ConvertTo-Json -Compress)
        exit 0
    }

    $preserveReplicaWorkloadNames = [Collections.Generic.List[string]]::new()
    foreach ($workloadTarget in (@(
        @{ plane = "control"; namespace = $namespaces.control; name = "platform-control" },
        @{ plane = "control"; namespace = $namespaces.control; name = "web-console" },
        @{ plane = "runtime"; namespace = $namespaces.runtime; name = "runtime-gateway" },
        @{ plane = "runtime"; namespace = $namespaces.runtime; name = "workflow-runtime" },
        @{ plane = "runtime"; namespace = $namespaces.runtime; name = "workflow-worker" },
        @{ plane = "runtime"; namespace = $namespaces.runtime; name = "sandbox-manager" },
        @{ plane = "dependencies"; namespace = $namespaces.dependencies; name = "agentx-egress-gateway" },
        @{ plane = "observability"; namespace = $namespaces.observability; name = "observability" }
    ) | Where-Object { $_.namespace -and $planes -contains $_.plane })) {
        if ($Action -notin @("Upgrade", "Rollback")) { continue }
        $existing = (& kubectl -n $workloadTarget.namespace get deployment $workloadTarget.name --ignore-not-found -o name 2>$null) -join ""
        if ($existing) { $preserveReplicaWorkloadNames.Add($workloadTarget.name) }
    }
    Remove-LegacyAutoscalingResources $planes $namespaces
    $rendered = Get-RenderedManifest $planes $profile $namespaces $preserveReplicaWorkloadNames.ToArray()
    $applicationNames = @(
        "platform-control", "web-console", "runtime-gateway", "workflow-runtime",
        "workflow-worker", "sandbox-manager", "agentx-egress-gateway", "observability"
    )
    $applicationManifest = ""
    $gatewayManifest = ""
    if ($Action -eq "Rollback") {
        $rendered = (($rendered -split '(?m)^---\s*$') | Where-Object { (Get-YamlResourceName $_) -notin @("control-migrate", "runtime-migrate", "clickhouse-migrate") }) -join "---`n"
        Invoke-KubectlInput $rendered @("apply", "-f", "-")
        if ($planes -contains "dependencies") { Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "deployment/agentx-egress-gateway", "--timeout=300s") }
    } else {
        $documents = @(($rendered -split '(?m)^---\s*$') | Where-Object { $_.Trim() })
        $applicationDocuments = @($documents | Where-Object {
            (Get-YamlResourceKind $_) -eq "Deployment" -and
            (Get-YamlResourceName $_) -in $applicationNames
        })
        $gatewayDocuments = @($applicationDocuments | Where-Object { (Get-YamlResourceName $_) -eq "agentx-egress-gateway" })
        $applicationDocuments = @($applicationDocuments | Where-Object { (Get-YamlResourceName $_) -ne "agentx-egress-gateway" })
        $foundationDocuments = @($documents | Where-Object {
            -not ((Get-YamlResourceKind $_) -eq "Deployment" -and
                (Get-YamlResourceName $_) -in $applicationNames)
        })
        $applicationManifest = $applicationDocuments -join "---`n"
        $gatewayManifest = $gatewayDocuments -join "---`n"
        if ($planes -contains "dependencies" -and @($foundationDocuments | Where-Object { (Get-YamlResourceKind $_) -eq "Job" -and (Get-YamlResourceName $_) -eq "vault-bootstrap" }).Count -gt 0) {
            Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "delete", "job", "vault-bootstrap", "--ignore-not-found", "--wait=true")
        }
        Invoke-KubectlInput ($foundationDocuments -join "---`n") @("apply", "-f", "-")
        if ($gatewayManifest) {
            Invoke-KubectlInput $gatewayManifest @("apply", "-f", "-")
            Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "deployment/agentx-egress-gateway", "--timeout=300s")
        }
    }

    if ($planes -contains "control" -and $profile.components.controlMysql.mode -eq "bundled") { Invoke-Kubectl -Arguments @("-n", $namespaces.control, "rollout", "status", "statefulset/control-mysql", "--timeout=300s") }
    if ($planes -contains "runtime" -and $profile.components.runtimeMysql.mode -eq "bundled") { Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "rollout", "status", "statefulset/runtime-mysql", "--timeout=300s") }
    if ($planes -contains "runtime" -and $profile.components.runtimeRedis.mode -eq "bundled") { Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "rollout", "status", "statefulset/runtime-redis", "--timeout=300s") }
    if ($planes -contains "observability" -and $profile.components.clickhouse.mode -eq "bundled") { Invoke-Kubectl -Arguments @("-n", $namespaces.observability, "rollout", "status", "statefulset/clickhouse", "--timeout=300s") }
    if ($planes -contains "dependencies" -and $profile.environment -ne "production") {
        Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "statefulset/object-storage", "--timeout=300s")
        Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "statefulset/vault", "--timeout=300s")
        Wait-V2Job $namespaces.dependencies "vault-bootstrap"
        Wait-V2Job $namespaces.dependencies "object-storage-bootstrap"
    }
    if ($Action -ne "Rollback") {
        if ($planes -contains "control") { Wait-V2Job $namespaces.control "control-migrate" }
        if ($planes -contains "runtime") { Wait-V2Job $namespaces.runtime "runtime-migrate" }
        if ($planes -contains "observability") { Wait-V2Job $namespaces.observability "clickhouse-migrate" }
    }

    foreach ($plane in $planes | Where-Object { $_ -ne "dependencies" }) {
        if ($Action -eq "Install") {
            Invoke-OpsJob "bootstrap" $plane "$plane-bootstrap-1" $profile $namespaces
            Invoke-OpsJob "bootstrap" $plane "$plane-bootstrap-2" $profile $namespaces
        }
        Invoke-OpsJob "doctor" $plane "$plane-doctor" $profile $namespaces
    }
    if ($Action -ne "Rollback" -and $applicationManifest) {
        Invoke-KubectlInput $applicationManifest @("apply", "-f", "-")
    }
    if ($planes -contains "control") { foreach ($workload in @("platform-control", "web-console")) {
        Invoke-Kubectl -Arguments @("-n", $namespaces.control, "rollout", "status", "deployment/$workload", "--timeout=300s")
    } }
    if ($planes -contains "runtime") { foreach ($workload in @("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager")) {
        Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "rollout", "status", "deployment/$workload", "--timeout=300s")
    } }
    if ($planes -contains "observability") { Invoke-Kubectl -Arguments @("-n", $namespaces.observability, "rollout", "status", "deployment/observability", "--timeout=300s") }
    if ($planes -contains "dependencies" -and $Action -ne "Rollback") { Invoke-Kubectl -Arguments @("-n", $namespaces.dependencies, "rollout", "status", "deployment/agentx-egress-gateway", "--timeout=300s") }
    $appliedManifest = if ($Action -eq "Rollback") { $PreviousReleaseManifest } else { $ReleaseManifest }
    Set-ReleaseState $planes $namespaces (Get-ReleaseDescriptor $profile $appliedManifest $Action)
    Write-Output (@{ status = "ready"; action = $Action; target = $Target; apiVersion = $profile.apiVersion; namespaces = $namespaces } | ConvertTo-Json -Depth 5 -Compress)
}
catch {
    if ($CleanupOnFailure -and $profile.environment -ne "production" -and $Target -eq "All") {
        foreach ($namespace in @($namespaces.control, $namespaces.runtime) | Select-Object -Unique) {
            if ((& kubectl get namespace $namespace --ignore-not-found -o name 2>$null) -join "") {
                Invoke-Kubectl -Arguments @("-n", $namespace, "delete", "ingress", "--all", "--ignore-not-found", "--wait=true")
            }
        }
        Remove-IngressController $namespaces
        Remove-V2Namespaces $namespaces
    }
    throw
}
