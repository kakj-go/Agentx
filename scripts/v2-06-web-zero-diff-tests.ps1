$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$baselinePath = Join-Path $root "docs/planv2/contracts/v2-06-web-source-baseline.json"
$baseline = Get-Content -Raw -LiteralPath $baselinePath | ConvertFrom-Json

if ($baseline.schemaVersion -ne 1 -or $baseline.algorithm -ne "sha256-normalized-lf-path-content-v1") {
    throw "Unsupported V2-06 Web source baseline format."
}

$files = @(rg --files (Join-Path $root $baseline.root) | Sort-Object)
if ($files.Count -ne $baseline.fileCount) {
    throw "V2-06 must not add or remove apps/web/src files. Expected $($baseline.fileCount), got $($files.Count)."
}

$builder = [Text.StringBuilder]::new()
foreach ($file in $files) {
    $relative = [IO.Path]::GetRelativePath($root, $file).Replace("\", "/")
    $content = (Get-Content -Raw -LiteralPath $file).Replace("`r`n", "`n").Replace("`r", "`n")
    [void]$builder.Append($relative).Append("`n").Append($content).Append("`n")
}
$bytes = [Text.Encoding]::UTF8.GetBytes($builder.ToString())
$actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
if ($actual -cne $baseline.sha256) {
    throw "V2-06 apps/web/src zero-diff gate failed. Expected $($baseline.sha256), got $actual."
}

Write-Output "V2-06 apps/web/src zero-diff tests passed"
