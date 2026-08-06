Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

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
$script:CurlImage = "curlimages/curl:8.14.1@sha256:9a1ed35addb45476afa911696297f8e115993df459278ed036182dd2cd22b67b"
$script:ServiceImages = @("web", "platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "echo-mcp", "echo-node")
$script:ReleaseImages = @("web", "platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer")
$script:StatefulModes = @("mysql", "redis", "clickhouse", "objectStorage")

function Invoke-AgentxDeployment {
    param(
        [Parameter(Mandatory)][ValidateSet("Doctor", "Install", "Upgrade", "Status", "Uninstall")][string]$Action,
        [ValidateSet("Full", "Custom")][string]$Profile = "Full",
        [string]$ConfigFile,
        [string]$Namespace,
        [switch]$NonInteractive,
        [switch]$DryRun,
        [ValidateSet("all", "services", "infrastructure", "addons", "sandbox", "ingress")][string]$Target = "all",
        [switch]$DeleteData,
        [switch]$DeleteNamespace,
        [switch]$RotateSecrets,
        [ValidateSet("all", "expand", "contract")][string]$MigrationPhase = "all",
        [switch]$MigrationOnly,
        [switch]$SkipMigrations
    )
    $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    if ($Action -in @("Status", "Uninstall") -and -not $ConfigFile) {
        $profileValue = Get-DeployedProfile -Namespace $(if ($Namespace) { $Namespace } else { "agentx" })
        if (-not $profileValue -and $Action -eq "Status") { throw "No Agentx deployment state was found. Pass -Namespace or -ConfigFile." }
        if (-not $profileValue) { $profileValue = Read-DeploymentProfile -RepoRoot $repoRoot -Profile $Profile -ConfigFile $ConfigFile -Namespace $Namespace -NonInteractive:$NonInteractive }
    } else {
        $profileValue = Read-DeploymentProfile -RepoRoot $repoRoot -Profile $Profile -ConfigFile $ConfigFile -Namespace $Namespace -NonInteractive:$NonInteractive
    }
    Assert-DeploymentProfile -Profile $profileValue -RepoRoot $repoRoot

    switch ($Action) {
        "Doctor" { Invoke-Doctor -Profile $profileValue -RepoRoot $repoRoot -DryRun:$DryRun }
        "Install" { Invoke-Install -Profile $profileValue -RepoRoot $repoRoot -Target $Target -DryRun:$DryRun -RotateSecrets:$RotateSecrets -IsUpgrade:$false -MigrationPhase $MigrationPhase -MigrationOnly:$MigrationOnly -SkipMigrations:$SkipMigrations }
        "Upgrade" { Invoke-Install -Profile $profileValue -RepoRoot $repoRoot -Target $Target -DryRun:$DryRun -RotateSecrets:$RotateSecrets -IsUpgrade:$true -MigrationPhase $MigrationPhase -MigrationOnly:$MigrationOnly -SkipMigrations:$SkipMigrations }
        "Status" { Show-DeploymentStatus -Profile $profileValue }
        "Uninstall" { Invoke-Uninstall -Profile $profileValue -RepoRoot $repoRoot -Target $Target -DeleteData:$DeleteData -DeleteNamespace:$DeleteNamespace -DryRun:$DryRun -NonInteractive:$NonInteractive }
    }
}

function Read-DeploymentProfile {
    param([string]$RepoRoot, [string]$Profile, [string]$ConfigFile, [string]$Namespace, [switch]$NonInteractive)
    if ($ConfigFile) {
        $path = (Resolve-Path -LiteralPath $ConfigFile).Path
        $json = Get-Content -LiteralPath $path -Raw
    } elseif ($Profile -eq "Full") {
        $json = Get-Content (Join-Path $RepoRoot "deploy/profiles/full-local.json") -Raw
    } elseif ($NonInteractive) {
        throw "Custom non-interactive deployment requires -ConfigFile."
    } else {
        return New-InteractiveProfile -Namespace $(if ($Namespace) { $Namespace } else { "agentx" })
    }
    $schema = Join-Path $RepoRoot "deploy/profiles/deployment-profile.schema.json"
    if (Get-Command Test-Json -ErrorAction SilentlyContinue) {
        if (-not ($json | Test-Json -SchemaFile $schema)) { throw "Deployment profile does not match $schema." }
    }
    $value = $json | ConvertFrom-Json -Depth 30
    if ($Namespace) { $value.namespace = $Namespace }
    return $value
}

function New-InteractiveProfile {
    param([string]$Namespace)
    $root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $base = Get-Content (Join-Path $root "deploy/profiles/full-local.json") -Raw | ConvertFrom-Json -Depth 30
    $base.namespace = $Namespace
    $base.environment = Read-Choice "Environment" @("local", "test", "production") "local"
    $base.secrets.provider = Read-Choice "Secret provider" @("local_encrypted", "vault_kv_v2") $(if ($base.environment -eq "production") { "vault_kv_v2" } else { $base.secrets.provider })
    if ($base.secrets.provider -eq "vault_kv_v2") {
        $base.secrets.vaultAddress = Read-RequiredText "Vault address" ([string]$base.secrets.vaultAddress)
        $base.secrets.vaultMount = Read-RequiredText "Vault KV v2 mount" $(if ($base.secrets.vaultMount) { [string]$base.secrets.vaultMount } else { "secret" })
    }
    $base.images.mode = Read-Choice "Image mode" @("local-build", "registry") $base.images.mode
    if ($base.images.mode -eq "registry") { $base.images.registry = Read-RequiredText "Image registry" $base.images.registry }
    foreach ($name in @("mysql", "redis", "clickhouse")) {
        $component = $base.components.$name
        $component.mode = Read-Choice "$name mode" @("bundled", "external") $component.mode
        if ($component.mode -eq "external") { Read-ExternalComponent -Name $name -Component $component }
    }
    $base.components.objectStorage.mode = Read-Choice "object storage mode" @("bundled-minio", "external-s3") $base.components.objectStorage.mode
    if ($base.components.objectStorage.mode -eq "external-s3") { Read-ExternalComponent -Name "objectStorage" -Component $base.components.objectStorage }
    foreach ($addon in @(@("rag", "LightRAG"), @("memory", "Mem0"))) {
        $component = $base.components.($addon[0])
        $component.mode = Read-Choice "$($addon[1]) mode" @("bundled", "external", "disabled") $component.mode
        if ($component.mode -eq "external") { $component.endpoint = Read-RequiredText "$($addon[1]) endpoint" ([string]$component.endpoint) }
        if ($component.mode -eq "bundled") { Read-AddonProvider -Component $component -Label $addon[1] }
    }
    $base.components.sandbox.mode = Read-Choice "Sandbox mode" @("disabled", "remote") $base.components.sandbox.mode
    if ($base.components.sandbox.mode -eq "remote") {
        $base.components.sandbox.endpoint = Read-RequiredText "OpenSandbox endpoint" ([string]$base.components.sandbox.endpoint)
        $base.components.sandbox.secureAccess = $base.environment -eq "production"
        $base.components.sandbox.useServerProxy = $true
        $base.components.sandbox.allowedHosts = @(([Uri]$base.components.sandbox.endpoint).Host)
    }
    $base.secrets.mode = Read-Choice "Secret mode" @("managed", "existing") $base.secrets.mode
    if ($base.secrets.mode -eq "managed") { Read-ManagedSecretInputs -Profile $base }
    $base.ingress.host = Read-RequiredText "Ingress host" $base.ingress.host
    $base.ingress.controllerServiceType = Read-Choice "Ingress service type" @("LoadBalancer", "NodePort") $base.ingress.controllerServiceType
    return $base
}

function Read-ExternalComponent {
    param([string]$Name, $Component)
    switch ($Name) {
        "mysql" {
            $Component.host = Read-RequiredText "MySQL host" $Component.host
            $Component.port = [int](Read-RequiredText "MySQL port" ([string]$Component.port))
            $Component.tlsMode = Read-Choice "MySQL TLS mode" @("disabled", "preferred", "required", "verify_ca", "verify_identity") $Component.tlsMode
            if ($Component.tlsMode -in @("verify_ca", "verify_identity")) { Set-ObjectProperty -Object $Component.tls -Name "caFile" -Value (Read-RequiredText "MySQL CA file" ([string](Get-PropertyValue $Component.tls "caFile"))) }
        }
        "redis" {
            $Component.url = Read-RequiredText "Redis URL" $Component.url
        }
        "clickhouse" {
            $Component.url = Read-RequiredText "ClickHouse URL" $Component.url
            $ca = Read-Host "ClickHouse CA file (blank for system CA)"
            if ($ca) { Set-ObjectProperty -Object $Component.tls -Name "caFile" -Value $ca }
        }
        "objectStorage" {
            $Component.endpoint = Read-RequiredText "S3 endpoint" $Component.endpoint
            $Component.allowHttp = $false
            $Component.pathStyle = (Read-Choice "S3 path style" @("true", "false") ([string]$Component.pathStyle)) -eq "true"
        }
    }
}

function Read-AddonProvider {
    param($Component, [string]$Label)
    $provider = $Component.provider
    if (-not $provider) { $provider = [pscustomobject]@{}; Set-ObjectProperty $Component "provider" $provider }
    Set-ObjectProperty $provider "baseUrl" (Read-RequiredText "$Label provider base URL" ([string](Get-PropertyValue $provider "baseUrl")))
    Set-ObjectProperty $provider "llmModel" (Read-RequiredText "$Label LLM model" ([string](Get-PropertyValue $provider "llmModel")))
    Set-ObjectProperty $provider "embeddingModel" (Read-RequiredText "$Label embedding model" ([string](Get-PropertyValue $provider "embeddingModel")))
    Set-ObjectProperty $provider "embeddingDimension" ([int](Read-RequiredText "$Label embedding dimension" ([string](Get-PropertyValue $provider "embeddingDimension"))))
}

function Read-ManagedSecretInputs {
    param($Profile)
    if ($Profile.components.mysql.mode -eq "external") { Read-SecretToEnvironment "AGENTX_DEPLOY_MYSQL_PASSWORD" "MySQL password" }
    if ($Profile.components.redis.mode -eq "external") { Read-SecretToEnvironment "AGENTX_DEPLOY_REDIS_PASSWORD" "Redis password" }
    if ($Profile.components.clickhouse.mode -eq "external") { Read-SecretToEnvironment "AGENTX_DEPLOY_CLICKHOUSE_PASSWORD" "ClickHouse password" }
    if ($Profile.components.objectStorage.mode -eq "external-s3") {
        Read-SecretToEnvironment "AGENTX_DEPLOY_S3_ACCESS_KEY" "S3 access key"
        Read-SecretToEnvironment "AGENTX_DEPLOY_S3_SECRET_KEY" "S3 secret key"
        $session = Read-Host "S3 session token (blank if none)"
        if ($session) { Set-Item "Env:AGENTX_DEPLOY_S3_SESSION_TOKEN" $session }
    }
    if ($Profile.components.sandbox.mode -eq "remote") { Read-SecretToEnvironment "AGENTX_DEPLOY_OPENSANDBOX_API_KEY" "OpenSandbox API key" }
    if ($Profile.components.rag.mode -eq "bundled") { Read-SecretToEnvironment "AGENTX_DEPLOY_LIGHTRAG_OPENAI_API_KEY" "LightRAG provider API key" }
    if ($Profile.components.memory.mode -eq "bundled") { Read-SecretToEnvironment "AGENTX_DEPLOY_MEM0_OPENAI_API_KEY" "Mem0 provider API key" }
    if ($Profile.secrets.provider -eq "vault_kv_v2") { Read-SecretToEnvironment "AGENTX_DEPLOY_VAULT_TOKEN" "Vault token" }
}

function Read-RequiredText { param([string]$Label, [string]$Default) $value = Read-Host "$Label [$Default]"; if (-not $value) { $value = $Default }; if (-not $value) { throw "$Label is required." }; return $value }
function Read-SecretToEnvironment { param([string]$Name, [string]$Label) $value = Read-Host -Prompt $Label -MaskInput; if (-not $value) { throw "$Label is required." }; Set-Item "Env:$Name" $value }
function Read-Choice { param([string]$Label, [string[]]$Values, [string]$Default) $value = Read-Host "$Label ($($Values -join '/')) [$Default]"; if (-not $value) { return $Default }; if ($value -notin $Values) { throw "$Label must be one of: $($Values -join ', ')" }; return $value }
function Get-PropertyValue { param($Object, [string]$Name) if ($null -eq $Object) { return $null }; $property = $Object.PSObject.Properties[$Name]; if ($property) { return $property.Value }; return $null }
function Set-ObjectProperty { param($Object, [string]$Name, $Value) $property = $Object.PSObject.Properties[$Name]; if ($property) { $property.Value = $Value } else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value } }

function Assert-DeploymentProfile {
    param($Profile, [string]$RepoRoot)
    if ($Profile.apiVersion -ne "agentx.io/deployment/v1alpha1") { throw "Unsupported deployment profile apiVersion." }
    if ($Profile.environment -eq "production" -and $Profile.components.objectStorage.allowHttp) { throw "Production object storage cannot enable allowHttp." }
    if ($Profile.images.mode -eq "registry" -and -not $Profile.images.registry) { throw "Registry image mode requires images.registry." }
    if ($Profile.environment -eq "production") {
        if ($Profile.images.mode -ne "registry") { throw "Production requires registry image mode." }
        if ($Profile.secrets.provider -ne "vault_kv_v2") { throw "Production requires secrets.provider=vault_kv_v2." }
        $requiredImages = @($script:ReleaseImages)
        if ($Profile.components.sandbox.mode -eq "remote") { $requiredImages += "sandbox-manager" }
        foreach ($name in $requiredImages) {
            $digest = Get-PropertyValue $Profile.images.digests $name
            if (-not $digest -or $digest -notmatch '^sha256:[a-f0-9]{64}$') { throw "Production image $name requires a sha256 digest." }
        }
        if (@($Profile.network.allowedEgressCidrs).Count -eq 0) { throw "Production requires at least one controlled IPv4 or IPv6 egress CIDR." }
    }
    if ($Profile.secrets.provider -eq "vault_kv_v2" -and (-not $Profile.secrets.vaultAddress -or -not $Profile.secrets.vaultMount)) { throw "Vault Secret provider requires vaultAddress and vaultMount." }
    $sandbox = $Profile.components.sandbox
    if ($sandbox.runtimeClass -eq "runc" -and $sandbox.isolationLevel -ne "standard") { throw "runc can only declare isolationLevel=standard." }
    if ($sandbox.runtimeClass -eq "custom" -and -not $sandbox.runtimeClassName) { throw "custom Sandbox RuntimeClass requires runtimeClassName." }
    if ($sandbox.isolationLevel -eq "strong") { Assert-IsolationEvidence -Sandbox $sandbox }
    foreach ($name in @("mysql", "redis", "clickhouse", "objectStorage")) {
        $component = $Profile.components.$name
        if ($component.mode -in @("external", "external-s3")) {
            if ($name -eq "mysql" -and $component.tlsMode -in @("verify_ca", "verify_identity") -and -not (Get-PropertyValue $component.tls "caFile")) { throw "External MySQL verify mode requires tls.caFile." }
            if ($name -eq "redis" -and -not $component.url) { throw "Redis external mode requires url." }
            if ($name -eq "clickhouse" -and -not $component.url) { throw "ClickHouse external mode requires url." }
            if ($name -eq "objectStorage" -and -not $component.endpoint) { throw "Object storage external mode requires endpoint." }
        }
        if ($component.tls) {
            foreach ($field in @("caFile", "clientCertFile", "clientKeyFile")) {
                $path = Get-PropertyValue $component.tls $field
                if ($path -and -not [IO.Path]::IsPathRooted([string]$path)) { throw "$name tls.$field must be an absolute host path." }
            }
        }
    }
    foreach ($name in @("mysql", "redis")) {
        $tls = $Profile.components.$name.tls
        if ($tls -and [bool](Get-PropertyValue $tls "clientCertFile") -ne [bool](Get-PropertyValue $tls "clientKeyFile")) { throw "$name TLS client certificate and key must be configured together." }
    }
    foreach ($name in @("clickhouse", "objectStorage")) {
        $tls = $Profile.components.$name.tls
        if ($tls -and ((Get-PropertyValue $tls "clientCertFile") -or (Get-PropertyValue $tls "clientKeyFile"))) { throw "$name mTLS client certificates are not supported by this deployment profile." }
    }
    foreach ($addon in @("rag", "memory")) {
        $component = $Profile.components.$addon
        if ($component.mode -eq "external" -and -not $component.endpoint) { throw "$addon external mode requires endpoint." }
        if ($component.mode -eq "bundled") {
            if (-not $component.endpoint -or -not $component.provider) { throw "$addon bundled mode requires endpoint and provider configuration." }
            if ($Profile.environment -ne "local" -and (Get-PropertyValue $component.provider "baseUrl") -match "echo-mcp") { throw "$addon bundled provider cannot use the local echo fixture outside local environment." }
        }
    }
    if ($Profile.components.sandbox.mode -eq "remote") {
        if (-not $Profile.components.sandbox.endpoint) { throw "Sandbox remote mode requires endpoint." }
        if (-not $Profile.components.sandbox.allowedHosts -or $Profile.components.sandbox.allowedHosts.Count -eq 0) { throw "Sandbox remote mode requires an allowed host list." }
    }
    foreach ($file in Get-ProfileCertificateFiles -Profile $Profile) { if (-not (Test-Path -LiteralPath $file.Path -PathType Leaf)) { throw "$($file.Name) file does not exist: $($file.Path)" } }
    if (-not (Test-Path (Join-Path $RepoRoot "deploy/k8s/services/core/kustomization.yaml"))) { throw "Agentx deployment manifests are incomplete." }
}

function Assert-IsolationEvidence {
    param($Sandbox)
    if ($Sandbox.runtimeClass -eq "runc") { throw "Strong isolation requires a non-runc RuntimeClass." }
    $path = [string]$Sandbox.isolationEvidence
    if (-not $path -or -not [IO.Path]::IsPathRooted($path) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Strong isolation requires an absolute isolationEvidence file produced by verify-runtime-isolation.ps1."
    }
    $evidence = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json -Depth 20
    $expected = if ($Sandbox.runtimeClass -eq "custom") { [string]$Sandbox.runtimeClassName } else { [string]$Sandbox.runtimeClass }
    if ($evidence.status -ne "passed" -or $evidence.isolationLevel -ne "strong" -or $evidence.runtimeClass -ne $expected -or [int]$evidence.podCount -lt 1) {
        throw "Sandbox isolationEvidence does not prove the selected strong RuntimeClass."
    }
}

function Invoke-Doctor {
    param($Profile, [string]$RepoRoot, [switch]$DryRun)
    foreach ($command in @("kubectl")) { if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { throw "$command is required." } }
    if ($Profile.images.mode -eq "local-build") { if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { throw "docker is required for local-build image mode." }; docker info | Out-Null }
    $helm = Get-Helm -RepoRoot $RepoRoot
    kubectl cluster-info | Out-Null
    kubectl auth can-i create deployments --namespace $Profile.namespace | ForEach-Object { if ($_ -ne "yes") { throw "Current Kubernetes identity cannot create deployments in $($Profile.namespace)." } }
    Get-IngressChart -RepoRoot $RepoRoot | Out-Null
    & $helm template agentx-ingress-nginx (Get-IngressChart -RepoRoot $RepoRoot) --namespace agentx-ingress -f (Join-Path $RepoRoot "deploy/ingress-nginx/values.yaml") --set "controller.service.type=$($Profile.ingress.controllerServiceType)" | Out-Null
    $components = @("services/core", "services/migrations")
    foreach ($item in @(@("mysql", "mysql", "bundled"), @("redis", "redis", "bundled"), @("clickhouse", "clickhouse", "bundled"), @("objectStorage", "minio", "bundled-minio"))) { if ($Profile.components.$($item[0]).mode -eq $item[2]) { $components += "infrastructure/$($item[1])" } }
    if ($Profile.components.sandbox.mode -eq "remote") { $components += "services/sandbox-manager" }
    if ($Profile.components.rag.mode -eq "bundled") { $components += "addons/lightrag" }
    if ($Profile.components.memory.mode -eq "bundled") { $components += "addons/mem0" }
    if ($Profile.environment -eq "local" -and $Profile.components.rag.mode -eq "bundled") { $components += "fixtures/echo-mcp" }
    foreach ($component in $components | Select-Object -Unique) { Invoke-ComponentRender -Profile $Profile -RepoRoot $RepoRoot -Component $component | Out-Null }
    [pscustomobject]@{ status = "ready"; namespace = $Profile.namespace; context = kubectl config current-context; profileHash = Get-ProfileHash $Profile; dryRun = [bool]$DryRun } | ConvertTo-Json
}

function Invoke-Install {
    param($Profile, [string]$RepoRoot, [string]$Target, [switch]$DryRun, [switch]$RotateSecrets, [bool]$IsUpgrade, [string]$MigrationPhase, [switch]$MigrationOnly, [switch]$SkipMigrations)
    Invoke-Doctor -Profile $Profile -RepoRoot $RepoRoot -DryRun:$DryRun | Out-Null
    $previous = Get-DeployedProfile -Namespace $Profile.namespace
    if ($IsUpgrade -and -not $previous) { throw "Upgrade requires an existing agentx-deployment-state ConfigMap." }
    if ($previous) { Assert-StatefulModesUnchanged -Before $previous -After $Profile; Assert-SandboxTransitionAllowed -Before $previous -After $Profile }
    if ($RotateSecrets -and $Target -ne "all") { throw "Secret rotation requires -Target all so every consumer is restarted." }
    if ($MigrationOnly -and $Target -notin @("all", "services")) { throw "MigrationOnly requires Target all or services." }
    if ($MigrationOnly -and $SkipMigrations) { throw "MigrationOnly cannot be combined with SkipMigrations." }
    if ($RotateSecrets -and $Profile.secrets.mode -eq "existing") { throw "Secret rotation is not supported for secrets.mode=existing; rotate that Secret outside Agentx deploy." }
    if ($RotateSecrets -and $previous -and $previous.components.sandbox.mode -eq "remote") { Assert-SandboxDrained -Profile $previous }
    if ($DryRun) { Write-Output "Dry run passed for $($Profile.namespace); no Kubernetes resources were changed."; return }

    Ensure-Namespace -Namespace $Profile.namespace
    Ensure-Secrets -Profile $Profile -Rotate:$RotateSecrets
    Ensure-VaultSecret -Profile $Profile -Rotate:$RotateSecrets
    Ensure-TrustBundle -Profile $Profile
    Apply-AgentxConfig -Profile $Profile
    Apply-ExternalEgressPolicy -Profile $Profile
    if (Test-Target $Target "ingress") { Install-IngressController -Profile $Profile -RepoRoot $RepoRoot }
    if ($Profile.images.mode -eq "local-build") { Build-LocalImages -Profile $Profile -RepoRoot $RepoRoot -Target $Target }
    if (Test-Target $Target "infrastructure") { Apply-BundledInfrastructure -Profile $Profile -RepoRoot $RepoRoot }
    if ($Target -in @("all", "services", "infrastructure")) { Invoke-DependencyDoctor -Profile $Profile -RepoRoot $RepoRoot }
    if (Test-Target $Target "services") {
        if (-not $SkipMigrations) { Apply-Migrations -Profile $Profile -RepoRoot $RepoRoot -MigrationPhase $MigrationPhase }
        if ($MigrationOnly) { Write-Output "Migration phase '$MigrationPhase' completed in $($Profile.namespace)."; return }
        Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "services/core"
        Restart-ComponentDeployments -Namespace $Profile.namespace -Names @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer", "web")
        Wait-CoreServices -Profile $Profile
    }
    if (Test-Target $Target "sandbox") {
        if ($Profile.components.sandbox.mode -eq "remote") {
            Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "services/sandbox-manager"
            Restart-ComponentDeployments -Namespace $Profile.namespace -Names @("sandbox-manager")
            kubectl -n $Profile.namespace rollout status deployment/sandbox-manager --timeout=300s
            Invoke-SandboxDoctor -Profile $Profile -RepoRoot $RepoRoot
        } elseif ($previous -and $previous.components.sandbox.mode -eq "remote") {
            kubectl -n $Profile.namespace delete deployment,service -l "agentx.io/component=sandbox-manager,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found
        }
    }
    if (Test-Target $Target "addons") { Ensure-AddonSecrets -Profile $Profile -Rotate:$RotateSecrets; Apply-Addons -Profile $Profile -RepoRoot $RepoRoot }
    if (Test-Target $Target "addons") { Invoke-ExternalAddonDoctors -Profile $Profile }
    if (Test-Target $Target "ingress") { Apply-WebIngress -Profile $Profile }
    Save-DeploymentState -Profile $Profile -Target $Target
    Show-DeploymentStatus -Profile $Profile
}

function Test-Target { param([string]$Target, [string]$Expected) return $Target -eq "all" -or $Target -eq $Expected }

function Ensure-Namespace {
    param([string]$Namespace)
    $existingJson = kubectl get namespace $Namespace --ignore-not-found -o json 2>$null
    if ($existingJson) {
        $existing = $existingJson | ConvertFrom-Json
        if (Get-PropertyValue $existing.metadata "deletionTimestamp") {
            kubectl wait --for=delete "namespace/$Namespace" --timeout=300s | Out-Null
        } else {
            return
        }
    }
    $manifest = @{ apiVersion = "v1"; kind = "Namespace"; metadata = @{ name = $Namespace; labels = @{ "app.kubernetes.io/managed-by" = "agentx-deploy" }; annotations = @{ "agentx.io/owned" = "true" } } } | ConvertTo-Json -Depth 10
    $manifest | kubectl apply -f - | Out-Null
}

function Ensure-Secrets {
    param($Profile, [switch]$Rotate)
    $existingName = kubectl -n $Profile.namespace get secret $Profile.secrets.name --ignore-not-found -o name
    if ($Profile.secrets.mode -eq "existing") {
        if (-not $existingName) { throw "Profile requires an existing Secret named $($Profile.secrets.name)." }
        Assert-SecretKeys -Namespace $Profile.namespace -Name $Profile.secrets.name -Keys (Get-CoreSecretKeys -Profile $Profile)
        return
    }
    if ($existingName -and -not $Rotate) { Assert-SecretKeys -Namespace $Profile.namespace -Name $Profile.secrets.name -Keys (Get-CoreSecretKeys -Profile $Profile); return }
    if ($existingName) { Assert-SecretKeys -Namespace $Profile.namespace -Name $Profile.secrets.name -Keys (Get-CoreSecretKeys -Profile $Profile) }
    $data = @{}
    $hasExistingSecret = [bool]$existingName
    $data.AGENTX_MYSQL_PASSWORD = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_MYSQL_PASSWORD" -InputName "AGENTX_DEPLOY_MYSQL_PASSWORD" -Existing:$hasExistingSecret -PreserveExisting:($Profile.components.mysql.mode -eq "bundled") -Required:($Profile.components.mysql.mode -eq "external")
    $data.AGENTX_MYSQL_ROOT_PASSWORD = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_MYSQL_ROOT_PASSWORD" -InputName "AGENTX_DEPLOY_MYSQL_ROOT_PASSWORD" -Existing:$hasExistingSecret -PreserveExisting:$true
    $data.AGENTX_CLICKHOUSE_PASSWORD = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_CLICKHOUSE_PASSWORD" -InputName "AGENTX_DEPLOY_CLICKHOUSE_PASSWORD" -Existing:$hasExistingSecret -PreserveExisting:($Profile.components.clickhouse.mode -eq "bundled") -Required:($Profile.components.clickhouse.mode -eq "external")
    $data.AGENTX_S3_ACCESS_KEY = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_S3_ACCESS_KEY" -InputName "AGENTX_DEPLOY_S3_ACCESS_KEY" -Existing:$hasExistingSecret -PreserveExisting:($Profile.components.objectStorage.mode -eq "bundled-minio") -Required:($Profile.components.objectStorage.mode -eq "external-s3")
    $data.AGENTX_S3_SECRET_KEY = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_S3_SECRET_KEY" -InputName "AGENTX_DEPLOY_S3_SECRET_KEY" -Existing:$hasExistingSecret -PreserveExisting:($Profile.components.objectStorage.mode -eq "bundled-minio") -Required:($Profile.components.objectStorage.mode -eq "external-s3")
    $sessionToken = Get-OptionalDeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_S3_SESSION_TOKEN" -InputName "AGENTX_DEPLOY_S3_SESSION_TOKEN" -Existing:$hasExistingSecret
    if ($sessionToken) { $data.AGENTX_S3_SESSION_TOKEN = $sessionToken }
    $data.AGENTX_REDIS_PASSWORD = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_REDIS_PASSWORD" -InputName "AGENTX_DEPLOY_REDIS_PASSWORD" -Existing:$hasExistingSecret -PreserveExisting:($Profile.components.redis.mode -eq "bundled") -Required:($Profile.components.redis.mode -eq "external")
    $data.AGENTX_JWT_SIGNING_SECRET = New-RandomSecret 48
    $data.AGENTX_REMOTE_NODE_AUTH_TOKEN = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_REMOTE_NODE_AUTH_TOKEN" -InputName "AGENTX_DEPLOY_REMOTE_NODE_AUTH_TOKEN" -Existing:$hasExistingSecret -PreserveExisting:$true
    if ($Profile.secrets.provider -eq "vault_kv_v2") {
        $data.AGENTX_CREDENTIAL_BROKER_TOKEN = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_CREDENTIAL_BROKER_TOKEN" -InputName "AGENTX_DEPLOY_CREDENTIAL_BROKER_TOKEN" -Existing:$hasExistingSecret -PreserveExisting:$true
    } else {
        $data.AGENTX_CREDENTIAL_ACTIVE_KEY_ID = "deploy-v1"
        $credentialKey = if ($Rotate -and $existingName) { Get-ExistingSecretValue -Namespace $Profile.namespace -Name $Profile.secrets.name -Key "AGENTX_CREDENTIAL_KEYS_JSON" } else { $null }
        $data.AGENTX_CREDENTIAL_KEYS_JSON = if ($credentialKey) { $credentialKey } else { (@{ keys = @{ "deploy-v1" = [Convert]::ToBase64String((New-RandomBytes 32)) } } | ConvertTo-Json -Compress) }
    }
    if ($Profile.components.sandbox.mode -eq "remote") {
        $data.AGENTX_OPENSANDBOX_API_KEY = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $Profile.secrets.name -Key "AGENTX_OPENSANDBOX_API_KEY" -InputName "AGENTX_DEPLOY_OPENSANDBOX_API_KEY" -Existing:$hasExistingSecret -Required:$true
        $data.AGENTX_SANDBOX_RPC_TOKEN = New-RandomSecret 32
        $data.AGENTX_SANDBOX_LEASE_SIGNING_KEY = New-RandomSecret 32
        if ($Profile.secrets.provider -eq "vault_kv_v2") {
            $data.AGENTX_SANDBOX_ENDPOINT_ACTIVE_KEY_ID = "sandbox-endpoint-v1"
            $endpointKey = if ($Rotate -and $existingName) { Get-ExistingSecretValue -Namespace $Profile.namespace -Name $Profile.secrets.name -Key "AGENTX_SANDBOX_ENDPOINT_KEYS_JSON" } else { $null }
            $data.AGENTX_SANDBOX_ENDPOINT_KEYS_JSON = if ($endpointKey) { $endpointKey } else { (@{ keys = @{ "sandbox-endpoint-v1" = [Convert]::ToBase64String((New-RandomBytes 32)) } } | ConvertTo-Json -Compress) }
        }
    }
    Apply-Secret -Namespace $Profile.namespace -Name $Profile.secrets.name -Data $data -Labels (Managed-Labels)
}

function Ensure-VaultSecret {
    param($Profile, [switch]$Rotate)
    $name = $Profile.secrets.vaultName
    if ($Profile.secrets.provider -ne "vault_kv_v2") {
        if ($Profile.secrets.mode -eq "managed") { Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name $name }
        return
    }
    $existing = [bool](kubectl -n $Profile.namespace get secret $name --ignore-not-found -o name)
    if ($Profile.secrets.mode -eq "existing") {
        if (-not $existing) { throw "Profile requires an existing Secret named $name." }
        Assert-SecretKeys -Namespace $Profile.namespace -Name $name -Keys @("AGENTX_VAULT_TOKEN")
        return
    }
    if ($existing -and -not $Rotate) {
        Assert-SecretKeys -Namespace $Profile.namespace -Name $name -Keys @("AGENTX_VAULT_TOKEN")
        return
    }
    $token = Get-DeploySecretValue -Namespace $Profile.namespace -SecretName $name -Key "AGENTX_VAULT_TOKEN" -InputName "AGENTX_DEPLOY_VAULT_TOKEN" -Existing:$existing -PreserveExisting:$false -Required:$true
    Apply-Secret -Namespace $Profile.namespace -Name $name -Data @{ AGENTX_VAULT_TOKEN = $token } -Labels (Managed-Labels)
}

function Get-CoreSecretKeys {
    param($Profile)
    $keys = @("AGENTX_MYSQL_PASSWORD", "AGENTX_REDIS_PASSWORD", "AGENTX_CLICKHOUSE_PASSWORD", "AGENTX_S3_ACCESS_KEY", "AGENTX_S3_SECRET_KEY", "AGENTX_JWT_SIGNING_SECRET", "AGENTX_REMOTE_NODE_AUTH_TOKEN")
    if ($Profile.secrets.provider -eq "vault_kv_v2") { $keys += @("AGENTX_CREDENTIAL_BROKER_TOKEN") } else { $keys += @("AGENTX_CREDENTIAL_ACTIVE_KEY_ID", "AGENTX_CREDENTIAL_KEYS_JSON") }
    if ($Profile.components.mysql.mode -eq "bundled") { $keys += "AGENTX_MYSQL_ROOT_PASSWORD" }
    if ($Profile.components.sandbox.mode -eq "remote") {
        $keys += @("AGENTX_OPENSANDBOX_API_KEY", "AGENTX_SANDBOX_RPC_TOKEN", "AGENTX_SANDBOX_LEASE_SIGNING_KEY")
        if ($Profile.secrets.provider -eq "vault_kv_v2") { $keys += @("AGENTX_SANDBOX_ENDPOINT_ACTIVE_KEY_ID", "AGENTX_SANDBOX_ENDPOINT_KEYS_JSON") }
    }
    return $keys
}

function Ensure-AddonSecrets {
    param($Profile, [switch]$Rotate)
    if ($Profile.secrets.mode -eq "existing") {
        if ($Profile.components.rag.mode -eq "bundled") { Assert-SecretKeys -Namespace $Profile.namespace -Name $Profile.secrets.lightragName -Keys @("LIGHTRAG_API_KEY", "LLM_BINDING_API_KEY", "EMBEDDING_BINDING_API_KEY") }
        if ($Profile.components.memory.mode -eq "bundled") { Assert-SecretKeys -Namespace $Profile.namespace -Name $Profile.secrets.mem0Name -Keys @("POSTGRES_PASSWORD", "OPENAI_API_KEY", "JWT_SECRET") }
        return
    }
    if ($Profile.components.rag.mode -eq "bundled") {
        $name = $Profile.secrets.lightragName; $existing = kubectl -n $Profile.namespace get secret $name --ignore-not-found -o name
        if (-not $existing -or $Rotate) {
            $providerKey = Get-AddonProviderSecret -Name "AGENTX_DEPLOY_LIGHTRAG_OPENAI_API_KEY" -Local ($Profile.environment -eq "local")
            $apiKey = if ($existing) { Get-ExistingSecretValue -Namespace $Profile.namespace -Name $name -Key "LIGHTRAG_API_KEY" } else { Get-RequiredOrGeneratedSecret "AGENTX_DEPLOY_LIGHTRAG_API_KEY" $false }
            Apply-Secret -Namespace $Profile.namespace -Name $name -Data @{ LIGHTRAG_API_KEY = $apiKey; LLM_BINDING_API_KEY = $providerKey; EMBEDDING_BINDING_API_KEY = $providerKey } -Labels (Addon-Labels "lightrag")
        } else { Assert-SecretKeys -Namespace $Profile.namespace -Name $name -Keys @("LIGHTRAG_API_KEY", "LLM_BINDING_API_KEY", "EMBEDDING_BINDING_API_KEY") }
    }
    if ($Profile.components.memory.mode -eq "bundled") {
        $name = $Profile.secrets.mem0Name; $existing = kubectl -n $Profile.namespace get secret $name --ignore-not-found -o name
        if (-not $existing -or $Rotate) {
            $postgresPassword = if ($existing) { Get-ExistingSecretValue -Namespace $Profile.namespace -Name $name -Key "POSTGRES_PASSWORD" } else { Get-RequiredOrGeneratedSecret "AGENTX_DEPLOY_MEM0_POSTGRES_PASSWORD" $false }
            $jwtSecret = if ($existing) { Get-ExistingSecretValue -Namespace $Profile.namespace -Name $name -Key "JWT_SECRET" } else { New-RandomSecret 48 }
            Apply-Secret -Namespace $Profile.namespace -Name $name -Data @{ POSTGRES_PASSWORD = $postgresPassword; OPENAI_API_KEY = (Get-AddonProviderSecret -Name "AGENTX_DEPLOY_MEM0_OPENAI_API_KEY" -Local ($Profile.environment -eq "local")); JWT_SECRET = $jwtSecret } -Labels (Addon-Labels "mem0")
        } else { Assert-SecretKeys -Namespace $Profile.namespace -Name $name -Keys @("POSTGRES_PASSWORD", "OPENAI_API_KEY", "JWT_SECRET") }
    }
}

function Apply-Secret {
    param([string]$Namespace, [string]$Name, [hashtable]$Data, [hashtable]$Labels)
    $encoded = @{}; foreach ($entry in $Data.GetEnumerator()) { $encoded[$entry.Key] = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$entry.Value)) }
    @{ apiVersion = "v1"; kind = "Secret"; metadata = @{ name = $Name; namespace = $Namespace; labels = $Labels }; type = "Opaque"; data = $encoded } | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}

function Assert-SecretKeys {
    param([string]$Namespace, [string]$Name, [string[]]$Keys)
    $raw = kubectl -n $Namespace get secret $Name -o json
    if (-not $raw) { throw "Secret $Namespace/$Name was not found." }
    $present = @((($raw | ConvertFrom-Json).data).PSObject.Properties.Name)
    foreach ($key in $Keys) { if ($key -notin $present) { throw "Secret $Namespace/$Name is missing required key $key." } }
}

function Get-ExistingSecretValue { param([string]$Namespace, [string]$Name, [string]$Key) $raw = kubectl -n $Namespace get secret $Name -o json | ConvertFrom-Json; $value = Get-PropertyValue $raw.data $Key; if (-not $value) { return $null }; return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String([string]$value)) }
function Get-RequiredOrGeneratedSecret { param([string]$Name, [bool]$Required) if (Test-Path "Env:$Name") { return (Get-Item "Env:$Name").Value }; if ($Required) { throw "$Name is required." }; return New-RandomSecret 32 }
function Get-DeploySecretValue {
    param([string]$Namespace, [string]$SecretName, [string]$Key, [string]$InputName, [bool]$Existing, [bool]$PreserveExisting, [bool]$Required = $false)
    if ($Existing -and $PreserveExisting) { return Get-ExistingSecretValue -Namespace $Namespace -Name $SecretName -Key $Key }
    return Get-RequiredOrGeneratedSecret -Name $InputName -Required $Required
}
function Get-OptionalDeploySecretValue {
    param([string]$Namespace, [string]$SecretName, [string]$Key, [string]$InputName, [bool]$Existing)
    if (Test-Path "Env:$InputName") { return (Get-Item "Env:$InputName").Value }
    if ($Existing) { return Get-ExistingSecretValue -Namespace $Namespace -Name $SecretName -Key $Key }
    return $null
}
function Get-AddonProviderSecret { param([string]$Name, [bool]$Local) if (Test-Path "Env:$Name") { return (Get-Item "Env:$Name").Value }; if ($Local) { return "m5-model-secret" }; throw "$Name is required." }
function New-RandomBytes { param([int]$Length) $bytes = [byte[]]::new($Length); [Security.Cryptography.RandomNumberGenerator]::Fill($bytes); return $bytes }
function New-RandomSecret {
    param([int]$Length)
    return [Convert]::ToBase64String((New-RandomBytes $Length)).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}
function Managed-Labels { return @{ "app.kubernetes.io/part-of" = "agentx"; "app.kubernetes.io/managed-by" = "agentx-deploy" } }
function Addon-Labels { param([string]$Name) $labels = Managed-Labels; $labels["agentx.io/component"] = $Name; return $labels }

function Ensure-TrustBundle {
    param($Profile)
    $files = @(Get-ProfileCertificateFiles -Profile $Profile)
    if (-not $files) { Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name "agentx-trust-bundle"; return }
    $data = @{}; foreach ($file in $files) { $data[$file.Key] = [IO.File]::ReadAllText($file.Path) }
    Apply-Secret -Namespace $Profile.namespace -Name "agentx-trust-bundle" -Data $data -Labels (Managed-Labels)
}

function Get-ProfileCertificateFiles {
    param($Profile)
    foreach ($name in @("mysql", "redis", "clickhouse", "objectStorage")) {
        $tls = $Profile.components.$name.tls
        if (-not $tls) { continue }
        foreach ($field in @("caFile", "clientCertFile", "clientKeyFile")) { $path = Get-PropertyValue $tls $field; if ($path) { [pscustomobject]@{ Name = "$name $field"; Key = "$name-$field.pem".ToLowerInvariant(); Path = [string]$path } } }
    }
}

function Apply-AgentxConfig {
    param($Profile)
    $sandbox = $Profile.components.sandbox
    $ragProvider = Get-PropertyValue $Profile.components.rag "provider"; $memoryProvider = Get-PropertyValue $Profile.components.memory "provider"
    $hosts = @($Profile.components.rag.endpoint, $Profile.components.memory.endpoint, (Get-PropertyValue $ragProvider "baseUrl"), (Get-PropertyValue $memoryProvider "baseUrl")) | Where-Object { $_ } | ForEach-Object { try { ([Uri]$_).Host } catch { $null } } | Where-Object { $_ } | Select-Object -Unique
    $data = @{
        AGENTX_ENV = $Profile.environment; AGENTX_MYSQL_HOST = $Profile.components.mysql.host; AGENTX_MYSQL_PORT = [string]$Profile.components.mysql.port; AGENTX_MYSQL_DATABASE = $Profile.components.mysql.database; AGENTX_MYSQL_USER = $Profile.components.mysql.user; AGENTX_MYSQL_TLS_MODE = $Profile.components.mysql.tlsMode
        AGENTX_REDIS_URL = $Profile.components.redis.url; AGENTX_CLICKHOUSE_URL = $Profile.components.clickhouse.url; AGENTX_CLICKHOUSE_DATABASE = $Profile.components.clickhouse.database; AGENTX_CLICKHOUSE_USER = $Profile.components.clickhouse.user
        AGENTX_S3_ENDPOINT = $Profile.components.objectStorage.endpoint; AGENTX_S3_BUCKET = $Profile.components.objectStorage.bucket; AGENTX_S3_REGION = $Profile.components.objectStorage.region; AGENTX_S3_ALLOW_HTTP = ([string]$Profile.components.objectStorage.allowHttp).ToLowerInvariant(); AGENTX_S3_PATH_STYLE = ([string]$Profile.components.objectStorage.pathStyle).ToLowerInvariant()
        AGENTX_RUNTIME_COORDINATOR_URL = "http://workflow-coordinator:9090"; AGENTX_NODE_BROKER_URL = "http://workflow-worker:8080"; AGENTX_CREDENTIAL_BROKER_URL = "http://platform-api:8080"; AGENTX_SECRET_PROVIDER = $Profile.secrets.provider; AGENTX_NODE_HANDLE_TTL_SECONDS = "300"; AGENTX_CHECKPOINT_ARTIFACT_THRESHOLD_BYTES = "65536"; AGENTX_WORKER_CAPABILITIES = "builtin,declarative_http,remote_action,agent,model,mcp_tool,skill,rag,memory,sandbox"
        AGENTX_COOKIE_SECURE = $(if ($Profile.environment -eq "production") { "true" } else { "false" }); AGENTX_CONNECTION_ALLOW_PRIVATE_NETWORKS = $(if ($Profile.environment -eq "local") { "true" } else { "false" }); AGENTX_CONNECTION_ALLOWED_HOSTS = ($hosts -join ","); AGENTX_CONNECTION_ALLOWED_CIDRS = ""
    }
    if ($Profile.secrets.provider -eq "vault_kv_v2") {
        $data.AGENTX_VAULT_ADDR = $Profile.secrets.vaultAddress
        $data.AGENTX_VAULT_KV_MOUNT = $Profile.secrets.vaultMount
    }
    Add-TlsConfig $data "MYSQL" $Profile.components.mysql.tls "mysql"; Add-TlsConfig $data "REDIS" $Profile.components.redis.tls "redis"; Add-TlsConfig $data "CLICKHOUSE" $Profile.components.clickhouse.tls "clickhouse"; Add-TlsConfig $data "S3" $Profile.components.objectStorage.tls "objectstorage"
    if ($sandbox.mode -eq "remote") {
        $data.AGENTX_SANDBOX_MANAGER_URL = "http://sandbox-manager:9091"; $data.AGENTX_OPENSANDBOX_ENDPOINT = $sandbox.endpoint; $data.AGENTX_OPENSANDBOX_SECURE_ACCESS = ([string]$sandbox.secureAccess).ToLowerInvariant(); $data.AGENTX_OPENSANDBOX_USE_SERVER_PROXY = ([string]$sandbox.useServerProxy).ToLowerInvariant(); $data.AGENTX_OPENSANDBOX_ALLOWED_ENDPOINT_HOSTS = @($sandbox.allowedHosts) -join ","; $data.AGENTX_OPENSANDBOX_ALLOWED_ENDPOINT_CIDRS = @($sandbox.allowedCidrs) -join ","; $data.AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT = "16"
    }
    @{ apiVersion = "v1"; kind = "ConfigMap"; metadata = @{ name = "agentx-config"; namespace = $Profile.namespace; labels = (Managed-Labels) }; data = $data } | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}

function Add-TlsConfig { param([hashtable]$Data, [string]$Prefix, $Tls, [string]$KeyPrefix) if (-not $Tls) { return }; if (Get-PropertyValue $Tls "caFile") { $Data["AGENTX_${Prefix}_TLS_CA_PATH"] = "/etc/agentx/trust/$KeyPrefix-cafile.pem" }; if (Get-PropertyValue $Tls "clientCertFile") { $Data["AGENTX_${Prefix}_TLS_CLIENT_CERT_PATH"] = "/etc/agentx/trust/$KeyPrefix-clientcertfile.pem"; $Data["AGENTX_${Prefix}_TLS_CLIENT_KEY_PATH"] = "/etc/agentx/trust/$KeyPrefix-clientkeyfile.pem" } }

function Build-LocalImages {
    param($Profile, [string]$RepoRoot, [string]$Target)
    $services = @(); $buildWeb = Test-Target $Target "services"
    if ($buildWeb) { $services += @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer") }
    if ((Test-Target $Target "sandbox") -and $Profile.components.sandbox.mode -eq "remote") { $services += "sandbox-manager" }
    if ((Test-Target $Target "addons") -and $Profile.environment -eq "local" -and $Profile.components.rag.mode -eq "bundled") { $services += "echo-mcp" }
    if ($services.Count -gt 0 -or $buildWeb) {
        if ($buildWeb) { & (Join-Path $RepoRoot "scripts/build-images.ps1") -Tag $Profile.images.tag -Namespace $Profile.namespace -Services ($services | Select-Object -Unique) }
        else { & (Join-Path $RepoRoot "scripts/build-images.ps1") -Tag $Profile.images.tag -Namespace $Profile.namespace -Services ($services | Select-Object -Unique) -SkipWeb }
    }
    if ((Test-Target $Target "addons") -and $Profile.components.memory.mode -eq "bundled") { & (Join-Path $RepoRoot "scripts/build-images.ps1") -Tag $Profile.images.tag -Namespace $Profile.namespace -Services @() -SkipWeb -BuildMem0 }
}

function Apply-BundledInfrastructure {
    param($Profile, [string]$RepoRoot)
    foreach ($item in @(@("mysql", "mysql", "bundled"), @("redis", "redis", "bundled"), @("clickhouse", "clickhouse", "bundled"), @("objectStorage", "minio", "bundled-minio"))) {
        if ($Profile.components.$($item[0]).mode -ne $item[2]) { continue }
        Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "infrastructure/$($item[1])"
        kubectl -n $Profile.namespace rollout status "statefulset/$($item[1])" --timeout=420s
        if ($item[1] -eq "minio") { kubectl -n $Profile.namespace wait --for=condition=complete job/minio-bucket-init --timeout=300s }
    }
}

function Apply-Migrations {
    param($Profile, [string]$RepoRoot, [ValidateSet("all", "expand", "contract")][string]$MigrationPhase = "all")
    kubectl -n $Profile.namespace delete job platform-api-migrate trace-writer-migrate --ignore-not-found | Out-Null
    $arguments = if ($MigrationPhase -eq "expand") { @("migrate", "--through", "16") } else { @("migrate") }
    $rendered = Invoke-ComponentRender -Profile $Profile -RepoRoot $RepoRoot -Component "services/migrations" -MigrationArguments $arguments
    $rendered | kubectl apply -f - | Out-Null
    kubectl -n $Profile.namespace wait --for=condition=complete job/platform-api-migrate --timeout=300s
    kubectl -n $Profile.namespace wait --for=condition=complete job/trace-writer-migrate --timeout=300s
}

function Apply-Addons {
    param($Profile, [string]$RepoRoot)
    if ($Profile.environment -eq "local" -and $Profile.components.rag.mode -eq "bundled") { Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "fixtures/echo-mcp"; kubectl -n $Profile.namespace rollout status deployment/echo-mcp --timeout=300s }
    if ($Profile.components.rag.mode -eq "bundled") {
        $lightragExisted = Test-DeploymentExists -Namespace $Profile.namespace -Name "lightrag"
        Apply-AddonConfig -Profile $Profile -Addon "rag"
        Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "addons/lightrag"
        if ($lightragExisted) { Restart-ComponentDeployments -Namespace $Profile.namespace -Names @("lightrag") }
        kubectl -n $Profile.namespace rollout status deployment/lightrag --timeout=600s
    }
    else { Remove-AddonRuntime -Profile $Profile -Addon "rag" }
    if ($Profile.components.memory.mode -eq "bundled") {
        $mem0Existed = Test-DeploymentExists -Namespace $Profile.namespace -Name "mem0"
        Apply-AddonConfig -Profile $Profile -Addon "memory"
        Apply-Component -Profile $Profile -RepoRoot $RepoRoot -Component "addons/mem0"
        if ($mem0Existed) { Restart-ComponentDeployments -Namespace $Profile.namespace -Names @("mem0") }
        kubectl -n $Profile.namespace rollout status deployment/mem0-postgres --timeout=420s
        kubectl -n $Profile.namespace rollout status deployment/mem0 --timeout=600s
    }
    else { Remove-AddonRuntime -Profile $Profile -Addon "memory" }
}

function Apply-AddonConfig {
    param($Profile, [ValidateSet("rag", "memory")][string]$Addon)
    $component = $Profile.components.$Addon; $provider = $component.provider
    if ($Addon -eq "rag") {
        $data = @{ HOST = "0.0.0.0"; PORT = "9621"; WORKING_DIR = "/app/data/rag_storage"; INPUT_DIR = "/app/data/inputs"; PROMPT_DIR = "/app/data/prompts"; LLM_BINDING = "openai"; EMBEDDING_BINDING = "openai"; LLM_BINDING_HOST = $provider.baseUrl; EMBEDDING_BINDING_HOST = $provider.baseUrl; LLM_MODEL = $provider.llmModel; EMBEDDING_MODEL = $provider.embeddingModel; EMBEDDING_DIM = [string]$provider.embeddingDimension; EMBEDDING_USE_BASE64 = "false" }; $name = "agentx-lightrag-config"
    } else {
        $data = @{ POSTGRES_HOST = "mem0-postgres"; POSTGRES_PORT = "5432"; POSTGRES_DB = "postgres"; POSTGRES_USER = "postgres"; POSTGRES_COLLECTION_NAME = "memories"; APP_DB_NAME = "mem0_app"; AUTH_DISABLED = "true"; MEM0_TELEMETRY = "false"; HISTORY_DB_PATH = "/app/history/history.db"; OPENAI_BASE_URL = $provider.baseUrl; MEM0_DEFAULT_LLM_MODEL = $provider.llmModel; MEM0_DEFAULT_EMBEDDER_MODEL = $provider.embeddingModel }; $name = "agentx-mem0-config"
    }
    $labelName = if ($Addon -eq "rag") { "lightrag" } else { "mem0" }
    @{ apiVersion = "v1"; kind = "ConfigMap"; metadata = @{ name = $name; namespace = $Profile.namespace; labels = (Addon-Labels $labelName) }; data = $data } | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}

function Remove-AddonRuntime {
    param($Profile, [string]$Addon)
    if ($Addon -eq "rag") { $names = @("lightrag"); $component = "lightrag"; $config = "agentx-lightrag-config"; $secret = $Profile.secrets.lightragName } else { $names = @("mem0", "mem0-postgres"); $component = "mem0"; $config = "agentx-mem0-config"; $secret = $Profile.secrets.mem0Name }
    kubectl -n $Profile.namespace delete deployment,service -l "agentx.io/component=$component,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found | Out-Null
    Remove-ManagedResource -Namespace $Profile.namespace -Kind configmap -Name $config -Component $component
    Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name $secret -Component $component
}

function Apply-Component {
    param($Profile, [string]$RepoRoot, [string]$Component)
    $rendered = Invoke-ComponentRender -Profile $Profile -RepoRoot $RepoRoot -Component $Component
    $rendered | kubectl apply -f - | Out-Null
}

function Invoke-ComponentRender {
    param($Profile, [string]$RepoRoot, [string]$Component, [string[]]$MigrationArguments)
    $temp = Join-Path $RepoRoot ("deploy/k8s/.agentx-render-" + [Guid]::NewGuid().ToString("N")); New-Item -ItemType Directory -Path $temp | Out-Null
    try {
        $resolvedComponent = (Resolve-Path (Join-Path $RepoRoot "deploy/k8s/$Component")).Path
        $componentPath = [IO.Path]::GetRelativePath($temp, $resolvedComponent).Replace('\', '/')
        $lines = @("apiVersion: kustomize.config.k8s.io/v1beta1", "kind: Kustomization", "namespace: $($Profile.namespace)", "resources:", "  - $componentPath", "images:")
        foreach ($name in $script:ServiceImages) {
            $newName = if ($Profile.images.mode -eq "registry") { "$($Profile.images.registry.TrimEnd('/'))/$name" } else { "agentx/$name" }
            $digest = Get-PropertyValue $Profile.images.digests $name
            $lines += @("  - name: agentx/$name", "    newName: $newName")
            if ($digest) { $lines += "    digest: $digest" } else { $lines += "    newTag: $($Profile.images.tag)" }
        }
        if ($Component -eq "addons/mem0") { $newName = if ($Profile.images.mode -eq "registry") { "$($Profile.images.registry.TrimEnd('/'))/mem0-server" } else { "agentx/mem0-server" }; $lines += @("  - name: agentx/mem0-server", "    newName: $newName", "    newTag: v2.0.15") }
        $policy = $Profile.images.pullPolicy
        $workloads = @(Get-ComponentWorkloads -Component $Component)
        if ($workloads.Count -gt 0) { $lines += "patches:" }
        foreach ($workload in $workloads) { $lines += @("  - target:", "      kind: $($workload.kind)", "      name: $($workload.name)", "    patch: |-", "      - op: add", "        path: /spec/template/spec/containers/0/imagePullPolicy", "        value: $policy") }
        if ($Component -eq "services/migrations" -and $MigrationArguments) {
            $argumentsJson = ConvertTo-Json -InputObject ([string[]]$MigrationArguments) -Compress
            $lines += @("  - target:", "      kind: Job", "      name: platform-api-migrate", "    patch: |-", "      - op: replace", "        path: /spec/template/spec/containers/0/args", "        value: $argumentsJson")
        }
        if ($Component -eq "addons/mem0") { $lines += @("  - target:", "      kind: Deployment", "      name: mem0", "    patch: |-", "      - op: add", "        path: /spec/template/spec/initContainers/0/imagePullPolicy", "        value: $policy") }
        [IO.File]::WriteAllLines((Join-Path $temp "kustomization.yaml"), $lines, [Text.UTF8Encoding]::new($false))
        return (kubectl kustomize $temp --load-restrictor LoadRestrictionsNone)
    } finally { Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue }
}

function Get-ComponentWorkloads {
    param([string]$Component)
    switch ($Component) {
        "services/core" { return @(@{ kind = "Deployment"; name = "web" }, @{ kind = "Deployment"; name = "platform-api" }, @{ kind = "Deployment"; name = "trigger-gateway" }, @{ kind = "Deployment"; name = "workflow-coordinator" }, @{ kind = "Deployment"; name = "workflow-worker" }, @{ kind = "Deployment"; name = "trace-writer" }) }
        "services/migrations" { return @(@{ kind = "Job"; name = "platform-api-migrate" }, @{ kind = "Job"; name = "trace-writer-migrate" }) }
        "services/sandbox-manager" { return @(@{ kind = "Deployment"; name = "sandbox-manager" }) }
        "fixtures/echo-mcp" { return @(@{ kind = "Deployment"; name = "echo-mcp" }) }
        "fixtures/echo-node" { return @(@{ kind = "Deployment"; name = "echo-node" }) }
        "addons/lightrag" { return @(@{ kind = "Deployment"; name = "lightrag" }) }
        "addons/mem0" { return @(@{ kind = "Deployment"; name = "mem0" }, @{ kind = "Deployment"; name = "mem0-postgres" }) }
        default { return @() }
    }
}

function Restart-ComponentDeployments { param([string]$Namespace, [string[]]$Names) foreach ($name in $Names) { kubectl -n $Namespace rollout restart "deployment/$name" | Out-Null } }
function Test-DeploymentExists { param([string]$Namespace, [string]$Name) return [bool](kubectl -n $Namespace get deployment $Name --ignore-not-found -o name) }
function Wait-CoreServices { param($Profile) foreach ($name in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer", "web")) { kubectl -n $Profile.namespace rollout status "deployment/$name" --timeout=300s } }

function Invoke-DependencyDoctor {
    param($Profile, [string]$RepoRoot)
    $image = Get-AgentxImage -Profile $Profile -Name "platform-api"
    Invoke-DoctorJob -Profile $Profile -Name "agentx-infrastructure-doctor" -Image $image -Arguments @("doctor-infrastructure") -Component "doctor"
}
function Invoke-SandboxDoctor { param($Profile, [string]$RepoRoot) Invoke-DoctorJob -Profile $Profile -Name "agentx-opensandbox-doctor" -Image (Get-AgentxImage -Profile $Profile -Name "sandbox-manager") -Arguments @("doctor-opensandbox") -Component "doctor" }
function Invoke-ExternalAddonDoctors {
    param($Profile)
    foreach ($name in @("rag", "memory")) { $endpoint = $Profile.components.$name.endpoint; if ($Profile.components.$name.mode -ne "external" -or -not $endpoint) { continue }; Invoke-DoctorJob -Profile $Profile -Name "agentx-$name-connectivity-doctor" -Image $script:CurlImage -Arguments @("--output", "/dev/null", "--write-out", "%{http_code}", "--max-time", "15", $endpoint) -Component "doctor" }
}
function Get-AgentxImage {
    param($Profile, [string]$Name)
    $base = if ($Profile.images.mode -eq "registry") { "$($Profile.images.registry.TrimEnd('/'))/$Name" } else { "agentx/$Name" }
    $digest = Get-PropertyValue $Profile.images.digests $Name
    if ($digest) { return "$base@$digest" }
    return "$base`:$($Profile.images.tag)"
}

function Apply-ExternalEgressPolicy {
    param($Profile)
    $name = "agentx-controlled-external-egress"
    $cidrs = @($Profile.network.allowedEgressCidrs)
    if ($cidrs.Count -eq 0) {
        kubectl -n $Profile.namespace delete networkpolicy $name --ignore-not-found | Out-Null
        return
    }
    $egress = @()
    foreach ($cidr in $cidrs) { $egress += @{ to = @(@{ ipBlock = @{ cidr = $cidr } }) } }
    $labels = Managed-Labels
    $labels["agentx.io/component"] = "core"
    $policy = @{
        apiVersion = "networking.k8s.io/v1"
        kind = "NetworkPolicy"
        metadata = @{ name = $name; namespace = $Profile.namespace; labels = $labels }
        spec = @{
            podSelector = @{ matchExpressions = @(@{ key = "agentx.io/component"; operator = "In"; values = @("core", "sandbox-manager") }) }
            policyTypes = @("Egress")
            egress = $egress
        }
    }
    $policy | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}
function Invoke-DoctorJob {
    param($Profile, [string]$Name, [string]$Image, [string[]]$Arguments, [string]$Component)
    kubectl -n $Profile.namespace delete job $Name --ignore-not-found --wait=true | Out-Null
    $job = @{ apiVersion = "batch/v1"; kind = "Job"; metadata = @{ name = $Name; namespace = $Profile.namespace; labels = (Addon-Labels $Component) }; spec = @{ backoffLimit = 0; ttlSecondsAfterFinished = 300; template = @{ metadata = @{ labels = (Addon-Labels $Component) }; spec = @{ restartPolicy = "Never"; containers = @(@{ name = "doctor"; image = $Image; imagePullPolicy = $Profile.images.pullPolicy; args = $Arguments; envFrom = @(@{ configMapRef = @{ name = "agentx-config" } }, @{ secretRef = @{ name = $Profile.secrets.name } }); volumeMounts = @(@{ name = "trust"; mountPath = "/etc/agentx/trust"; readOnly = $true }) }); volumes = @(@{ name = "trust"; secret = @{ secretName = "agentx-trust-bundle"; optional = $true } }) } } } }
    $job | ConvertTo-Json -Depth 30 | kubectl apply -f - | Out-Null
    try { kubectl -n $Profile.namespace wait --for=condition=complete "job/$Name" --timeout=180s | Out-Null; kubectl -n $Profile.namespace logs "job/$Name" } catch { kubectl -n $Profile.namespace logs "job/$Name" --all-containers=true; throw }
}

function Install-IngressController {
    param($Profile, [string]$RepoRoot)
    $helm = Get-Helm -RepoRoot $RepoRoot; $chart = Get-IngressChart -RepoRoot $RepoRoot
    $namespaceExisted = [bool](kubectl get namespace agentx-ingress --ignore-not-found -o name)
    $releaseExists = $false
    try { & $helm status agentx-ingress-nginx -n agentx-ingress 2>$null | Out-Null; $releaseExists = $true } catch { $releaseExists = $false }
    if ($releaseExists -and -not (Test-ManagedIngressController)) { throw "Ingress release agentx-ingress/agentx-ingress-nginx exists without Agentx ownership; refusing to take it over." }
    & $helm upgrade --install agentx-ingress-nginx $chart --namespace agentx-ingress --create-namespace --version $script:IngressChartVersion -f (Join-Path $RepoRoot "deploy/ingress-nginx/values.yaml") --set "controller.service.type=$($Profile.ingress.controllerServiceType)" --wait --timeout 10m | Out-Null
    if (-not $namespaceExisted) { kubectl label namespace agentx-ingress "app.kubernetes.io/managed-by=agentx-deploy" --overwrite | Out-Null; kubectl annotate namespace agentx-ingress "agentx.io/owned=true" --overwrite | Out-Null }
    @{ apiVersion = "v1"; kind = "ConfigMap"; metadata = @{ name = "agentx-ingress-ownership"; namespace = "agentx-ingress"; labels = (Managed-Labels) }; data = @{ release = "agentx-ingress-nginx"; ingressClass = "agentx-nginx" } } | ConvertTo-Json -Depth 20 | kubectl apply -f - | Out-Null
}

function Test-ManagedIngressController {
    $raw = kubectl -n agentx-ingress get configmap agentx-ingress-ownership --ignore-not-found -o json 2>$null
    if (-not $raw) { return $false }
    return Test-IsManagedResource -Resource ($raw | ConvertFrom-Json)
}

function Get-Helm {
    param([string]$RepoRoot)
    $existing = Get-Command helm -ErrorAction SilentlyContinue
    if ($existing) { return $existing.Source }
    $runtime = [Runtime.InteropServices.RuntimeInformation]
    $os = if ($runtime::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Windows)) { "windows" } elseif ($runtime::IsOSPlatform([Runtime.InteropServices.OSPlatform]::OSX)) { "darwin" } else { "linux" }
    $arch = if ($runtime::OSArchitecture -eq [Runtime.InteropServices.Architecture]::Arm64) { "arm64" } else { "amd64" }
    $platform = "$os-$arch"; if (-not $script:HelmDigests.ContainsKey($platform)) { throw "Helm auto-download is not supported on $platform; install Helm $($script:HelmVersion) manually." }
    $extension = if ($os -eq "windows") { "zip" } else { "tar.gz" }; $asset = "helm-v$($script:HelmVersion)-$platform.$extension"; $cache = Join-Path $RepoRoot ".local/deploy-cache/helm/$platform"; $binary = Join-Path $cache $(if ($os -eq "windows") { "helm.exe" } else { "helm" })
    if (-not (Test-Path $binary)) {
        New-Item -ItemType Directory -Force -Path $cache | Out-Null; $archive = Join-Path $cache $asset
        Invoke-WebRequest -UseBasicParsing -Uri "https://get.helm.sh/$asset" -OutFile $archive
        if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $script:HelmDigests[$platform]) { Remove-Item $archive -Force; throw "Helm archive SHA-256 mismatch." }
        if ($os -eq "windows") { Expand-Archive -LiteralPath $archive -DestinationPath $cache -Force; Move-Item -Force (Join-Path $cache "windows-amd64/helm.exe") $binary } else { tar -xzf $archive -C $cache; Move-Item -Force (Join-Path $cache "$platform/helm") $binary }
        Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    }
    return $binary
}

function Get-IngressChart {
    param([string]$RepoRoot)
    $cache = Join-Path $RepoRoot ".local/deploy-cache"; New-Item -ItemType Directory -Force -Path $cache | Out-Null; $chart = Join-Path $cache "ingress-nginx-$($script:IngressChartVersion).tgz"
    if (-not (Test-Path $chart) -or (Get-FileHash $chart -Algorithm SHA256).Hash.ToLowerInvariant() -ne $script:IngressChartDigest) { Invoke-WebRequest -UseBasicParsing -Uri $script:IngressChartUrl -OutFile $chart }
    if ((Get-FileHash $chart -Algorithm SHA256).Hash.ToLowerInvariant() -ne $script:IngressChartDigest) { throw "ingress-nginx chart SHA-256 mismatch." }; return $chart
}

function Apply-WebIngress {
    param($Profile)
    $spec = @{ ingressClassName = "agentx-nginx"; rules = @(@{ host = $Profile.ingress.host; http = @{ paths = @(@{ path = "/"; pathType = "Prefix"; backend = @{ service = @{ name = "web"; port = @{ number = 80 } } } }) } }) }; if ($Profile.ingress.tlsSecretName) { $spec.tls = @(@{ hosts = @($Profile.ingress.host); secretName = $Profile.ingress.tlsSecretName }) }
    @{ apiVersion = "networking.k8s.io/v1"; kind = "Ingress"; metadata = @{ name = "agentx-web"; namespace = $Profile.namespace; labels = (Addon-Labels "ingress") }; spec = $spec } | ConvertTo-Json -Depth 30 | kubectl apply -f - | Out-Null
}

function Save-DeploymentState {
    param($Profile, [string]$Target)
    $profileJson = $Profile | ConvertTo-Json -Depth 30 -Compress; $data = @{ "profile.json" = $profileJson; "profile.sha256" = Get-ProfileHash $Profile; "lastTarget" = $Target; "updatedAt" = [DateTimeOffset]::UtcNow.ToString("O") }
    @{ apiVersion = "v1"; kind = "ConfigMap"; metadata = @{ name = "agentx-deployment-state"; namespace = $Profile.namespace; labels = (Managed-Labels) }; data = $data } | ConvertTo-Json -Depth 30 | kubectl apply -f - | Out-Null
}

function Get-DeployedProfile { param([string]$Namespace) $json = kubectl -n $Namespace get configmap agentx-deployment-state --ignore-not-found -o json 2>$null; if (-not $json) { return $null }; return (($json | ConvertFrom-Json).data.'profile.json' | ConvertFrom-Json -Depth 30) }
function Get-CanonicalValue {
    param($Value)
    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Collections.IDictionary]) { $ordered = [ordered]@{}; foreach ($key in $Value.Keys | Sort-Object) { $ordered[[string]$key] = Get-CanonicalValue $Value[$key] }; return $ordered }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) { return @($Value | ForEach-Object { Get-CanonicalValue $_ }) }
    if ($Value -is [pscustomobject]) { $ordered = [ordered]@{}; foreach ($property in $Value.PSObject.Properties | Sort-Object Name) { $ordered[$property.Name] = Get-CanonicalValue $property.Value }; return $ordered }
    return $Value
}
function Get-ProfileHash { param($Profile) $json = (Get-CanonicalValue $Profile | ConvertTo-Json -Depth 30 -Compress); return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($json))).ToLowerInvariant() }

function Assert-StatefulModesUnchanged { param($Before, $After) foreach ($name in $script:StatefulModes) { if ($Before.components.$name.mode -ne $After.components.$name.mode) { throw "Upgrade cannot change $name mode. Follow the documented data migration and reinstall procedure." } } }
function Assert-SandboxDrained { param($Profile) $active = kubectl -n $Profile.namespace get deployment sandbox-manager --ignore-not-found -o name; if (-not $active) { throw "Cannot verify Sandbox leases because the existing Sandbox Manager is unavailable." }; kubectl -n $Profile.namespace exec deployment/sandbox-manager -- /usr/local/bin/agentx-service doctor-drain | Out-Null }
function Assert-SandboxTransitionAllowed { param($Before, $After) if ($Before.components.sandbox.mode -eq "remote" -and $After.components.sandbox.mode -eq "disabled") { Assert-SandboxDrained -Profile $Before } }

function Show-DeploymentStatus {
    param($Profile)
    Write-Output "Agentx namespace: $($Profile.namespace)"; Write-Output "Ingress: $($Profile.ingress.host)"; kubectl -n $Profile.namespace get deployment,statefulset,job,service,ingress,pvc -l app.kubernetes.io/part-of=agentx -o wide; kubectl -n agentx-ingress get deployment,service -l app.kubernetes.io/name=ingress-nginx -o wide --ignore-not-found
}

function Invoke-Uninstall {
    param($Profile, [string]$RepoRoot, [string]$Target, [switch]$DeleteData, [switch]$DeleteNamespace, [switch]$DryRun, [switch]$NonInteractive)
    if ($DryRun) { Write-Output "Would uninstall target '$Target' from namespace '$($Profile.namespace)' without touching external dependencies."; return }
    if (($DeleteData -or $DeleteNamespace) -and -not $NonInteractive) { $answer = Read-Host "Type the namespace name '$($Profile.namespace)' to confirm destructive uninstall"; if ($answer -cne $Profile.namespace) { throw "Uninstall cancelled." } }
    if ($Target -in @("all", "services")) { kubectl -n $Profile.namespace delete deployment,service,networkpolicy -l "agentx.io/component=core,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found; kubectl -n $Profile.namespace delete job -l "agentx.io/component=migration,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found }
    if ($Target -in @("all", "addons")) { foreach ($component in @("lightrag", "mem0")) { kubectl -n $Profile.namespace delete deployment,service -l "agentx.io/component=$component,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found; Remove-ManagedResource -Namespace $Profile.namespace -Kind configmap -Name $(if ($component -eq "lightrag") { "agentx-lightrag-config" } else { "agentx-mem0-config" }) -Component $component; Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name $(if ($component -eq "lightrag") { $Profile.secrets.lightragName } else { $Profile.secrets.mem0Name }) -Component $component } }
    if ($Target -in @("all", "infrastructure")) { foreach ($component in @("mysql", "redis", "clickhouse", "object-storage")) { kubectl -n $Profile.namespace delete statefulset,service,job -l "agentx.io/component=$component,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found } }
    if ($Target -in @("all", "sandbox")) { $manager = kubectl -n $Profile.namespace get deployment sandbox-manager --ignore-not-found -o name; if ($manager) { kubectl -n $Profile.namespace exec deployment/sandbox-manager -- /usr/local/bin/agentx-service doctor-drain | Out-Null }; kubectl -n $Profile.namespace delete deployment,service,networkpolicy -l "agentx.io/component=sandbox-manager,app.kubernetes.io/managed-by=agentx-deploy" --ignore-not-found }
    if ($DeleteData) { $claims = @(); if ($Target -in @("all", "infrastructure")) { $claims += @("data-mysql-0", "data-redis-0", "data-clickhouse-0", "data-minio-0") }; if ($Target -in @("all", "addons")) { $claims += @("lightrag-data", "mem0-history-data", "mem0-postgres-data") }; foreach ($claim in $claims) { Remove-ManagedPvc -Namespace $Profile.namespace -Name $claim } }
    if ($Target -eq "all") { Remove-ManagedResource -Namespace $Profile.namespace -Kind configmap -Name "agentx-config"; Remove-ManagedResource -Namespace $Profile.namespace -Kind configmap -Name "agentx-deployment-state"; Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name $Profile.secrets.name; Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name $Profile.secrets.vaultName; Remove-ManagedResource -Namespace $Profile.namespace -Kind secret -Name "agentx-trust-bundle" }
    if ($Target -in @("all", "ingress")) {
        Remove-ManagedResource -Namespace $Profile.namespace -Kind ingress -Name "agentx-web" -Component "ingress"
        $other = kubectl get ingress -A -o json | ConvertFrom-Json
        $users = @($other.items | Where-Object { $_.spec.ingressClassName -eq "agentx-nginx" })
        if (-not $users -and (Test-ManagedIngressController)) {
            $helm = Get-Helm -RepoRoot $RepoRoot
            & $helm uninstall agentx-ingress-nginx -n agentx-ingress --ignore-not-found | Out-Null
            $ingressNamespace = kubectl get namespace agentx-ingress --ignore-not-found -o json 2>$null | ConvertFrom-Json
            if ($ingressNamespace -and (Get-PropertyValue $ingressNamespace.metadata.annotations "agentx.io/owned") -eq "true") {
                kubectl delete namespace agentx-ingress --wait=false | Out-Null
                try {
                    kubectl wait --for=delete namespace/agentx-ingress --timeout=$(if ($Profile.environment -eq "local") { "120s" } else { "300s" }) | Out-Null
                }
                catch {
                    if ($Profile.environment -ne "local") { throw }
                    $controller = kubectl -n agentx-ingress get service agentx-ingress-nginx-controller --ignore-not-found -o json 2>$null | ConvertFrom-Json
                    if ($controller) {
                        $finalizers = @($controller.metadata.finalizers)
                        if ($finalizers -contains "service.kubernetes.io/load-balancer-cleanup") {
                            kubectl -n agentx-ingress patch service agentx-ingress-nginx-controller --type=merge -p '{"metadata":{"finalizers":[]}}' | Out-Null
                        }
                    }
                    if (kubectl get namespace agentx-ingress --ignore-not-found -o name) {
                        kubectl wait --for=delete namespace/agentx-ingress --timeout=60s | Out-Null
                    }
                }
            }
            else { Remove-ManagedResource -Namespace "agentx-ingress" -Kind configmap -Name "agentx-ingress-ownership" }
        }
    }
    if ($DeleteNamespace) { $owned = kubectl get namespace $Profile.namespace -o json --ignore-not-found | ConvertFrom-Json; if (-not $owned -or (Get-PropertyValue $owned.metadata.annotations "agentx.io/owned") -ne "true") { throw "Namespace $($Profile.namespace) is not owned by Agentx deploy; refusing to delete it." }; kubectl delete namespace $Profile.namespace --ignore-not-found --wait=true }
}

function Test-IsManagedResource { param($Resource, [string]$Component) if ((Get-PropertyValue $Resource.metadata.labels "app.kubernetes.io/managed-by") -ne "agentx-deploy") { return $false }; if ($Component -and (Get-PropertyValue $Resource.metadata.labels "agentx.io/component") -ne $Component) { return $false }; return $true }
function Remove-ManagedResource { param([string]$Namespace, [string]$Kind, [string]$Name, [string]$Component) $raw = kubectl -n $Namespace get $Kind $Name --ignore-not-found -o json 2>$null; if (-not $raw) { return }; $item = $raw | ConvertFrom-Json; if (-not (Test-IsManagedResource -Resource $item -Component $Component)) { return }; kubectl -n $Namespace delete $Kind $Name --ignore-not-found | Out-Null }
function Remove-ManagedPvc { param([string]$Namespace, [string]$Name) Remove-ManagedResource -Namespace $Namespace -Kind pvc -Name $Name }

Export-ModuleMember -Function Invoke-AgentxDeployment
