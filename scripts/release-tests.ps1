$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$scriptPath = Join-Path $root "scripts/release-images.ps1"
$schemaPath = Join-Path $root "deploy/release/release-manifest.schema.json"
foreach ($candidate in @(
    $scriptPath,
    (Join-Path $root "scripts/verify-runtime-isolation.ps1"),
    (Join-Path $root "scripts/vault-integration.ps1"),
    (Join-Path $root "scripts/m7-capacity.ps1"),
    (Join-Path $root "scripts/m7-failure-tests.ps1"),
    (Join-Path $root "scripts/m7-security-tests.ps1"),
    (Join-Path $root "scripts/m7-upgrade-tests.ps1"),
    (Join-Path $root "scripts/m7-release-gate.ps1")
)) {
    $tokens = $null
    $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($candidate, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -ne 0) { throw "$candidate has parse errors: $($errors -join '; ')" }
}

$capacity = [ordered]@{
    schemaVersion = "agentx.io/m7-capacity-evidence/v1"
    status = "passed"
    runId = "m7-release-test"
    startedAt = [DateTimeOffset]::UtcNow.AddHours(-2).ToString("O")
    completedAt = [DateTimeOffset]::UtcNow.ToString("O")
    metrics = [ordered]@{
        executionCount = 100
        completedExecutionCount = 100
        nodeExecutionCount = 500
        sseClientCount = 200
        successfulSseClientCount = 200
        evaluationCaseCount = 1000
        workflowNodeCount = 200
        stabilityDurationSeconds = 7200
        stabilityProbeCount = 120
        projectorP95Seconds = 1.25
        activeQuotaReservations = 0
    }
    failures = @()
} | ConvertTo-Json -Depth 20
if (-not ($capacity | Test-Json -SchemaFile (Join-Path $root "deploy/release/m7-capacity-evidence.schema.json"))) {
    throw "valid M7 Capacity evidence was rejected"
}

foreach ($type in @("failure", "security", "upgrade")) {
    $operational = [ordered]@{
        schemaVersion = "agentx.io/m7-operational-evidence/v1"
        evidenceType = $type
        status = "passed"
        runId = "m7-release-test"
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @([ordered]@{ name = "test"; status = "passed"; evidence = "release-tests.ps1" })
    } | ConvertTo-Json -Depth 20
    if (-not ($operational | Test-Json -SchemaFile (Join-Path $root "deploy/release/m7-operational-evidence.schema.json"))) {
        throw "valid $type operational evidence was rejected"
    }
}

$digest = "sha256:" + ("a" * 64)
$images = foreach ($name in @("web", "platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer")) {
    [ordered]@{
        name = $name
        reference = "registry.example.test/agentx/$name@$digest"
        digest = $digest
        sbom = "$name.cdx.json"
        sbomSha256 = "b" * 64
        signatureVerified = $true
    }
}
$manifest = [ordered]@{
    schemaVersion = "agentx.io/release/v1"
    version = "0.1.0-rc.1"
    gitCommit = "c" * 40
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    isolationLevel = "standard"
    runtimeClass = "runc"
    images = @($images)
} | ConvertTo-Json -Depth 20
if (-not ($manifest | Test-Json -SchemaFile $schemaPath)) { throw "valid Release Manifest was rejected" }

$invalid = $manifest | ConvertFrom-Json -Depth 20
$invalid.images[0].digest = "latest"
if (($invalid | ConvertTo-Json -Depth 20) | Test-Json -SchemaFile $schemaPath -ErrorAction SilentlyContinue) {
    throw "Release Manifest schema accepted a mutable image reference"
}

try {
    & $scriptPath -Version test -Registry registry.example.test/agentx -CosignKey test.pub -RuntimeClass runc -IsolationLevel strong -SkipBuild
    throw "release script accepted runc as strong isolation"
}
catch {
    if ($_.Exception.Message -eq "release script accepted runc as strong isolation") { throw }
}

[ordered]@{ status = "passed"; imageCount = $images.Count } | ConvertTo-Json
