param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Registry,
    [Parameter(Mandatory = $true)][string]$CosignKey,
    [string]$OutputDirectory = "artifacts/release",
    [ValidateSet("runc", "gvisor", "kata", "custom")][string]$RuntimeClass = "runc",
    [string]$RuntimeClassName,
    [ValidateSet("standard", "strong")][string]$IsolationLevel = "standard",
    [string]$IsolationEvidence,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$services = @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer")

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "$Name is required." }
}

function Invoke-Native([string]$Name, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE." }
}

if ($IsolationLevel -eq "strong") {
    if ($RuntimeClass -eq "runc") { throw "Strong isolation requires a verified non-runc RuntimeClass." }
    if (-not $IsolationEvidence -or -not (Test-Path -LiteralPath $IsolationEvidence -PathType Leaf)) {
        throw "Strong isolation requires an isolation evidence file."
    }
    $evidence = Get-Content -Raw -LiteralPath $IsolationEvidence | ConvertFrom-Json -Depth 20
    $expectedRuntimeClass = if ($RuntimeClass -eq "custom") { $RuntimeClassName } else { $RuntimeClass }
    if ($evidence.status -ne "passed" -or $evidence.isolationLevel -ne "strong" -or $evidence.runtimeClass -ne $expectedRuntimeClass -or [int]$evidence.podCount -lt 1) {
        throw "Isolation evidence does not prove the selected RuntimeClass."
    }
}
if ($RuntimeClass -eq "custom" -and -not $RuntimeClassName) {
    throw "A custom RuntimeClass requires RuntimeClassName."
}

foreach ($command in @("docker", "syft", "cosign", "git")) { Require-Command $command }
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Path $output -Force | Out-Null
$registryBase = $Registry.TrimEnd('/')
$images = @()

Push-Location $root
try {
    foreach ($service in $services + @("web")) {
        $tagged = "$registryBase/$service`:$Version"
        if (-not $SkipBuild) {
            if ($service -eq "web") {
                Invoke-Native "build $service" { docker build --file deploy/docker/web.Dockerfile --tag $tagged . }
            } else {
                Invoke-Native "build $service" { docker build --file deploy/docker/backend.Dockerfile --build-arg "APP=$service" --tag $tagged . }
            }
            Invoke-Native "push $service" { docker push $tagged }
        }
        $inspection = docker buildx imagetools inspect $tagged --format "{{json .Manifest}}" | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or -not $inspection.digest -or $inspection.digest -notmatch '^sha256:[a-f0-9]{64}$') {
            throw "Could not resolve an immutable digest for $tagged."
        }
        $digest = [string]$inspection.digest
        $reference = "$registryBase/$service@$digest"
        $sbomName = "$service.cdx.json"
        $sbomPath = Join-Path $output $sbomName
        Invoke-Native "SBOM $service" { syft $reference -o "cyclonedx-json=$sbomPath" }
        Invoke-Native "sign $service" { cosign sign --yes --key $CosignKey $reference }
        Invoke-Native "attest SBOM $service" { cosign attest --yes --key $CosignKey --type cyclonedx --predicate $sbomPath $reference }
        Invoke-Native "verify $service" { cosign verify --key $CosignKey $reference | Out-Null }
        $images += [ordered]@{
            name = $service
            reference = $reference
            digest = $digest
            sbom = $sbomName
            sbomSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $sbomPath).Hash.ToLowerInvariant()
            signatureVerified = $true
        }
    }
    $runtimeClassValue = if ($RuntimeClass -eq "custom") { $RuntimeClassName } else { $RuntimeClass }
    $manifest = [ordered]@{
        schemaVersion = "agentx.io/release/v1"
        version = $Version
        gitCommit = (git rev-parse HEAD).Trim()
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        isolationLevel = $IsolationLevel
        runtimeClass = $runtimeClassValue
        images = $images
    }
    $manifestPath = Join-Path $output "release-manifest.json"
    $manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/release-manifest.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $manifestPath) | Test-Json -SchemaFile $schema)) {
        throw "Generated Release Manifest does not match $schema."
    }
    $signaturePath = "$manifestPath.sig"
    Invoke-Native "sign Release Manifest" { cosign sign-blob --yes --key $CosignKey --output-signature $signaturePath $manifestPath }
    Invoke-Native "verify Release Manifest" { cosign verify-blob --key $CosignKey --signature $signaturePath $manifestPath | Out-Null }
    Write-Output $manifestPath
}
finally {
    Pop-Location
}
