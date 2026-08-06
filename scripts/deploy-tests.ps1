$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$module = Import-Module (Join-Path $PSScriptRoot "deploy/Agentx.Deployment.psm1") -Force -PassThru

& $module {
    param($RepoRoot)

    function Assert-True([bool]$Condition, [string]$Message) {
        if (-not $Condition) { throw $Message }
    }

    function Assert-Throws([scriptblock]$Action, [string]$Message) {
        try { & $Action; throw "Expected failure: $Message" }
        catch { if ($_.Exception.Message -eq "Expected failure: $Message") { throw } }
    }

    $schema = Join-Path $RepoRoot "deploy/profiles/deployment-profile.schema.json"
    foreach ($sample in @("full-local.json", "custom.example.json")) {
        $json = Get-Content (Join-Path $RepoRoot "deploy/profiles/$sample") -Raw
        Assert-True ($json | Test-Json -SchemaFile $schema) "$sample does not match the deployment schema"
        Assert-True ($json -notmatch '(?i)(password|api.?key|secret)\s*:\s*"[^"]+"') "$sample appears to contain a plaintext secret"
    }

    $templateJson = Get-Content (Join-Path $RepoRoot "deploy/profiles/full-local.json") -Raw
    $count = 0
    foreach ($mysql in @("bundled", "external")) {
        foreach ($redis in @("bundled", "external")) {
            foreach ($clickhouse in @("bundled", "external")) {
                foreach ($storage in @("bundled-minio", "external-s3")) {
                    foreach ($rag in @("bundled", "external", "disabled")) {
                        foreach ($memory in @("bundled", "external", "disabled")) {
                            foreach ($sandbox in @("disabled", "remote")) {
                                foreach ($images in @("local-build", "registry")) {
                                    $profile = $templateJson | ConvertFrom-Json -Depth 30
                                    $profile.components.mysql.mode = $mysql
                                    $profile.components.redis.mode = $redis
                                    $profile.components.clickhouse.mode = $clickhouse
                                    $profile.components.objectStorage.mode = $storage
                                    $profile.components.rag.mode = $rag
                                    $profile.components.memory.mode = $memory
                                    $profile.components.sandbox.mode = $sandbox
                                    $profile.images.mode = $images
                                    if ($sandbox -eq "remote") {
                                        $profile.components.sandbox.endpoint = "https://sandbox.example.test"
                                        $profile.components.sandbox.allowedHosts = @("sandbox.example.test")
                                    }
                                    Assert-DeploymentProfile -Profile $profile -RepoRoot $RepoRoot
                                    $count++
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Assert-True ($count -eq 576) "not all deployment mode combinations were validated"

    $missing = $templateJson | ConvertFrom-Json -Depth 30
    $missing.components.rag.provider = $null
    Assert-Throws { Assert-DeploymentProfile -Profile $missing -RepoRoot $RepoRoot } "bundled RAG without provider"

    $invalidJson = $templateJson.Replace('"mode": "bundled", "host": "mysql"', '"mode": "disabled", "host": "mysql"')
    Assert-True (-not ($invalidJson | Test-Json -SchemaFile $schema -ErrorAction SilentlyContinue)) "schema accepted disabled MySQL"
    $missingNamespace = $templateJson | ConvertFrom-Json -Depth 30
    $missingNamespace.PSObject.Properties.Remove("namespace")
    Assert-True (-not (($missingNamespace | ConvertTo-Json -Depth 30) | Test-Json -SchemaFile $schema -ErrorAction SilentlyContinue)) "schema accepted a missing required namespace"

    $left = [ordered]@{ z = 1; nested = [ordered]@{ b = 2; a = 1 } }
    $right = [ordered]@{ nested = [ordered]@{ a = 1; b = 2 }; z = 1 }
    Assert-True ((Get-ProfileHash $left) -eq (Get-ProfileHash $right)) "profile hash is sensitive to JSON property order"

    $before = $templateJson | ConvertFrom-Json -Depth 30
    $after = $templateJson | ConvertFrom-Json -Depth 30
    $after.components.mysql.mode = "external"
    Assert-Throws { Assert-StatefulModesUnchanged -Before $before -After $after } "stateful mode switch"

    $full = $templateJson | ConvertFrom-Json -Depth 30
    $fullKeys = @(Get-CoreSecretKeys -Profile $full)
    Assert-True ($fullKeys -contains "AGENTX_MYSQL_ROOT_PASSWORD") "bundled MySQL root secret key was not required"
    Assert-True ($fullKeys -contains "AGENTX_REMOTE_NODE_AUTH_TOKEN") "remote node auth token was not required"
    $external = $templateJson | ConvertFrom-Json -Depth 30
    $external.components.mysql.mode = "external"
    Assert-True (@(Get-CoreSecretKeys -Profile $external) -notcontains "AGENTX_MYSQL_ROOT_PASSWORD") "external MySQL unexpectedly requires a root password"

    $localImage = $templateJson | ConvertFrom-Json -Depth 30
    $localImage.images.mode = "local-build"
    $localImage.images.registry = "registry.example.test/ignored"
    $rendered = (Invoke-ComponentRender -Profile $localImage -RepoRoot $RepoRoot -Component "services/core") -join "`n"
    Assert-True ($rendered -match 'image: agentx/platform-api:dev') "local-build render did not use local Agentx images"
    Assert-True ($rendered -notmatch 'registry\.example\.test/ignored/platform-api') "local-build render incorrectly used images.registry"
    Assert-True ($rendered -match 'automountServiceAccountToken: false') "core Pods still mount ServiceAccount tokens"
    Assert-True ($rendered -match 'readOnlyRootFilesystem: true') "core Pods do not use a read-only root filesystem"
    Assert-True ($rendered -match 'type: RuntimeDefault') "core Pods do not use RuntimeDefault seccomp"
    Assert-True ($rendered -match 'name: agentx-core-default-deny') "core default-deny NetworkPolicy is missing"
    Assert-True ($rendered -match 'ephemeral-storage: 256Mi') "core ephemeral storage limits are missing"
    Assert-True (([regex]::Matches($rendered, 'name: AGENTX_VAULT_TOKEN')).Count -eq 1) "Vault token is exposed outside Platform API"

    $customRender = $templateJson | ConvertFrom-Json -Depth 30
    $customRender.namespace = "agentx-render-check"
    $customRender.images.mode = "registry"
    $customRender.images.registry = "registry.example.test/agentx"
    $customRender.components.mysql.mode = "external"
    $customRender.components.redis.mode = "external"
    $customRender.components.clickhouse.mode = "external"
    $customRender.components.objectStorage.mode = "external-s3"
    $customRender.components.rag.mode = "disabled"
    $customRender.components.memory.mode = "external"
    $customRender.components.memory.endpoint = "https://memory.example.test"
    $customRender.components.sandbox.mode = "disabled"
    Assert-DeploymentProfile -Profile $customRender -RepoRoot $RepoRoot
    $customCore = (Invoke-ComponentRender -Profile $customRender -RepoRoot $RepoRoot -Component "services/core") -join "`n"
    Assert-True ($customCore -match 'namespace: agentx-render-check') "custom render ignored the requested namespace"
    Assert-True ($customCore -match 'image: registry\.example\.test/agentx/platform-api:dev') "registry render did not rewrite Agentx images"
    Assert-True ($customCore -notmatch 'AGENTX_SANDBOX_MANAGER_URL') "sandbox-disabled core render injected a Sandbox Manager URL"

    $remoteRender = $customRender | ConvertTo-Json -Depth 30 | ConvertFrom-Json -Depth 30
    $remoteRender.namespace = "agentx-remote-render-check"
    $remoteRender.components.sandbox.mode = "remote"
    $remoteRender.components.sandbox.endpoint = "https://sandbox.example.test"
    $remoteRender.components.sandbox.allowedHosts = @("sandbox.example.test")
    Assert-DeploymentProfile -Profile $remoteRender -RepoRoot $RepoRoot
    $manager = (Invoke-ComponentRender -Profile $remoteRender -RepoRoot $RepoRoot -Component "services/sandbox-manager") -join "`n"
    Assert-True ($manager -match 'namespace: agentx-remote-render-check') "remote Sandbox render ignored the requested namespace"
    Assert-True ($manager -match 'image: registry\.example\.test/agentx/sandbox-manager:dev') "remote Sandbox render did not use the registry image"
    Assert-True ($manager -match 'name: agentx-sandbox-manager-default-deny') "Sandbox Manager default-deny NetworkPolicy is missing"

    $production = $templateJson | ConvertFrom-Json -Depth 30
    $production.environment = "production"
    $production.images.mode = "registry"
    $production.images.registry = "registry.example.test/agentx"
    $production.secrets.provider = "vault_kv_v2"
    $production.secrets.vaultAddress = "https://vault.example.test"
    $production.secrets.vaultMount = "agentx"
    $production.components.objectStorage.allowHttp = $false
    $production.components.rag.mode = "disabled"
    $production.components.memory.mode = "disabled"
    $production.network.allowedEgressCidrs = @("10.20.0.0/16", "2001:db8:20::/64")
    foreach ($name in @("web", "platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "trace-writer")) {
        $production.images.digests | Add-Member -NotePropertyName $name -NotePropertyValue ("sha256:" + ("a" * 64))
    }
    Assert-DeploymentProfile -Profile $production -RepoRoot $RepoRoot
    $productionCore = (Invoke-ComponentRender -Profile $production -RepoRoot $RepoRoot -Component "services/core") -join "`n"
    Assert-True ($productionCore -match 'image: registry\.example\.test/agentx/platform-api@sha256:[a-f0-9]{64}') "production render did not pin the Platform API digest"
    $missingDigest = $production | ConvertTo-Json -Depth 30 | ConvertFrom-Json -Depth 30
    $missingDigest.images.digests.PSObject.Properties.Remove("workflow-worker")
    Assert-Throws { Assert-DeploymentProfile -Profile $missingDigest -RepoRoot $RepoRoot } "production image without digest"
    $invalidIsolation = $production | ConvertTo-Json -Depth 30 | ConvertFrom-Json -Depth 30
    $invalidIsolation.components.sandbox.isolationLevel = "strong"
    Assert-Throws { Assert-DeploymentProfile -Profile $invalidIsolation -RepoRoot $RepoRoot } "runc strong isolation claim"

    $owned = [pscustomobject]@{ metadata = [pscustomobject]@{ labels = [pscustomobject]@{ "app.kubernetes.io/managed-by" = "agentx-deploy"; "agentx.io/component" = "mysql" } } }
    $foreign = [pscustomobject]@{ metadata = [pscustomobject]@{ labels = [pscustomobject]@{ "app.kubernetes.io/managed-by" = "helm"; "agentx.io/component" = "mysql" } } }
    Assert-True (Test-IsManagedResource $owned "mysql") "owned resource was rejected"
    Assert-True (-not (Test-IsManagedResource $owned "redis")) "component ownership was ignored"
    Assert-True (-not (Test-IsManagedResource $foreign "mysql")) "foreign resource was accepted"

    foreach ($attempt in 1..256) {
        $generatedSecret = New-RandomSecret 32
        Assert-True ($generatedSecret -match '^[A-Za-z0-9_-]+$') "generated Secret was not URL-safe"
        Assert-True ($generatedSecret.Length -ge 43) "generated Secret did not preserve 256 bits of entropy"
    }

    $minioResources = Get-Content (Join-Path $RepoRoot "deploy/k8s/infrastructure/minio/resources.yaml") -Raw
    Assert-True ($minioResources -match 'mc alias set -- local') "MinIO initialization does not terminate options before random credentials"

    [pscustomobject]@{ status = "passed"; modeCombinations = $count; profileHash = Get-ProfileHash ($templateJson | ConvertFrom-Json -Depth 30) } | ConvertTo-Json
} $root
