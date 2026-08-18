[CmdletBinding()]
param(
    [string]$OpenApiPath = (Join-Path $PSScriptRoot "../openapi/platform-api.json"),
    [string]$OutputPath,
    [switch]$FailOnMigrationRequired
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$openApiPath = (Resolve-Path $OpenApiPath).Path
$openApi = Get-Content -Raw -LiteralPath $openApiPath | ConvertFrom-Json
$sourceRoot = Join-Path $root "services/platform-control/src"

function Normalize-Route([string]$Route) {
    return [regex]::Replace($Route, '\{[^}]+\}', '{}')
}

function Resolve-Owner([string]$Path) {
    if ($Path -match '^/api/v1/executions') {
        return @{ owner = "runtime_bff"; plane = "runtime" }
    }
    if ($Path -match '^/api/v1/(approvals|evaluations|notifications|retention-runs)') {
        return @{ owner = "governance_api"; plane = "control_projection" }
    }
    return @{ owner = "control_api"; plane = "control" }
}

$source = (Get-ChildItem -LiteralPath $sourceRoot -Recurse -Filter *.rs |
    ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }) -join "`n"
$implemented = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$implementedOperations = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$routeMatches = @([regex]::Matches($source, '(?s)\.route\(\s*"(?<path>/api/v1/[^"\s]+)"'))
for ($index = 0; $index -lt $routeMatches.Count; $index++) {
    $match = $routeMatches[$index]
    $path = Normalize-Route $match.Groups['path'].Value
    [void]$implemented.Add($path)
    $end = if ($index + 1 -lt $routeMatches.Count) { $routeMatches[$index + 1].Index } else { $source.Length }
    $length = [Math]::Min($end - $match.Index, 2000)
    $segment = $source.Substring($match.Index, $length)
    foreach ($method in [regex]::Matches($segment, '(?<![A-Za-z])(get|post|put|patch|delete)\s*\(')) {
        [void]$implementedOperations.Add("$($method.Groups[1].Value) $path")
    }
}

# V2 routers intentionally parse action suffixes in a single typed handler.
$routeAliases = @{
    "/api/v1/applications/{}/deployments/{}:rollback" = "/api/v1/applications/{}/deployments/{}"
    "/api/v1/applications/{}/publish-attempts/{}:retry" = "/api/v1/applications/{}/publish-attempts/{}"
}

$items = foreach ($property in $openApi.paths.PSObject.Properties | Sort-Object Name) {
    $path = [string]$property.Name
    $normalized = Normalize-Route $path
    $lookup = if ($routeAliases.ContainsKey($normalized)) { $routeAliases[$normalized] } else { $normalized }
    $assignment = Resolve-Owner $path
    $operations = @($property.Value.PSObject.Properties.Name | Where-Object { $_ -in @("get", "post", "put", "patch", "delete") } | Sort-Object)
    $missingOperations = @($operations | Where-Object { -not $implementedOperations.Contains("$_ $lookup") })
    $status = if ($implemented.Contains($lookup) -and $missingOperations.Count -eq 0) { "implemented" } else { "migration_required" }
    [ordered]@{
        path = $path
        normalizedPath = $normalized
        operations = $operations
        missingOperations = $missingOperations
        owner = $assignment.owner
        dataPlane = $assignment.plane
        status = $status
        replacement = if ($status -eq "implemented") { "platform-control" } else { $null }
        deletionGate = "V2C-001"
    }
}

$unresolved = @($items | Where-Object status -eq "migration_required")
$report = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    source = $openApiPath.Substring($root.Length).TrimStart('\', '/') -replace '\\', '/'
    totalPaths = @($items).Count
    totalOperations = @($items | ForEach-Object { $_.operations.Count } | Measure-Object -Sum).Sum
    implementedPaths = @($items).Count - $unresolved.Count
    migrationRequiredPaths = $unresolved.Count
    deletionAllowed = $unresolved.Count -eq 0
    items = @($items)
}
$json = $report | ConvertTo-Json -Depth 12
if ($OutputPath) {
    $parent = Split-Path -Parent $OutputPath
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    Set-Content -LiteralPath $OutputPath -Value $json -Encoding utf8
} else {
    $json
}
if ($FailOnMigrationRequired -and $unresolved.Count -ne 0) {
    throw "Platform API cutover is blocked by $($unresolved.Count) unresolved path(s)."
}
