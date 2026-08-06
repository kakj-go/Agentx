param(
    [int]$Port = 18200,
    [string]$OutputDirectory = "artifacts/m7",
    [switch]$KeepContainer
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$output = Join-Path $root "$OutputDirectory/$runId"
$container = "agentx-m7-vault-$PID"
$image = "hashicorp/vault@sha256:1262354cd28697b7982ea3b9b6f159a996bdaad0b5270765e31b67797ce15bea"
$token = "agentx-m7-vault-$([Guid]::NewGuid().ToString('N'))"
$started = $false

function Invoke-Native([string]$Name, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE." }
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { throw "docker is required." }
New-Item -ItemType Directory -Path $output -Force | Out-Null

Push-Location $root
try {
    Invoke-Native "start Vault" {
        docker run --detach --rm --name $container `
            --publish "127.0.0.1:${Port}:8200" `
            --env "VAULT_DEV_ROOT_TOKEN_ID=$token" `
            --env "VAULT_DEV_LISTEN_ADDRESS=0.0.0.0:8200" `
            $image
    }
    $started = $true
    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        try {
            $health = Invoke-RestMethod -TimeoutSec 2 -Uri "http://127.0.0.1:$Port/v1/sys/health"
            if ($health.initialized -and -not $health.sealed) { $ready = $true; break }
        }
        catch {
            Start-Sleep -Milliseconds 500
        }
    }
    if (-not $ready) { throw "Vault did not become ready." }

    $env:AGENTX_VAULT_ADDR = "http://127.0.0.1:$Port"
    $env:AGENTX_VAULT_TOKEN = $token
    $env:AGENTX_VAULT_KV_MOUNT = "secret"
    Invoke-Native "Vault KV v2 integration test" {
        cargo test -p agentx-infrastructure `
            credential::tests::vault_kv_v2_rotates_reads_versions_and_destroys_old_material `
            -- --ignored --exact
    }

    $evidence = [ordered]@{
        schemaVersion = "agentx.io/vault-evidence/v1"
        status = "passed"
        runId = $runId
        image = $image
        endpoint = "http://127.0.0.1:$Port"
        kvMount = "secret"
        rotationVerified = $true
        oldVersionDestroyed = $true
        generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    }
    $path = Join-Path $output "vault-evidence.json"
    $evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $path -Encoding utf8NoBOM
    $schema = Join-Path $root "deploy/release/vault-evidence.schema.json"
    if (-not ((Get-Content -Raw -LiteralPath $path) | Test-Json -SchemaFile $schema)) {
        throw "Vault evidence is invalid."
    }
    Write-Output $path
}
finally {
    Remove-Item Env:AGENTX_VAULT_ADDR -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_VAULT_TOKEN -ErrorAction SilentlyContinue
    Remove-Item Env:AGENTX_VAULT_KV_MOUNT -ErrorAction SilentlyContinue
    if ($started) {
        # Container logs can contain the dev root token. Keep only non-sensitive run metadata.
        $logPath = Join-Path $output "vault.log"
        if (Test-Path -LiteralPath $logPath) { Remove-Item -LiteralPath $logPath -Force }
        if (-not $KeepContainer) { docker rm --force $container | Out-Null }
    }
    Pop-Location
}
