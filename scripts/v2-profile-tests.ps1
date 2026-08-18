$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$profilePath = Join-Path $root "deploy/profiles/v2-full-local.json"
$schemaPath = Join-Path $root "deploy/profiles/deployment-profile-v2.schema.json"
$json = Get-Content -Raw -LiteralPath $profilePath
if (Get-Command Test-Json -ErrorAction SilentlyContinue) {
    if (-not ($json | Test-Json -SchemaFile $schemaPath)) { throw "V2 profile does not match its schema." }
}

function Assert-V2Isolation {
    param($Profile)
    if ($Profile.apiVersion -ne "agentx.io/deployment/v2alpha3") { throw "V2 apiVersion is required." }
    $namespaces = @($Profile.namespaces.control, $Profile.namespaces.runtime, $Profile.namespaces.dependencies)
    if (($namespaces | Select-Object -Unique).Count -ne 3) { throw "V2 physical namespaces must be distinct." }
    $control = $Profile.components.controlMysql
    $runtime = $Profile.components.runtimeMysql
    if ($control.host -eq $runtime.host) { throw "Control and Runtime MySQL must use independent endpoints." }
    if ($control.appUser -eq $runtime.appUser -or $control.migrateUser -eq $runtime.migrateUser) { throw "Control and Runtime MySQL users must be distinct." }
    if ($control.appUser -eq $control.migrateUser -or $runtime.appUser -eq $runtime.migrateUser) { throw "Application and Migration MySQL users must be distinct." }
    foreach ($mysql in @($control, $runtime)) {
        if ([int64]$mysql.maxConnections * 100 -gt [int64]$mysql.serverMaxConnections * 70) {
            throw "MySQL pool budget must not exceed 70% of serverMaxConnections."
        }
    }
    foreach ($service in @($Profile.services.webConsole, $Profile.services.platformControl, $Profile.services.runtimeGateway, $Profile.services.workflowRuntime, $Profile.services.workflowWorker, $Profile.services.sandboxManager, $Profile.services.egressGateway, $Profile.services.observability)) {
        if ([int64]$service.replicas -gt [int64]$service.maxReplicas) { throw "Service replicas must not exceed maxReplicas." }
    }
    $controlPool = [int64]$Profile.services.platformControl.maxReplicas * [int64]$Profile.services.platformControl.mysqlPool
    $runtimePool = ([int64]$Profile.services.runtimeGateway.maxReplicas * [int64]$Profile.services.runtimeGateway.mysqlPool) +
        ([int64]$Profile.services.workflowRuntime.maxReplicas * [int64]$Profile.services.workflowRuntime.mysqlPool) +
        ([int64]$Profile.services.workflowWorker.maxReplicas * [int64]$Profile.services.workflowWorker.mysqlPool) +
        ([int64]$Profile.services.sandboxManager.maxReplicas * [int64]$Profile.services.sandboxManager.mysqlPool)
    if ($controlPool * 100 -gt [int64]$control.serverMaxConnections * 70) { throw "Control service pool budget exceeds 70 percent." }
    if ($runtimePool * 100 -gt [int64]$runtime.serverMaxConnections * 70) { throw "Runtime service pool budget exceeds 70 percent." }
    if ($runtimePool -ne 104 -or [int64]$runtime.maxConnections -ne 105) { throw "V2-06A Runtime max-replica connection budget must remain 104/105." }
    foreach ($role in @("api", "publisher", "projector", "retention")) {
        if (@($Profile.services.platformControl.roles) -notcontains $role) { throw "Platform Control is missing role $role." }
    }
    foreach ($role in @("coordinator", "trigger", "command", "outbox", "recovery", "artifact", "quota", "trace-relay")) {
        if (@($Profile.services.workflowRuntime.roles) -notcontains $role) { throw "Workflow Runtime is missing role $role." }
    }
    if ($Profile.services.workflowWorker.capability -ne "all") { throw "V2-04 requires the general Worker capability pool." }
    foreach ($role in @("trace-consumer", "query")) {
        if (@($Profile.services.observability.roles) -notcontains $role) { throw "Observability is missing role $role." }
    }
    $buckets = @($Profile.components.objectStorage.domains.control.bucket, $Profile.components.objectStorage.domains.runtime.bucket, $Profile.components.objectStorage.domains.observability.bucket)
    if (($buckets | Select-Object -Unique).Count -ne 3) { throw "Control, Runtime and Observability buckets must be distinct." }
    $users = @($Profile.components.objectStorage.domains.control.user, $Profile.components.objectStorage.domains.runtime.user, $Profile.components.objectStorage.domains.observability.user)
    if (($users | Select-Object -Unique).Count -ne 3) { throw "Object storage domain users must be distinct." }
    $clickhouseUsers = @($Profile.components.clickhouse.queryUser, $Profile.components.clickhouse.consumerUser, $Profile.components.clickhouse.migrateUser)
    if (($clickhouseUsers | Select-Object -Unique).Count -ne 3) { throw "ClickHouse Query, Consumer and Migration users must be distinct." }
    if ($Profile.ingress.controlHost -eq $Profile.ingress.runtimeHost) { throw "Control and Runtime hosts must be distinct." }
    if ($Profile.environment -eq "production" -and (-not $Profile.ingress.controlTlsSecretName -or -not $Profile.ingress.runtimeTlsSecretName)) { throw "Production ingress requires distinct TLS Secrets." }
    if ($Profile.environment -eq "production" -and -not [bool]$Profile.components.sandbox.secureAccess) { throw "Production OpenSandbox requires secureAccess." }
    if ($Profile.components.secretProvider.mode -ne "vault_kv_v2") { throw "V2 Runtime secrets require Vault KV v2." }
}

function Copy-Profile {
    return ($json | ConvertFrom-Json | ConvertTo-Json -Depth 20 | ConvertFrom-Json)
}

function Get-ModeManifest {
    param($Profile)
    $documents = [Collections.Generic.List[object]]::new()
    foreach ($name in @("controlMysql", "runtimeMysql", "runtimeRedis", "clickhouse", "objectStorage")) {
        $component = $Profile.components.$name
        $documents.Add([ordered]@{
            apiVersion = "v1"
            kind = "ConfigMap"
            metadata = @{ name = "v2-$($name.ToLowerInvariant())-mode" }
            data = @{ mode = [string]$component.mode }
        })
    }
    return ($documents | ForEach-Object { $_ | ConvertTo-Json -Depth 8 -Compress }) -join "`n---`n"
}

function Assert-Rejected {
    param([string]$Name, [scriptblock]$Mutation)
    $candidate = Copy-Profile
    & $Mutation $candidate
    try {
        Assert-V2Isolation $candidate
    }
    catch {
        return
    }
    throw "Negative V2 Profile fixture '$Name' was unexpectedly accepted."
}

$profile = $json | ConvertFrom-Json
Assert-V2Isolation $profile
$legacyProfile = Copy-Profile
$legacyProfile.apiVersion = "agentx.io/deployment/v2alpha2"
$legacyProfilePath = [IO.Path]::GetTempFileName()
try {
    $legacyProfile | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $legacyProfilePath
    $legacyRejected = $false
    try { & (Join-Path $root "scripts/deploy-v2.ps1") -Action Validate -ConfigFile $legacyProfilePath 2>$null | Out-Null } catch { $legacyRejected = $_.Exception.Message -match "v2alpha2 and earlier must be upgraded" }
    if (-not $legacyRejected) { throw "The V2 deployer did not explicitly reject the v2alpha2 Profile." }
} finally {
    Remove-Item -LiteralPath $legacyProfilePath -Force -ErrorAction SilentlyContinue
}
$modeNames = @("controlMysql", "runtimeMysql", "runtimeRedis", "clickhouse", "objectStorage")
for ($mask = 0; $mask -lt 32; $mask++) {
    $candidate = Copy-Profile
    for ($index = 0; $index -lt $modeNames.Count; $index++) {
        $name = $modeNames[$index]
        $candidate.components.$name.mode = if (($mask -band (1 -shl $index)) -eq 0) {
            if ($name -eq "objectStorage") { "bundled-minio" } else { "bundled" }
        }
        else {
            if ($name -eq "objectStorage") { "external-s3" } else { "external" }
        }
    }
    $candidateJson = $candidate | ConvertTo-Json -Depth 20
    if (Get-Command Test-Json -ErrorAction SilentlyContinue) {
        if (-not ($candidateJson | Test-Json -SchemaFile $schemaPath)) { throw "V2 mode combination $mask failed schema validation." }
    }
    Assert-V2Isolation $candidate
    $modeManifest = Get-ModeManifest $candidate
    $renderedCombination = ($modeManifest | & kubectl create --dry-run=client --validate=false -f - -o yaml) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $renderedCombination -notmatch "kind: ConfigMap") {
        throw "V2 mode combination $mask failed declarative manifest rendering."
    }
}
Assert-Rejected "shared namespace" { param($value) $value.namespaces.runtime = $value.namespaces.control }
Assert-Rejected "shared MySQL endpoint" { param($value) $value.components.runtimeMysql.host = $value.components.controlMysql.host }
Assert-Rejected "shared MySQL application user" { param($value) $value.components.runtimeMysql.appUser = $value.components.controlMysql.appUser }
Assert-Rejected "application user is migration user" { param($value) $value.components.controlMysql.migrateUser = $value.components.controlMysql.appUser }
Assert-Rejected "MySQL pool exceeds 70 percent" { param($value) $value.components.runtimeMysql.maxConnections = 106 }
Assert-Rejected "service pool exceeds 70 percent" { param($value) $value.services.workflowRuntime.mysqlPool = 11 }
Assert-Rejected "replicas exceed capacity budget" { param($value) $value.services.runtimeGateway.replicas = 5 }
Assert-Rejected "publisher role missing" { param($value) $value.services.platformControl.roles = @("api") }
Assert-Rejected "retention role missing" { param($value) $value.services.platformControl.roles = @("api", "publisher") }
Assert-Rejected "projector role missing" { param($value) $value.services.platformControl.roles = @("api", "publisher", "retention") }
Assert-Rejected "runtime outbox role missing" { param($value) $value.services.workflowRuntime.roles = @("coordinator", "command", "recovery", "trigger") }
Assert-Rejected "runtime trigger role missing" { param($value) $value.services.workflowRuntime.roles = @("coordinator", "command", "outbox", "recovery") }
Assert-Rejected "runtime trace relay role missing" { param($value) $value.services.workflowRuntime.roles = @("coordinator", "trigger", "command", "outbox", "recovery", "artifact", "quota") }
Assert-Rejected "observability query role missing" { param($value) $value.services.observability.roles = @("trace-consumer") }
Assert-Rejected "shared public host" { param($value) $value.ingress.runtimeHost = $value.ingress.controlHost }
Assert-Rejected "shared object bucket" { param($value) $value.components.objectStorage.domains.runtime.bucket = $value.components.objectStorage.domains.control.bucket }
Assert-Rejected "shared object user" { param($value) $value.components.objectStorage.domains.runtime.user = $value.components.objectStorage.domains.control.user }
Assert-Rejected "shared ClickHouse user" { param($value) $value.components.clickhouse.migrateUser = $value.components.clickhouse.queryUser }
Assert-Rejected "shared ClickHouse consumer user" { param($value) $value.components.clickhouse.consumerUser = $value.components.clickhouse.queryUser }
Assert-Rejected "production OpenSandbox without secure access" {
    param($value)
    $value.environment = "production"
    $value.ingress.controlTlsSecretName = "control-tls"
    $value.ingress.runtimeTlsSecretName = "runtime-tls"
    $value.components.sandbox.secureAccess = $false
}

$rendered = (& kubectl kustomize (Join-Path $root "deploy/k8s/v2")) -join "`n"
if ($LASTEXITCODE -ne 0) { throw "V2 Kustomize render failed." }
foreach ($forbidden in @("kind: Secret", "replace-me", "AGENTX_MYSQL_", "AGENTX_REDIS_")) {
    if ($rendered.Contains($forbidden)) { throw "V2 Kustomize render contains forbidden token: $forbidden" }
}
$runtimeApplications = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/runtime/applications.yaml")
$gatewayDocument = ($runtimeApplications -split "(?m)^---\s*$" | Where-Object { $_ -match "(?m)^metadata:\s*\{\s*name:\s*runtime-gateway," -and $_ -match "(?m)^kind: Deployment\s*$" } | Select-Object -First 1)
if (-not $gatewayDocument) { throw "Runtime Gateway Deployment is missing." }
if (-not $gatewayDocument.Contains("AGENTX_RUNTIME_REDIS_URL")) { throw "Runtime Gateway must carry Runtime Redis only for SSE wakeups." }
if (-not $gatewayDocument.Contains("AGENTX_RUNTIME_VAULT_TOKEN")) { throw "Runtime Gateway must carry the read-only Webhook Vault identity." }
if (-not $gatewayDocument.Contains("AGENTX_RUNTIME_GATEWAY_RATE_LIMIT_RPS") -or -not $gatewayDocument.Contains("AGENTX_RUNTIME_GATEWAY_RATE_LIMIT_BURST")) { throw "Runtime Gateway local rate limit configuration is missing." }
$workflowDocument = ($runtimeApplications -split "(?m)^---\s*$" | Where-Object { $_ -match "(?m)^metadata:\s*\{\s*name:\s*workflow-runtime," -and $_ -match "(?m)^kind: Deployment\s*$" } | Select-Object -First 1)
if ($workflowDocument -match "AGENTX_RUNTIME_USER_JWT|AGENTX_RUNTIME_VAULT") { throw "Workflow Runtime must not carry Gateway user JWT or Vault identity." }
$workerDocument = ($runtimeApplications -split "(?m)^---\s*$" | Where-Object { $_ -match "(?m)^metadata:\s*\{\s*name:\s*workflow-worker," -and $_ -match "(?m)^kind: Deployment\s*$" } | Select-Object -First 1)
if (-not $workerDocument -or -not $workerDocument.Contains("AGENTX_RUNTIME_VAULT_ENDPOINT") -or -not $workerDocument.Contains("AGENTX_RUNTIME_VAULT_TOKEN")) { throw "Workflow Worker must carry the read-only Runtime Vault identity." }
$runtimeResources = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/runtime/resources.yaml")
$workerVaultPolicy = ($runtimeResources -split "(?m)^---\s*$" | Where-Object { $_ -match "(?m)^metadata:\s*\{\s*name:\s*workflow-worker-vault-egress," } | Select-Object -First 1)
if (-not $workerVaultPolicy -or $workerVaultPolicy -notmatch 'app.kubernetes.io/name:\s*workflow-worker' -or $workerVaultPolicy -notmatch 'agentx.io/plane:\s*dependencies' -or $workerVaultPolicy -notmatch 'app.kubernetes.io/name:\s*vault' -or $workerVaultPolicy -notmatch 'port:\s*8200') { throw "Workflow Worker Vault egress must be restricted to the dependencies Vault service on TCP 8200." }
$controlDocument = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/control/applications.yaml")
if ($controlDocument -match "AGENTX_RUNTIME_VAULT_TOKEN") { throw "Control must not carry the Runtime read-only Vault identity." }
$observabilityDocument = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/observability/applications.yaml")
foreach ($forbidden in @("AGENTX_CONTROL_MYSQL", "AGENTX_RUNTIME_MYSQL", "AGENTX_RUNTIME_S3", "AGENTX_CONTROL_S3")) {
    if ($observabilityDocument.Contains($forbidden)) { throw "Observability application contains forbidden credential: $forbidden" }
}
foreach ($required in @("AGENTX_CLICKHOUSE_QUERY_USER", "AGENTX_CLICKHOUSE_CONSUMER_USER", "AGENTX_OBSERVABILITY_REDIS_USERNAME", "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON")) {
    if (-not $observabilityDocument.Contains($required)) { throw "Observability application is missing $required." }
}
$deployScript = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/deploy-v2.ps1")
if ($deployScript -notmatch '(?s)user observability.*\+xpending') {
    throw 'Observability Redis ACL must allow XPENDING for the low-cardinality Trace backlog metric.'
}
$observabilityResources = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/observability/resources.yaml")
$observabilityApplicationPolicy = ($observabilityResources -split "(?m)^---\s*$" | Where-Object { $_ -match 'name:\s*observability-data-access' } | Select-Object -First 1)
if ($observabilityApplicationPolicy -notmatch 'app.kubernetes.io/name:\s*runtime-redis' -or $observabilityApplicationPolicy -match 'namespaceSelector:.*agentx.io/plane:\s*runtime' -or $observabilityApplicationPolicy -match 'app.kubernetes.io/name:\s*object-storage') { throw "Observability NetworkPolicy must use the same-Namespace Runtime Redis path and deny Object Storage." }
$clickhouseIngress = ($observabilityResources -split "(?m)^---\s*$" | Where-Object { $_ -match 'name:\s*clickhouse-ingress' } | Select-Object -First 1)
if ($clickhouseIngress -notmatch 'agentx.io/ops-job:\s*"true"') { throw "ClickHouse Ingress must admit the restricted Observability Bootstrap and Doctor jobs." }
$vaultDocument = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/dependencies/resources.yaml")
if ($vaultDocument -notmatch 'agentx-control-webhook-writer' -or $vaultDocument -notmatch 'agentx-control-runtime-credential-writer' -or $vaultDocument -notmatch 'agentx-control-credential-manager') { throw "Control Vault path-scoped policies are missing." }
if ($vaultDocument -notmatch 'secret/data/tenants/\+/credentials/\+" \{ capabilities = \["create", "update", "read"\]' -or $vaultDocument -notmatch 'secret/destroy/tenants/\+/credentials/\+" \{ capabilities = \["update"\]') { throw "Control credential Vault lifecycle permissions are incomplete." }
if ($vaultDocument -notmatch 'agentx-runtime-secret-reader' -or $vaultDocument -notmatch 'secret/data/tenants/\+/credentials/\+" \{ capabilities = \["read"\]') { throw "Runtime credential Vault read-only policy is missing." }
if ($vaultDocument -notmatch 'secret/data/tenants/\+/webhooks/\+') { throw "Vault policy must use one-segment wildcards for Tenant and Webhook IDs." }
if ($vaultDocument -notmatch 'secret/data/tenants/\+/runtime-credentials/\+') { throw "Vault policy must isolate versioned Runtime credentials per Tenant and credential ID." }
$controlPolicy = [regex]::Match($vaultDocument, 'printf ''path "secret/data/tenants/\+/webhooks/\+" \{ capabilities = \[(.*?)\] \}').Groups[1].Value
if ($controlPolicy -match 'read') { throw "Control Vault policy must not read Webhook secrets." }
$runtimePolicy = [regex]::Matches($vaultDocument, 'printf ''path "secret/data/tenants/\+/webhooks/\+" \{ capabilities = \[(.*?)\] \}') | Select-Object -Last 1
if ($runtimePolicy.Groups[1].Value -match 'create|update|delete') { throw "Runtime Vault policy must not write Webhook secrets." }
$runtimeCredentialPolicy = [regex]::Match($vaultDocument, 'secret/data/tenants/\+/runtime-credentials/\+" \{ capabilities = \[("read")\] \}').Groups[1].Value
if ($runtimeCredentialPolicy -ne '"read"') { throw "Runtime credential Vault policy must be read-only." }
$controlIngress = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/control/ingress.yaml")
$runtimeIngress = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/runtime/ingress.yaml")
if ($controlIngress -notmatch 'path: /api' -or $controlIngress -notmatch 'path: /') { throw "Control Ingress must expose the API and Web Console." }
if ($runtimeIngress -notmatch 'path: /gateway/v1' -or $runtimeIngress -match '/internal/runtime/v1') { throw "Runtime Ingress must expose only /gateway/v1." }
$deploySource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/deploy-v2.ps1")
foreach ($profileField in @('ingress.className','ingress.controlHost','ingress.runtimeHost','ingress.controlTlsSecretName','ingress.runtimeTlsSecretName','secretProvider.endpoint','secretProvider.mount')) {
    if (-not $deploySource.Contains($profileField)) { throw "V2 deploy does not render Profile field $profileField." }
}
if ($deploySource -notmatch "\^06-" -or $deploySource -notmatch "02\|03\|04\|05\|06") { throw "V2 deploy does not recognize the V2-06 RunId namespace prefix." }
if ($deploySource -notmatch 'Remove-LegacyAutoscalingResources' -or $deploySource -notmatch 'PreserveReplicaWorkloadNames') { throw "V2 deploy must remove legacy Agentx autoscaling resources and preserve live replicas on Upgrade/Rollback." }
if ($deploySource -notmatch "spec\.template\.metadata\.labels\.'agentx.io/egress-client'") { throw "Dependencies uninstall must inspect Runtime Pod template Egress references." }
if ($deploySource -notmatch '&agentx:v2:invocation:wakeup:\*') { throw "Runtime Redis ACL must permit only the V2 SSE wakeup Pub/Sub channel family." }
$buildImagesSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/build-images.ps1")
if ($buildImagesSource -match 'create namespace \$Namespace --dry-run=client.*kubectl apply') {
    throw "Image loading must not re-apply Namespace metadata or remove V2 plane labels."
}
if ($buildImagesSource -notmatch 'get namespace \$Namespace --ignore-not-found') {
    throw "Image loading must preserve an existing V2 Namespace and its plane labels."
}
if ($buildImagesSource -notmatch 'agentx-observability') { throw "V2 image build does not map the Observability image to its Cargo binary." }
if ($buildImagesSource -notmatch 'agentx-egress-smoke' -or $buildImagesSource -notmatch 'egress-smoke') { throw "V2 image build does not map the Egress smoke image to its test binary." }
$egressE2eSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-egress-e2e.ps1")
foreach ($required in @('cloudflare/cloudflared@sha256:', 'StabilityOnlyEndpoint', 'agentx-egress-smoke', 'AGENTX_EGRESS_SMOKE_STABILITY_SECONDS', 'agentx-egress-bypass', 'docker rm --force', 'finally')) {
    if (-not $egressE2eSource.Contains($required)) { throw "V2 Egress E2E is missing: $required" }
}
if ($egressE2eSource.Contains('ngrok')) { throw "V2 Egress E2E must not depend on the removed ngrok fixture path." }
$webDockerfile = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/docker/web.Dockerfile")
if ($webDockerfile -notmatch 'COPY --chown=101:101 --from=builder .*/dist /opt/agentx-web') { throw "Web Console immutable assets must be staged for the read-only runtime container." }
if ($webDockerfile -notmatch 'touch /workspace/apps/web/dist/runtime-config.js') { throw "Web Console must pre-create runtime-config.js before copying assets to the unprivileged image." }
$controlApplications = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/control/applications.yaml")
if ($controlApplications -notmatch 'name: copy-web-assets' -or $controlApplications -notmatch 'mountPath: /usr/share/nginx/html' -or $controlApplications -notmatch 'readOnlyRootFilesystem: true') { throw "Web Console must copy immutable assets into an EmptyDir before starting with a read-only root filesystem." }
$controlResources = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/k8s/v2/control/resources.yaml")
if ($controlResources -notmatch 'name: web-console-to-platform-control' -or $controlResources -notmatch 'app.kubernetes.io/name: web-console' -or $controlResources -notmatch 'app.kubernetes.io/name: platform-control') {
    throw "Web Console must have an explicit same-Namespace NetworkPolicy path to Platform Control."
}
$v204E2eSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-04-e2e.ps1")
if ($v204E2eSource -match '\[Net\.Dns\]::GetHostAddresses' -or -not $v204E2eSource.Contains('"getent", "ahostsv4"')) {
    throw "V2-04 OpenSandbox egress must resolve the allowed host from the Sandbox Manager Pod network."
}
$v203E2eSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-03-e2e.ps1")
if (-not $v203E2eSource.Contains('$Stage -in @("05", "06")') -or $v203E2eSource -notmatch '\$controlUrl/api/v1/executions/') {
    throw "The V2-05/V2-06 composed baseline must use the frozen browser Execution BFF path."
}
$v205E2eSource = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/v2-05-e2e.ps1")
foreach ($required in @(
    '-Stage 05',
    'runtime_projection_status', 'runtime_query_receipts', 'integration_event_sequence', 'trace_ingest_conflicts',
    'statefulset/clickhouse', 'PROJECTION_REBUILDING', 'runtimeToControlMySql=denied', '-Action Uninstall'
)) {
    if (-not $v205E2eSource.Contains($required)) { throw "V2-05 E2E is missing the required assertion: $required" }
}
if ($v205E2eSource -notmatch 'finally\s*\{' -or $v205E2eSource -match '(?i)skip.*(mysql|redis|clickhouse).*success') {
    throw "V2-05 E2E must always clean up and must not convert a real dependency failure into success."
}
$bootstrapSource = Get-Content -Raw -LiteralPath (Join-Path $root "crates/agentx-v2-ops/src/lib.rs")
foreach ($forbiddenBootstrapFact in @('INSERT INTO tenants', 'INSERT INTO users', 'INSERT INTO workflow_service_identities', 'INSERT INTO tenant_admission')) {
    if ($bootstrapSource.Contains($forbiddenBootstrapFact)) { throw "The ops Bootstrap must not create product fact: $forbiddenBootstrapFact" }
}
$publicBootstrapSource = Get-Content -Raw -LiteralPath (Join-Path $root "services/platform-control/src/bootstrap_api.rs")
foreach ($permission in @('workflow:edit', 'execution:view', 'trace:view')) {
    if (-not $publicBootstrapSource.Contains('("' + $permission + '",')) { throw "The public Bootstrap is missing $permission." }
}
$renderProfile = Copy-Profile
$renderProfile.environment = "test"
$renderProfile.ingress.className = "v2-test-ingress"
$renderProfile.ingress.controlHost = "control.example.test"
$renderProfile.ingress.runtimeHost = "runtime.example.test"
$renderProfile.ingress.controlTlsSecretName = "control-test-tls"
$renderProfile.ingress.runtimeTlsSecretName = "runtime-test-tls"
$renderProfile.components.secretProvider.endpoint = "https://vault.example.test"
$renderProfile.components.secretProvider.mount = "agentx-v2"
$renderProfile.components.sandbox.endpoint = "https://opensandbox.example.test:8443"
$renderProfile.components.sandbox.secureAccess = $true
$renderProfilePath = [IO.Path]::GetTempFileName()
try {
    $renderProfile | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $renderProfilePath
    $profileRender = (& (Join-Path $root "scripts/deploy-v2.ps1") -Action Render -ConfigFile $renderProfilePath) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "Parameterized V2 Profile render failed." }
    foreach ($expected in @("ingressClassName: v2-test-ingress", "host: control.example.test", "host: runtime.example.test", "secretName: control-test-tls", "secretName: runtime-test-tls", "https://runtime.example.test", "https://vault.example.test", "https://opensandbox.example.test:8443", "value: agentx-v2", "cors-allow-origin: https://control.example.test")) {
        if (-not $profileRender.Contains($expected)) { throw "Parameterized V2 Profile render omitted: $expected" }
    }
    if ($profileRender -notmatch '(?ms)name: AGENTX_RUNTIME_ROLES\s+value: coordinator,trigger,command,outbox,recovery,artifact,quota,trace-relay\s*$') { throw "Workflow Runtime roles were not rendered from the V2 Profile." }
    if ($profileRender -notmatch '(?ms)name: AGENTX_CONTROL_ROLES\s+value: api,publisher,projector,retention\s*$') { throw "Platform Control roles were not rendered from the V2 Profile." }
    if ($profileRender -notmatch '(?ms)name: AGENTX_OBSERVABILITY_ROLES\s+value: trace-consumer,query\s*$') { throw "Observability roles were not rendered from the V2 Profile." }
    if ($profileRender -notmatch '(?ms)name: AGENTX_OPENSANDBOX_SECURE_ACCESS\s+value: "true"\s*$') { throw "OpenSandbox secure access was not rendered from the V2 Profile." }
    if ($profileRender.Contains("trigger,trigger")) { throw "Workflow Runtime trigger role was rendered more than once." }
}
finally {
    Remove-Item -LiteralPath $renderProfilePath -Force -ErrorAction SilentlyContinue
}
$runScopedProfile = Copy-Profile
$runScopedProfile.namespaces.control = "agentx-v2-custom-control"
$runScopedProfile.namespaces.runtime = "agentx-v2-custom-runtime"
$runScopedProfile.namespaces.dependencies = "agentx-v2-custom-deps"
$runScopedProfilePath = [IO.Path]::GetTempFileName()
try {
    $runScopedProfile | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $runScopedProfilePath
    $runScopedRender = (& (Join-Path $root "scripts/deploy-v2.ps1") -Action Render -Target All -ConfigFile $runScopedProfilePath -RunId "08-profile-render") -join "`n"
    if ($LASTEXITCODE -ne 0) { throw "V2-08 run-scoped Profile render failed." }
    foreach ($plane in @("control", "runtime", "deps")) {
        if (-not $runScopedRender.Contains("agentx-v2-08-$plane-profile-render")) { throw "V2-08 run-scoped render omitted the $plane Namespace." }
    }
    foreach ($endpoint in @(
        "control-mysql.agentx-v2-08-control-profile-render.svc",
        "runtime-mysql.agentx-v2-08-runtime-profile-render.svc",
        "clickhouse.agentx-v2-08-runtime-profile-render.svc",
        "object-storage.agentx-v2-08-deps-profile-render.svc"
    )) {
        if (-not $runScopedRender.Contains($endpoint)) { throw "V2-08 run-scoped render omitted the scoped endpoint $endpoint." }
    }
    if ($runScopedRender -match '(?m)^\s*namespace: agentx-v2-(?:custom-)?(?:control|runtime|deps)\s*$') {
        throw "V2-08 run-scoped render leaked a base or Profile Namespace."
    }
    if ($runScopedRender -match 'agentx-v2-(?:custom-)?(?:control|runtime|deps)\.svc') {
        throw "V2-08 run-scoped render leaked a base or Profile service endpoint."
    }
    $runScopedNodePortMatch = [regex]::Match($runScopedRender, '"nodePort"\s*:\s*(\d+)')
    if (-not $runScopedNodePortMatch.Success) { throw "V2-08 run-scoped render omitted its Sandbox Egress NodePort." }
    $runScopedNodePort = [int]$runScopedNodePortMatch.Groups[1].Value
    if ($runScopedNodePort -lt 32000 -or $runScopedNodePort -gt 32767 -or $runScopedNodePort -eq 31429) {
        throw "V2-08 run-scoped Sandbox Egress NodePort is not isolated: $runScopedNodePort"
    }
    if (-not $runScopedRender.Contains("https://host.docker.internal:$runScopedNodePort")) {
        throw "V2-08 run-scoped Sandbox Manager does not use its isolated NodePort."
    }
}
finally {
    Remove-Item -LiteralPath $runScopedProfilePath -Force -ErrorAction SilentlyContinue
}
$localProfileRender = (& (Join-Path $root "scripts/deploy-v2.ps1") -Action Render -ConfigFile $profilePath) -join "`n"
if ($LASTEXITCODE -ne 0 -or $localProfileRender -notmatch '(?ms)name: AGENTX_OPENSANDBOX_SECURE_ACCESS\s+value: "false"\s*$') {
    throw "Local V2 Profile must render OpenSandbox secure access as false for the Docker functional baseline."
}
$hpaCount = ([regex]::Matches($localProfileRender, '(?m)^kind: HorizontalPodAutoscaler\s*$')).Count
$pdbCount = ([regex]::Matches($localProfileRender, '(?m)^kind: PodDisruptionBudget\s*$')).Count
if ($hpaCount -ne 0 -or $pdbCount -ne 8) { throw "Agentx must render zero HPA and eight PDB resources." }
$physicalNamespaces = @([regex]::Matches($localProfileRender, '(?m)^\s*namespace:\s*(agentx-v2-[a-z0-9-]+)\s*$') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
if (($physicalNamespaces -join ',') -ne 'agentx-v2-control,agentx-v2-deps,agentx-v2-runtime') { throw "V2 must render exactly the control/runtime/dependencies physical Namespaces." }
if ($localProfileRender -match '(?m)^\s*namespace:\s*(?:agentx-ingress|agentx-v2-observability)\s*$') { throw "V2 render leaked a retired physical Namespace." }
foreach ($workload in @('platform-control','web-console','runtime-gateway','workflow-runtime','workflow-worker','sandbox-manager','agentx-egress-gateway','observability')) {
    $expectedReplicas = switch ($workload) {
        'platform-control' { [int]$profile.services.platformControl.replicas }
        'web-console' { [int]$profile.services.webConsole.replicas }
        'runtime-gateway' { [int]$profile.services.runtimeGateway.replicas }
        'workflow-runtime' { [int]$profile.services.workflowRuntime.replicas }
        'workflow-worker' { [int]$profile.services.workflowWorker.replicas }
        'sandbox-manager' { [int]$profile.services.sandboxManager.replicas }
        'agentx-egress-gateway' { [int]$profile.services.egressGateway.replicas }
        'observability' { [int]$profile.services.observability.replicas }
    }
    $deployment = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Deployment\s*$' -and $_ -match "(?m)^  name: $workload\s*$" } | Select-Object -First 1)
    if (-not $deployment -or $deployment -notmatch "(?m)^  replicas: $expectedReplicas\s*$" -or $deployment -notmatch 'maxUnavailable: 1' -or $deployment -notmatch 'terminationGracePeriodSeconds: 60') { throw "Invalid V2-06A lifecycle Deployment for $workload." }
}
$egressDeployment = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Deployment\s*$' -and $_ -match '(?m)^  name: agentx-egress-gateway\s*$' } | Select-Object -First 1)
foreach ($required in @('AGENTX_EGRESS_ALLOWED_PUBLIC_PORTS','AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON','containerPort: 3128','containerPort: 3129','containerPort: 9092','/health/drain')) {
    if (-not $egressDeployment.Contains($required)) { throw "Egress Gateway is missing $required." }
}
foreach ($forbidden in @('MYSQL','REDIS','VAULT','S3_ACCESS','S3_SECRET','PROVIDER_CREDENTIAL')) {
    if ($egressDeployment.Contains($forbidden)) { throw "Egress Gateway contains forbidden data credential token $forbidden." }
}
foreach ($runtimeClient in @('runtime-gateway','workflow-runtime','workflow-worker')) {
    $deployment = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Deployment\s*$' -and $_ -match "(?m)^  name: $runtimeClient\s*$" } | Select-Object -First 1)
    if ($deployment -notmatch 'agentx.io/egress-client: managed' -or $deployment -notmatch 'AGENTX_EGRESS_PROXY_URL') { throw "$runtimeClient is not bound to the managed Egress Gateway." }
}
if ($localProfileRender -notmatch '"name"\s*:\s*"agentx-egress-sandbox"' -or $localProfileRender -notmatch '"nodePort"\s*:\s*31429' -or $localProfileRender -notmatch '"type"\s*:\s*"NodePort"') {
    throw 'Local Sandbox egress Service must use the fixed private NodePort.'
}
foreach ($backend in @('platform-control','runtime-gateway','workflow-runtime','workflow-worker','sandbox-manager','observability')) {
    $deployment = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Deployment\s*$' -and $_ -match "(?m)^  name: $backend\s*$" } | Select-Object -First 1)
    foreach ($required in @('AGENTX_INSTANCE_ID','startupProbe:','readinessProbe:','livenessProbe:','/health/drain','port: admin','containerPort: 9091','containerPort: 9092')) {
        if (-not $deployment.Contains($required)) { throw "$backend is missing V2-06A lifecycle field $required." }
    }
    $service = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Service\s*$' -and $_ -match "(?m)^  name: $backend(?:-public|-internal|-metrics)?\s*$" }) -join "`n"
    if ($service -match '(?:port|targetPort):\s*(?:admin|9091)') { throw "$backend exposes its Drain admin port through a Service." }
}
$webDeployment = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Deployment\s*$' -and $_ -match '(?m)^  name: web-console\s*$' } | Select-Object -First 1)
foreach ($required in @('startupProbe:','readinessProbe:','livenessProbe:','nginx -s quit')) {
    if (-not $webDeployment.Contains($required)) { throw "web-console is missing its Nginx lifecycle exception field $required." }
}
if ($webDeployment -match 'containerPort:\s*909[12]' -or $webDeployment -match '/health/drain') {
    throw 'web-console must use the documented Nginx drain exception without Rust admin/metrics ports.'
}
$webServices = ($localProfileRender -split '(?m)^---\s*$' | Where-Object { $_ -match '(?m)^kind: Service\s*$' -and $_ -match '(?m)^  name: web-console(?:-metrics)?\s*$' }) -join "`n"
if ($webServices -match '(?:port|targetPort):\s*(?:admin|metrics|909[12])') {
    throw 'web-console must not expose backend admin or metrics services.'
}
if (Test-Path -LiteralPath (Join-Path $root 'deploy/k8s/v2/dependencies/metrics.yaml')) {
    throw 'Agentx must not ship a bundled metrics stack manifest.'
}
foreach ($forbiddenMetricsResource in @('app.kubernetes.io/name: prometheus','prometheus-adapter','metrics-server','external.metrics.k8s.io','metrics.k8s.io')) {
    if ($localProfileRender.Contains($forbiddenMetricsResource)) { throw "Rendered manifest contains removed metrics resource: $forbiddenMetricsResource" }
}
foreach ($scalingPath in @('deploy/k8s/v2/control/scaling.yaml','deploy/k8s/v2/runtime/scaling.yaml','deploy/k8s/v2/observability/scaling.yaml')) {
    $scaling = Get-Content -Raw -LiteralPath (Join-Path $root $scalingPath)
    if ($scaling -notmatch 'agentx.io/metrics-access:\s*"true"') { throw "$scalingPath must admit explicitly labelled external metrics collectors." }
}
$serviceKitSource = Get-Content -Raw -LiteralPath (Join-Path $root 'crates/agentx-service-kit/src/lib.rs')
foreach ($metric in @('agentx_queue_ready_items','agentx_queue_oldest_ready_seconds','agentx_active_leases','agentx_role_processing_seconds','agentx_http_inflight_requests','agentx_sse_connections','agentx_drain_inflight','agentx_mysql_pool_waiters')) {
    if (-not $serviceKitSource.Contains($metric)) { throw "Service Kit is missing low-cardinality metric $metric." }
}
foreach ($required in @('RoleProgressWatchdog','ROLE_WATCHDOG_TIMEOUT_SECONDS','role_schedulers_live','role_stalled','processed_since')) {
    if (-not $serviceKitSource.Contains($required)) { throw "Service Kit is missing Role progress watchdog behavior $required." }
}
foreach ($sourcePath in @(
    'services/platform-control/src/role_health.rs',
    'services/platform-control/src/projector.rs',
    'services/platform-control/src/retention.rs',
    'services/agentx-v2-runtime/src/bin/runtime-gateway.rs',
    'services/agentx-v2-runtime/src/bin/workflow-runtime.rs',
    'services/agentx-v2-runtime/src/bin/workflow-worker.rs',
    'services/agentx-v2-runtime/src/bin/sandbox-manager.rs',
    'services/observability/src/main.rs'
)) {
    $roleSource = Get-Content -Raw -LiteralPath (Join-Path $root $sourcePath)
    if (-not $roleSource.Contains('RoleProgressWatchdog')) { throw "$sourcePath is missing the V2-06A Role progress watchdog." }
}
$platformControlSource = Get-Content -Raw -LiteralPath (Join-Path $root 'services/platform-control/src/main.rs')
foreach ($role in @('publisher','admission-outbox','projector','retention')) {
    if ($platformControlSource -notmatch [regex]::Escape('role_health::watchdog("' + $role + '"')) { throw "Platform Control role $role is not wired to the progress watchdog." }
}
$v206E2ePath = Join-Path $root 'scripts/v2-06-e2e.ps1'
$v206E2eSource = Get-Content -Raw -LiteralPath $v206E2ePath
$v206Tokens = $null
$v206ParseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($v206E2ePath, [ref]$v206Tokens, [ref]$v206ParseErrors) | Out-Null
if ($v206ParseErrors.Count -ne 0) { throw "V2-06A E2E script has PowerShell parse errors: $($v206ParseErrors[0].Message)" }
foreach ($required in @(
    'v2-04-e2e.ps1', '-Stage 06', '"scale", "deployment"', '/health/drain',
    'worker_capabilities', 'Last-Event-ID', 'kind = "Eviction"', 'staleRows=0', 'Remove-WorkerBacklog',
    'projection_receipts', 'trace_outbox', 'XPENDING', 'bundle_retention_holds',
    'v2-06-web-zero-diff-tests.ps1', 'deferred = @("V2S-006"'
)) {
    if (-not $v206E2eSource.Contains($required)) { throw "V2-06A E2E script is missing required coverage: $required" }
}
if ($v206E2eSource.Contains("status='reserved'") -or $v206E2eSource.Contains('app.kubernetes.io/managed-by=agentx-v2')) {
    throw 'V2-06A E2E contains a stale Quota state or ownership label.'
}
$v208E2eSource = Get-Content -Raw -LiteralPath (Join-Path $root 'scripts/v2-08-e2e.ps1')
foreach ($required in @('control-$safeRunId.agentx.localhost', 'runtime-$safeRunId.agentx.localhost', 'Remove-OwnedMetricsClusterResources', "'agentx.io/metrics-owner'", '-RunId "08-$safeRunId"', '"service/web-console" 18081 8080')) {
    if (-not $v208E2eSource.Contains($required)) { throw "V2-08A E2E is missing isolated install or cleanup behavior: $required" }
}
$deployV2Source = Get-Content -Raw -LiteralPath (Join-Path $root 'scripts/deploy-v2.ps1')
if ($deployV2Source -match 'return\s+if\s*\(') { throw 'V2 deploy uses an invalid return-if expression.' }
$gatewaySource = Get-Content -Raw -LiteralPath (Join-Path $root "services/agentx-v2-runtime/src/gateway.rs")
$nonSseSource = $gatewaySource -replace '(?s)async fn invocation_events\(.*?\n\}', ''
if ($nonSseSource -match 'redis::|\.redis') { throw "Runtime Gateway Redis usage escaped the SSE wakeup module." }
Write-Output "V2 profile isolation tests passed"
