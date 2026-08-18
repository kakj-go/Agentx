param(
    [string]$ConfigFile = "deploy/profiles/v2-full-local.json",
    [string]$RunId = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss"),
    [switch]$BuildImages,
    [switch]$ScaleDownDevelopment,
    [switch]$KeepOnFailure
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$deploy = Join-Path $PSScriptRoot "deploy-v2.ps1"
$artifactDirectory = Join-Path $root "artifacts/v2/$RunId/v2-01"
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
"scripts/v2-01-e2e.ps1 -ConfigFile $ConfigFile -RunId $RunId -BuildImages:$BuildImages -ScaleDownDevelopment:$ScaleDownDevelopment" | Set-Content -LiteralPath (Join-Path $artifactDirectory "command.txt")

function Invoke-Kubectl {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    $output = & kubectl @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed: $($output -join [Environment]::NewLine)" }
    return @($output)
}

function Invoke-KubectlInput {
    param([string]$Content, [string[]]$Arguments)
    $output = $Content | & kubectl @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "kubectl $($Arguments -join ' ') failed: $($output -join [Environment]::NewLine)" }
    return @($output)
}

function Assert-SecretKeys {
    param([string]$Namespace, [string]$Secret, [string[]]$Required, [string[]]$Forbidden)
    $keys = @((Invoke-Kubectl -Arguments @("-n", $Namespace, "get", "secret", $Secret, "-o", "json")) -join "`n" | ConvertFrom-Json | ForEach-Object { $_.data.PSObject.Properties.Name })
    foreach ($key in $Required) { if ($key -notin $keys) { throw "$Namespace/$Secret lacks $key." } }
    foreach ($key in $Forbidden) { if ($key -in $keys) { throw "$Namespace/$Secret unexpectedly contains $key." } }
}

function Invoke-TestPod {
    param(
        [string]$Namespace,
        [string]$Name,
        [string]$Plane,
        [string]$Image,
        [string[]]$Command,
        [array]$Environment = @(),
        [bool]$ExpectSuccess = $true,
        [hashtable]$ExtraLabels = @{}
    )
    & kubectl -n $Namespace delete pod $Name --ignore-not-found --wait=true | Out-Null
    $labels = @{ "agentx.io/plane" = $Plane; "app.kubernetes.io/name" = $Name }
    foreach ($entry in $ExtraLabels.GetEnumerator()) { $labels[$entry.Key] = $entry.Value }
    $pod = @{
        apiVersion = "v1"
        kind = "Pod"
        metadata = @{ name = $Name; namespace = $Namespace; labels = $labels }
        spec = @{
            automountServiceAccountToken = $false
            restartPolicy = "Never"
            activeDeadlineSeconds = 45
            containers = @(@{ name = "probe"; image = $Image; imagePullPolicy = "IfNotPresent"; command = $Command; env = $Environment })
        }
    } | ConvertTo-Json -Depth 20 -Compress
    Invoke-KubectlInput $pod @("apply", "-f", "-") | Out-Null
    $deadline = (Get-Date).AddSeconds(60)
    do {
        $phase = ((Invoke-Kubectl -Arguments @("-n", $Namespace, "get", "pod", $Name, "-o", "jsonpath={.status.phase}")) -join "")
        if ($phase -in @("Succeeded", "Failed")) { break }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    $logs = (& kubectl -n $Namespace logs $Name 2>&1) -join "`n"
    $exitCode = (& kubectl -n $Namespace get pod $Name -o 'jsonpath={.status.containerStatuses[0].state.terminated.exitCode}' 2>$null) -join ""
    if (-not $exitCode) {
        $podState = ((Invoke-Kubectl -Arguments @("-n", $Namespace, "get", "pod", $Name, "-o", "json")) -join "`n") | ConvertFrom-Json
        if (-not $ExpectSuccess -and $podState.status.reason -eq "DeadlineExceeded") { return $logs }
        throw "Probe $Namespace/$Name did not terminate (phase=$($podState.status.phase), reason=$($podState.status.reason)). Logs: $logs"
    }
    $succeeded = [int]$exitCode -eq 0
    if ($succeeded -ne $ExpectSuccess) { throw "Probe $Namespace/$Name expected success=$ExpectSuccess, exit=$exitCode. Logs: $logs" }
    return $logs
}

function Install-HttpFixture {
    param(
        [string]$Namespace,
        [string]$Name,
        [string]$Plane,
        [int]$Port,
        [hashtable]$ExtraLabels = @{}
    )
    $labels = @{ "agentx.io/plane" = $Plane; "app.kubernetes.io/name" = $Name }
    foreach ($entry in $ExtraLabels.GetEnumerator()) { $labels[$entry.Key] = $entry.Value }
    $fixture = @{
        apiVersion = "v1"
        kind = "List"
        items = @(
            @{
                apiVersion = "v1"
                kind = "Pod"
                metadata = @{ name = $Name; namespace = $Namespace; labels = $labels }
                spec = @{
                    automountServiceAccountToken = $false
                    restartPolicy = "Always"
                    containers = @(@{
                        name = "http"
                        image = "hashicorp/http-echo:1.0"
                        args = @("-listen=:$Port", "-text=ok")
                        ports = @(@{ name = "http"; containerPort = $Port })
                    })
                }
            },
            @{
                apiVersion = "v1"
                kind = "Service"
                metadata = @{ name = $Name; namespace = $Namespace }
                spec = @{ selector = @{ "app.kubernetes.io/name" = $Name }; ports = @(@{ name = "http"; port = $Port; targetPort = "http" }) }
            }
        )
    }
    Invoke-KubectlInput ($fixture | ConvertTo-Json -Depth 20) @("apply", "-f", "-") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $Namespace, "wait", "--for=condition=Ready", "pod/$Name", "--timeout=120s") | Out-Null
}

function New-SecretEnvironment {
    param([string]$Name, [string]$Secret, [string]$Key)
    return @{ name = $Name; valueFrom = @{ secretKeyRef = @{ name = $Secret; key = $Key } } }
}

function New-ValueEnvironment {
    param([string]$Name, [string]$Value)
    return @{ name = $Name; value = $Value }
}

function Invoke-ConcurrentMigrationReplay {
    param([string]$Plane, [string]$Namespace, $Profile)
    $jobs = @("$Plane-migrate-replay-a", "$Plane-migrate-replay-b")
    foreach ($name in $jobs) {
        & kubectl -n $Namespace delete job $name --ignore-not-found --wait=true | Out-Null
        if ($Plane -eq "control") {
            $mysql = $Profile.components.controlMysql
            $secret = "agentx-control-secrets"
            $passwordKey = "AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD"
            $prefix = "AGENTX_CONTROL_MYSQL"
            $databaseHost = "control-mysql"
            $environment = @(
                (New-ValueEnvironment "$($prefix)_HOST" $databaseHost),
                (New-ValueEnvironment "$($prefix)_PORT" ([string]$mysql.port)),
                (New-ValueEnvironment "$($prefix)_DATABASE" ([string]$mysql.database)),
                (New-ValueEnvironment "$($prefix)_USER" ([string]$mysql.migrateUser)),
                (New-SecretEnvironment "$($prefix)_PASSWORD" $secret $passwordKey),
                (New-ValueEnvironment "$($prefix)_TLS_MODE" ([string]$mysql.tlsMode))
            )
        }
        elseif ($Plane -eq "runtime") {
            $mysql = $Profile.components.runtimeMysql
            $secret = "agentx-runtime-secrets"
            $passwordKey = "AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD"
            $prefix = "AGENTX_RUNTIME_MYSQL"
            $databaseHost = "runtime-mysql"
            $environment = @(
                (New-ValueEnvironment "$($prefix)_HOST" $databaseHost),
                (New-ValueEnvironment "$($prefix)_PORT" ([string]$mysql.port)),
                (New-ValueEnvironment "$($prefix)_DATABASE" ([string]$mysql.database)),
                (New-ValueEnvironment "$($prefix)_USER" ([string]$mysql.migrateUser)),
                (New-SecretEnvironment "$($prefix)_PASSWORD" $secret $passwordKey),
                (New-ValueEnvironment "$($prefix)_TLS_MODE" ([string]$mysql.tlsMode))
            )
        }
        else {
            $clickhouse = $Profile.components.clickhouse
            $environment = @(
                (New-ValueEnvironment "AGENTX_CLICKHOUSE_URL" "http://clickhouse:8123"),
                (New-ValueEnvironment "AGENTX_CLICKHOUSE_DATABASE" ([string]$clickhouse.database)),
                (New-ValueEnvironment "AGENTX_CLICKHOUSE_USER" ([string]$clickhouse.migrateUser)),
                (New-SecretEnvironment "AGENTX_CLICKHOUSE_PASSWORD" "agentx-observability-secrets" "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD")
            )
        }
        $job = @{
            apiVersion = "batch/v1"
            kind = "Job"
            metadata = @{ name = $name; namespace = $Namespace }
            spec = @{
                backoffLimit = 0
                template = @{
                    metadata = @{ labels = @{ "agentx.io/plane" = $Plane; "app.kubernetes.io/name" = $name } }
                    spec = @{
                        automountServiceAccountToken = $false
                        restartPolicy = "Never"
                        containers = @(@{
                            name = "migrate"
                            image = "$($Profile.images.registry)/agentx-migrate:$($Profile.images.tag)"
                            imagePullPolicy = [string]$Profile.images.pullPolicy
                            args = @($Plane)
                            env = $environment
                        })
                    }
                }
            }
        } | ConvertTo-Json -Depth 20 -Compress
        Invoke-KubectlInput $job @("apply", "-f", "-") | Out-Null
    }
    foreach ($name in $jobs) {
        Invoke-Kubectl -Arguments @("-n", $Namespace, "wait", "--for=condition=complete", "job/$name", "--timeout=180s") | Out-Null
    }
}

function Install-InternalApiFixture {
    param([string]$Namespace)
    $fixtureItems = @(
        @{
            apiVersion = "v1"
            kind = "Pod"
            metadata = @{ name = "runtime-internal-fixture"; namespace = $Namespace; labels = @{ "agentx.io/plane" = "runtime"; "agentx.io/internal-api" = "runtime-v1"; "app.kubernetes.io/name" = "runtime-internal-fixture" } }
            spec = @{
                automountServiceAccountToken = $false
                restartPolicy = "Always"
                containers = @(@{ name = "http"; image = "hashicorp/http-echo:1.0"; args = @("-listen=:8080", "-text=ok"); ports = @(@{ name = "http"; containerPort = 8080 }) })
            }
        },
        @{
            apiVersion = "v1"
            kind = "Service"
            metadata = @{ name = "runtime-internal-fixture"; namespace = $Namespace }
            spec = @{ selector = @{ "app.kubernetes.io/name" = "runtime-internal-fixture" }; ports = @(@{ name = "http"; port = 8080; targetPort = "http" }) }
        }
    )
    $fixture = @{ apiVersion = "v1"; kind = "List"; items = $fixtureItems }
    Invoke-KubectlInput ($fixture | ConvertTo-Json -Depth 20) @("apply", "-f", "-") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $Namespace, "wait", "--for=condition=Ready", "pod/runtime-internal-fixture", "--timeout=120s") | Out-Null
}

function Install-NetworkFixtures {
    param([hashtable]$Namespaces)
    Install-HttpFixture $Namespaces.dependencies "runtime-provider-fixture" "dependencies" 8081 @{ "agentx.io/runtime-provider" = "allowed" }
    foreach ($name in @("model-provider", "mcp-provider", "rag-provider", "memory-provider", "sandbox-provider")) {
        $service = @{
            apiVersion = "v1"
            kind = "Service"
            metadata = @{ name = $name; namespace = $Namespaces.dependencies }
            spec = @{ selector = @{ "app.kubernetes.io/name" = "runtime-provider-fixture" }; ports = @(@{ name = "http"; port = 8081; targetPort = "http" }) }
        }
        Invoke-KubectlInput ($service | ConvertTo-Json -Depth 10) @("apply", "-f", "-") | Out-Null
    }
    Install-HttpFixture $Namespaces.dependencies "unapproved-provider" "dependencies" 8081
    Install-HttpFixture $Namespaces.control "control-public-fixture" "control" 8080 @{ "agentx.io/public-api" = "control" }
    Install-HttpFixture $Namespaces.runtime "runtime-public-fixture" "runtime" 8080 @{ "agentx.io/public-api" = "runtime" }
    Install-HttpFixture $Namespaces.observability "observability-public-fixture" "observability" 8080 @{ "agentx.io/public-api" = "observability" }
}

function Get-DevelopmentReplicas {
    $replicas = @()
    foreach ($kind in @("deployment", "statefulset")) {
        $items = ((Invoke-Kubectl -Arguments @("-n", "agentx", "get", $kind, "-o", "json")) -join "`n" | ConvertFrom-Json).items
        foreach ($item in $items) {
            $replicas += [pscustomobject]@{ kind = $kind; name = [string]$item.metadata.name; replicas = [int]$item.spec.replicas }
        }
    }
    return $replicas
}

function Set-DevelopmentReplicas {
    param([array]$Replicas, [bool]$Stop)
    foreach ($item in $Replicas) {
        $target = if ($Stop) { 0 } else { $item.replicas }
        Invoke-Kubectl -Arguments @("-n", "agentx", "scale", "$($item.kind)/$($item.name)", "--replicas=$target") | Out-Null
    }
}

$profilePath = if ([IO.Path]::IsPathRooted($ConfigFile)) { $ConfigFile } else { Join-Path $root $ConfigFile }
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json
$namespaces = @{
    control = "agentx-v2-01-control-$RunId"
    runtime = "agentx-v2-01-runtime-$RunId"
    observability = "agentx-v2-01-runtime-$RunId"
    dependencies = "agentx-v2-01-deps-$RunId"
}
$developmentReplicas = @()
$succeeded = $false
$timeline = [Collections.Generic.List[string]]::new()

try {
    $timeline.Add("$(Get-Date -Format o) start run=$RunId context=$((Invoke-Kubectl -Arguments @('config','current-context')) -join '')")
    $developmentReplicas = @(Get-DevelopmentReplicas)
    $developmentReplicas | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $artifactDirectory "development-replicas.json")
    if ($ScaleDownDevelopment) {
        Set-DevelopmentReplicas $developmentReplicas $true
        $timeline.Add("$(Get-Date -Format o) development namespace scaled to zero")
    }

    & $deploy -Action Install -ConfigFile $profilePath -RunId $RunId -BuildImages:$BuildImages
    if ($LASTEXITCODE -ne 0) { throw "V2 deployment failed." }
    $timeline.Add("$(Get-Date -Format o) V2 infrastructure, migrations, bootstrap replay and doctors ready")
    Copy-Item -LiteralPath $profilePath -Destination (Join-Path $artifactDirectory "profile.json") -Force
    foreach ($plane in @("control", "runtime", "observability")) {
        $doctor = (Invoke-Kubectl -Arguments @("-n", $namespaces[$plane], "logs", "job/$plane-doctor")) -join "`n"
        $doctor | Set-Content -LiteralPath (Join-Path $artifactDirectory "$plane-doctor.json")
    }

    Assert-SecretKeys $namespaces.control "agentx-control-secrets" @("AGENTX_CONTROL_MYSQL_PASSWORD", "AGENTX_CONTROL_S3_SECRET_KEY") @("AGENTX_RUNTIME_MYSQL_PASSWORD", "AGENTX_RUNTIME_REDIS_PASSWORD", "AGENTX_REDIS_PASSWORD")
    Assert-SecretKeys $namespaces.runtime "agentx-runtime-secrets" @("AGENTX_RUNTIME_MYSQL_PASSWORD", "AGENTX_RUNTIME_REDIS_PASSWORD", "AGENTX_RUNTIME_S3_SECRET_KEY") @("AGENTX_CONTROL_MYSQL_PASSWORD")
    Assert-SecretKeys $namespaces.observability "agentx-observability-secrets" @("AGENTX_CLICKHOUSE_QUERY_PASSWORD", "AGENTX_OBSERVABILITY_S3_SECRET_KEY") @("AGENTX_RUNTIME_MYSQL_PASSWORD")

    Invoke-ConcurrentMigrationReplay "control" $namespaces.control $profile
    Invoke-ConcurrentMigrationReplay "runtime" $namespaces.runtime $profile
    Invoke-ConcurrentMigrationReplay "observability" $namespaces.observability $profile
    Install-InternalApiFixture $namespaces.runtime
    Install-NetworkFixtures $namespaces
    Invoke-TestPod $namespaces.control "control-runtime-internal-positive" "control" "curlimages/curl:8.12.1" @("sh", "-ec", "curl --fail --silent --show-error http://runtime-internal-fixture.$($namespaces.runtime).svc:8080 | grep '^ok$'") @() $true | Out-Null
    Invoke-TestPod $namespaces.runtime "runtime-provider-positive" "runtime" "curlimages/curl:8.12.1" @("sh", "-ec", "for provider in model mcp rag memory sandbox; do curl --fail --silent http://`${provider}-provider.$($namespaces.dependencies).svc:8081 | grep '^ok$'; done") @() $true | Out-Null
    Invoke-TestPod $namespaces.runtime "runtime-provider-denied" "runtime" "curlimages/curl:8.12.1" @("sh", "-ec", "curl --connect-timeout 3 --fail http://unapproved-provider.$($namespaces.dependencies).svc:8081") @() $false | Out-Null
    foreach ($case in @(
        @{ plane = "control"; service = "control-public-fixture"; namespace = $namespaces.control },
        @{ plane = "runtime"; service = "runtime-public-fixture"; namespace = $namespaces.runtime },
        @{ plane = "observability"; service = "observability-public-fixture"; namespace = $namespaces.observability }
    )) {
        Invoke-TestPod $namespaces.dependencies "ingress-$($case.plane)-positive" "dependencies" "curlimages/curl:8.12.1" @("sh", "-ec", "curl --fail --silent http://$($case.service).$($case.namespace).svc:8080 | grep '^ok$'") @() $true @{ "agentx.io/ingress-client" = "allowed" } | Out-Null
        Invoke-TestPod $namespaces.dependencies "ingress-$($case.plane)-denied" "dependencies" "curlimages/curl:8.12.1" @("sh", "-ec", "curl --connect-timeout 3 --fail http://$($case.service).$($case.namespace).svc:8080") @() $false | Out-Null
    }

    $controlDbEnv = @(
        (New-SecretEnvironment "MYSQL_PWD" "agentx-control-secrets" "AGENTX_CONTROL_MYSQL_PASSWORD")
    )
    Invoke-TestPod $namespaces.control "control-db-positive" "control" "mysql:8.4" @("sh", "-ec", "mysql -h control-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.controlMysql.database) -e 'SELECT COUNT(*) FROM tenants'") $controlDbEnv $true | Out-Null
    $controlConstraintSql = @"
set -eu
t1=11111111-1111-7111-8111-111111111111
t2=22222222-2222-7222-8222-222222222222
u1=33333333-3333-7333-8333-333333333333
w1=44444444-4444-7444-8444-444444444444
mysql -h control-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.controlMysql.database) -e "INSERT INTO tenants(id,name,normalized_name,status,version) VALUES(UUID_TO_BIN('`$t1'),'constraint-a','constraint-a','active',1),(UUID_TO_BIN('`$t2'),'constraint-b','constraint-b','active',1); INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status) VALUES(UUID_TO_BIN('`$u1'),UUID_TO_BIN('`$t1'),'constraint-user','constraint-user','Constraint User','active'); INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(UUID_TO_BIN('`$w1'),UUID_TO_BIN('`$t1'),'constraint-workflow','active','private',UUID_TO_BIN('`$u1'),UUID_TO_BIN('`$u1'));"
if mysql -h control-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.controlMysql.database) -e "INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('`$t2'),'cross-tenant-owner','active','private',UUID_TO_BIN('`$u1'),UUID_TO_BIN('`$u1'));"; then exit 9; fi
mysql -h control-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.controlMysql.database) -e "DELETE FROM workflows WHERE id=UUID_TO_BIN('`$w1'); DELETE FROM users WHERE id=UUID_TO_BIN('`$u1'); DELETE FROM tenants WHERE id IN (UUID_TO_BIN('`$t1'),UUID_TO_BIN('`$t2'));"
"@
    Invoke-TestPod $namespaces.control "control-schema-constraints" "control" "mysql:8.4" @("sh", "-ec", $controlConstraintSql) $controlDbEnv $true | Out-Null
    Invoke-TestPod $namespaces.control "control-app-ddl-denied" "control" "mysql:8.4" @("sh", "-ec", "if mysql -h control-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.controlMysql.database) -e 'CREATE TABLE forbidden_ddl(id INT)'; then exit 9; else exit 0; fi") $controlDbEnv $true | Out-Null
    Invoke-TestPod $namespaces.control "runtime-credential-denied" "control" "mysql:8.4" @("sh", "-ec", "if mysql -h control-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.controlMysql.database) -e 'SELECT 1'; then exit 9; else exit 0; fi") @((New-SecretEnvironment "MYSQL_PWD" "agentx-control-secrets" "AGENTX_CONTROL_MYSQL_PASSWORD")) $true | Out-Null

    $runtimeDbEnv = @((New-SecretEnvironment "MYSQL_PWD" "agentx-runtime-secrets" "AGENTX_RUNTIME_MYSQL_PASSWORD"))
    Invoke-TestPod $namespaces.runtime "runtime-db-positive" "runtime" "mysql:8.4" @("sh", "-ec", "mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e 'SELECT COUNT(*) FROM workflow_executions'") $runtimeDbEnv $true | Out-Null
    $runtimeConstraintSql = @"
set -eu
tenant=55555555-5555-7555-8555-555555555555
app=66666666-6666-7666-8666-666666666666
bundle=77777777-7777-7777-8777-777777777777
head=88888888-8888-7888-8888-888888888888
inv=99999999-9999-7999-8999-999999999999
mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "INSERT INTO deployment_bundles(id,tenant_id,application_id,sequence_number,schema_version,content_hash,signature,payload_json,status) VALUES(UUID_TO_BIN('`$bundle'),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$app'),1,1,CONCAT('sha256:',REPEAT('a',64)),X'01',JSON_OBJECT(),'prepared'); INSERT INTO deployment_heads(tenant_id,application_id,bundle_id,sequence_number,admission_epoch,version) VALUES(UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$app'),UUID_TO_BIN('`$bundle'),1,1,1); UPDATE deployment_heads SET version=2 WHERE tenant_id=UUID_TO_BIN('`$tenant') AND application_id=UUID_TO_BIN('`$app') AND version=1;"
changed=`$(mysql -N -s -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "UPDATE deployment_heads SET version=3 WHERE tenant_id=UUID_TO_BIN('`$tenant') AND application_id=UUID_TO_BIN('`$app') AND version=1; SELECT ROW_COUNT();" | tail -n1)
test "`$changed" = 0
mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "INSERT INTO application_invocations(id,tenant_id,application_id,workflow_version_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(UUID_TO_BIN('`$inv'),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$app'),UUID_TO_BIN('`$bundle'),'user',UUID_TO_BIN('`$head'),REPEAT('b',64),'same-key','queued');"
if mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "INSERT INTO application_invocations(id,tenant_id,application_id,workflow_version_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$app'),UUID_TO_BIN('`$bundle'),'user',UUID_TO_BIN('`$head'),REPEAT('c',64),'same-key','queued');"; then exit 9; fi
if mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$bundle'),UUID_TO_BIN('`$bundle'),UUID_TO_BIN(UUID()),'manual','not-a-state',UTC_TIMESTAMP(6));"; then exit 9; fi
if mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "INSERT INTO tenant_admission(tenant_id,status,admission_epoch,policy_version) VALUES(UUID_TO_BIN(UUID()),'active',0,1);"; then exit 9; fi
mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "START TRANSACTION; INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(UUID_TO_BIN('`$head'),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$bundle'),UUID_TO_BIN('`$bundle'),UUID_TO_BIN('`$head'),'manual','created',UTC_TIMESTAMP(6)); INSERT INTO execution_outbox(id,tenant_id,execution_id,message_type,payload_json) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN('`$tenant'),UUID_TO_BIN('`$head'),'runtime_event',JSON_OBJECT()); ROLLBACK;"
test "`$(mysql -N -s -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "SELECT COUNT(*) FROM workflow_executions WHERE id=UUID_TO_BIN('`$head')")" = 0
test "`$(mysql -N -s -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "SELECT COUNT(*) FROM execution_outbox WHERE execution_id=UUID_TO_BIN('`$head')")" = 0
mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e "DELETE FROM application_invocations WHERE id=UUID_TO_BIN('`$inv'); DELETE FROM deployment_heads WHERE tenant_id=UUID_TO_BIN('`$tenant') AND application_id=UUID_TO_BIN('`$app'); DELETE FROM deployment_bundles WHERE id=UUID_TO_BIN('`$bundle');"
"@
    Invoke-TestPod $namespaces.runtime "runtime-schema-constraints" "runtime" "mysql:8.4" @("sh", "-ec", $runtimeConstraintSql) $runtimeDbEnv $true | Out-Null
    Invoke-TestPod $namespaces.runtime "runtime-app-ddl-denied" "runtime" "mysql:8.4" @("sh", "-ec", "if mysql -h runtime-mysql -u $($profile.components.runtimeMysql.appUser) $($profile.components.runtimeMysql.database) -e 'CREATE TABLE forbidden_ddl(id INT)'; then exit 9; else exit 0; fi") $runtimeDbEnv $true | Out-Null
    Invoke-TestPod $namespaces.runtime "control-credential-denied" "runtime" "mysql:8.4" @("sh", "-ec", "if mysql -h runtime-mysql -u $($profile.components.controlMysql.appUser) $($profile.components.runtimeMysql.database) -e 'SELECT 1'; then exit 9; else exit 0; fi") @((New-SecretEnvironment "MYSQL_PWD" "agentx-runtime-secrets" "AGENTX_RUNTIME_MYSQL_PASSWORD")) $true | Out-Null

    $clickhouseEnv = @(
        (New-SecretEnvironment "CLICKHOUSE_PASSWORD" "agentx-observability-secrets" "AGENTX_CLICKHOUSE_QUERY_PASSWORD"),
        (New-SecretEnvironment "CLICKHOUSE_MIGRATE_PASSWORD" "agentx-observability-secrets" "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD")
    )
    Invoke-TestPod $namespaces.observability "observability-query-positive" "observability" "curlimages/curl:8.12.1" @("sh", "-ec", "curl --fail --silent --show-error --user '$($profile.components.clickhouse.queryUser):'`$CLICKHOUSE_PASSWORD --data-binary 'SELECT count() FROM workflow_trace_events' 'http://clickhouse:8123/?database=$($profile.components.clickhouse.database)'") $clickhouseEnv $true | Out-Null
    $traceId = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa"
    $traceTenant = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb"
    $traceExecution = "cccccccc-cccc-7ccc-8ccc-cccccccccccc"
    $traceInsert = "INSERT INTO workflow_trace_events(event_id,tenant_id,trace_id,span_id,execution_id,workflow_id,event_type,status,event_time,run_index,iteration_index,cost_micros,partial,attributes_json,row_version) VALUES ('$traceId','$traceTenant','$traceId','$traceId','$traceExecution','$traceExecution','probe','old',toDateTime64('2026-08-13 00:00:00',6,'UTC'),0,0,0,0,'{}',1),('$traceId','$traceTenant','$traceId','$traceId','$traceExecution','$traceExecution','probe','new',toDateTime64('2026-08-13 00:00:00',6,'UTC'),0,0,0,0,'{}',2)"
    $traceDedupCommand = @"
curl --fail --silent --user '$($profile.components.clickhouse.migrateUser):'`$CLICKHOUSE_MIGRATE_PASSWORD --data-binary "$traceInsert" 'http://clickhouse:8123/?database=$($profile.components.clickhouse.database)'
test "`$(curl --fail --silent --user '$($profile.components.clickhouse.queryUser):'`$CLICKHOUSE_PASSWORD --data-binary "SELECT count() FROM workflow_trace_events FINAL WHERE event_id='$traceId' AND status='new'" 'http://clickhouse:8123/?database=$($profile.components.clickhouse.database)')" = 1
"@
    Invoke-TestPod $namespaces.observability "observability-dedup" "observability" "curlimages/curl:8.12.1" @("sh", "-ec", $traceDedupCommand) $clickhouseEnv $true | Out-Null
    Invoke-TestPod $namespaces.observability "observability-ddl-denied" "observability" "curlimages/curl:8.12.1" @("sh", "-ec", "if curl --fail --silent --user '$($profile.components.clickhouse.queryUser):'`$CLICKHOUSE_PASSWORD --data-binary 'CREATE TABLE forbidden_ddl(id UInt64) ENGINE=Memory' 'http://clickhouse:8123/?database=$($profile.components.clickhouse.database)'; then exit 9; else exit 0; fi") $clickhouseEnv $true | Out-Null

    $runtimeRedisEnv = @((New-SecretEnvironment "REDIS_PASSWORD" "agentx-runtime-secrets" "AGENTX_RUNTIME_REDIS_PASSWORD"))
    Invoke-TestPod $namespaces.runtime "runtime-redis-positive" "runtime" "redis:7.4-alpine" @("sh", "-ec", 'redis-cli -h runtime-redis -a "$REDIS_PASSWORD" SET v2-e2e value >/dev/null && redis-cli -h runtime-redis -a "$REDIS_PASSWORD" GET v2-e2e | grep value') $runtimeRedisEnv $true | Out-Null
    Invoke-TestPod $namespaces.control "control-runtime-redis-denied" "control" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 runtime-redis.$($namespaces.runtime).svc 6379") @() $false | Out-Null
    Invoke-TestPod $namespaces.runtime "runtime-control-mysql-denied" "runtime" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 control-mysql.$($namespaces.control).svc 3306") @() $false | Out-Null
    Invoke-TestPod $namespaces.control "control-runtime-mysql-denied" "control" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 runtime-mysql.$($namespaces.runtime).svc 3306") @() $false | Out-Null
    Invoke-TestPod $namespaces.observability "observability-runtime-mysql-denied" "observability" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 runtime-mysql.$($namespaces.runtime).svc 3306") @() $false | Out-Null
    Invoke-TestPod $namespaces.control "control-clickhouse-denied" "control" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 clickhouse.$($namespaces.observability).svc 8123") @() $false | Out-Null
    Invoke-TestPod $namespaces.control "control-migrate-runtime-db-denied" "control" "busybox:1.37" @("sh", "-ec", "nc -z -w 3 runtime-mysql.$($namespaces.runtime).svc 3306") @() $false | Out-Null

    foreach ($case in @(
        @{ plane = "control"; own = "agentx-control"; denied = "agentx-runtime"; secret = "agentx-control-secrets"; key = "AGENTX_CONTROL_S3_SECRET_KEY"; user = "control_object" },
        @{ plane = "runtime"; own = "agentx-runtime"; denied = "agentx-control"; secret = "agentx-runtime-secrets"; key = "AGENTX_RUNTIME_S3_SECRET_KEY"; user = "runtime_object" },
        @{ plane = "observability"; own = "agentx-observability"; denied = "agentx-runtime"; secret = "agentx-observability-secrets"; key = "AGENTX_OBSERVABILITY_S3_SECRET_KEY"; user = "observability_object" }
    )) {
        $namespace = [string]$namespaces[$case.plane]
        $env = @(
            (New-ValueEnvironment "S3_USER" $case.user),
            (New-SecretEnvironment "S3_PASSWORD" $case.secret $case.key)
        )
        $command = "mc alias set local http://object-storage.$($namespaces.dependencies).svc:9000 `$S3_USER `$S3_PASSWORD >/dev/null; echo probe >/tmp/probe; mc cp /tmp/probe local/$($case.own)/$RunId/probe >/dev/null; mc stat local/$($case.own)/$RunId/probe >/dev/null; if mc stat local/$($case.denied) >/dev/null 2>&1; then exit 9; fi; if mc cp /tmp/probe local/$($case.denied)/$RunId/forbidden-write >/dev/null 2>&1; then mc rm local/$($case.denied)/$RunId/forbidden-write >/dev/null 2>&1 || true; exit 9; fi; mc rm local/$($case.own)/$RunId/probe >/dev/null"
        Invoke-TestPod $namespace "$($case.plane)-object-boundary" $case.plane "quay.io/minio/mc:latest" @("sh", "-ec", $command) $env $true | Out-Null
    }

    Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "scale", "statefulset/runtime-redis", "--replicas=0") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "delete", "pvc", "data-runtime-redis-0", "--wait=true", "--timeout=120s") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "scale", "statefulset/runtime-redis", "--replicas=1") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "rollout", "status", "statefulset/runtime-redis", "--timeout=180s") | Out-Null
    Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "wait", "--for=condition=Ready", "pod/runtime-redis-0", "--timeout=180s") | Out-Null
    $redisExists = (Invoke-Kubectl -Arguments @("-n", $namespaces.runtime, "exec", "runtime-redis-0", "--", "sh", "-ec", 'redis-cli --raw -a "$REDIS_PASSWORD" EXISTS v2-e2e 2>/dev/null')) -join ""
    if ($redisExists.Trim() -ne "0") { throw "Runtime Redis rebuild retained v2-e2e (EXISTS=$redisExists)." }
    $timeline.Add("$(Get-Date -Format o) database, Redis, OSS and NetworkPolicy assertions passed")

    foreach ($plane in @("control", "runtime", "dependencies")) {
        Invoke-Kubectl -Arguments @("-n", $namespaces[$plane], "get", "all,networkpolicy,secret", "-o", "wide") | Set-Content -LiteralPath (Join-Path $artifactDirectory "$plane-resources.txt")
    }
    $assertions = [ordered]@{
        runId = $RunId
        profileApiVersion = [string]$profile.apiVersion
        migrations = "control/runtime/observability complete; concurrent replay passed in all three domains"
        bootstrapReplay = "two consecutive runs per domain passed"
        doctors = "control/runtime/observability passed"
        mysql = @{
            independentInstances = "passed"
            domainDml = "passed"
            applicationDdlDenied = "passed"
            crossDomainUsersDenied = "passed"
        }
        clickhouse = @{
            queryAccountSelect = "passed"
            queryAccountDdlDenied = "passed"
            duplicateEventFinalQuery = "passed"
        }
        networkPolicy = @{
            controlToRuntimeInternalApiAllowed = "passed"
            controlToRuntimeRedisDenied = "passed"
            runtimeToControlMysqlDenied = "passed"
            controlToRuntimeMysqlDenied = "passed"
            observabilityToRuntimeMysqlDenied = "passed"
            controlToClickHouseDenied = "passed"
            runtimeApprovedProvidersAllowed = "passed"
            runtimeUnapprovedProviderDenied = "passed"
            publicIngressAllowAndDeny = "passed"
        }
        objectStorage = @{
            controlOwnBucket = "passed"
            runtimeOwnBucket = "passed"
            observabilityOwnBucket = "passed"
            crossDomainReadsDenied = "passed"
            crossDomainWritesDenied = "passed"
        }
        schemaBehavior = @{
            controlTenantForeignKeys = "passed"
            runtimeHeadCas = "passed"
            runtimeIdempotencyConstraint = "passed"
            runtimeStatusAndEpochConstraints = "passed"
            runtimeDomainOutboxRollbackAtomic = "passed"
        }
        runtimeRedis = @{
            authenticatedReadWrite = "passed"
            pvcEmptyRebuild = "passed"
        }
        secrets = "keys asserted; values not retained"
        temporaryNamespaces = "scheduled for cleanup in finally"
    }
    $assertions | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $artifactDirectory "assertions.json")
    $imageEvidence = foreach ($image in @("agentx/agentx-migrate:$($profile.images.tag)", "agentx/agentx-bootstrap:$($profile.images.tag)", "agentx/agentx-doctor:$($profile.images.tag)")) {
        $inspect = ((& docker image inspect $image) -join "`n") | ConvertFrom-Json
        [ordered]@{ image = $image; id = [string]$inspect.Id; created = [string]$inspect.Created }
    }
    $imageEvidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $artifactDirectory "images.json")
    $succeeded = $true
}
finally {
    $timeline.Add("$(Get-Date -Format o) cleanup begin success=$succeeded")
    if (-not $KeepOnFailure -or $succeeded) {
        & $deploy -Action Uninstall -ConfigFile $profilePath -RunId $RunId
        if ($LASTEXITCODE -ne 0) { Write-Warning "V2 namespace cleanup failed." }
    }
    if ($ScaleDownDevelopment -and $developmentReplicas.Count -gt 0) {
        Set-DevelopmentReplicas $developmentReplicas $false
        foreach ($item in $developmentReplicas | Where-Object { $_.replicas -gt 0 }) {
            Invoke-Kubectl -Arguments @("-n", "agentx", "rollout", "status", "$($item.kind)/$($item.name)", "--timeout=300s") | Out-Null
        }
        $timeline.Add("$(Get-Date -Format o) development namespace replicas restored")
    }
    try {
        $remainingNamespaces = @(& kubectl get namespace -o name | Where-Object { $_ -like "namespace/agentx-v2-01-*-$RunId" })
        $developmentStatus = @(((& kubectl -n agentx get deployment,statefulset -o json) -join "`n" | ConvertFrom-Json).items | ForEach-Object {
            [ordered]@{ kind = [string]$_.kind; name = [string]$_.metadata.name; desired = [int]$_.spec.replicas; ready = [int]$_.status.readyReplicas }
        })
        [ordered]@{ temporaryNamespacesRemaining = $remainingNamespaces; developmentWorkloads = $developmentStatus } |
            ConvertTo-Json -Depth 8 |
            Set-Content -LiteralPath (Join-Path $artifactDirectory "post-cleanup.json")
        if ($succeeded -and $remainingNamespaces.Count -ne 0) { throw "Temporary V2 namespaces remain after cleanup." }
        if ($succeeded -and @($developmentStatus | Where-Object { $_.desired -ne $_.ready }).Count -ne 0) { throw "Development namespace did not recover all desired replicas." }
    }
    catch {
        if ($succeeded) { throw }
        Write-Warning "Post-cleanup evidence failed: $_"
    }
    $timeline.Add("$(Get-Date -Format o) cleanup end")
    $timeline | Set-Content -LiteralPath (Join-Path $artifactDirectory "timeline.txt")
}

if (-not $succeeded) { throw "V2-01 E2E failed." }
Write-Output "V2-01 E2E passed; artifacts: $artifactDirectory"
