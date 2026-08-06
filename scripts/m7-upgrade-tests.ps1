param(
    [Parameter(Mandatory = $true)][string]$PreviousReleaseManifest,
    [Parameter(Mandatory = $true)][string]$CandidateReleaseManifest,
    [string]$Namespace = "agentx-e2e",
    [string]$OutputDirectory = "artifacts/m7"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "$OutputDirectory/$runId/upgrade"
$assertions = [Collections.Generic.List[object]]::new()
$failures = [Collections.Generic.List[string]]::new()
$workloads = @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web")
$originalImages = @{}
$originalReplicas = @{}

function Read-Manifest([string]$Path) {
    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $raw = Get-Content -Raw -LiteralPath $resolved
    $schema = Join-Path $root "deploy/release/release-manifest.schema.json"
    if (-not ($raw | Test-Json -SchemaFile $schema)) { throw "Release Manifest is invalid: $Path" }
    $raw | ConvertFrom-Json -Depth 30
}

function Image-Map($Manifest) {
    $map = @{}
    foreach ($image in $Manifest.images) { $map[[string]$image.name] = [string]$image.reference }
    $map
}

function Invoke-MySql([string]$Sql) {
    $value = $Sql | kubectl -n $Namespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"'
    if ($LASTEXITCODE -ne 0) { throw "MySQL upgrade evidence query failed." }
    [string]($value | Select-Object -Last 1)
}

function Set-WorkloadImage([string]$Name, [string]$Reference) {
    $container = if ($Name -eq "web") { "web" } else { $Name }
    kubectl -n $Namespace set image "deployment/$Name" "$container=$Reference" | Out-Null
    kubectl -n $Namespace rollout status "deployment/$Name" --timeout=300s | Out-Null
}

function Add-Assertion([string]$Name, [scriptblock]$Check) {
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
        $failures.Add("${Name}: $message")
    }
}

if (-not (kubectl get namespace $Namespace --ignore-not-found -o name)) {
    throw "Upgrade tests require the live E2E Namespace '$Namespace'."
}
$previous = Read-Manifest $PreviousReleaseManifest
$candidate = Read-Manifest $CandidateReleaseManifest
$previousImages = Image-Map $previous
$candidateImages = Image-Map $candidate
foreach ($name in $workloads) {
    if (-not $previousImages[$name] -or -not $candidateImages[$name]) { throw "Both manifests must contain $name." }
    if ($previousImages[$name] -notmatch '@sha256:[a-f0-9]{64}$' -or $candidateImages[$name] -notmatch '@sha256:[a-f0-9]{64}$') {
        throw "$name does not use immutable digest references."
    }
}
if (@($workloads | Where-Object { $previousImages[$_] -ne $candidateImages[$_] }).Count -lt 1) {
    throw "Previous and Candidate manifests must differ."
}
New-Item -ItemType Directory -Path $output -Force | Out-Null

try {
    foreach ($name in $workloads) {
        $deployment = kubectl -n $Namespace get "deployment/$name" -o json | ConvertFrom-Json -Depth 50
        $originalImages[$name] = [string]$deployment.spec.template.spec.containers[0].image
        $originalReplicas[$name] = [int]$deployment.spec.replicas
    }
    Add-Assertion "expand_migration" {
        $applied = Invoke-MySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=16 AND success=TRUE;"
        if ($applied -ne "1") { throw "Migration 0016 is not successfully applied." }
        "migration=0016_m7_expand`napplied=$applied"
    }
    foreach ($name in $workloads) { Set-WorkloadImage $name $previousImages[$name] }
    Add-Assertion "rolling_upgrade" {
        $rollout = [Collections.Generic.List[string]]::new()
        foreach ($name in $workloads) {
            $replicas = [Math]::Max(2, $originalReplicas[$name])
            kubectl -n $Namespace scale "deployment/$name" --replicas=$replicas | Out-Null
            kubectl -n $Namespace rollout status "deployment/$name" --timeout=300s | Out-Null
            Set-WorkloadImage $name $candidateImages[$name]
            $ready = kubectl -n $Namespace get "deployment/$name" -o jsonpath='{.status.readyReplicas}'
            if ([int]$ready -ne $replicas) { throw "$name has $ready/$replicas ready replicas after rolling upgrade." }
            $rollout.Add("$name=$($previousImages[$name]) -> $($candidateImages[$name]); ready=$ready")
        }
        $rollout
    }
    Add-Assertion "worker_capability_gate" {
        $transcript = & cargo test -p agentx-infrastructure runtime_queue::tests -- --nocapture 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Runtime queue compatibility tests failed: $($transcript -join [Environment]::NewLine)" }
        $ready = Invoke-MySql "SELECT COUNT(*) FROM worker_capabilities WHERE status='ready' AND heartbeat_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 60 SECOND) AND node_protocol_version='1.0' AND JSON_CONTAINS(ir_schema_versions_json,JSON_QUOTE('3.0'));"
        if ([int]$ready -lt 1) { throw "No compatible Worker Capability heartbeat is ready." }
        @($transcript) + "readyCapabilities=$ready"
    }
    Add-Assertion "contract_migration" {
        $applied = Invoke-MySql "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=17 AND success=TRUE;"
        $contract = Invoke-MySql "SELECT CONCAT(schema_version,':',minimum_application_version) FROM release_schema_contract WHERE contract_name='m7-runtime-integration';"
        if ($applied -ne "1" -or $contract -ne "17:0.1.0") { throw "Migration 0017 contract marker is invalid." }
        "migration=0017_m7_contract`napplied=$applied`ncontract=$contract"
    }
    Add-Assertion "application_rollback" {
        foreach ($name in @("platform-api", "trigger-gateway")) { Set-WorkloadImage $name $previousImages[$name] }
        $platformReady = kubectl -n $Namespace get deployment/platform-api -o jsonpath='{.status.readyReplicas}'
        $gatewayReady = kubectl -n $Namespace get deployment/trigger-gateway -o jsonpath='{.status.readyReplicas}'
        if ([int]$platformReady -lt 1 -or [int]$gatewayReady -lt 1) { throw "Application rollback did not become Ready." }
        $executions = Invoke-MySql "SELECT COUNT(*) FROM workflow_executions;"
        if ([int]$executions -lt 1) { throw "Runtime state was lost during rollback." }
        foreach ($name in @("platform-api", "trigger-gateway")) { Set-WorkloadImage $name $candidateImages[$name] }
        "platformReady=$platformReady`ngatewayReady=$gatewayReady`nexecutions=$executions"
    }
    Add-Assertion "migration_persistence" {
        $versions = Invoke-MySql "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE version IN (16,17) AND success=TRUE;"
        $contract = Invoke-MySql "SELECT COUNT(*) FROM release_schema_contract WHERE contract_name='m7-runtime-integration' AND schema_version='17';"
        if ($versions -ne "16,17" -or $contract -ne "1") { throw "Migration state did not persist through application rollback." }
        "versions=$versions`ncontractRows=$contract"
    }
}
finally {
    foreach ($name in $workloads) {
        if ($candidateImages[$name]) {
            try { Set-WorkloadImage $name $candidateImages[$name] } catch { $failures.Add("restore ${name}: $($_.Exception.Message)") }
        }
        if ($originalReplicas.ContainsKey($name)) {
            kubectl -n $Namespace scale "deployment/$name" --replicas=$originalReplicas[$name] 2>$null | Out-Null
        }
    }
    $evidence = [ordered]@{
        schemaVersion = "agentx.io/m7-operational-evidence/v1"
        evidenceType = "upgrade"
        status = $(if ($failures.Count -eq 0) { "passed" } else { "failed" })
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @($assertions)
    }
    $path = Join-Path $output "upgrade-evidence.json"
    $evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/m7-operational-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Upgrade evidence is invalid." }
    Write-Output $path
}
if ($failures.Count -gt 0) { throw "M7 upgrade matrix failed: $($failures -join '; ')" }
