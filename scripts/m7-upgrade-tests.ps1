param(
    [Parameter(Mandatory = $true)][string]$PreviousReleaseManifest,
    [Parameter(Mandatory = $true)][string]$CandidateReleaseManifest,
    [string]$Namespace = "agentx-e2e",
    [string]$GatewayBaseUrl,
    [string]$ApplicationSlug,
    [string]$BearerToken,
    [string]$ContractProfile,
    [int]$ProbeDurationSeconds = 900,
    [switch]$SkipCertificateCheck,
    [switch]$RequireSandbox,
    [string]$BaselineFile,
    [string]$OutputDirectory = "artifacts/m7"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "$OutputDirectory/$runId/upgrade"
$assertions = [Collections.Generic.List[object]]::new()
$failures = [Collections.Generic.List[string]]::new()
$workloads = @("trace-writer", "sandbox-manager", "workflow-worker", "workflow-coordinator", "platform-api", "trigger-gateway", "web")
$originalImages = @{}
$originalReplicas = @{}
$probeProcess = $null
$probePath = Join-Path $output "probes.ndjson"
$probeStopPath = Join-Path $output "probe.stop"

function Read-Manifest([string]$Path) {
    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $raw = Get-Content -Raw -LiteralPath $resolved
    $schema = Join-Path $root "deploy/release/release-manifest.schema.json"
    if (-not ($raw | Test-Json -SchemaFile $schema)) { throw "Release Manifest is invalid: $Path" }
    $raw | ConvertFrom-Json -Depth 50
}

function Image-Map($Manifest) {
    $map = @{}
    foreach ($image in $Manifest.images) { $map[[string]$image.name] = [string]$image.reference }
    $map
}

function Invoke-MySql([string]$Sql) {
    $value = $Sql | kubectl -n $Namespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"'
    if ($LASTEXITCODE -ne 0) { throw "MySQL upgrade evidence query failed." }
    [string]($value | Select-Object -Last 1).Trim()
}

function Get-Counts {
    [ordered]@{
        workflows = [int64](Invoke-MySql "SELECT COUNT(*) FROM workflows;")
        versions = [int64](Invoke-MySql "SELECT COUNT(*) FROM workflow_versions;")
        applications = [int64](Invoke-MySql "SELECT COUNT(*) FROM applications;")
        executions = [int64](Invoke-MySql "SELECT COUNT(*) FROM workflow_executions;")
    }
}

function Set-WorkloadImage([string]$Name, [string]$Reference) {
    $existing = kubectl -n $Namespace get "deployment/$Name" --ignore-not-found -o name 2>$null
    if (-not $existing) {
        if ($Name -eq "sandbox-manager" -and -not $RequireSandbox) { return "$Name=not-deployed" }
        throw "Required deployment '$Name' does not exist."
    }
    kubectl -n $Namespace set image "deployment/$Name" "$Name=$Reference" | Out-Null
    kubectl -n $Namespace rollout status "deployment/$Name" --timeout=300s | Out-Null
    "$Name=$Reference"
}

function Add-Assertion([string]$Name, [scriptblock]$Check) {
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $detailPath = Join-Path $output "$Name.txt"
    try {
        $detail = & $Check
        if (-not $detail) { $detail = "$Name passed" }
        @($detail) | Set-Content -LiteralPath $detailPath -Encoding utf8NoBOM
        $assertions.Add([ordered]@{ name = $Name; status = "passed"; evidence = [IO.Path]::GetRelativePath($root, $detailPath).Replace('\', '/') })
    }
    catch {
        $message = $_.Exception.Message
        $message | Set-Content -LiteralPath $detailPath -Encoding utf8NoBOM
        $assertions.Add([ordered]@{ name = $Name; status = "failed"; evidence = [IO.Path]::GetRelativePath($root, $detailPath).Replace('\', '/') })
    }
}

function Assert-CriticalAssertionsPassed([string[]]$Names) {
    $failed = [Collections.Generic.List[string]]::new()
    foreach ($name in $Names) {
        $matches = @($assertions | Where-Object name -eq $name)
        if ($matches.Count -ne 1 -or $matches[0].status -ne "passed") { $failed.Add($name) }
    }
    if ($failed.Count -gt 0) { throw "Contract migration is blocked by failed critical assertions: $($failed -join ', ')." }
}

function Start-Probe {
    if (-not $GatewayBaseUrl -or -not $ApplicationSlug -or -not $BearerToken) { throw "GatewayBaseUrl, ApplicationSlug and BearerToken are required for the continuous Invocation probe." }
    $pwsh = (Get-Command pwsh -ErrorAction Stop).Source
    Remove-Item -LiteralPath $probeStopPath -Force -ErrorAction SilentlyContinue
    $arguments = @("-NoProfile", "-File", (Join-Path $root "scripts/m7-invocation-probe.ps1"), "-BaseUrl", $GatewayBaseUrl, "-ApplicationSlug", $ApplicationSlug, "-OutputPath", $probePath, "-StopPath", $probeStopPath, "-DurationSeconds", [string]([Math]::Max(30, $ProbeDurationSeconds)))
    if ($SkipCertificateCheck) { $arguments += "-SkipCertificateCheck" }
    $previousToken = $env:AGENTX_M7_PROBE_TOKEN
    try {
        $env:AGENTX_M7_PROBE_TOKEN = $BearerToken
        $script:probeProcess = Start-Process -FilePath $pwsh -ArgumentList $arguments -PassThru -WindowStyle Hidden
    } finally {
        $env:AGENTX_M7_PROBE_TOKEN = $previousToken
    }
}

function Stop-Probe {
    if ($probeProcess -and -not $probeProcess.HasExited) {
        New-Item -ItemType File -Path $probeStopPath -Force | Out-Null
    }
}

function Wait-Probe {
    if (-not $probeProcess) { return }
    $process = $probeProcess
    $script:probeProcess = $null
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Continuous Invocation probe exited with code $($process.ExitCode)." }
    if (-not (Test-Path -LiteralPath $probePath -PathType Leaf)) { throw "Continuous Invocation probe produced no probes.ndjson." }
    $rows = @(Get-Content -LiteralPath $probePath | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json -Depth 20 })
    if ($rows.Count -lt 1 -or @($rows | Where-Object status -eq "failed").Count -gt 0) { throw "Continuous Invocation probe contains failed rows." }
    if (@($rows | Where-Object { [long]$_.eventCursor -le 0 }).Count -gt 0) { throw "Continuous Invocation probe contains an empty Runtime Event Cursor." }
    $executionIds = @($rows | ForEach-Object { [string]$_.executionId })
    foreach ($executionId in $executionIds) {
        if ($executionId -notmatch '^[a-f0-9-]{36}$') { throw "Continuous Invocation probe returned an invalid Execution ID." }
    }
    if (@($executionIds | Select-Object -Unique).Count -ne $executionIds.Count) { throw "Continuous Invocation probe reused an Execution across unique idempotency keys." }
    $selectors = @($executionIds | ForEach-Object { "UUID_TO_BIN('$_')" }) -join ','
    $executionCount = Invoke-MySql "SELECT COUNT(*) FROM workflow_executions WHERE id IN ($selectors);"
    $attemptCount = Invoke-MySql "SELECT COUNT(*) FROM node_attempts WHERE execution_id IN ($selectors);"
    if ([int]$executionCount -ne $executionIds.Count -or [int]$attemptCount -lt $executionIds.Count) {
        throw "Continuous Invocation probe could not resolve every Execution and Node Attempt."
    }
    "probeCount=$($rows.Count);eventCursorSamples=$($rows.Count);executionCount=$executionCount;nodeAttemptCount=$attemptCount"
}

function Invoke-PostRollbackProbe {
    $key = "m7-rollback-$runId"
    $headers = @{ Authorization = "Bearer $BearerToken"; "Idempotency-Key" = $key }
    $parameters = @{ Uri = "$($GatewayBaseUrl.TrimEnd('/'))/gateway/v1/applications/$ApplicationSlug/invocations"; Method = "POST"; Headers = $headers; ContentType = "application/json"; Body = (@{ input = @{ source = "m7-post-rollback" } } | ConvertTo-Json -Compress); TimeoutSec = 90 }
    if ($SkipCertificateCheck) { $parameters.SkipCertificateCheck = $true }
    $created = Invoke-RestMethod @parameters
    if (-not $created.id) { throw "Post-rollback invocation did not return an id." }
    $replay = Invoke-RestMethod @parameters
    if ([string]$replay.id -ne [string]$created.id) { throw "Post-rollback idempotency replay created another Invocation." }
    $get = @{ Uri = "$($GatewayBaseUrl.TrimEnd('/'))/gateway/v1/invocations/$($created.id)"; Headers = @{ Authorization = "Bearer $BearerToken" }; TimeoutSec = 90 }
    if ($SkipCertificateCheck) { $get.SkipCertificateCheck = $true }
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(180)
    do {
        Start-Sleep -Milliseconds 500
        $terminal = Invoke-RestMethod @get
    } while ($terminal.status -notin @("completed", "failed", "cancelled") -and [DateTimeOffset]::UtcNow -lt $deadline)
    if ($terminal.status -ne "completed") { throw "Post-rollback invocation ended as $($terminal.status)." }
    if (-not $terminal.executionId) { throw "Post-rollback invocation has no real Execution." }
    "invocation=$($created.id);execution=$($terminal.executionId);idempotencyKey=$key"
}

if (-not (kubectl get namespace $Namespace --ignore-not-found -o name)) { throw "Upgrade tests require Namespace '$Namespace'." }
if (-not $ContractProfile) { throw "ContractProfile is required so Migration 0017 runs after Candidate rollout." }
$previous = Read-Manifest $PreviousReleaseManifest
$candidate = Read-Manifest $CandidateReleaseManifest
if ($previous.gitCommit -eq $candidate.gitCommit) { throw "Previous and Candidate manifests must have different source commits." }
$previousImages = Image-Map $previous
$candidateImages = Image-Map $candidate
foreach ($name in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web")) {
    if (-not $previousImages[$name] -or -not $candidateImages[$name]) { throw "Both manifests must contain $name." }
    if ($previousImages[$name] -notmatch '@sha256:[a-f0-9]{64}$' -or $candidateImages[$name] -notmatch '@sha256:[a-f0-9]{64}$') { throw "$name does not use immutable digest references." }
}
New-Item -ItemType Directory -Path $output -Force | Out-Null

try {
    foreach ($name in $workloads) {
        $deployment = kubectl -n $Namespace get "deployment/$name" --ignore-not-found -o json 2>$null
        if (-not $deployment) { if ($name -eq "sandbox-manager" -and -not $RequireSandbox) { continue }; throw "Required deployment '$name' does not exist." }
        $parsed = $deployment | ConvertFrom-Json -Depth 50
        $originalImages[$name] = [string]$parsed.spec.template.spec.containers[0].image
        $originalReplicas[$name] = [int]$parsed.spec.replicas
    }
    $before = Get-Counts
    foreach ($key in @("workflows", "versions", "applications", "executions")) {
        if ([int64]$before[$key] -lt 1) { throw "Upgrade baseline must contain at least one $key row." }
    }
    if ($BaselineFile -and (Test-Path -LiteralPath $BaselineFile)) {
        $expected = Get-Content -Raw -LiteralPath $BaselineFile | ConvertFrom-Json -Depth 20
        foreach ($key in @("workflows", "versions", "applications")) { if ([int64]$expected.$key -ne $before[$key]) { throw "Baseline $key differs from live database." } }
    }
    Add-Assertion "expand_migration" {
        $applied = Invoke-MySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=16 AND success=TRUE;"
        if ($applied -ne "1") { throw "Migration 0016 is not successfully applied." }
        "migration=0016_m7_expand`napplied=$applied"
    }
    foreach ($name in $workloads) { if ($previousImages[$name]) { Set-WorkloadImage $name $previousImages[$name] | Out-Null } }
    Start-Probe
    Add-Assertion "rolling_upgrade" {
        $rollout = [Collections.Generic.List[string]]::new()
        foreach ($name in $workloads) {
            $value = Set-WorkloadImage $name $candidateImages[$name]
            $rollout.Add("$value;order=$($rollout.Count + 1)")
        }
        $rollout
    }
    Add-Assertion "worker_capability_gate" {
        $ready = Invoke-MySql "SELECT COUNT(*) FROM worker_capabilities WHERE status='ready' AND heartbeat_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 60 SECOND) AND node_protocol_version='1.0' AND JSON_CONTAINS(ir_schema_versions_json,JSON_QUOTE('3.0'));"
        if ([int]$ready -lt 1) { throw "No compatible Worker Capability heartbeat is ready." }
        "readyCapabilities=$ready"
    }
    Add-Assertion "pre_contract_baseline" {
        $current = Get-Counts
        foreach ($key in @("workflows", "versions", "applications")) {
            if ([int64]$current[$key] -ne [int64]$before[$key]) { throw "Pre-contract $key count differs from the upgrade baseline." }
        }
        if ([int64]$current.executions -lt [int64]$before.executions) { throw "Pre-contract Execution count decreased from the upgrade baseline." }
        "workflowCount=$($current.workflows)`nversionCount=$($current.versions)`napplicationCount=$($current.applications)`nexecutionCount=$($current.executions)"
    }
    Assert-CriticalAssertionsPassed @("expand_migration", "rolling_upgrade", "worker_capability_gate", "pre_contract_baseline")
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $ContractProfile -NonInteractive -Target services -MigrationPhase contract -MigrationOnly
    Add-Assertion "contract_migration" {
        $applied = Invoke-MySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=17 AND success=TRUE;"
        $contract = Invoke-MySql "SELECT CONCAT(schema_version,':',minimum_application_version) FROM release_schema_contract WHERE contract_name='m7-runtime-integration';"
        if ($applied -ne "1" -or $contract -ne "17:0.1.0") { throw "Migration 0017 contract marker is invalid." }
        "migration=0017_m7_contract`napplied=$applied`ncontract=$contract"
    }
    Add-Assertion "invocation_continuity" {
        Stop-Probe
        Wait-Probe
    }
    Add-Assertion "pre_rollback_runtime_cleanup" {
        for ($attempt = 0; $attempt -lt 120; $attempt++) {
            $commands = Invoke-MySql "SELECT COUNT(*) FROM runtime_commands WHERE status IN ('pending','processing');"
            $outbox = Invoke-MySql "SELECT COUNT(*) FROM outbox_events WHERE published_at IS NULL;"
            $quota = Invoke-MySql "SELECT COUNT(*) FROM quota_reservations WHERE status='active';"
            if ($commands -eq "0" -and $outbox -eq "0" -and $quota -eq "0") { break }
            Start-Sleep -Seconds 1
        }
        if ($commands -ne "0" -or $outbox -ne "0" -or $quota -ne "0") {
            throw "Candidate runtime did not drain before rollback: commands=$commands outbox=$outbox quota=$quota."
        }
        "pendingRuntimeCommands=$commands`nunpublishedOutboxEvents=$outbox`nactiveQuotaReservations=$quota"
    }
    Add-Assertion "application_rollback" {
        foreach ($name in @("platform-api", "trigger-gateway")) {
            if (-not $originalImages.ContainsKey($name)) { continue }
            kubectl -n $Namespace scale "deployment/$name" --replicas=0 | Out-Null
            Set-WorkloadImage $name $previousImages[$name] | Out-Null
            kubectl -n $Namespace scale "deployment/$name" --replicas=$originalReplicas[$name] | Out-Null
            kubectl -n $Namespace rollout status "deployment/$name" --timeout=300s | Out-Null
        }
        foreach ($name in @("workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web")) {
            if ($originalImages.ContainsKey($name)) { Set-WorkloadImage $name $previousImages[$name] | Out-Null }
        }
        $counts = Get-Counts
        if ($counts.executions -lt $before.executions) { throw "Execution count decreased during application rollback." }
        if ($GatewayBaseUrl -and $ApplicationSlug -and $BearerToken) { Invoke-PostRollbackProbe }
        "executionsBefore=$($before.executions)`nexecutionsAfter=$($counts.executions)"
    }
    Add-Assertion "migration_persistence" {
        $versions = Invoke-MySql "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE version IN (16,17) AND success=TRUE;"
        $contract = Invoke-MySql "SELECT COUNT(*) FROM release_schema_contract WHERE contract_name='m7-runtime-integration' AND schema_version='17';"
        $after = Get-Counts
        if ($versions -ne "16,17" -or $contract -ne "1") { throw "Migration state did not persist through application rollback." }
        if ($after.workflows -ne $before.workflows -or $after.versions -ne $before.versions -or $after.applications -ne $before.applications) { throw "Workflow/Version/Application baseline changed during rollback." }
        "versions=$versions`ncontractRows=$contract`nworkflowCount=$($after.workflows)`nversionCount=$($after.versions)`napplicationCount=$($after.applications)"
    }
    Add-Assertion "runtime_cleanup" {
        for ($attempt = 0; $attempt -lt 60; $attempt++) {
            $commands = Invoke-MySql "SELECT COUNT(*) FROM runtime_commands WHERE status IN ('pending','processing');"
            $outbox = Invoke-MySql "SELECT COUNT(*) FROM outbox_events WHERE published_at IS NULL;"
            $quota = Invoke-MySql "SELECT COUNT(*) FROM quota_reservations WHERE status='active';"
            if ($commands -eq "0" -and $outbox -eq "0" -and $quota -eq "0") { break }
            Start-Sleep -Seconds 1
        }
        if ($commands -ne "0" -or $outbox -ne "0" -or $quota -ne "0") { throw "Runtime cleanup failed: commands=$commands outbox=$outbox quota=$quota." }
        "pendingRuntimeCommands=$commands`nunpublishedOutboxEvents=$outbox`nactiveQuotaReservations=$quota"
    }
}
finally {
    Stop-Probe
    try { Wait-Probe } catch { $failures.Add("invocation_probe: $($_.Exception.Message)") }
    foreach ($name in $workloads) {
        if ($originalImages.ContainsKey($name)) { try { Set-WorkloadImage $name $originalImages[$name] | Out-Null } catch { $failures.Add("restore ${name}: $($_.Exception.Message)") } }
        if ($originalReplicas.ContainsKey($name)) { kubectl -n $Namespace scale "deployment/$name" --replicas=$originalReplicas[$name] 2>$null | Out-Null }
    }
    $failedAssertions = @($assertions | Where-Object status -ne "passed")
    $allFailures = @($failures) + @($failedAssertions | ForEach-Object { $_.name })
    $evidence = [ordered]@{
        schemaVersion = "agentx.io/m7-operational-evidence/v1"
        evidenceType = "upgrade"
        status = $(if ($allFailures.Count -eq 0) { "passed" } else { "failed" })
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @($assertions)
    }
    $path = Join-Path $output "upgrade-evidence.json"
    $evidence | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $cases = @($assertions | ForEach-Object {
        $name = [Security.SecurityElement]::Escape([string]$_.name)
        if ($_.status -eq "passed") { '<testcase name="' + $name + '"/>' } else { '<testcase name="' + $name + '"><failure message="assertion failed"/></testcase>' }
    }) -join ""
    $errorCases = @($failures | ForEach-Object { '<testcase name="harness"><error message="' + [Security.SecurityElement]::Escape([string]$_) + '"/></testcase>' }) -join ""
    $testCount = $assertions.Count + $failures.Count
    $junit = '<testsuite name="m7-upgrade" tests="' + $testCount + '" failures="' + $failedAssertions.Count + '" errors="' + $failures.Count + '" skipped="0">' + $cases + $errorCases + '</testsuite>'
    $junit | Set-Content -LiteralPath (Join-Path $output "junit.xml") -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/m7-operational-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Upgrade evidence is invalid." }
    Write-Output $path
}
if ($failures.Count -gt 0 -or @($assertions | Where-Object status -ne "passed").Count -gt 0) { throw "M7 upgrade matrix failed." }
