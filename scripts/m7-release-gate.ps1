param(
    [Parameter(Mandatory = $true)][string]$E2ERunDirectory,
    [Parameter(Mandatory = $true)][string]$CapacityEvidence,
    [Parameter(Mandatory = $true)][string]$VaultEvidence,
    [Parameter(Mandatory = $true)][string]$FailureEvidence,
    [Parameter(Mandatory = $true)][string]$SecurityEvidence,
    [Parameter(Mandatory = $true)][string]$UpgradeEvidence,
    [Parameter(Mandatory = $true)][string]$IsolationEvidence,
    [Parameter(Mandatory = $true)][string]$ReleaseManifest,
    [Parameter(Mandatory = $true)][string]$CosignPublicKey,
    [string]$OutputDirectory = "artifacts/release"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$requiredSuites = @(
    "m2.1-control-plane",
    "m3-control-plane",
    "m3-observability",
    "m4-runtime-recovery",
    "m5-agent-sandbox",
    "m6-workflow-studio",
    "m7-business-closure"
)
$requiredOperationalAssertions = @{
    failure = @("coordinator_restart", "worker_kill", "redis_outage", "clickhouse_outage", "minio_latency", "opensandbox_timeout", "opensandbox_residual", "vault_outage", "sse_reconnect")
    security = @("tenant_id_guessing", "grant_bypass", "log_secret_leak", "credential_replay", "handle_revocation", "ipv4_egress", "ipv6_egress")
    upgrade = @("expand_migration", "rolling_upgrade", "worker_capability_gate", "contract_migration", "application_rollback", "migration_persistence")
}
$inventory = [Collections.Generic.List[object]]::new()

function Resolve-RequiredFile([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Label does not exist: $Path" }
    (Resolve-Path -LiteralPath $Path).Path
}

function Add-Evidence([string]$Kind, [string]$Path) {
    $resolved = Resolve-RequiredFile $Path $Kind
    $inventory.Add([ordered]@{
        kind = $Kind
        path = [IO.Path]::GetRelativePath($root, $resolved).Replace('\', '/')
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToLowerInvariant()
    })
}

function Read-SchemaJson([string]$Path, [string]$SchemaName) {
    $resolved = Resolve-RequiredFile $Path $SchemaName
    $schema = Join-Path $root "deploy/release/$SchemaName"
    $raw = Get-Content -Raw -LiteralPath $resolved
    if (-not ($raw | Test-Json -SchemaFile $schema)) { throw "$resolved does not match $SchemaName." }
    $raw | ConvertFrom-Json -Depth 50
}

function Assert-OperationalEvidence([string]$Path, [string]$ExpectedType) {
    $value = Read-SchemaJson $Path "m7-operational-evidence.schema.json"
    if ($value.evidenceType -ne $ExpectedType -or $value.status -ne "passed") {
        throw "$Path is not passed $ExpectedType evidence."
    }
    $allAssertions = @($value.assertions)
    $failedAssertions = @($allAssertions | Where-Object status -ne "passed")
    if ($failedAssertions.Count -gt 0) {
        throw "$ExpectedType evidence contains failed assertions: $($failedAssertions.name -join ', ')."
    }
    $names = @($allAssertions | ForEach-Object name)
    if (@($names | Select-Object -Unique).Count -ne $names.Count) {
        throw "$ExpectedType evidence contains duplicate assertion names."
    }
    foreach ($required in $requiredOperationalAssertions[$ExpectedType]) {
        if ($names -notcontains $required) { throw "$ExpectedType evidence is missing '$required'." }
    }
    foreach ($assertion in $allAssertions) {
        $evidencePath = [string]$assertion.evidence
        $candidate = if ([IO.Path]::IsPathRooted($evidencePath)) { $evidencePath } else { Join-Path $root $evidencePath }
        $resolvedEvidence = Resolve-RequiredFile $candidate "$ExpectedType/$($assertion.name) assertion evidence"
        if (-not $resolvedEvidence.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
            throw "$ExpectedType/$($assertion.name) evidence is outside the repository evidence root."
        }
        Add-Evidence "$ExpectedType`:$($assertion.name)" $resolvedEvidence
    }
    Add-Evidence $ExpectedType $Path
}

if (-not (Get-Command cosign -ErrorAction SilentlyContinue)) { throw "cosign is required." }
$e2e = (Resolve-Path -LiteralPath $E2ERunDirectory -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $e2e -PathType Container)) { throw "E2ERunDirectory must be a directory." }
$runId = Split-Path -Leaf $e2e

$junit = [ordered]@{ files = 0; tests = 0; failures = 0; errors = 0; skipped = 0 }
foreach ($suite in $requiredSuites) {
    $suiteDirectory = Join-Path $e2e $suite
    $junitPath = Resolve-RequiredFile (Join-Path $suiteDirectory "junit.xml") "$suite JUnit"
    Resolve-RequiredFile (Join-Path $suiteDirectory "playwright-report/index.html") "$suite HTML report" | Out-Null
    $traces = @(Get-ChildItem (Join-Path $suiteDirectory "artifacts") -Recurse -File -Filter trace.zip -ErrorAction SilentlyContinue)
    if ($traces.Count -lt 1) { throw "$suite has no Playwright trace.zip." }
    [xml]$document = Get-Content -Raw -LiteralPath $junitPath
    $suites = @($document.SelectNodes('//testsuite[not(testsuite)]'))
    if ($suites.Count -eq 0) { throw "$junitPath does not contain a testsuite." }
    $junit.files++
    foreach ($value in $suites) {
        $junit.tests += [int]$value.tests
        $junit.failures += [int]$value.failures
        $junit.errors += [int]$value.errors
        $junit.skipped += [int]$value.skipped
    }
    Add-Evidence "junit:$suite" $junitPath
    Add-Evidence "trace:$suite" $traces[0].FullName
}
if ($junit.tests -lt 1 -or $junit.failures -ne 0 -or $junit.errors -ne 0 -or $junit.skipped -ne 0) {
    throw "JUnit gate failed: tests=$($junit.tests), failures=$($junit.failures), errors=$($junit.errors), skipped=$($junit.skipped)."
}
foreach ($name in @("resources.txt", "events.txt", "m4-database-evidence.txt", "m5-database-evidence.txt", "m5-sandbox-cleanup.txt", "m7-database-evidence.txt")) {
    Add-Evidence "kubernetes:$name" (Join-Path $e2e $name)
}
$m7Database = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $e2e "m7-database-evidence.txt")) {
    $parts = $line -split '=', 2
    if ($parts.Count -eq 2) { $m7Database[$parts[0]] = $parts[1] }
}
foreach ($entry in @{
    pendingRuntimeCommands = "0"
    unpublishedOutboxEvents = "0"
    runtimeEventsWithoutProjection = "0"
    terminalExecutionsWithoutEvent = "0"
    activeQuotaReservations = "0"
    remotePollInvocations = "1"
    remoteScheduleInvocations = "1"
    activeBindingsForDisabledApplications = "0"
}.GetEnumerator()) {
    if ($m7Database[$entry.Key] -ne $entry.Value) {
        throw "M7 database evidence $($entry.Key) is '$($m7Database[$entry.Key])'; expected '$($entry.Value)'."
    }
}

$capacity = Read-SchemaJson $CapacityEvidence "m7-capacity-evidence.schema.json"
if ($capacity.status -ne "passed") { throw "Capacity evidence is not passed." }
$capacityThresholds = @{
    executionCount = 100
    completedExecutionCount = 100
    nodeExecutionCount = 500
    sseClientCount = 200
    successfulSseClientCount = 200
    evaluationCaseCount = 1000
    workflowNodeCount = 200
    stabilityDurationSeconds = 7200
}
foreach ($entry in $capacityThresholds.GetEnumerator()) {
    if ([double]$capacity.metrics.($entry.Key) -lt [double]$entry.Value) {
        throw "Capacity metric $($entry.Key) is $($capacity.metrics.($entry.Key)); required $($entry.Value)."
    }
}
if ([double]$capacity.metrics.projectorP95Seconds -ge 5) { throw "Projector p95 must be below 5 seconds." }
if ([int]$capacity.metrics.activeQuotaReservations -ne 0) { throw "Quota Reservations did not converge to zero." }
Add-Evidence "capacity" $CapacityEvidence

$vault = Read-SchemaJson $VaultEvidence "vault-evidence.schema.json"
if ($vault.status -ne "passed") { throw "Vault evidence is not passed." }
Add-Evidence "vault" $VaultEvidence
Assert-OperationalEvidence $FailureEvidence "failure"
Assert-OperationalEvidence $SecurityEvidence "security"
Assert-OperationalEvidence $UpgradeEvidence "upgrade"

$isolation = Read-SchemaJson $IsolationEvidence "isolation-evidence.schema.json"
if ($isolation.status -ne "passed") { throw "Isolation evidence is not passed." }
Add-Evidence "isolation" $IsolationEvidence

$manifest = Read-SchemaJson $ReleaseManifest "release-manifest.schema.json"
if ($manifest.isolationLevel -ne $isolation.isolationLevel -or $manifest.runtimeClass -ne $isolation.runtimeClass) {
    throw "Release Manifest isolation does not match Pod evidence."
}
$manifestSignature = Resolve-RequiredFile "$ReleaseManifest.sig" "Release Manifest signature"
& cosign verify-blob --key $CosignPublicKey --signature $manifestSignature $ReleaseManifest | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Release Manifest signature verification failed." }
$manifestDirectory = Split-Path -Parent (Resolve-Path -LiteralPath $ReleaseManifest)
foreach ($image in $manifest.images) {
    & cosign verify --key $CosignPublicKey $image.reference | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Image signature verification failed for $($image.name)." }
    $sbom = Resolve-RequiredFile (Join-Path $manifestDirectory $image.sbom) "$($image.name) SBOM"
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $sbom).Hash.ToLowerInvariant()
    if ($hash -ne $image.sbomSha256) { throw "$($image.name) SBOM hash does not match Release Manifest." }
    Add-Evidence "sbom:$($image.name)" $sbom
}
Add-Evidence "release-manifest" $ReleaseManifest
Add-Evidence "release-manifest-signature" $manifestSignature

$result = [ordered]@{
    schemaVersion = "agentx.io/m7-acceptance-evidence/v1"
    status = "passed"
    runId = $runId
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    isolationLevel = $isolation.isolationLevel
    runtimeClass = $isolation.runtimeClass
    junit = $junit
    evidence = @($inventory)
}
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Path $output -Force | Out-Null
$resultPath = Join-Path $output "m7-acceptance-$runId.json"
$result | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $resultPath -Encoding utf8NoBOM
$schema = Join-Path $root "deploy/release/m7-acceptance-evidence.schema.json"
if (-not ((Get-Content -Raw -LiteralPath $resultPath) | Test-Json -SchemaFile $schema)) {
    throw "Generated M7 acceptance evidence is invalid."
}
Write-Output $resultPath
