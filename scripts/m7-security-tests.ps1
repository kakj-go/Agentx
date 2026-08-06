param(
    [Parameter(Mandatory = $true)][string]$E2ERunDirectory,
    [string]$Namespace = "agentx-e2e",
    [string]$AdminUsername = "admin",
    [string]$AdminPassword = "agentx-e2e-admin-password",
    [int]$PlatformPort = 19080,
    [string]$OutputDirectory = "artifacts/m7"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "$OutputDirectory/$runId/security"
$assertions = [Collections.Generic.List[object]]::new()
$failures = [Collections.Generic.List[string]]::new()
$forward = $null
$forwardOut = Join-Path $output "platform-port-forward.out.log"
$forwardError = Join-Path $output "platform-port-forward.err.log"

function Invoke-MySql([string]$Sql) {
    $value = $Sql | kubectl -n $Namespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"'
    if ($LASTEXITCODE -ne 0) { throw "MySQL security fixture query failed." }
    [string]($value | Select-Object -Last 1)
}

function Wait-TcpPort([int]$Port) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $client = [Net.Sockets.TcpClient]::new()
        try { $client.Connect("127.0.0.1", $Port); return }
        catch { Start-Sleep -Seconds 1 }
        finally { $client.Dispose() }
    }
    throw "Timed out waiting for port $Port."
}

function Invoke-Status([string]$Uri, [string]$Method, [string]$Token, $Body = $null) {
    $parameters = @{ Uri = $Uri; Method = $Method; Headers = @{ Authorization = "Bearer $Token" }; SkipHttpErrorCheck = $true; TimeoutSec = 30 }
    if ($null -ne $Body) {
        $parameters.ContentType = "application/json"
        $parameters.Body = $Body | ConvertTo-Json -Depth 20 -Compress
    }
    $response = Invoke-WebRequest @parameters
    [pscustomobject]@{ status = [int]$response.StatusCode; body = [string]$response.Content }
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

if (-not (Get-Command kubectl -ErrorAction SilentlyContinue)) { throw "kubectl is required." }
$e2e = (Resolve-Path -LiteralPath $E2ERunDirectory -ErrorAction Stop).Path
if (-not (kubectl get namespace $Namespace --ignore-not-found -o name)) {
    throw "Security tests require the live E2E Namespace '$Namespace'. Run scripts/e2e.ps1 -KeepNamespace first."
}
New-Item -ItemType Directory -Path $output -Force | Out-Null

try {
    $forward = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", "svc/platform-api", "${PlatformPort}:8080") -WindowStyle Hidden -PassThru -RedirectStandardOutput $forwardOut -RedirectStandardError $forwardError
    Wait-TcpPort $PlatformPort
    $base = "http://127.0.0.1:$PlatformPort/api/v1"
    $login = Invoke-RestMethod -Method Post -Uri "$base/auth/login" -ContentType "application/json" -Body (@{ username = $AdminUsername; password = $AdminPassword } | ConvertTo-Json -Compress)
    $victimToken = [string]$login.accessToken
    $victimTenant = Invoke-MySql "SELECT BIN_TO_UUID(tenant_id) FROM users WHERE username_normalized=LOWER('$AdminUsername') ORDER BY created_at LIMIT 1;"
    $victimExecution = Invoke-MySql "SELECT BIN_TO_UUID(id) FROM workflow_executions WHERE tenant_id=UUID_TO_BIN('$victimTenant') ORDER BY created_at DESC LIMIT 1;"
    $victimVersion = Invoke-MySql "SELECT BIN_TO_UUID(id) FROM workflow_versions WHERE tenant_id=UUID_TO_BIN('$victimTenant') ORDER BY created_at DESC LIMIT 1;"
    if (-not $victimExecution -or -not $victimVersion) { throw "Victim Runtime records were not found." }

    $attackerTenant = [Guid]::NewGuid().ToString()
    $attackerDepartment = [Guid]::NewGuid().ToString()
    $attackerUser = [Guid]::NewGuid().ToString()
    $attackerRole = [Guid]::NewGuid().ToString()
    $attackerUsername = "m7-attacker-$($runId.ToLowerInvariant())"
    $fixtureSql = @"
START TRANSACTION;
INSERT INTO tenants(id,name,normalized_name) VALUES(UUID_TO_BIN('$attackerTenant'),'M7 Security Attacker $runId','m7 security attacker $runId');
INSERT INTO tenant_settings(tenant_id,locale,timezone) VALUES(UUID_TO_BIN('$attackerTenant'),'en-US','UTC');
INSERT INTO workflow_environments(id,tenant_id,code,name,is_builtin) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('$attackerTenant'),'development','Development',TRUE),(UUID_TO_BIN(UUID()),UUID_TO_BIN('$attackerTenant'),'production','Production',TRUE);
INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(UUID_TO_BIN('$attackerDepartment'),UUID_TO_BIN('$attackerTenant'),'Root','root',TRUE);
INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(UUID_TO_BIN('$attackerTenant'),UUID_TO_BIN('$attackerDepartment'),UUID_TO_BIN('$attackerDepartment'),0);
INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status,password_change_required) VALUES(UUID_TO_BIN('$attackerUser'),UUID_TO_BIN('$attackerTenant'),'$attackerUsername','$attackerUsername','M7 Attacker','active',FALSE);
INSERT INTO user_credentials(user_id,password_hash) SELECT UUID_TO_BIN('$attackerUser'),c.password_hash FROM user_credentials c JOIN users u ON u.id=c.user_id WHERE u.tenant_id=UUID_TO_BIN('$victimTenant') AND u.username_normalized=LOWER('$AdminUsername') LIMIT 1;
INSERT INTO user_departments(tenant_id,user_id,department_id) VALUES(UUID_TO_BIN('$attackerTenant'),UUID_TO_BIN('$attackerUser'),UUID_TO_BIN('$attackerDepartment'));
INSERT INTO roles(id,tenant_id,code,name,data_scope,is_builtin) VALUES(UUID_TO_BIN('$attackerRole'),UUID_TO_BIN('$attackerTenant'),'admin','Administrator','company',TRUE);
INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT UUID_TO_BIN('$attackerTenant'),UUID_TO_BIN('$attackerRole'),id FROM permissions;
INSERT INTO user_roles(id,tenant_id,user_id,role_id,scope_department_id) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('$attackerTenant'),UUID_TO_BIN('$attackerUser'),UUID_TO_BIN('$attackerRole'),UUID_TO_BIN('$attackerDepartment'));
INSERT INTO quota_policies(tenant_id,dimension_key,hard_limit,period_seconds,updated_by) SELECT UUID_TO_BIN('$attackerTenant'),dimension_key,hard_limit,period_seconds,UUID_TO_BIN('$attackerUser') FROM quota_policies WHERE tenant_id=UUID_TO_BIN('$victimTenant');
COMMIT;
"@
    Invoke-MySql $fixtureSql | Out-Null
    $attackerLogin = Invoke-RestMethod -Method Post -Uri "$base/auth/login" -ContentType "application/json" -Body (@{ username = $attackerUsername; password = $AdminPassword } | ConvertTo-Json -Compress)
    $attackerToken = [string]$attackerLogin.accessToken

    Add-Assertion "tenant_id_guessing" {
        $execution = Invoke-Status "$base/executions/$victimExecution" "GET" $attackerToken
        if ($execution.status -ne 404) { throw "Cross-tenant Execution lookup returned HTTP $($execution.status)." }
        $victim = Invoke-Status "$base/executions/$victimExecution" "GET" $victimToken
        if ($victim.status -ne 200) { throw "Victim could not read its own Execution; HTTP $($victim.status)." }
        "victimTenant=$victimTenant`nattackerTenant=$attackerTenant`nexecution=$victimExecution`nattackerStatus=$($execution.status)`nvictimStatus=$($victim.status)"
    }
    Add-Assertion "grant_bypass" {
        $attempt = Invoke-Status "$base/workflow-versions/$victimVersion/executions" "POST" $attackerToken @{ input = @{}; idempotencyKey = "m7-security-$runId" }
        if ($attempt.status -notin @(403, 404)) { throw "Cross-tenant Version execution returned HTTP $($attempt.status)." }
        $created = Invoke-MySql "SELECT COUNT(*) FROM workflow_executions WHERE tenant_id=UUID_TO_BIN('$attackerTenant') AND workflow_version_id=UUID_TO_BIN('$victimVersion');"
        if ($created -ne "0") { throw "Grant bypass created a cross-tenant Execution." }
        "workflowVersion=$victimVersion`nstatus=$($attempt.status)`ncrossTenantExecutions=$created"
    }
    Add-Assertion "log_secret_leak" {
        $marker = "m5-model-secret"
        $matches = [Collections.Generic.List[string]]::new()
        foreach ($deployment in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer")) {
            if (kubectl -n $Namespace get "deployment/$deployment" --ignore-not-found -o name) {
                $log = kubectl -n $Namespace logs "deployment/$deployment" --all-containers --tail=5000
                if (($log -join "`n").Contains($marker)) { $matches.Add($deployment) }
            }
        }
        if ($matches.Count -gt 0) { throw "Secret marker appeared in logs: $($matches -join ', ')." }
        "markerSha256=$([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($marker))).ToLowerInvariant())`nmatchedDeployments=0"
    }
    $brokerTranscript = $null
    Add-Assertion "credential_replay" {
        $script:brokerTranscript = & cargo test -p platform-api credentials::tests::broker_consumes_tokens_and_allows_outer_handle_id_consumption_without_deadlock -- --exact 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Credential Handle replay test failed: $($script:brokerTranscript -join [Environment]::NewLine)" }
        $script:brokerTranscript
    }
    Add-Assertion "handle_revocation" {
        $m5 = @{}
        foreach ($line in Get-Content -LiteralPath (Join-Path $e2e "m5-database-evidence.txt")) {
            $parts = $line -split '=', 2
            if ($parts.Count -eq 2) { $m5[$parts[0]] = $parts[1] }
        }
        if ($m5.unrevokedCredentialHandles -ne "0") { throw "Credential Handles remained valid after Sandbox cleanup." }
        if (-not $brokerTranscript) { throw "Credential replay test transcript is missing." }
        "unrevokedCredentialHandles=$($m5.unrevokedCredentialHandles)`nbrokerReplayTest=passed"
    }
    foreach ($family in @("ipv4", "ipv6")) {
        Add-Assertion "${family}_egress" {
            $junit = Join-Path $e2e "m5-agent-sandbox/junit.xml"
            [xml]$document = Get-Content -Raw -LiteralPath $junit
            $case = $document.SelectSingleNode("//testcase[contains(@name,'network policies')]")
            if ($null -eq $case -or $case.failure -or $case.skipped) { throw "M5 dual-stack network policy E2E did not pass." }
            $source = Get-Content -Raw -LiteralPath (Join-Path $root "services/platform-api/src/bin/m5-fixture.rs")
            $testSource = Get-Content -Raw -LiteralPath (Join-Path $root "apps/e2e/tests/m5-agent-sandbox.spec.ts")
            if (-not $source.Contains("socket.AF_INET") -or -not $source.Contains("socket.AF_INET6") -or -not $source.Contains("m5-{label}-denied")) {
                throw "The E2E fixture does not exercise both address families with a denied output." 
            }
            if (-not $testSource.Contains("m5-${family}-denied")) { throw "The Playwright E2E assertion does not verify $family deny." }
            "junit=$junit`ntest=$($case.name)`nfixtureAssertion=m5-${family}-denied"
        }
    }
}
finally {
    if ($forward -and -not $forward.HasExited) { Stop-Process -Id $forward.Id -Force -ErrorAction SilentlyContinue }
    $evidence = [ordered]@{
        schemaVersion = "agentx.io/m7-operational-evidence/v1"
        evidenceType = "security"
        status = $(if ($failures.Count -eq 0) { "passed" } else { "failed" })
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        assertions = @($assertions)
    }
    $path = Join-Path $output "security-evidence.json"
    $evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/m7-operational-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) { throw "Security evidence is invalid." }
    Write-Output $path
}
if ($failures.Count -gt 0) { throw "M7 security matrix failed: $($failures -join '; ')" }
