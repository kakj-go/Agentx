param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Registry,
    [Parameter(Mandatory = $true)][string]$CosignPrivateKey,
    [Parameter(Mandatory = $true)][string]$CosignPublicKey,
    [string]$SourceDirectory,
    [string]$OutputDirectory,
    [ValidateSet("runc", "gvisor", "kata", "custom")][string]$RuntimeClass = "runc",
    [string]$RuntimeClassName,
    [ValidateSet("standard", "strong")][string]$IsolationLevel = "standard",
    [string]$IsolationEvidence,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$source = if ($SourceDirectory) { (Resolve-Path -LiteralPath $SourceDirectory -ErrorAction Stop).Path } else { $root }
$services = @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web")

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "$Name is required." }
}

function Invoke-Native([string]$Name, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE." }
}

function Get-CanonicalValue($Value) {
    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Collections.IDictionary]) {
        $ordered = [ordered]@{}
        foreach ($key in $Value.Keys | Sort-Object) { $ordered[[string]$key] = Get-CanonicalValue $Value[$key] }
        return $ordered
    }
    if ($Value -is [pscustomobject]) {
        $ordered = [ordered]@{}
        foreach ($property in $Value.PSObject.Properties | Sort-Object Name) { $ordered[$property.Name] = Get-CanonicalValue $property.Value }
        return $ordered
    }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        return @($Value | ForEach-Object { Get-CanonicalValue $_ })
    }
    return $Value
}

function Get-JsonCanonicalHash($Value) {
    $json = Get-CanonicalValue $Value | ConvertTo-Json -Depth 100 -Compress
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($json))).ToLowerInvariant()
}

function Read-AttestationStatements([string[]]$Output) {
    $raw = ($Output -join "`n").Trim()
    if (-not $raw) { throw "Cosign returned no attestation payload." }
    $items = @($raw | ConvertFrom-Json -Depth 100)
    foreach ($item in $items) {
        $payload = if ($item.payload) { [string]$item.payload } elseif ($item.dsseEnvelope.payload) { [string]$item.dsseEnvelope.payload } else { $null }
        if (-not $payload) { continue }
        $json = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($payload))
        $json | ConvertFrom-Json -Depth 100
    }
}

function Assert-CycloneDxAttestation([string]$Reference, [string]$Digest, [string]$SbomPath) {
    $verification = @(& cosign verify-attestation --key $CosignPublicKey --type cyclonedx --output json $Reference)
    if ($LASTEXITCODE -ne 0) { throw "Attestation verification failed for $Reference." }
    $expectedDigest = $Digest.Substring("sha256:".Length)
    $localSbom = Get-Content -Raw -LiteralPath $SbomPath | ConvertFrom-Json -Depth 100
    $localCanonicalHash = Get-JsonCanonicalHash $localSbom
    $matching = $null
    foreach ($statement in Read-AttestationStatements $verification) {
        $subjectMatches = @($statement.subject | Where-Object { $_.digest.sha256 -eq $expectedDigest }).Count -gt 0
        if ($subjectMatches -and $statement.predicateType -match 'cyclonedx|cyclone') { $matching = $statement; break }
    }
    if (-not $matching) { throw "No CycloneDX statement targets $Digest." }
    if ($matching.predicate.bomFormat -ne "CycloneDX") { throw "Attestation predicate is not a CycloneDX SBOM." }
    $attestedHash = Get-JsonCanonicalHash $matching.predicate
    if ($attestedHash -ne $localCanonicalHash) { throw "Attested SBOM content does not match $SbomPath." }
    [ordered]@{ predicateType = [string]$matching.predicateType; canonicalSha256 = $attestedHash }
}

if ($IsolationLevel -eq "strong") {
    if ($RuntimeClass -eq "runc") { throw "Strong isolation requires a verified non-runc RuntimeClass." }
    if (-not $IsolationEvidence -or -not (Test-Path -LiteralPath $IsolationEvidence -PathType Leaf)) {
        throw "Strong isolation requires an isolation evidence file."
    }
    $isolation = Get-Content -Raw -LiteralPath $IsolationEvidence | ConvertFrom-Json -Depth 20
    $expectedRuntimeClass = if ($RuntimeClass -eq "custom") { $RuntimeClassName } else { $RuntimeClass }
    if ($isolation.status -ne "passed" -or $isolation.isolationLevel -ne "strong" -or $isolation.runtimeClass -ne $expectedRuntimeClass -or [int]$isolation.podCount -lt 1) {
        throw "Isolation evidence does not prove the selected RuntimeClass."
    }
}
if ($RuntimeClass -eq "custom" -and -not $RuntimeClassName) { throw "A custom RuntimeClass requires RuntimeClassName." }

foreach ($command in @("docker", "syft", "cosign", "git")) { Require-Command $command }
foreach ($key in @($CosignPrivateKey, $CosignPublicKey)) {
    if (-not (Test-Path -LiteralPath $key -PathType Leaf)) { throw "Cosign key does not exist: $key" }
}
$dirty = @(git -C $source status --porcelain --untracked-files=normal)
if ($LASTEXITCODE -ne 0) { throw "SourceDirectory is not a Git worktree: $source" }
if ($dirty.Count -gt 0) { throw "SourceDirectory must be clean before release: $source" }
$sourceCommit = ([string](git -C $source rev-parse HEAD)).Trim().ToLowerInvariant()
if ($sourceCommit -notmatch '^[a-f0-9]{40}$') { throw "Could not resolve the source commit for $source." }

$output = if ($OutputDirectory) {
    if ([IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory } else { Join-Path $root $OutputDirectory }
} else {
    Join-Path $root "artifacts/release/$Version"
}
New-Item -ItemType Directory -Path $output -Force | Out-Null
$registryBase = $Registry.TrimEnd('/')
$images = [Collections.Generic.List[object]]::new()

Push-Location $source
try {
    foreach ($service in $services) {
        $tagged = "$registryBase/$service`:$Version"
        if (-not $SkipBuild) {
            if ($service -eq "web") {
                Invoke-Native "build $service" { docker build --file deploy/docker/web.Dockerfile --tag $tagged . }
            } else {
                Invoke-Native "build $service" { docker build --file deploy/docker/backend.Dockerfile --build-arg "APP=$service" --tag $tagged . }
            }
            Invoke-Native "push $service" { docker push $tagged }
        }
        $inspectionRaw = docker buildx imagetools inspect $tagged --format "{{json .Manifest}}"
        if ($LASTEXITCODE -ne 0) { throw "Could not inspect $tagged." }
        $inspection = $inspectionRaw | ConvertFrom-Json
        if (-not $inspection.digest -or [string]$inspection.digest -notmatch '^sha256:[a-f0-9]{64}$') { throw "Could not resolve an immutable digest for $tagged." }
        $digest = [string]$inspection.digest
        $reference = "$registryBase/$service@$digest"
        $sbomName = "$service.cdx.json"
        $sbomPath = Join-Path $output $sbomName
        Invoke-Native "SBOM $service" { syft $reference -o "cyclonedx-json=$sbomPath" }
        $sbom = Get-Content -Raw -LiteralPath $sbomPath | ConvertFrom-Json -Depth 100
        if ($sbom.bomFormat -ne "CycloneDX") { throw "$service SBOM is not CycloneDX." }
        Invoke-Native "sign $service" { cosign sign --yes --key $CosignPrivateKey $reference }
        Invoke-Native "attest SBOM $service" { cosign attest --yes --key $CosignPrivateKey --type cyclonedx --predicate $sbomPath $reference }
        Invoke-Native "verify $service" { cosign verify --key $CosignPublicKey $reference | Out-Null }
        $attestation = Assert-CycloneDxAttestation $reference $digest $sbomPath
        $images.Add([ordered]@{
            name = $service
            reference = $reference
            digest = $digest
            sbom = $sbomName
            sbomSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $sbomPath).Hash.ToLowerInvariant()
            sbomCanonicalSha256 = $attestation.canonicalSha256
            predicateType = $attestation.predicateType
            signatureVerified = $true
            attestationVerified = $true
        })
    }
} finally {
    Pop-Location
}

$runtimeClassValue = if ($RuntimeClass -eq "custom") { $RuntimeClassName } else { $RuntimeClass }
$manifest = [ordered]@{
    schemaVersion = "agentx.io/release/v1"
    version = $Version
    gitCommit = $sourceCommit
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    isolationLevel = $IsolationLevel
    runtimeClass = $runtimeClassValue
    images = @($images)
}
$manifestPath = Join-Path $output "release-manifest.json"
$manifest | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
$schema = Join-Path $root "deploy/release/release-manifest.schema.json"
if (-not ((Get-Content -Raw -LiteralPath $manifestPath) | Test-Json -SchemaFile $schema)) { throw "Generated Release Manifest does not match $schema." }
$signaturePath = "$manifestPath.sig"
Invoke-Native "sign Release Manifest" { cosign sign-blob --yes --key $CosignPrivateKey --output-signature $signaturePath $manifestPath }
Invoke-Native "verify Release Manifest" { cosign verify-blob --key $CosignPublicKey --signature $signaturePath $manifestPath | Out-Null }

$evidence = [ordered]@{
    schemaVersion = "agentx.io/supply-chain-evidence/v1"
    status = "passed"
    environment = "local-docker-desktop"
    registry = "local-tls"
    trustScope = "local-only"
    isolationLevel = $IsolationLevel
    version = $Version
    sourceCommit = $sourceCommit
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    imageCount = $images.Count
    signedImageCount = @($images | Where-Object signatureVerified).Count
    attestedImageCount = @($images | Where-Object attestationVerified).Count
    manifestSignatureVerified = $true
}
$evidencePath = Join-Path $output "supply-chain-evidence.json"
$evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $evidencePath -Encoding utf8NoBOM
$evidenceSchema = Join-Path $root "deploy/release/supply-chain-evidence.schema.json"
if (-not ((Get-Content -Raw -LiteralPath $evidencePath) | Test-Json -SchemaFile $evidenceSchema)) { throw "Generated supply-chain evidence is invalid." }
Write-Output $manifestPath
