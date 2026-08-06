param(
    [Parameter(Mandatory = $true)][string]$RegistryHost,
    [int]$RegistryPort = 30500,
    [Parameter(Mandatory = $true)][string]$CosignPrivateKey,
    [Parameter(Mandatory = $true)][string]$CosignPublicKey,
    [string]$CandidateCommit,
    [string]$M6PreviousCommit = "2bdfd50",
    [string]$M7PreviousCommit = "f343333",
    [string]$SeedScript,
    [string]$ApplicationSlug,
    [string]$BearerToken,
    [string]$BaseUrl,
    [int]$Port = 18091,
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OpenSandboxApiKey = "agentx-local-opensandbox-key",
    [string]$RegistryNamespace = "agentx-m7-registry",
    [string]$TestNamespace = "agentx-m7-local",
    [switch]$SkipBuild,
    [switch]$KeepNamespace
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$SeedScript = if ($SeedScript) { $SeedScript } else { Join-Path $root "scripts/m7-local-seed.ps1" }
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "artifacts/m7/local/$runId"
$temporary = Join-Path ([IO.Path]::GetTempPath()) "agentx-m7-local-$runId"
$registry = "$RegistryHost`:$RegistryPort/agentx"
$createdWorktrees = [Collections.Generic.List[string]]::new()
$replicaSnapshots = @{}
$portForward = $null
$installedCaThumbprint = $null
$nodeTrustInstalled = $false
$registryNamespaceOwned = $false
$testNamespaceOwned = $false
$failure = $null
$originalPath = $env:PATH
$int009 = "failed"
$int011 = "failed"
$registrySmokeStatus = "failed"
$stageAStatus = "failed"
$stageBStatus = "failed"
$int011PositiveStatus = "failed"
$int011NegativeStatus = "failed"
$secretScanStatus = "failed"
$requestedApplicationSlug = $ApplicationSlug
$requestedBearerToken = $BearerToken
$sensitiveValues = [Collections.Generic.List[string]]::new()
$openSandboxHeaders = @{ "OPEN-SANDBOX-API-KEY" = $OpenSandboxApiKey }
$clusterOpenSandbox = [UriBuilder]$OpenSandboxEndpoint
if ($clusterOpenSandbox.Host -in @("127.0.0.1", "localhost", "::1")) { $clusterOpenSandbox.Host = "host.docker.internal" }
$openSandboxCidrs = @()
$resolverImage = "busybox:1.37@sha256:9db7b59979c38555a39def84a31fb98b5296952f9e3afd4f6f11f05b07adfab0"
$originalOpenSandboxDeployKey = $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY
$openSandboxDeployKeyWasSet = Test-Path Env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY

function Invoke-Native([string]$Name, [scriptblock]$Action) {
    & $Action
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE." }
}

function Wait-TcpPort([int]$TargetPort) {
    for ($attempt = 0; $attempt -lt 90; $attempt++) {
        $client = [Net.Sockets.TcpClient]::new()
        try { $client.Connect("127.0.0.1", $TargetPort); return } catch { Start-Sleep -Seconds 1 } finally { $client.Dispose() }
    }
    throw "Timed out waiting for local port $TargetPort."
}

function Restart-DockerDesktop {
    Invoke-Native "restart Docker Desktop" { docker desktop restart --timeout 300 | Out-Null }
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        docker info *> $null
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Seconds 1
    }
    if ($LASTEXITCODE -ne 0) { throw "Docker Engine did not recover after Docker Desktop restart." }
    kubectl wait --for=condition=Ready nodes --all --timeout=300s | Out-Null
}

function Assert-SafeLocalParameters {
    if ($RegistryPort -lt 30000 -or $RegistryPort -gt 32767) { throw "RegistryPort must be in the Kubernetes NodePort range 30000-32767." }
    if ($RegistryHost -notmatch '^[A-Za-z0-9.-]+$') { throw "RegistryHost contains unsupported characters." }
    foreach ($namespace in @($RegistryNamespace, $TestNamespace)) {
        if ($namespace -notmatch '^[a-z0-9]([-a-z0-9]*[a-z0-9])?$') { throw "Invalid Kubernetes Namespace: $namespace" }
    }
    if ($RegistryNamespace -eq $TestNamespace -or $RegistryNamespace -in @("agentx", "agentx-e2e") -or $TestNamespace -in @("agentx", "agentx-e2e")) {
        throw "Local test Namespaces must be distinct and cannot replace shared Agentx Namespaces."
    }
    if ($clusterOpenSandbox.Scheme -notin @("http", "https") -or -not $clusterOpenSandbox.Host) {
        throw "OpenSandboxEndpoint must be an absolute HTTP or HTTPS URL."
    }
}

function Get-OpenSandboxIds {
    $response = Invoke-RestMethod -TimeoutSec 10 -Headers $openSandboxHeaders -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/v1/sandboxes?pageSize=100"
    if ($null -eq $response.items) { throw "OpenSandbox Lifecycle response did not contain an items array." }
    @($response.items | ForEach-Object { [string]$_.id })
}

function Assert-OpenSandboxReady {
    $health = Invoke-RestMethod -TimeoutSec 5 -Uri "$($OpenSandboxEndpoint.TrimEnd('/'))/health"
    if ([string]$health.status -ne "healthy") { throw "OpenSandbox health response was not healthy at $OpenSandboxEndpoint." }
    [void](Get-OpenSandboxIds)
}

function Resolve-ClusterEndpointCidrs([string]$HostName) {
    $parsed = $null
    if ([Net.IPAddress]::TryParse($HostName, [ref]$parsed)) {
        return @("$($parsed.IPAddressToString)/$(if ($parsed.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork) { 32 } else { 128 })")
    }
    $pod = "agentx-m7-egress-resolver"
    kubectl -n $RegistryNamespace delete pod $pod --ignore-not-found --wait=true | Out-Null
    try {
        kubectl run $pod -n $RegistryNamespace --image=$resolverImage --restart=Never --command -- nslookup $HostName | Out-Null
        kubectl -n $RegistryNamespace wait --for=jsonpath='{.status.phase}'=Succeeded "pod/$pod" --timeout=60s | Out-Null
        $resolverOutput = (kubectl -n $RegistryNamespace logs "pod/$pod") -join "`n"
        $escapedHost = [Regex]::Escape($HostName)
        $matches = [Regex]::Matches($resolverOutput, "(?im)^Name:\s*$escapedHost\s*`r?`nAddress:\s*([^\s]+)")
        $cidrs = @($matches | ForEach-Object {
            $address = [Net.IPAddress]::Parse($_.Groups[1].Value)
            "$($address.IPAddressToString)/$(if ($address.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork) { 32 } else { 128 })"
        } | Select-Object -Unique)
        if ($cidrs.Count -eq 0) { throw "Cluster DNS did not resolve $HostName to an IP address." }
        $cidrs
    }
    finally {
        kubectl -n $RegistryNamespace delete pod $pod --ignore-not-found --wait=true | Out-Null
    }
}

function Protect-SecretText([string]$Value) {
    if ($null -eq $Value) { return $null }
    $protected = $Value
    foreach ($secret in $sensitiveValues) {
        if ($secret) { $protected = $protected.Replace($secret, "[REDACTED]") }
    }
    $protected
}

function Add-SensitiveValue([string]$Value) {
    if ($Value -and -not $sensitiveValues.Contains($Value)) { $sensitiveValues.Add($Value) }
}

function Record-Failure([string]$Message) {
    if (-not $script:failure) { $script:failure = Protect-SecretText $Message }
}

function Assert-DockerDesktopNodeAddress {
    $addresses = @((kubectl get nodes -o json | ConvertFrom-Json -Depth 30).items[0].status.addresses | Where-Object type -eq "InternalIP" | ForEach-Object { [string]$_.address })
    if ($addresses.Count -ne 1 -or $RegistryHost -ne $addresses[0]) {
        throw "RegistryHost '$RegistryHost' must equal the Docker Desktop node InternalIP '$($addresses -join ',')'."
    }
}

function Get-ReplicaSnapshot([string]$Namespace) {
    if (-not (kubectl get namespace $Namespace --ignore-not-found -o name)) { return @() }
    $resources = kubectl -n $Namespace get deployments,statefulsets -o json | ConvertFrom-Json -Depth 30
    @($resources.items | ForEach-Object {
        [ordered]@{ kind = $(if ($_.kind -eq "Deployment") { "deployment" } else { "statefulset" }); name = [string]$_.metadata.name; replicas = $(if ($null -eq $_.spec.replicas) { 1 } else { [int]$_.spec.replicas }) }
    })
}

function Scale-Snapshot([string]$Namespace, $Snapshot, [switch]$Restore) {
    foreach ($item in $Snapshot) {
        $replicas = if ($Restore) { [int]$item.replicas } else { 0 }
        kubectl -n $Namespace scale "$($item.kind)/$($item.name)" --replicas=$replicas | Out-Null
    }
}

function New-RegistryCertificates([string]$Directory) {
    New-Item -ItemType Directory -Path $Directory -Force | Out-Null
    $rsa = [Security.Cryptography.RSA]::Create(4096)
    $caRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new("CN=Agentx M7 Local Registry CA $runId", $rsa, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $caRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true))
    $caRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyCertSign -bor [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::CrlSign, $true))
    $ca = $caRequest.CreateSelfSigned([DateTimeOffset]::UtcNow.AddMinutes(-5), [DateTimeOffset]::UtcNow.AddDays(7))

    $serverRsa = [Security.Cryptography.RSA]::Create(4096)
    $serverRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new("CN=$RegistryHost", $serverRsa, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $san = [Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
    $address = $null
    if ([Net.IPAddress]::TryParse($RegistryHost, [ref]$address)) { $san.AddIpAddress($address) } else { $san.AddDnsName($RegistryHost) }
    $san.AddDnsName("host.docker.internal")
    $san.AddDnsName("registry.$RegistryNamespace.svc")
    $serverRequest.CertificateExtensions.Add($san.Build())
    $serverRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature -bor [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyEncipherment, $true))
    $serial = [Security.Cryptography.RandomNumberGenerator]::GetBytes(16)
    $issued = $serverRequest.Create($ca, [DateTimeOffset]::UtcNow.AddMinutes(-5), [DateTimeOffset]::UtcNow.AddDays(7), $serial)
    $server = $issued.CopyWithPrivateKey($serverRsa)

    $caPem = Join-Path $Directory "ca.crt"
    $caDer = Join-Path $Directory "ca.cer"
    $certPem = Join-Path $Directory "tls.crt"
    $keyPem = Join-Path $Directory "tls.key"
    [IO.File]::WriteAllText($caPem, $ca.ExportCertificatePem(), [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllBytes($caDer, $ca.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
    [IO.File]::WriteAllText($certPem, $server.ExportCertificatePem(), [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($keyPem, $serverRsa.ExportPkcs8PrivateKeyPem(), [Text.UTF8Encoding]::new($false))
    [ordered]@{ caPem = $caPem; caDer = $caDer; certPem = $certPem; keyPem = $keyPem }
}

function Install-Registry([hashtable]$Certificates) {
    kubectl create namespace $RegistryNamespace | Out-Null
    $script:registryNamespaceOwned = $true
    kubectl label namespace $RegistryNamespace "app.kubernetes.io/managed-by=agentx-m7-local" --overwrite | Out-Null
    kubectl -n $RegistryNamespace create secret tls registry-tls --cert=$Certificates.certPem --key=$Certificates.keyPem | Out-Null
    kubectl -n $RegistryNamespace create secret generic registry-ca --from-file=ca.crt=$Certificates.caPem | Out-Null
    $manifest = @"
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: registry-data
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 2Gi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: registry
spec:
  replicas: 1
  selector:
    matchLabels: { app: agentx-m7-registry }
  template:
    metadata:
      labels: { app: agentx-m7-registry }
    spec:
      containers:
        - name: registry
          image: registry:2
          env:
            - { name: REGISTRY_HTTP_ADDR, value: 0.0.0.0:5000 }
            - { name: REGISTRY_HTTP_TLS_CERTIFICATE, value: /certs/tls.crt }
            - { name: REGISTRY_HTTP_TLS_KEY, value: /certs/tls.key }
            - { name: REGISTRY_STORAGE_DELETE_ENABLED, value: "true" }
          ports:
            - { containerPort: 5000, name: https }
          readinessProbe:
            httpGet: { path: /v2/, port: https, scheme: HTTPS }
          volumeMounts:
            - { name: data, mountPath: /var/lib/registry }
            - { name: tls, mountPath: /certs, readOnly: true }
      volumes:
        - name: data
          persistentVolumeClaim: { claimName: registry-data }
        - name: tls
          secret: { secretName: registry-tls }
---
apiVersion: v1
kind: Service
metadata:
  name: registry
spec:
  type: NodePort
  selector: { app: agentx-m7-registry }
  ports:
    - name: https
      port: 5000
      targetPort: https
      nodePort: $RegistryPort
"@
    $manifest | kubectl -n $RegistryNamespace apply -f - | Out-Null
    kubectl -n $RegistryNamespace rollout status deployment/registry --timeout=300s | Out-Null
}

function Install-NodeRegistryTrust {
    $hostRegistry = "$RegistryHost`:$RegistryPort"
    kubectl -n $RegistryNamespace delete daemonset registry-node-trust --ignore-not-found | Out-Null
    $trust = @"
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: registry-node-trust
spec:
  selector:
    matchLabels: { app: registry-node-trust }
  template:
    metadata:
      labels: { app: registry-node-trust }
    spec:
      containers:
        - name: install
          image: busybox:1.37
          securityContext: { privileged: true }
          command: ["sh", "-c"]
          args:
            - 'mkdir -p "/host/$hostRegistry"; cp /ca/ca.crt "/host/$hostRegistry/ca.crt"; printf "server = \"https://$hostRegistry\"\n[host.\"https://$hostRegistry\"]\n  capabilities = [\"pull\", \"resolve\"]\n  ca = \"/etc/containerd/certs.d/$hostRegistry/ca.crt\"\n" > "/host/$hostRegistry/hosts.toml"; sleep 3600'
          volumeMounts:
            - { name: certs, mountPath: /host }
            - { name: ca, mountPath: /ca, readOnly: true }
      volumes:
        - name: certs
          hostPath: { path: /etc/containerd/certs.d, type: DirectoryOrCreate }
        - name: ca
          secret: { secretName: registry-ca }
"@
    $trust | kubectl -n $RegistryNamespace apply -f - | Out-Null
    $script:nodeTrustInstalled = $true
    kubectl -n $RegistryNamespace rollout status daemonset/registry-node-trust --timeout=180s | Out-Null
}

function Remove-NodeRegistryTrust {
    if (-not $nodeTrustInstalled -or -not (kubectl get namespace $RegistryNamespace --ignore-not-found -o name)) { return }
    $hostRegistry = "$RegistryHost`:$RegistryPort"
    $cleanup = @"
apiVersion: v1
kind: Pod
metadata:
  name: registry-node-trust-cleanup
spec:
  restartPolicy: Never
  containers:
    - name: cleanup
      image: busybox:1.37
      securityContext: { privileged: true }
      command: ["sh", "-c", "rm -f '/host/$hostRegistry/ca.crt' '/host/$hostRegistry/hosts.toml'; rmdir '/host/$hostRegistry' 2>/dev/null || true"]
      volumeMounts:
        - { name: certs, mountPath: /host }
  volumes:
    - name: certs
      hostPath: { path: /etc/containerd/certs.d, type: DirectoryOrCreate }
"@
    kubectl -n $RegistryNamespace delete daemonset registry-node-trust --ignore-not-found --wait=true | Out-Null
    $cleanup | kubectl -n $RegistryNamespace apply -f - | Out-Null
    kubectl -n $RegistryNamespace wait --for=jsonpath='{.status.phase}'=Succeeded pod/registry-node-trust-cleanup --timeout=120s | Out-Null
}

function Test-RegistrySmoke([string]$CaPath) {
    $smokeDirectory = Join-Path $output "registry"
    New-Item -ItemType Directory -Path $smokeDirectory -Force | Out-Null
    Invoke-Native "pull smoke image" { docker pull alpine:3.20 | Out-Null }
    $tag = "$registry/registry-smoke:$runId"
    docker tag alpine:3.20 $tag
    Invoke-Native "push smoke image" { docker push $tag | Out-Null }
    $digest = [string]((docker buildx imagetools inspect $tag --format "{{json .Manifest}}" | ConvertFrom-Json).digest)
    if ($digest -notmatch '^sha256:[a-f0-9]{64}$') { throw "Registry smoke image did not resolve a digest." }
    $reference = "$registry/registry-smoke@$digest"
    Invoke-Native "pull smoke digest" { docker pull $reference | Out-Null }
    kubectl -n $RegistryNamespace delete pod registry-pull-smoke --ignore-not-found --wait=true | Out-Null
    kubectl -n $RegistryNamespace run registry-pull-smoke --image=$reference --restart=Never --command -- sh -c "exit 0" | Out-Null
    kubectl -n $RegistryNamespace wait --for=jsonpath='{.status.phase}'=Succeeded pod/registry-pull-smoke --timeout=180s | Out-Null
    $deleteUrl = "https://$RegistryHost`:$RegistryPort/v2/agentx/registry-smoke/manifests/$digest"
    Invoke-Native "delete smoke manifest" { curl.exe --fail --silent --show-error --cacert $CaPath -X DELETE $deleteUrl | Out-Null }
    try { docker image rm $tag $reference 2>$null | Out-Null } catch {}
    [ordered]@{ status = "passed"; reference = $reference; dockerPush = $true; dockerPull = $true; kubernetesPull = $true; registryDelete = $true } | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $smokeDirectory "smoke.json") -Encoding utf8NoBOM
}

function Ensure-Worktree([string]$Path, [string]$Commit) {
    git cat-file -e "$Commit^{commit}" 2>$null
    if ($LASTEXITCODE -ne 0) { throw "Git commit does not exist: $Commit" }
    if (Test-Path -LiteralPath $Path) {
        $existing = ([string](git -C $Path rev-parse HEAD)).Trim()
        if (-not $existing.StartsWith($Commit, [StringComparison]::OrdinalIgnoreCase)) { throw "Existing worktree $Path is not at $Commit." }
        if (@(git -C $Path status --porcelain).Count -gt 0) { throw "Existing worktree is dirty: $Path" }
        return
    }
    git worktree add --detach $Path $Commit | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not create worktree $Path." }
    $createdWorktrees.Add($Path)
}

function Invoke-Release([string]$Version, [string]$Source, [string]$Directory) {
    $arguments = @{ Version = $Version; Registry = $registry; CosignPrivateKey = $CosignPrivateKey; CosignPublicKey = $CosignPublicKey; SourceDirectory = $Source; OutputDirectory = $Directory; RuntimeClass = "runc"; IsolationLevel = "standard" }
    if ($SkipBuild) { $arguments.SkipBuild = $true }
    $result = @(& (Join-Path $root "scripts/release-images.ps1") @arguments)
    $manifestPath = [string]($result | Select-Object -Last 1)
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw "$Version did not produce a Release Manifest." }
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json -Depth 50
    $expectedCommit = ([string](git -C $Source rev-parse HEAD)).Trim().ToLowerInvariant()
    $evidencePath = Join-Path (Split-Path -Parent $manifestPath) "supply-chain-evidence.json"
    $evidence = Get-Content -Raw -LiteralPath $evidencePath | ConvertFrom-Json -Depth 30
    if ($manifest.gitCommit -ne $expectedCommit -or $manifest.images.Count -ne 7 -or $evidence.status -ne "passed" -or $evidence.sourceCommit -ne $expectedCommit) {
        throw "$Version supply-chain evidence does not match source commit $expectedCommit."
    }
    $manifestPath
}

function Copy-ReleaseEvidence([string]$ManifestPath, [string]$Name) {
    $destination = Join-Path $output "supply-chain/$Name"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -Depth 50
    $sourceDirectory = Split-Path -Parent $ManifestPath
    foreach ($file in @("release-manifest.json", "release-manifest.json.sig", "supply-chain-evidence.json") + @($manifest.images | ForEach-Object { [string]$_.sbom })) {
        Copy-Item -LiteralPath (Join-Path $sourceDirectory $file) -Destination (Join-Path $destination $file) -Force
    }
}

function New-DeploymentProfile([string]$ManifestPath, [string]$Path) {
    $manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -Depth 50
    $profile = Get-Content -Raw -LiteralPath (Join-Path $root "deploy/profiles/full-local.json") | ConvertFrom-Json -Depth 50
    $profile.namespace = $TestNamespace
    $profile.images.mode = "registry"
    $profile.images.registry = $registry
    $profile.images.tag = $manifest.version
    $profile.images.pullPolicy = "IfNotPresent"
    $profile.images.digests = [pscustomobject]@{}
    foreach ($image in $manifest.images) { $profile.images.digests | Add-Member -NotePropertyName $image.name -NotePropertyValue $image.digest }
    $profile.components.sandbox.mode = "remote"
    $profile.components.sandbox.endpoint = $clusterOpenSandbox.Uri.AbsoluteUri.TrimEnd('/')
    $profile.components.sandbox.allowedHosts = @($clusterOpenSandbox.Host)
    $profile.components.sandbox.allowedCidrs = @($openSandboxCidrs)
    $profile.network.allowedEgressCidrs = @($profile.network.allowedEgressCidrs + $openSandboxCidrs | Select-Object -Unique)
    $profile.ingress.host = "$TestNamespace.localhost"
    [IO.File]::WriteAllText($Path, ($profile | ConvertTo-Json -Depth 50), [Text.UTF8Encoding]::new($false))
}

function Invoke-TestMySql([string]$Sql) {
    $value = $Sql | kubectl -n $TestNamespace exec -i statefulset/mysql -- sh -c 'MYSQL_PWD="$MYSQL_PASSWORD" exec mysql -N -u"$MYSQL_USER" "$MYSQL_DATABASE"'
    if ($LASTEXITCODE -ne 0) { throw "MySQL query failed." }
    ([string]($value | Select-Object -Last 1)).Trim()
}

function Get-DatabaseCounts {
    [ordered]@{
        workflows = [int64](Invoke-TestMySql "SELECT COUNT(*) FROM workflows;")
        versions = [int64](Invoke-TestMySql "SELECT COUNT(*) FROM workflow_versions;")
        applications = [int64](Invoke-TestMySql "SELECT COUNT(*) FROM applications;")
        executions = [int64](Invoke-TestMySql "SELECT COUNT(*) FROM workflow_executions;")
    }
}

function Assert-NonEmptyBaseline($Counts, [string]$Phase) {
    foreach ($key in @("workflows", "versions", "applications", "executions")) {
        if ([int64]$Counts[$key] -lt 1) { throw "$Phase baseline must contain at least one $key row created through UI/public API." }
    }
}

function Get-MigrationFacts {
    $tables = Invoke-TestMySql "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('runtime_commands','projection_receipts','worker_capabilities','evaluation_run_cases','evaluation_rule_results','trigger_bindings','quota_policies','quota_reservations','quota_usage_ledger','artifact_references','retention_policies','retention_runs','retention_items','release_schema_contract');"
    $secretColumns = Invoke-TestMySql "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema=DATABASE() AND ((table_name='credential_secret_versions' AND column_name IN ('provider','secret_ref','provider_version')) OR (table_name='application_webhooks' AND column_name IN ('secret_provider','secret_ref','secret_provider_version')));"
    $indexes = Invoke-TestMySql "SELECT COUNT(*) FROM information_schema.statistics WHERE table_schema=DATABASE() AND index_name IN ('idx_runtime_commands_pending','idx_worker_capability_ready','idx_quota_reservation_reaper','idx_retention_run_status');"
    $migrations = Invoke-TestMySql "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE version IN (16,17) AND success=TRUE;"
    $contract = Invoke-TestMySql "SELECT CONCAT(schema_version,':',minimum_application_version) FROM release_schema_contract WHERE contract_name='m7-runtime-integration';"
    if ($tables -ne "14" -or $secretColumns -ne "6" -or [int]$indexes -lt 4 -or $migrations -ne "16,17" -or $contract -ne "17:0.1.0") {
        throw "Stage A schema facts are incomplete: tables=$tables secretColumns=$secretColumns indexes=$indexes migrations=$migrations contract=$contract."
    }
    [ordered]@{ tables = [int]$tables; secretColumns = [int]$secretColumns; indexes = [int]$indexes; migrations = $migrations; contract = $contract }
}

function Wait-WorkerCapability {
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        $ready = Invoke-TestMySql "SELECT COUNT(*) FROM worker_capabilities WHERE status='ready' AND heartbeat_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 60 SECOND) AND node_protocol_version='1.0' AND JSON_CONTAINS(ir_schema_versions_json,JSON_QUOTE('3.0'));"
        if ([int]$ready -ge 1) { return [int]$ready }
        Start-Sleep -Seconds 1
    }
    throw "No compatible M7 Worker Capability heartbeat became ready."
}

function Start-WebForward {
    if ($BaseUrl) { return $BaseUrl.TrimEnd('/') }
    $script:portForward = Start-Process kubectl -ArgumentList @("proxy", "--address=127.0.0.1", "--port=$Port", "--accept-hosts=^.*$") -PassThru -WindowStyle Hidden
    Wait-TcpPort $Port
    "http://127.0.0.1:$Port/api/v1/namespaces/$TestNamespace/services/http:web:80/proxy"
}

function Test-WebAndApiHealth([string]$Url) {
    Invoke-WebRequest -Uri "$($Url.TrimEnd('/'))/health/live" -TimeoutSec 30 | Out-Null
    $status = Invoke-RestMethod -Uri "$($Url.TrimEnd('/'))/api/v1/bootstrap/status" -TimeoutSec 30
    if ($status.required -ne $false) { throw "Platform API did not retain the bootstrapped M6 data." }
}

function Stop-WebForward {
    if ($portForward -and -not $portForward.HasExited) { Stop-Process -Id $portForward.Id -Force -ErrorAction SilentlyContinue; $portForward.WaitForExit() }
    $script:portForward = $null
}

function Invoke-Seed([string]$Phase, [string]$Url) {
    $seed = @(& pwsh -NoProfile -File $SeedScript -Namespace $TestNamespace -BaseUrl $Url -Phase $Phase)
    $result = ([string]($seed | Select-Object -Last 1)) | ConvertFrom-Json -Depth 20
    foreach ($name in @("workflowId", "workflowVersionId", "applicationId", "applicationSlug", "executionId", "bearerToken")) {
        if (-not $result.$name) { throw "SeedScript result for $Phase is missing $name." }
    }
    Add-SensitiveValue ([string]$result.bearerToken)
    $safe = [ordered]@{}
    foreach ($property in $result.PSObject.Properties | Where-Object Name -ne "bearerToken") { $safe[$property.Name] = $property.Value }
    $safe | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $output "migration/seed-$Phase.json") -Encoding utf8NoBOM
    $result
}

function Reset-TestNamespace {
    Stop-WebForward
    if (kubectl get namespace $TestNamespace --ignore-not-found -o name) { kubectl delete namespace $TestNamespace --wait=true --timeout=600s | Out-Null }
    $script:testNamespaceOwned = $false
}

function Invoke-SecretScan {
    $findings = [Collections.Generic.List[object]]::new()
    $files = @(Get-ChildItem -LiteralPath $output -Recurse -File -ErrorAction SilentlyContinue | Where-Object Name -ne "secret-scan.json")
    $privateKey = Get-Content -Raw -LiteralPath $CosignPrivateKey
    foreach ($file in $files) {
        try { $content = [IO.File]::ReadAllText($file.FullName) } catch { continue }
        $relative = [IO.Path]::GetRelativePath($output, $file.FullName).Replace('\', '/')
        if ($content -match '-----BEGIN (?:ENCRYPTED )?(?:EC |RSA )?PRIVATE KEY-----') { $findings.Add([ordered]@{ path = $relative; rule = "private-key-pem" }) }
        if ($content -match '(?i)Authorization\s*:\s*Bearer') { $findings.Add([ordered]@{ path = $relative; rule = "authorization-bearer" }) }
        if ($privateKey -and $content.Contains($privateKey)) { $findings.Add([ordered]@{ path = $relative; rule = "cosign-private-key" }) }
        foreach ($secret in $sensitiveValues) {
            if ($secret -and $content.Contains($secret)) { $findings.Add([ordered]@{ path = $relative; rule = "bearer-token" }); break }
        }
    }
    $result = [ordered]@{
        status = $(if ($findings.Count -eq 0) { "passed" } else { "failed" })
        scannedFiles = $files.Count
        findings = @($findings)
    }
    $result | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $output "secret-scan.json") -Encoding utf8NoBOM
    $result
}

function Write-RootJunit {
    $cases = @(
        [ordered]@{ name = "registry_smoke"; status = $registrySmokeStatus },
        [ordered]@{ name = "int011_positive"; status = $int011PositiveStatus },
        [ordered]@{ name = "int011_negative"; status = $int011NegativeStatus },
        [ordered]@{ name = "stage_a_m6_to_m7"; status = $stageAStatus },
        [ordered]@{ name = "stage_b_m7_rolling_rollback"; status = $stageBStatus },
        [ordered]@{ name = "secret_scan"; status = $script:secretScanStatus }
    )
    $failed = @($cases | Where-Object status -ne "passed")
    $body = @($cases | ForEach-Object {
        $name = [Security.SecurityElement]::Escape([string]$_.name)
        if ($_.status -eq "passed") { '<testcase name="' + $name + '"/>' } else { '<testcase name="' + $name + '"><failure message="not passed"/></testcase>' }
    }) -join ""
    $xml = '<testsuite name="m7-local-release" tests="' + $cases.Count + '" failures="' + $failed.Count + '" errors="0" skipped="0">' + $body + '</testsuite>'
    $path = Join-Path $output "junit.xml"
    $xml | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    [ordered]@{ tests = $cases.Count; failures = $failed.Count; errors = 0; skipped = 0; path = [IO.Path]::GetRelativePath($root, $path).Replace('\', '/') }
}

function Write-EvidenceInventory {
    $inventoryPath = Join-Path $output "evidence-inventory.json"
    $items = @(
        Get-ChildItem -LiteralPath $output -Recurse -File |
            Where-Object { $_.FullName -notin @($inventoryPath, (Join-Path $output "m7-local-evidence.json")) } |
            Sort-Object FullName |
            ForEach-Object {
                [ordered]@{
                    kind = "m7-local"
                    path = [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/')
                    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
                }
            }
    )
    $items | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $inventoryPath -Encoding utf8NoBOM
    $items + @([ordered]@{
        kind = "inventory"
        path = [IO.Path]::GetRelativePath($root, $inventoryPath).Replace('\', '/')
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $inventoryPath).Hash.ToLowerInvariant()
    })
}

New-Item -ItemType Directory -Path $output, $temporary, (Join-Path $output "migration"), (Join-Path $output "upgrade"), (Join-Path $output "rollback"), (Join-Path $output "supply-chain") -Force | Out-Null
$summaryPath = Join-Path $output "m7-local-evidence.json"

try {
    Assert-SafeLocalParameters
    if (-not (Test-Path -LiteralPath $SeedScript -PathType Leaf)) { throw "SeedScript does not exist: $SeedScript" }
    if (-not (Test-Path -LiteralPath $CosignPrivateKey -PathType Leaf) -or -not (Test-Path -LiteralPath $CosignPublicKey -PathType Leaf)) { throw "Both Cosign key files must exist." }
    $privateKeyPath = (Resolve-Path -LiteralPath $CosignPrivateKey).Path
    if ($privateKeyPath.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { throw "CosignPrivateKey must be stored outside the repository." }
    Add-SensitiveValue $requestedBearerToken
    Add-SensitiveValue $OpenSandboxApiKey
    Assert-OpenSandboxReady
    $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY = $OpenSandboxApiKey

    $CandidateCommit = if ($CandidateCommit) { ([string](git rev-parse $CandidateCommit)).Trim() } else { ([string](git rev-parse HEAD)).Trim() }
    $m7Resolved = ([string](git rev-parse $M7PreviousCommit)).Trim()
    if ($CandidateCommit -eq $m7Resolved) { throw "CandidateCommit must differ from M7 Previous $M7PreviousCommit. Commit the implementation before running local acceptance." }

    & (Join-Path $root "scripts/m7-local-prereqs.ps1") -RunId $runId -OutputDirectory $output | Out-Null
    $env:PATH = "$(Join-Path $root '.local/m7-tools')$([IO.Path]::PathSeparator)$env:PATH"
    Assert-DockerDesktopNodeAddress
    foreach ($namespace in @("agentx", "agentx-e2e")) {
        $snapshot = @(Get-ReplicaSnapshot $namespace)
        $replicaSnapshots[$namespace] = $snapshot
        $snapshot | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $output "$namespace-original-replicas.json") -Encoding utf8NoBOM
        Scale-Snapshot $namespace $snapshot
    }

    if (kubectl get namespace $RegistryNamespace --ignore-not-found -o name) { throw "Registry Namespace '$RegistryNamespace' already exists; refusing to take ownership." }
    if (kubectl get namespace $TestNamespace --ignore-not-found -o name) { throw "Test Namespace '$TestNamespace' already exists; refusing to delete user state." }
    $certificates = New-RegistryCertificates (Join-Path $temporary "registry-certs")
    $imported = Import-Certificate -FilePath $certificates.caDer -CertStoreLocation Cert:\CurrentUser\Root
    $installedCaThumbprint = [string]$imported.Thumbprint
    Restart-DockerDesktop
    Install-Registry $certificates
    Install-NodeRegistryTrust
    Test-RegistrySmoke $certificates.caPem
    $registrySmokeStatus = "passed"
    $openSandboxCidrs = @(Resolve-ClusterEndpointCidrs -HostName $clusterOpenSandbox.Host)

    $m6Worktree = Join-Path $root ".tmp/m6-previous"
    $m7Worktree = Join-Path $root ".tmp/m7-previous"
    $candidateWorktree = Join-Path $root ".tmp/candidate"
    Ensure-Worktree $m6Worktree $M6PreviousCommit
    Ensure-Worktree $m7Worktree $M7PreviousCommit
    Ensure-Worktree $candidateWorktree $CandidateCommit
    $m6Manifest = Invoke-Release "m6-previous" $m6Worktree "artifacts/release/m6-previous"
    $m7Manifest = Invoke-Release "m7-previous" $m7Worktree "artifacts/release/m7-previous"
    $candidateManifest = Invoke-Release "m7-candidate" $candidateWorktree "artifacts/release/m7-candidate"
    Copy-ReleaseEvidence $m6Manifest "m6-previous"
    Copy-ReleaseEvidence $m7Manifest "m7-previous"
    Copy-ReleaseEvidence $candidateManifest "m7-candidate"
    $int011PositiveStatus = "passed"

    $wrongPrefix = Join-Path $temporary "wrong-cosign"
    $oldPassword = $env:COSIGN_PASSWORD
    try { $env:COSIGN_PASSWORD = ""; Invoke-Native "generate negative-test Cosign key" { cosign generate-key-pair --output-key-prefix $wrongPrefix | Out-Null } } finally { $env:COSIGN_PASSWORD = $oldPassword }
    $negativeResult = @(& (Join-Path $root "scripts/test-release-negative-cases.ps1") -ReleaseManifest $candidateManifest -CosignPrivateKey $CosignPrivateKey -CosignPublicKey $CosignPublicKey -WrongCosignPublicKey "$wrongPrefix.pub" -OutputDirectory (Join-Path $output "supply-chain/negative"))
    $negativePath = [string]($negativeResult | Select-Object -Last 1)
    $negative = Get-Content -Raw -LiteralPath $negativePath | ConvertFrom-Json -Depth 30
    if ($negative.status -ne "passed" -or $negative.assertions.Count -ne 8) { throw "Supply-chain negative evidence is incomplete." }
    $int011NegativeStatus = "passed"
    $int011 = "passed"

    $m6Profile = Join-Path $temporary "m6-profile.json"
    $m7Profile = Join-Path $temporary "m7-profile.json"
    $candidateProfile = Join-Path $temporary "candidate-profile.json"
    New-DeploymentProfile $m6Manifest $m6Profile
    New-DeploymentProfile $m7Manifest $m7Profile
    New-DeploymentProfile $candidateManifest $candidateProfile

    $testNamespaceOwned = $true
    & (Join-Path $root "scripts/deploy.ps1") -Action Install -ConfigFile $m6Profile -NonInteractive -Target infrastructure -SkipMigrations
    & (Join-Path $root "scripts/deploy.ps1") -Action Install -ConfigFile $m6Profile -NonInteractive -Target services
    & (Join-Path $root "scripts/deploy.ps1") -Action Install -ConfigFile $m6Profile -NonInteractive -Target sandbox -SkipMigrations
    $stageAUrl = Start-WebForward
    $stageASeed = Invoke-Seed "m6" $stageAUrl
    $stageABefore = Get-DatabaseCounts
    Assert-NonEmptyBaseline $stageABefore "Stage A"
    $stageABefore | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output "migration/stage-a-baseline.json") -Encoding utf8NoBOM

    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $candidateProfile -NonInteractive -Target services -MigrationPhase expand -MigrationOnly
    foreach ($name in @("platform-api", "trigger-gateway", "workflow-coordinator", "workflow-worker", "sandbox-manager", "trace-writer", "web")) {
        kubectl -n $TestNamespace rollout restart "deployment/$name" | Out-Null
        kubectl -n $TestNamespace rollout status "deployment/$name" --timeout=300s | Out-Null
    }
    Test-WebAndApiHealth $stageAUrl
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $m7Profile -NonInteractive -Target services -SkipMigrations
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $m7Profile -NonInteractive -Target sandbox -SkipMigrations
    $stageAWorkerCapabilities = Wait-WorkerCapability
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $candidateProfile -NonInteractive -Target services -MigrationPhase contract -MigrationOnly
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $candidateProfile -NonInteractive -Target services -MigrationPhase contract -MigrationOnly
    $stageAFacts = Get-MigrationFacts
    $stageAAfter = Get-DatabaseCounts
    foreach ($key in @("workflows", "versions", "applications", "executions")) {
        if ($stageABefore[$key] -ne $stageAAfter[$key]) { throw "Stage A changed baseline $key rows." }
    }
    [ordered]@{
        status = "passed"
        m6PreviousCommit = ([string](git rev-parse $M6PreviousCommit)).Trim()
        m7ServiceCommit = ([string](git rev-parse $M7PreviousCommit)).Trim()
        migrationCommit = $CandidateCommit
        before = $stageABefore
        after = $stageAAfter
        workerCapabilities = $stageAWorkerCapabilities
        schema = $stageAFacts
    } | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $output "migration/stage-a-evidence.json") -Encoding utf8NoBOM
    $stageAStatus = "passed"

    Reset-TestNamespace
    $ApplicationSlug = $requestedApplicationSlug
    $BearerToken = $requestedBearerToken
    $testNamespaceOwned = $true
    & (Join-Path $root "scripts/deploy.ps1") -Action Install -ConfigFile $candidateProfile -NonInteractive -Target infrastructure -SkipMigrations
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $candidateProfile -NonInteractive -Target services -MigrationPhase expand -MigrationOnly
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $m7Profile -NonInteractive -Target services -SkipMigrations
    & (Join-Path $root "scripts/deploy.ps1") -Action Upgrade -ConfigFile $m7Profile -NonInteractive -Target sandbox -SkipMigrations
    $stageBWorkerCapabilities = Wait-WorkerCapability
    $stageBUrl = Start-WebForward
    $stageBSeed = Invoke-Seed "m7" $stageBUrl
    $ApplicationSlug = [string]$stageBSeed.applicationSlug
    $BearerToken = [string]$stageBSeed.bearerToken
    $baseline = Get-DatabaseCounts
    Assert-NonEmptyBaseline $baseline "Stage B"
    $baselinePath = Join-Path $output "upgrade/stage-b-baseline.json"
    $baseline | ConvertTo-Json | Set-Content -LiteralPath $baselinePath -Encoding utf8NoBOM

    $upgradeResult = @(& (Join-Path $root "scripts/m7-upgrade-tests.ps1") -PreviousReleaseManifest $m7Manifest -CandidateReleaseManifest $candidateManifest -Namespace $TestNamespace -GatewayBaseUrl $stageBUrl -ApplicationSlug $ApplicationSlug -BearerToken $BearerToken -ContractProfile $candidateProfile -BaselineFile $baselinePath -ProbeDurationSeconds 900 -RequireSandbox -OutputDirectory "artifacts/m7/local/$runId")
    $upgradeEvidenceSource = [string]($upgradeResult | Select-Object -Last 1)
    if (-not (Test-Path -LiteralPath $upgradeEvidenceSource -PathType Leaf)) { throw "Stage B did not produce upgrade evidence." }
    $upgradeEvidenceValue = Get-Content -Raw -LiteralPath $upgradeEvidenceSource | ConvertFrom-Json -Depth 30
    if ($upgradeEvidenceValue.status -ne "passed") { throw "Stage B upgrade evidence is not passed." }
    Copy-Item -Path (Join-Path (Split-Path -Parent $upgradeEvidenceSource) "*") -Destination (Join-Path $output "upgrade") -Recurse -Force
    $probeSource = Join-Path (Split-Path -Parent $upgradeEvidenceSource) "probes.ndjson"
    if (-not (Test-Path -LiteralPath $probeSource -PathType Leaf)) { throw "Stage B produced no continuous probe evidence." }
    Copy-Item -LiteralPath $probeSource -Destination (Join-Path $output "probes.ndjson") -Force
    $stageBAfter = Get-DatabaseCounts
    [ordered]@{
        status = "passed"
        previousCommit = ([string](git rev-parse $M7PreviousCommit)).Trim()
        candidateCommit = $CandidateCommit
        before = $baseline
        after = $stageBAfter
        workerCapabilities = $stageBWorkerCapabilities
        upgradeEvidence = [IO.Path]::GetRelativePath($root, (Join-Path $output "upgrade/upgrade-evidence.json")).Replace('\', '/')
    } | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $output "rollback/stage-b-evidence.json") -Encoding utf8NoBOM
    $stageBStatus = "passed"
    $int009 = "passed"
}
catch {
    Record-Failure $_.Exception.Message
}
finally {
    Stop-WebForward
    if (-not $KeepNamespace) {
        try { Remove-NodeRegistryTrust } catch { Record-Failure $_.Exception.Message }
        foreach ($namespace in @(
            [ordered]@{ name = $TestNamespace; owned = $testNamespaceOwned },
            [ordered]@{ name = $RegistryNamespace; owned = $registryNamespaceOwned }
        )) {
            if ($namespace.owned) {
                try { kubectl delete namespace $namespace.name --wait=true --timeout=600s --ignore-not-found | Out-Null } catch { Record-Failure $_.Exception.Message }
            }
        }
        if ($installedCaThumbprint) {
            Remove-Item "Cert:\CurrentUser\Root\$installedCaThumbprint" -Force -ErrorAction SilentlyContinue
            try { Restart-DockerDesktop } catch { Record-Failure $_.Exception.Message }
        }
    }
    foreach ($entry in $replicaSnapshots.GetEnumerator()) {
        try { Scale-Snapshot $entry.Key $entry.Value -Restore } catch { Record-Failure $_.Exception.Message }
    }
    foreach ($worktree in $createdWorktrees) { try { git worktree remove --force $worktree | Out-Null } catch { Record-Failure $_.Exception.Message } }
    Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    $env:PATH = $originalPath
    if ($openSandboxDeployKeyWasSet) { $env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY = $originalOpenSandboxDeployKey } else { Remove-Item Env:AGENTX_DEPLOY_OPENSANDBOX_API_KEY -ErrorAction SilentlyContinue }

    try {
        $secretScan = Invoke-SecretScan
        $secretScanStatus = [string]$secretScan.status
        if ($secretScanStatus -ne "passed") { Record-Failure "Secret scan found sensitive material in the evidence directory." }
    } catch {
        Record-Failure $_.Exception.Message
        $secretScanStatus = "failed"
    }
    $junit = Write-RootJunit
    $inventory = @(Write-EvidenceInventory)
    $summary = [ordered]@{
        schemaVersion = "agentx.io/m7-local-evidence/v1"
        status = $(if ($int009 -eq "passed" -and $int011 -eq "passed" -and $secretScanStatus -eq "passed" -and -not $failure) { "passed" } else { "failed" })
        runId = $runId
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
        environment = "local-docker-desktop"
        registry = "local-tls"
        trustScope = "local-only"
        isolationLevel = "standard"
        int009 = $int009
        int011 = $int011
        int012 = "in_progress"
        int014 = "in_progress"
        m7 = "in_progress"
        failure = $failure
        junit = $junit
        evidence = $inventory
    }
    $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $summaryPath -Encoding utf8NoBOM
    if (-not ((Get-Content -Raw -LiteralPath $summaryPath) | Test-Json -SchemaFile (Join-Path $root "deploy/release/m7-local-evidence.schema.json"))) {
        Record-Failure "Generated local M7 evidence does not match its schema."
        $summary.status = "failed"
        $summary.failure = $failure
        $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $summaryPath -Encoding utf8NoBOM
    }
}

Write-Output $summaryPath
if ($failure -or $int009 -ne "passed" -or $int011 -ne "passed" -or $secretScanStatus -ne "passed") {
    throw "M7 local release acceptance failed. Evidence: $summaryPath. $failure"
}
