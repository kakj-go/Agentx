param(
    [switch]$NoDocker
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    cargo test -p agentx-infrastructure --test tls_clients -- --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "ClickHouse/S3 TLS integration contracts failed."
    }
    if (-not $NoDocker) {
        docker info --format '{{.ServerVersion}}' | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Docker is required for the MySQL/Redis TLS integration contracts."
        }
        cargo test -p agentx-infrastructure --test tls_clients mysql_and_redis_enforce_private_ca_mtls_and_authentication -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) {
            throw "MySQL/Redis TLS integration contracts failed."
        }
    }
}
finally {
    Pop-Location
}
