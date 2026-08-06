param(
    [Parameter(Mandatory = $true)][string]$ReleaseManifest,
    [Parameter(Mandatory = $true)][string]$CosignPrivateKey,
    [Parameter(Mandatory = $true)][string]$CosignPublicKey,
    [Parameter(Mandatory = $true)][string]$WrongCosignPublicKey,
    [string]$OutputDirectory = "artifacts/m7/local/supply-chain-negative"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest -ErrorAction Stop).Path
$manifestRaw = Get-Content -Raw -LiteralPath $manifestPath
$manifestSchema = Join-Path $root "deploy/release/release-manifest.schema.json"
if (-not ($manifestRaw | Test-Json -SchemaFile $manifestSchema)) { throw "Release Manifest is invalid." }
$manifest = $manifestRaw | ConvertFrom-Json -Depth 50
$output = if ([IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory } else { Join-Path $root $OutputDirectory }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$assertions = [Collections.Generic.List[object]]::new()
$temporary = Join-Path ([IO.Path]::GetTempPath()) "agentx-m7-negative-$runId"
$localNegativeImages = [Collections.Generic.List[string]]::new()
New-Item -ItemType Directory -Path $temporary -Force | Out-Null

function Add-Passed([string]$Name, [string]$Detail) {
    $detailPath = Join-Path $output "$Name.txt"
    $Detail | Set-Content -LiteralPath $detailPath -Encoding utf8NoBOM
    $assertions.Add([ordered]@{ name = $Name; status = "passed"; evidence = [IO.Path]::GetRelativePath($root, $detailPath).Replace('\', '/') })
}

function Invoke-ExpectedNativeFailure([string]$Name, [scriptblock]$Action) {
    $succeeded = $false
    try {
        & $Action 2>$null | Out-Null
        $succeeded = $LASTEXITCODE -eq 0
    } catch {
        $succeeded = $false
    }
    if ($succeeded) { throw "$Name unexpectedly succeeded." }
    Add-Passed $Name "The intentionally invalid operation was rejected."
}

function Get-RegistryBase([string]$Reference) {
    $withoutDigest = $Reference -replace '@sha256:[a-f0-9]{64}$', ''
    $withoutDigest.Substring(0, $withoutDigest.LastIndexOf('/'))
}

try {
    $first = $manifest.images[0]
    $manifestDirectory = Split-Path -Parent $manifestPath

    $tamperedSbom = Join-Path $temporary $first.sbom
    Copy-Item -LiteralPath (Join-Path $manifestDirectory $first.sbom) -Destination $tamperedSbom
    Add-Content -LiteralPath $tamperedSbom -Value " " -NoNewline
    $tamperedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $tamperedSbom).Hash.ToLowerInvariant()
    if ($tamperedHash -eq $first.sbomSha256) { throw "Tampered SBOM retained the original hash." }
    Add-Passed "tampered_sbom" "Manifest SBOM hash check rejected modified bytes."

    $tamperedManifest = Join-Path $temporary "release-manifest.json"
    Copy-Item -LiteralPath $manifestPath -Destination $tamperedManifest
    Add-Content -LiteralPath $tamperedManifest -Value " " -NoNewline
    Invoke-ExpectedNativeFailure "tampered_manifest" {
        cosign verify-blob --key $CosignPublicKey --signature "$manifestPath.sig" $tamperedManifest
    }

    Invoke-ExpectedNativeFailure "wrong_public_key" {
        cosign verify --key $WrongCosignPublicKey $first.reference
    }

    $registryBase = Get-RegistryBase $first.reference
    docker pull $first.reference | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not pull source image for negative tests." }

    $unsignedTag = "$registryBase/negative-unsigned:$runId"
    docker tag $first.reference $unsignedTag
    $localNegativeImages.Add($unsignedTag)
    docker push $unsignedTag | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not push unsigned negative image." }
    $unsignedDigest = [string]((docker buildx imagetools inspect $unsignedTag --format "{{json .Manifest}}" | ConvertFrom-Json).digest)
    $unsignedReference = "$registryBase/negative-unsigned@$unsignedDigest"
    $localNegativeImages.Add($unsignedReference)
    Invoke-ExpectedNativeFailure "missing_image_signature" {
        cosign verify --key $CosignPublicKey $unsignedReference
    }

    $noAttestationTag = "$registryBase/negative-no-attestation:$runId"
    docker tag $first.reference $noAttestationTag
    $localNegativeImages.Add($noAttestationTag)
    docker push $noAttestationTag | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not push no-Attestation negative image." }
    $noAttestationDigest = [string]((docker buildx imagetools inspect $noAttestationTag --format "{{json .Manifest}}" | ConvertFrom-Json).digest)
    $noAttestationReference = "$registryBase/negative-no-attestation@$noAttestationDigest"
    $localNegativeImages.Add($noAttestationReference)
    cosign sign --yes --key $CosignPrivateKey $noAttestationReference | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not sign no-Attestation negative image." }
    Invoke-ExpectedNativeFailure "missing_sbom_attestation" {
        cosign verify-attestation --key $CosignPublicKey --type cyclonedx $noAttestationReference
    }

    $tagManifest = $manifestRaw | ConvertFrom-Json -Depth 50
    $tagManifest.images[0].reference = "$registryBase/$($first.name):mutable"
    $tagManifest.images[0].digest = "latest"
    if (($tagManifest | ConvertTo-Json -Depth 50) | Test-Json -SchemaFile $manifestSchema -ErrorAction SilentlyContinue) {
        throw "Release Manifest schema accepted a mutable Tag."
    }
    Add-Passed "mutable_tag" "Release Manifest schema rejected a mutable Tag."

    $missingDigest = "sha256:" + ("f" * 64)
    Invoke-ExpectedNativeFailure "nonexistent_digest" {
        docker pull "$registryBase/$($first.name)@$missingDigest"
    }

    Invoke-ExpectedNativeFailure "unsigned_candidate_gate" {
        cosign verify --key $CosignPublicKey $unsignedReference
    }

    $evidence = [ordered]@{
        schemaVersion = "agentx.io/supply-chain-negative-evidence/v1"
        status = "passed"
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @($assertions)
    }
    $evidencePath = Join-Path $output "negative-evidence.json"
    $evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $evidencePath -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/supply-chain-negative-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $evidencePath) | Test-Json -SchemaFile $schema)) { throw "Negative supply-chain evidence is invalid." }
    Write-Output $evidencePath
} finally {
    foreach ($image in $localNegativeImages) {
        try { docker image rm $image 2>$null | Out-Null } catch {}
    }
    Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
