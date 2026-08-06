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
    (Join-Path $root "scripts/m7-local-prereqs.ps1"),
    (Join-Path $root "scripts/m7-local-release-tests.ps1"),
    (Join-Path $root "scripts/m7-local-seed.ps1"),
    (Join-Path $root "scripts/m7-invocation-probe.ps1"),
    (Join-Path $root "scripts/test-release-negative-cases.ps1"),
    (Join-Path $root "scripts/m7-release-gate.ps1")
)) {
    $tokens = $null
    $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($candidate, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -ne 0) { throw "$candidate has parse errors: $($errors -join '; ')" }
}

$localHarness = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/m7-local-release-tests.ps1")
$upgradeHarness = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/m7-upgrade-tests.ps1")
if ($localHarness -notmatch '\$profile\.components\.sandbox\.mode\s*=\s*"remote"' -or $localHarness -notmatch '-RequireSandbox') {
    throw "M7 local acceptance must deploy and require the remote Sandbox Manager."
}
if ($localHarness -notmatch '-Target sandbox' -or $localHarness -notmatch 'Assert-OpenSandboxReady') {
    throw "M7 local acceptance must validate OpenSandbox and deploy the sandbox target."
}
if ($localHarness -notmatch 'Start-Process kubectl -ArgumentList @\("proxy"' -or $localHarness -match 'port-forward.*service/web') {
    throw "M7 rolling probes must use the stable Kubernetes Service Proxy instead of a Pod-bound Web port-forward."
}
foreach ($staleExitMessage in @("SeedScript failed", "Supply-chain negative tests failed", "Stage B rolling upgrade test failed")) {
    if ($localHarness.Contains($staleExitMessage)) { throw "M7 local acceptance contains a stale LASTEXITCODE child-script check: $staleExitMessage" }
}
$criticalGate = 'Assert-CriticalAssertionsPassed @("expand_migration", "rolling_upgrade", "worker_capability_gate", "pre_contract_baseline")'
$criticalGateIndex = $upgradeHarness.IndexOf($criticalGate, [StringComparison]::Ordinal)
$contractIndex = $upgradeHarness.IndexOf('-MigrationPhase contract -MigrationOnly', [StringComparison]::Ordinal)
if ($criticalGateIndex -lt 0 -or $contractIndex -lt 0 -or $criticalGateIndex -gt $contractIndex) {
    throw "Migration 0017 must be gated by rollout, capability, and pre-contract baseline assertions."
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
        sbomCanonicalSha256 = "d" * 64
        predicateType = "https://cyclonedx.org/bom"
        signatureVerified = $true
        attestationVerified = $true
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

$duplicate = $manifest | ConvertFrom-Json -Depth 20
$duplicate.images[0].name = $duplicate.images[1].name
if (($duplicate | ConvertTo-Json -Depth 20) | Test-Json -SchemaFile $schemaPath -ErrorAction SilentlyContinue) {
    throw "Release Manifest schema accepted a duplicate image name"
}

$missing = $manifest | ConvertFrom-Json -Depth 20
$missing.images = @($missing.images | Where-Object name -ne "sandbox-manager")
if (($missing | ConvertTo-Json -Depth 20) | Test-Json -SchemaFile $schemaPath -ErrorAction SilentlyContinue) {
    throw "Release Manifest schema accepted a missing required image"
}

$localEvidence = [ordered]@{
    schemaVersion = "agentx.io/m7-local-evidence/v1"
    status = "passed"
    runId = "20260806T120000000Z"
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    environment = "local-docker-desktop"
    registry = "local-tls"
    trustScope = "local-only"
    isolationLevel = "standard"
    int009 = "passed"
    int011 = "passed"
    int012 = "in_progress"
    int014 = "in_progress"
    m7 = "in_progress"
    failure = $null
    junit = [ordered]@{ tests = 6; failures = 0; errors = 0; skipped = 0; path = "artifacts/m7/local/test/junit.xml" }
    evidence = @([ordered]@{ kind = "test"; path = "artifacts/m7/local/test/evidence.json"; sha256 = "e" * 64 })
}
$localSchema = Join-Path $root "deploy/release/m7-local-evidence.schema.json"
if (-not (($localEvidence | ConvertTo-Json -Depth 20) | Test-Json -SchemaFile $localSchema)) {
    throw "valid local M7 evidence was rejected"
}
$invalidLocalEvidence = $localEvidence | ConvertTo-Json -Depth 20 | ConvertFrom-Json -Depth 20
$invalidLocalEvidence.int009 = "failed"
if (($invalidLocalEvidence | ConvertTo-Json -Depth 20) | Test-Json -SchemaFile $localSchema -ErrorAction SilentlyContinue) {
    throw "passed local M7 evidence accepted a failed INT-009 result"
}

try {
    & $scriptPath -Version test -Registry registry.example.test/agentx -CosignPrivateKey test.key -CosignPublicKey test.pub -RuntimeClass runc -IsolationLevel strong -SkipBuild
    throw "release script accepted runc as strong isolation"
}
catch {
    if ($_.Exception.Message -eq "release script accepted runc as strong isolation") { throw }
}

[ordered]@{ status = "passed"; imageCount = $images.Count } | ConvertTo-Json
